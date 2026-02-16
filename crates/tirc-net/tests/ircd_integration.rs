use std::{env, time::Duration};

use anyhow::{Context, Result, bail};
use irc_proto::{Command, Message, Response};
use tokio::{
    io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, split},
    net::TcpStream,
    time,
};

#[tokio::test]
async fn connect_join_part_nick_against_real_ircd() -> Result<()> {
    if env::var("TIRC_RUN_IRCD_TESTS").as_deref() != Ok("1") {
        eprintln!("skipping IRC daemon integration test (set TIRC_RUN_IRCD_TESTS=1)");
        return Ok(());
    }

    let host = env::var("TIRC_IRCD_HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = env::var("TIRC_IRCD_PORT")
        .ok()
        .and_then(|raw| raw.parse::<u16>().ok())
        .unwrap_or(6667);

    let stream = TcpStream::connect((host.as_str(), port))
        .await
        .with_context(|| format!("connect to {host}:{port}"))?;

    let (reader_half, mut writer) = split(stream);
    let mut reader = BufReader::new(reader_half);
    let mut line = String::new();

    let suffix = rand::random::<u16>() % 10_000;
    let mut current_nick = format!("tirc_ci_{suffix}");
    let next_nick = format!("{}_x", current_nick);
    let channel = format!("#tirc_ci_{suffix}");

    write_line(&mut writer, "CAP LS 302").await?;
    write_line(&mut writer, "CAP END").await?;
    write_line(&mut writer, &format!("NICK {current_nick}")).await?;
    write_line(&mut writer, "USER tirc 0 * :TIRC CI").await?;

    let mut got_welcome = false;
    let mut joined = false;
    let mut nick_changed = false;
    let mut parted = false;
    let mut sent_join = false;
    let mut sent_nick = false;
    let mut sent_part = false;

    let deadline = time::Instant::now() + Duration::from_secs(30);

    while time::Instant::now() < deadline {
        line.clear();
        let read = time::timeout(Duration::from_secs(2), reader.read_line(&mut line)).await;
        let count = match read {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(err)) => return Err(err).context("read IRC line"),
            Err(_) => continue,
        };

        if count == 0 {
            bail!("IRCd closed connection before completing test scenario");
        }

        let Ok(message) = line.parse::<Message>() else {
            continue;
        };

        match message.command {
            Command::PING(server, token) => {
                write_line(&mut writer, &String::from(&Command::PONG(server, token))).await?;
            }
            Command::Response(Response::RPL_WELCOME, _) => {
                got_welcome = true;
                if !sent_join {
                    write_line(&mut writer, &format!("JOIN {channel}")).await?;
                    sent_join = true;
                }
            }
            Command::JOIN(joined_channel, _, _)
                if message.source_nickname() == Some(current_nick.as_str())
                    && joined_channel == channel =>
            {
                joined = true;
                write_line(
                    &mut writer,
                    &format!("PRIVMSG {channel} :hello from tirc CI"),
                )
                .await?;
                if !sent_nick {
                    write_line(&mut writer, &format!("NICK {next_nick}")).await?;
                    sent_nick = true;
                }
            }
            Command::NICK(nick) if message.source_nickname() == Some(current_nick.as_str()) => {
                current_nick = nick;
                nick_changed = true;
                if !sent_part {
                    write_line(&mut writer, &format!("PART {channel} :done")).await?;
                    sent_part = true;
                }
            }
            Command::PART(left_channel, _)
                if message.source_nickname() == Some(current_nick.as_str())
                    && left_channel == channel =>
            {
                parted = true;
                write_line(&mut writer, "QUIT :bye").await?;
                break;
            }
            Command::Response(code, args)
                if matches!(
                    code,
                    Response::ERR_NICKNAMEINUSE
                        | Response::ERR_NEEDMOREPARAMS
                        | Response::ERR_NOTREGISTERED
                        | Response::ERR_NOSUCHCHANNEL
                        | Response::ERR_CANNOTSENDTOCHAN
                ) =>
            {
                bail!("unexpected IRC error {code:?}: {}", args.join(" "));
            }
            _ => {}
        }
    }

    assert!(got_welcome, "did not receive RPL_WELCOME");
    assert!(joined, "did not observe self JOIN");
    assert!(nick_changed, "did not observe self NICK change");
    assert!(parted, "did not observe self PART");

    Ok(())
}

async fn write_line<W>(writer: &mut W, line: &str) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let mut payload = line.trim_end_matches(['\r', '\n']).to_owned();
    payload.push_str("\r\n");
    writer.write_all(payload.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}
