//! Async IRC networking runtime for TIRC.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Utc};
use irc_proto::{
    Command, Message, Response, chan::ChannelExt, command::CapSubCommand, message::Tag,
};
use rand::Rng;
use thiserror::Error;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader, split},
    net::TcpStream,
    sync::mpsc,
    task::JoinHandle,
    time,
};
use tokio_native_tls::TlsConnector;
use tracing::{debug, warn};

use tirc_config::{SaslMechanism, ServerProfile};
use tirc_core::NetworkEvent;

/// CTCP delimiter character.
const CTCP_DELIM: char = '\x01';

/// Version string for CTCP VERSION replies.
const TIRC_VERSION: &str = concat!(env!("CARGO_PKG_NAME"), " ", env!("CARGO_PKG_VERSION"));

/// Outbound command sent from app state/commands to a network worker.
#[derive(Debug, Clone)]
pub enum NetCommand {
    Raw(String),
    Join(String),
    Part {
        channel: String,
        reason: Option<String>,
    },
    List,
    Privmsg {
        target: String,
        body: String,
    },
    Nick(String),
    Disconnect,
}

/// Networking errors.
#[derive(Debug, Error)]
pub enum NetError {
    #[error("profile already connected: {0}")]
    AlreadyConnected(String),
    #[error("unknown profile: {0}")]
    UnknownNetwork(String),
    #[error("channel send failed")]
    ChannelClosed,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("tls error: {0}")]
    Tls(#[from] tokio_native_tls::native_tls::Error),
    #[error("too many nick collisions, giving up")]
    TooManyNickCollisions,
}

struct ConnectionHandle {
    tx: mpsc::Sender<NetCommand>,
    task: JoinHandle<()>,
}

/// Multi-server manager that owns one task per active connection.
pub struct NetManager {
    handles: HashMap<String, ConnectionHandle>,
    events_tx: mpsc::Sender<NetworkEvent>,
}

impl NetManager {
    pub fn new(events_tx: mpsc::Sender<NetworkEvent>) -> Self {
        Self {
            handles: HashMap::new(),
            events_tx,
        }
    }

    pub async fn connect_profile(&mut self, profile: ServerProfile) -> Result<(), NetError> {
        if self.handles.contains_key(&profile.name) {
            return Err(NetError::AlreadyConnected(profile.name));
        }

        let network_name = profile.name.clone();
        let (tx, rx) = mpsc::channel::<NetCommand>(512);
        let events = self.events_tx.clone();

        let task = tokio::spawn(async move {
            run_connection_task(profile, rx, events).await;
        });

        self.handles
            .insert(network_name, ConnectionHandle { tx, task });
        Ok(())
    }

    pub async fn send_command(&self, network: &str, command: NetCommand) -> Result<(), NetError> {
        let Some(handle) = self.handles.get(network) else {
            return Err(NetError::UnknownNetwork(network.to_owned()));
        };

        handle
            .tx
            .send(command)
            .await
            .map_err(|_| NetError::ChannelClosed)
    }

    pub async fn disconnect(&mut self, network: &str) -> Result<(), NetError> {
        let Some(handle) = self.handles.remove(network) else {
            return Err(NetError::UnknownNetwork(network.to_owned()));
        };

        let _ = handle.tx.send(NetCommand::Disconnect).await;
        handle.task.abort();
        Ok(())
    }

    pub async fn disconnect_all(&mut self) {
        let mut handles = HashMap::new();
        std::mem::swap(&mut self.handles, &mut handles);

        for (_, handle) in handles {
            let _ = handle.tx.send(NetCommand::Disconnect).await;
            handle.task.abort();
        }
    }
}

#[derive(Debug, Clone)]
struct ConnectionRuntime {
    requested_caps: BTreeSet<String>,
    acked_caps: BTreeSet<String>,
    current_nick: String,
    original_nick: String,
    nick_collision_count: u8,
    registration_complete: bool,
    sasl_in_progress: bool,
    sasl_done: bool,
}

impl ConnectionRuntime {
    fn new(profile: &ServerProfile) -> Self {
        Self {
            requested_caps: BTreeSet::new(),
            acked_caps: BTreeSet::new(),
            current_nick: profile.nick.clone(),
            original_nick: profile.nick.clone(),
            nick_collision_count: 0,
            registration_complete: false,
            sasl_in_progress: false,
            sasl_done: profile.sasl.is_none(),
        }
    }
}

async fn run_connection_task(
    profile: ServerProfile,
    mut command_rx: mpsc::Receiver<NetCommand>,
    events_tx: mpsc::Sender<NetworkEvent>,
) {
    let mut attempt = 0_u32;
    let mut should_exit = false;
    let mut joined_channels = profile.autojoin.iter().cloned().collect::<BTreeSet<_>>();

    while !should_exit {
        match connect_stream(&profile).await {
            Ok(stream) => {
                attempt = 0;
                if events_tx
                    .send(NetworkEvent::Connected {
                        network: profile.name.clone(),
                        server: format!("{}:{}", profile.host, profile.port),
                    })
                    .await
                    .is_err()
                {
                    return;
                }

                match run_live_connection(
                    stream,
                    &profile,
                    &mut command_rx,
                    &events_tx,
                    &mut joined_channels,
                )
                .await
                {
                    Ok(ConnectionExit::Requested) => {
                        should_exit = true;
                    }
                    Ok(ConnectionExit::Unexpected(reason)) => {
                        let _ = events_tx
                            .send(NetworkEvent::Disconnected {
                                network: profile.name.clone(),
                                reason,
                            })
                            .await;
                    }
                    Err(err) => {
                        let _ = events_tx
                            .send(NetworkEvent::Disconnected {
                                network: profile.name.clone(),
                                reason: err.to_string(),
                            })
                            .await;
                    }
                }
            }
            Err(err) => {
                let _ = events_tx
                    .send(NetworkEvent::Disconnected {
                        network: profile.name.clone(),
                        reason: format!("connection failed: {err}"),
                    })
                    .await;
            }
        }

        if should_exit {
            break;
        }

        attempt = attempt.saturating_add(1);
        let delay = reconnect_delay(attempt);

        let sleep = time::sleep(delay);
        tokio::pin!(sleep);
        loop {
            tokio::select! {
                _ = &mut sleep => break,
                maybe_command = command_rx.recv() => {
                    let Some(command) = maybe_command else {
                        return;
                    };

                    if apply_offline_command(&mut joined_channels, command) {
                        return;
                    }
                }
            }
        }
    }
}

fn apply_offline_command(joined_channels: &mut BTreeSet<String>, command: NetCommand) -> bool {
    match command {
        NetCommand::Disconnect => true,
        NetCommand::Join(channel) => {
            joined_channels.insert(channel);
            false
        }
        NetCommand::Part { channel, .. } => {
            joined_channels.remove(&channel);
            false
        }
        NetCommand::Raw(_)
        | NetCommand::Privmsg { .. }
        | NetCommand::Nick(_)
        | NetCommand::List => false,
    }
}

async fn run_live_connection(
    stream: DynStream,
    profile: &ServerProfile,
    command_rx: &mut mpsc::Receiver<NetCommand>,
    events_tx: &mpsc::Sender<NetworkEvent>,
    joined_channels: &mut BTreeSet<String>,
) -> Result<ConnectionExit, NetError> {
    let (reader_half, mut writer_half) = split(stream);
    let mut reader = BufReader::new(reader_half);
    let mut line_buf = Vec::with_capacity(1024);

    let mut runtime = ConnectionRuntime::new(profile);
    send_registration(&mut writer_half, profile).await?;

    loop {
        line_buf.clear();
        tokio::select! {
            maybe_command = command_rx.recv() => {
                let Some(command) = maybe_command else {
                    return Ok(ConnectionExit::Requested);
                };

                if handle_command(
                    command,
                    &profile.name,
                    &mut writer_half,
                    joined_channels,
                    events_tx,
                ).await? {
                    return Ok(ConnectionExit::Requested);
                }
            }
            read = reader.read_until(b'\n', &mut line_buf) => {
                let count = read?;
                if count == 0 {
                    return Ok(ConnectionExit::Unexpected("connection closed by server".to_owned()));
                }

                let line = decode_server_line(&line_buf);

                let result = handle_server_line(
                    &line,
                    profile,
                    &mut runtime,
                    &mut writer_half,
                    events_tx,
                    joined_channels,
                ).await;
                
                match result {
                    Ok(()) => {}
                    Err(NetError::TooManyNickCollisions) => {
                        return Ok(ConnectionExit::Unexpected(
                            "too many nick collisions".to_owned()
                        ));
                    }
                    Err(err) => {
                        warn!(network = profile.name, "failed to process incoming line: {err}");
                    }
                }
            }
        }
    }
}

fn decode_server_line(raw: &[u8]) -> String {
    let without_lf = raw.strip_suffix(b"\n").unwrap_or(raw);
    let trimmed = without_lf.strip_suffix(b"\r").unwrap_or(without_lf);
    String::from_utf8_lossy(trimmed).into_owned()
}

async fn send_registration<W: AsyncWrite + Unpin>(
    writer: &mut W,
    profile: &ServerProfile,
) -> Result<(), NetError> {
    let cap_ls = Command::CAP(None, CapSubCommand::LS, Some("302".to_owned()), None);
    write_command(writer, cap_ls).await?;
    write_command(writer, Command::NICK(profile.nick.clone())).await?;
    write_command(
        writer,
        Command::USER(
            profile.user.clone(),
            "0".to_owned(),
            profile.realname.clone(),
        ),
    )
    .await?;
    Ok(())
}

async fn handle_command<W: AsyncWrite + Unpin>(
    command: NetCommand,
    network: &str,
    writer: &mut W,
    joined_channels: &mut BTreeSet<String>,
    events_tx: &mpsc::Sender<NetworkEvent>,
) -> Result<bool, NetError> {
    match command {
        NetCommand::Raw(line) => {
            write_raw_line(writer, &line).await?;
        }
        NetCommand::Join(channel) => {
            joined_channels.insert(channel.clone());
            write_command(writer, Command::JOIN(channel.clone(), None, None)).await?;
            let _ = events_tx
                .send(NetworkEvent::Joined {
                    network: network.to_owned(),
                    channel,
                })
                .await;
        }
        NetCommand::Part { channel, reason } => {
            joined_channels.remove(&channel);
            write_command(writer, Command::PART(channel.clone(), reason.clone())).await?;
            let _ = events_tx
                .send(NetworkEvent::Parted {
                    network: network.to_owned(),
                    channel,
                    reason,
                })
                .await;
        }
        NetCommand::List => {
            write_command(writer, Command::LIST(None, None)).await?;
        }
        NetCommand::Privmsg { target, body } => {
            write_command(writer, Command::PRIVMSG(target, body)).await?;
        }
        NetCommand::Nick(nick) => {
            write_command(writer, Command::NICK(nick)).await?;
        }
        NetCommand::Disconnect => {
            let _ = write_command(writer, Command::QUIT(Some("leaving".to_owned()))).await;
            return Ok(true);
        }
    }

    Ok(false)
}

async fn handle_server_line<W: AsyncWrite + Unpin>(
    line: &str,
    profile: &ServerProfile,
    runtime: &mut ConnectionRuntime,
    writer: &mut W,
    events_tx: &mpsc::Sender<NetworkEvent>,
    joined_channels: &mut BTreeSet<String>,
) -> Result<(), NetError> {
    let message = match line.parse::<Message>() {
        Ok(message) => message,
        Err(err) => {
            debug!(
                network = profile.name,
                "could not parse incoming IRC line: {err}"
            );
            return Ok(());
        }
    };

    let tags = parse_tags(message.tags.clone());
    let ts = parse_timestamp(&tags).unwrap_or_else(Utc::now);

    let source_nick = message.source_nickname().map(ToOwned::to_owned);

    match message.command {
        Command::PING(server, token) => {
            write_command(writer, Command::PONG(server, token)).await?;
        }
        Command::PRIVMSG(target, body) => {
            let sender = source_nick.clone();
            
            // Check for CTCP request (wrapped in \x01)
            if body.starts_with(CTCP_DELIM) && body.ends_with(CTCP_DELIM) && body.len() > 2 {
                let ctcp_content = &body[1..body.len()-1];
                let (ctcp_cmd, ctcp_arg) = ctcp_content
                    .split_once(' ')
                    .map(|(c, a)| (c, Some(a)))
                    .unwrap_or((ctcp_content, None));
                
                // Handle known CTCP requests
                if let Some(nick) = &sender {
                    match ctcp_cmd.to_ascii_uppercase().as_str() {
                        "VERSION" => {
                            let reply = format!("{CTCP_DELIM}VERSION {TIRC_VERSION}{CTCP_DELIM}");
                            write_command(writer, Command::NOTICE(nick.clone(), reply)).await?;
                        }
                        "PING" => {
                            // Echo back the same payload for PING
                            let payload = ctcp_arg.unwrap_or("");
                            let reply = format!("{CTCP_DELIM}PING {payload}{CTCP_DELIM}");
                            write_command(writer, Command::NOTICE(nick.clone(), reply)).await?;
                        }
                        "TIME" => {
                            let now = chrono::Local::now().format("%a %b %d %H:%M:%S %Y");
                            let reply = format!("{CTCP_DELIM}TIME {now}{CTCP_DELIM}");
                            write_command(writer, Command::NOTICE(nick.clone(), reply)).await?;
                        }
                        "CLIENTINFO" => {
                            let reply = format!("{CTCP_DELIM}CLIENTINFO VERSION PING TIME CLIENTINFO SOURCE{CTCP_DELIM}");
                            write_command(writer, Command::NOTICE(nick.clone(), reply)).await?;
                        }
                        "SOURCE" => {
                            let reply = format!("{CTCP_DELIM}SOURCE https://github.com/user/tirc{CTCP_DELIM}");
                            write_command(writer, Command::NOTICE(nick.clone(), reply)).await?;
                        }
                        "ACTION" => {
                            // ACTION is displayed as a message, not auto-replied
                            let action_body = ctcp_arg.unwrap_or("");
                            let is_private = !target.is_channel_name();
                            let buffer = if is_private {
                                sender.clone().unwrap_or_else(|| target.clone())
                            } else {
                                target.clone()
                            };
                            let is_mention = action_body
                                .to_ascii_lowercase()
                                .contains(&runtime.current_nick.to_ascii_lowercase());
                            
                            let _ = events_tx
                                .send(NetworkEvent::Action {
                                    network: profile.name.clone(),
                                    buffer,
                                    sender,
                                    body: action_body.to_owned(),
                                    is_private,
                                    is_mention,
                                    ts,
                                })
                                .await;
                        }
                        _ => {
                            // Unknown CTCP, ignore
                            debug!("Received unknown CTCP: {ctcp_cmd}");
                        }
                    }
                }
                return Ok(());
            }
            
            let is_private = !target.is_channel_name();
            let buffer = if is_private {
                sender.clone().unwrap_or_else(|| target.clone())
            } else {
                target.clone()
            };
            let is_mention = body
                .to_ascii_lowercase()
                .contains(&runtime.current_nick.to_ascii_lowercase());

            let _ = events_tx
                .send(NetworkEvent::Message {
                    network: profile.name.clone(),
                    buffer,
                    sender,
                    body,
                    tags,
                    is_private,
                    is_mention,
                    ts,
                })
                .await;
        }
        Command::NOTICE(target, body) => {
            let sender = source_nick.clone();
            let _ = events_tx
                .send(NetworkEvent::Message {
                    network: profile.name.clone(),
                    buffer: if target.is_channel_name() {
                        target
                    } else {
                        "status".to_owned()
                    },
                    sender,
                    body,
                    tags,
                    is_private: false,
                    is_mention: false,
                    ts,
                })
                .await;
        }
        Command::JOIN(channel, _, _) => {
            if source_nick.as_deref() == Some(runtime.current_nick.as_str()) {
                joined_channels.insert(channel.clone());
                let _ = events_tx
                    .send(NetworkEvent::Joined {
                        network: profile.name.clone(),
                        channel,
                    })
                    .await;
            }
        }
        Command::PART(channel, reason) => {
            if source_nick.as_deref() == Some(runtime.current_nick.as_str()) {
                joined_channels.remove(&channel);
            }
            let _ = events_tx
                .send(NetworkEvent::Parted {
                    network: profile.name.clone(),
                    channel,
                    reason,
                })
                .await;
        }
        Command::NICK(new_nick) => {
            if source_nick.as_deref() == Some(runtime.current_nick.as_str()) {
                runtime.current_nick = new_nick.clone();
            }
            let _ = events_tx
                .send(NetworkEvent::System {
                    network: profile.name.clone(),
                    body: format!("Nick changed to {new_nick}"),
                    ts,
                })
                .await;
        }
        Command::QUIT(reason) => {
            if let Some(nick) = source_nick {
                // Find all channels this user was in
                // Note: We don't track other users' channel membership, so we send to all joined channels
                // The UI can filter based on nick list tracking
                let channels: Vec<String> = joined_channels.iter().cloned().collect();
                let _ = events_tx
                    .send(NetworkEvent::UserQuit {
                        network: profile.name.clone(),
                        nick,
                        reason,
                        channels,
                    })
                    .await;
            }
        }
        Command::CAP(_, subcommand, _, params) => {
            handle_cap_message(subcommand, params, profile, runtime, writer, events_tx).await?;
        }
        Command::AUTHENTICATE(data) => {
            if data == "+"
                && !runtime.sasl_done
                && runtime.sasl_in_progress
                && profile
                    .secret
                    .as_ref()
                    .and_then(|secret| secret.sasl_password.clone())
                    .is_some()
                && profile.sasl.is_some()
            {
                send_sasl_payload(writer, profile).await?;
            }
        }
        Command::Response(response, args) => {
            handle_response(
                response,
                args,
                profile,
                runtime,
                writer,
                events_tx,
                joined_channels,
                ts,
            )
            .await?;
        }
        Command::Raw(cmd, args) => {
            let _ = events_tx
                .send(NetworkEvent::System {
                    network: profile.name.clone(),
                    body: format!("{cmd} {}", args.join(" ")),
                    ts,
                })
                .await;
        }
        _ => {}
    }

    Ok(())
}

async fn handle_cap_message<W: AsyncWrite + Unpin>(
    subcommand: CapSubCommand,
    params: Option<String>,
    profile: &ServerProfile,
    runtime: &mut ConnectionRuntime,
    writer: &mut W,
    events_tx: &mpsc::Sender<NetworkEvent>,
) -> Result<(), NetError> {
    match subcommand {
        CapSubCommand::LS | CapSubCommand::NEW => {
            let advertised = params
                .unwrap_or_default()
                .split_whitespace()
                .map(ToOwned::to_owned)
                .collect::<BTreeSet<_>>();

            let mut desired_caps = vec!["server-time", "echo-message", "batch"];
            if profile.sasl.is_some() {
                desired_caps.push("sasl");
            }

            let request = desired_caps
                .into_iter()
                .filter(|cap| advertised.contains(*cap))
                .collect::<Vec<_>>();

            if request.is_empty() {
                write_command(writer, Command::CAP(None, CapSubCommand::END, None, None)).await?;
                return Ok(());
            }

            let req = request.join(" ");
            write_command(
                writer,
                Command::CAP(None, CapSubCommand::REQ, None, Some(req.clone())),
            )
            .await?;
            runtime
                .requested_caps
                .extend(request.into_iter().map(str::to_owned));
        }
        CapSubCommand::ACK => {
            let acked = params
                .unwrap_or_default()
                .split_whitespace()
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>();
            runtime.acked_caps.extend(acked.iter().cloned());

            if acked.iter().any(|cap| cap == "sasl") && !runtime.sasl_done && profile.sasl.is_some()
            {
                runtime.sasl_in_progress = true;
                write_command(writer, Command::AUTHENTICATE("PLAIN".to_owned())).await?;
            } else {
                write_command(writer, Command::CAP(None, CapSubCommand::END, None, None)).await?;
            }
        }
        CapSubCommand::NAK => {
            let _ = events_tx
                .send(NetworkEvent::System {
                    network: profile.name.clone(),
                    body: "server rejected requested capabilities".to_owned(),
                    ts: Utc::now(),
                })
                .await;
            write_command(writer, Command::CAP(None, CapSubCommand::END, None, None)).await?;
        }
        CapSubCommand::LIST | CapSubCommand::END | CapSubCommand::DEL | CapSubCommand::REQ => {}
    }

    Ok(())
}

async fn handle_response<W: AsyncWrite + Unpin>(
    response: Response,
    args: Vec<String>,
    profile: &ServerProfile,
    runtime: &mut ConnectionRuntime,
    writer: &mut W,
    events_tx: &mpsc::Sender<NetworkEvent>,
    joined_channels: &BTreeSet<String>,
    ts: DateTime<Utc>,
) -> Result<(), NetError> {
    match response {
        Response::RPL_WELCOME => {
            runtime.registration_complete = true;
            for channel in joined_channels {
                write_command(writer, Command::JOIN(channel.clone(), None, None)).await?;
            }
            let _ = events_tx
                .send(NetworkEvent::System {
                    network: profile.name.clone(),
                    body: args.join(" "),
                    ts,
                })
                .await;
        }
        Response::RPL_TOPIC => {
            if args.len() >= 3 {
                let _ = events_tx
                    .send(NetworkEvent::System {
                        network: profile.name.clone(),
                        body: format!("topic {}: {}", args[1], args[2]),
                        ts,
                    })
                    .await;
            }
        }
        Response::RPL_LISTSTART => {
            let _ = events_tx
                .send(NetworkEvent::ChannelListStart {
                    network: profile.name.clone(),
                })
                .await;
            let _ = events_tx
                .send(NetworkEvent::System {
                    network: profile.name.clone(),
                    body: "Channel list start".to_owned(),
                    ts,
                })
                .await;
        }
        Response::RPL_LIST => {
            if args.len() >= 4 {
                let channel = &args[1];
                let users = args[2].parse::<u32>().unwrap_or(0);
                let topic = &args[3];
                let _ = events_tx
                    .send(NetworkEvent::ChannelListItem {
                        network: profile.name.clone(),
                        channel: channel.to_owned(),
                        users,
                        topic: topic.to_owned(),
                    })
                    .await;
                let _ = events_tx
                    .send(NetworkEvent::System {
                        network: profile.name.clone(),
                        body: format!("{channel} ({users}) {topic}"),
                        ts,
                    })
                    .await;
            }
        }
        Response::RPL_LISTEND => {
            let _ = events_tx
                .send(NetworkEvent::ChannelListEnd {
                    network: profile.name.clone(),
                })
                .await;
            let _ = events_tx
                .send(NetworkEvent::System {
                    network: profile.name.clone(),
                    body: "Channel list end".to_owned(),
                    ts,
                })
                .await;
        }
        Response::RPL_NAMREPLY => {
            if args.len() >= 4 {
                let channel = args[2].clone();
                let nicks = args[3]
                    .split_whitespace()
                    .map(|nick| nick.trim_start_matches(['@', '+', '%', '&', '~']))
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>();
                let _ = events_tx
                    .send(NetworkEvent::NickList {
                        network: profile.name.clone(),
                        channel,
                        nicks,
                    })
                    .await;
            }
        }
        Response::RPL_MOTD | Response::RPL_MOTDSTART => {
            let _ = events_tx
                .send(NetworkEvent::System {
                    network: profile.name.clone(),
                    body: args.join(" "),
                    ts,
                })
                .await;
        }
        Response::RPL_SASLSUCCESS => {
            runtime.sasl_in_progress = false;
            runtime.sasl_done = true;
            write_command(writer, Command::CAP(None, CapSubCommand::END, None, None)).await?;
            let _ = events_tx
                .send(NetworkEvent::System {
                    network: profile.name.clone(),
                    body: "SASL authentication successful".to_owned(),
                    ts,
                })
                .await;
        }
        Response::ERR_SASLFAIL
        | Response::ERR_SASLTOOLONG
        | Response::ERR_SASLABORT
        | Response::ERR_SASLALREADY => {
            runtime.sasl_in_progress = false;
            runtime.sasl_done = true;
            write_command(writer, Command::CAP(None, CapSubCommand::END, None, None)).await?;
            let _ = events_tx
                .send(NetworkEvent::System {
                    network: profile.name.clone(),
                    body: format!("SASL issue: {:?} {}", response, args.join(" ")),
                    ts,
                })
                .await;
        }
        Response::ERR_NICKNAMEINUSE => {
            const MAX_NICK_COLLISIONS: u8 = 5;
            const MAX_NICK_LENGTH: usize = 16;
            
            runtime.nick_collision_count += 1;
            
            if runtime.nick_collision_count > MAX_NICK_COLLISIONS {
                let _ = events_tx
                    .send(NetworkEvent::System {
                        network: profile.name.clone(),
                        body: format!(
                            "Too many nick collisions ({MAX_NICK_COLLISIONS}), disconnecting"
                        ),
                        ts,
                    })
                    .await;
                return Err(NetError::TooManyNickCollisions);
            }
            
            // Use underscore suffix pattern: nick, nick_, nick_1, nick_2, etc.
            let suffix = if runtime.nick_collision_count == 1 {
                "_".to_owned()
            } else {
                format!("_{}", runtime.nick_collision_count - 1)
            };
            
            // Truncate base nick if resulting nick would exceed max length
            let max_base_len = MAX_NICK_LENGTH.saturating_sub(suffix.len());
            let base = if runtime.original_nick.len() > max_base_len {
                &runtime.original_nick[..max_base_len]
            } else {
                &runtime.original_nick
            };
            
            let fallback = format!("{base}{suffix}");
            runtime.current_nick = fallback.clone();
            
            let _ = events_tx
                .send(NetworkEvent::System {
                    network: profile.name.clone(),
                    body: format!("Nick in use, trying {fallback}"),
                    ts,
                })
                .await;
            
            write_command(writer, Command::NICK(fallback)).await?;
        }
        Response::ERR_NOTREGISTERED
        | Response::ERR_NEEDMOREPARAMS
        | Response::ERR_UNKNOWNCOMMAND
        | Response::ERR_NOSUCHCHANNEL
        | Response::ERR_CANNOTSENDTOCHAN
        | Response::ERR_NOSUCHNICK => {
            let _ = events_tx
                .send(NetworkEvent::System {
                    network: profile.name.clone(),
                    body: format!("error {:?}: {}", response, args.join(" ")),
                    ts,
                })
                .await;
        }
        _ => {}
    }

    Ok(())
}

async fn send_sasl_payload<W: AsyncWrite + Unpin>(
    writer: &mut W,
    profile: &ServerProfile,
) -> Result<(), NetError> {
    let Some(sasl) = profile.sasl.as_ref() else {
        return Ok(());
    };

    if !matches!(sasl.mechanism, SaslMechanism::Plain) {
        return Ok(());
    }

    let Some(secret) = profile.secret.as_ref() else {
        return Ok(());
    };
    let Some(password) = secret.sasl_password.as_ref().or(secret.password.as_ref()) else {
        return Ok(());
    };

    let payload = format!("{}\0{}\0{}", sasl.account, sasl.account, password);
    let encoded = STANDARD.encode(payload);
    write_command(writer, Command::AUTHENTICATE(encoded)).await
}

fn parse_tags(tags: Option<Vec<Tag>>) -> BTreeMap<String, String> {
    tags.unwrap_or_default()
        .into_iter()
        .map(|tag| (tag.0, tag.1.unwrap_or_default()))
        .collect()
}

fn parse_timestamp(tags: &BTreeMap<String, String>) -> Option<DateTime<Utc>> {
    tags.get("time")
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .map(|ts| ts.with_timezone(&Utc))
}

type DynStream = Box<dyn AsyncReadWrite>;

trait AsyncReadWrite: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T> AsyncReadWrite for T where T: AsyncRead + AsyncWrite + Send + Unpin {}

async fn connect_stream(profile: &ServerProfile) -> Result<DynStream, NetError> {
    let stream = TcpStream::connect((profile.host.as_str(), profile.port)).await?;
    stream.set_nodelay(true)?;

    if profile.tls {
        let connector = tokio_native_tls::native_tls::TlsConnector::builder().build()?;
        let connector = TlsConnector::from(connector);
        let tls = connector.connect(profile.host.as_str(), stream).await?;
        Ok(Box::new(tls))
    } else {
        Ok(Box::new(stream))
    }
}

async fn write_command<W: AsyncWrite + Unpin>(
    writer: &mut W,
    command: Command,
) -> Result<(), NetError> {
    let line = String::from(&command);
    write_raw_line(writer, &line).await
}

async fn write_raw_line<W: AsyncWrite + Unpin>(writer: &mut W, line: &str) -> Result<(), NetError> {
    let mut payload = line.trim_end_matches(['\r', '\n']).to_owned();
    payload.push_str("\r\n");
    writer.write_all(payload.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

enum ConnectionExit {
    Requested,
    Unexpected(String),
}

fn reconnect_delay(attempt: u32) -> Duration {
    let exp = 2_u64.saturating_pow(attempt.min(10));
    let base = Duration::from_secs(exp).min(Duration::from_secs(120));
    let jitter = rand::rng().random_range(0..1_000_u64);
    base + Duration::from_millis(jitter)
}
