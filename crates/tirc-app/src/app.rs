use std::{collections::BTreeMap, env, io, path::Path, process::Command, time::Duration};

use anyhow::{Context, Result};
use chrono::Utc;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc;
use tracing::{error, warn};

use tirc_config::{
    AppPaths, LoadedConfig, ServerProfile, available_keymaps, available_themes, load_config,
    load_keymap, load_theme, save_config,
};
use tirc_core::{
    AppEvent, CommandEvent, CommandRegistry, CommandSource, LogRecord, LogWriter, NetworkEvent,
    TimerEvent, UiEvent, build_session_snapshot, load_session, save_session,
};
use tirc_net::{NetCommand, NetManager};
use tirc_platform::PlatformNotifier;
use tirc_ui::{UiEngine, UiIntent};

use crate::cli::Cli;

pub async fn run(cli: Cli) -> Result<()> {
    let paths = AppPaths::discover()?;
    paths.ensure_dirs()?;

    let LoadedConfig {
        source_path: _,
        config,
        warning_plaintext_secret,
    } = load_config(&paths, cli.config.as_deref())?;

    let mut runtime_config = config;
    let mut state = tirc_core::AppState::new(runtime_config.ui.max_scrollback_lines);
    if warning_plaintext_secret {
        state
            .warnings
            .push("config contains plaintext secrets; file must remain permission 0600".to_owned());
        state.set_status_for(
            "Warning: plaintext credentials detected".to_owned(),
            Duration::from_secs(4),
        );
    }

    if runtime_config.profiles.is_empty() {
        state.warnings.push(
            "No IRC profiles configured. Edit ~/.config/tirc/config.toml to add at least one."
                .to_owned(),
        );
        state.set_status_for(
            "No profiles configured. Add one in ~/.config/tirc/config.toml".to_owned(),
            Duration::from_secs(6),
        );
    } else if runtime_config.profiles.len() == 1 && runtime_config.profiles[0].name == "default" {
        state.set_status(
            "Using starter profile 'default'. Run /connect default or edit config.toml".to_owned(),
        );
    }

    let theme = load_theme(&paths.themes_dir, &runtime_config.ui.theme)?;
    let keymap = load_keymap(&paths.keymaps_dir, &runtime_config.keymap.active_map)?;
    let theme_options = available_themes(&paths.themes_dir)?;
    let keymap_options = available_keymaps(&paths.keymaps_dir)?;

    if !cli.no_restore {
        restore_session(&paths.session_file, &mut state).await?;
    }

    let command_registry = CommandRegistry::default_registry();
    let mut ui = UiEngine::new(
        theme,
        keymap,
        build_palette_items(&state, &command_registry, &runtime_config.profiles),
        runtime_config.performance.max_idle_redraw_hz,
        theme_options,
        runtime_config.ui.theme.clone(),
        keymap_options,
        runtime_config.keymap.active_map.clone(),
    );

    let notifier = PlatformNotifier {
        enable_bell: runtime_config.notifications.bell_on_mention
            || runtime_config.notifications.bell_on_private_message,
        enable_macos_notifications: runtime_config.notifications.macos_notifications,
    };

    let logger = LogWriter::new(
        paths.logs_dir.clone(),
        runtime_config.logging.retention_days,
    );

    let (net_event_tx, mut net_event_rx) = mpsc::channel::<NetworkEvent>(1_024);
    let mut net_manager = NetManager::new(net_event_tx);

    if let Some(profile_name) = cli.profile.as_ref() {
        if let Some(profile) = runtime_config
            .profiles
            .iter()
            .find(|profile| profile.name == *profile_name)
        {
            net_manager.connect_profile(profile.clone()).await?;
            state.set_status_for(
                format!("Connecting profile: {profile_name}"),
                Duration::from_millis(2_000),
            );
        } else {
            state.set_status_for(
                format!("Profile not found: {profile_name}"),
                Duration::from_millis(2_000),
            );
        }
    }

    if state.buffers.is_empty() {
        state.ensure_buffer("local", "status");
    }

    let focus_missing = match state.focus.as_ref() {
        Some(buffer_id) => !state.buffers.contains_key(buffer_id),
        None => true,
    };
    if focus_missing {
        state.focus = Some(tirc_core::BufferId::new("local", "status"));
    }

    let mut terminal = setup_terminal()?;
    let mut needs_draw = true;
    let mut last_session_save = std::time::Instant::now();
    let mut last_log_cleanup = std::time::Instant::now();

    loop {
        while let Ok(network_event) = net_event_rx.try_recv() {
            process_network_event(
                &network_event,
                &mut state,
                &notifier,
                &logger,
                runtime_config.logging.enabled,
                &runtime_config,
            )
            .await;
            needs_draw = true;
        }

        if event::poll(Duration::from_millis(20))? {
            let evt = event::read()?;
            let intents = ui.handle_terminal_event(evt);
            if !intents.is_empty() {
                let open_config_editor = handle_ui_intents(
                    intents,
                    &mut state,
                    &mut ui,
                    &mut net_manager,
                    &command_registry,
                    &paths,
                    &mut runtime_config,
                    &logger,
                )
                .await;
                if open_config_editor {
                    if let Err(err) = open_config_in_editor(&mut terminal, &paths.config_file) {
                        state.set_status_for(
                            format!("failed to open config in editor: {err}"),
                            Duration::from_millis(2_000),
                        );
                    } else {
                        if let Err(err) =
                            reload_runtime_config(&paths, &mut runtime_config, &mut ui)
                        {
                            state.set_status_for(
                                format!("failed to reload config: {err}"),
                                Duration::from_millis(2_000),
                            );
                        } else {
                            state.set_status_for(
                                "Returned from config editor; configuration reloaded",
                                Duration::from_millis(2_000),
                            );
                        }
                    }
                }
                needs_draw = true;
            }
        }

        if ui.should_draw_heartbeat() {
            state.apply(AppEvent::Timer(TimerEvent::Heartbeat));
            needs_draw = true;
        }

        if last_session_save.elapsed() >= Duration::from_secs(5) {
            let session = build_session_snapshot(&state);
            if let Err(err) = save_session(&paths.session_file, &session).await {
                warn!("failed to save session: {err}");
            }
            last_session_save = std::time::Instant::now();
        }

        if last_log_cleanup.elapsed() >= Duration::from_secs(60 * 60) {
            if let Err(err) = logger.cleanup_retention(Utc::now()).await {
                warn!("failed to clean old logs: {err}");
            }
            last_log_cleanup = std::time::Instant::now();
        }

        if needs_draw {
            ui.set_palette_items(build_palette_items(
                &state,
                &command_registry,
                &runtime_config.profiles,
            ));
            let vm = state.view_model();
            ui.sync_input(&vm.input);
            terminal.draw(|frame| ui.draw(frame, &vm))?;
            needs_draw = false;
        }

        if state.should_quit {
            break;
        }
    }

    let session = build_session_snapshot(&state);
    if let Err(err) = save_session(&paths.session_file, &session).await {
        warn!("failed to save final session: {err}");
    }

    net_manager.disconnect_all().await;
    restore_terminal(&mut terminal)?;
    Ok(())
}

async fn process_network_event(
    network_event: &NetworkEvent,
    state: &mut tirc_core::AppState,
    notifier: &PlatformNotifier,
    logger: &LogWriter,
    logging_enabled: bool,
    config: &tirc_config::AppConfig,
) {
    state.apply(AppEvent::Network(network_event.clone()));

    if let NetworkEvent::Message {
        network,
        buffer,
        sender,
        body,
        tags,
        is_private,
        is_mention,
        ts,
    } = network_event
    {
        if logging_enabled {
            let record = LogRecord {
                ts: *ts,
                network: network.clone(),
                buffer: buffer.clone(),
                sender: sender.clone(),
                tags: tags.clone(),
                kind: if *is_private {
                    "private".to_owned()
                } else {
                    "channel".to_owned()
                },
                body: body.clone(),
            };

            if let Err(err) = logger.append(&record).await {
                warn!("failed to write log record: {err}");
            }
        }

        if (*is_mention && config.notifications.bell_on_mention)
            || (*is_private && config.notifications.bell_on_private_message)
        {
            notifier.ring_bell();
            let _ = notifier.notify(
                &format!("TIRC {network}/{buffer}"),
                &format!(
                    "{}: {body}",
                    sender.clone().unwrap_or_else(|| "*".to_owned())
                ),
            );
        }
    }
}

async fn handle_ui_intents(
    intents: Vec<UiIntent>,
    state: &mut tirc_core::AppState,
    ui: &mut UiEngine,
    net_manager: &mut NetManager,
    command_registry: &CommandRegistry,
    paths: &AppPaths,
    runtime_config: &mut tirc_config::AppConfig,
    logger: &LogWriter,
) -> bool {
    let mut open_config_editor = false;

    for intent in intents {
        match intent {
            UiIntent::Event(UiEvent::PaletteExecute(selected)) => {
                if selected == "edit config" {
                    open_config_editor = true;
                    state.set_status("Opening config in $EDITOR...");
                    continue;
                }

                if let Some(line) = palette_to_command_line(&selected) {
                    execute_command_line(
                        &line,
                        CommandSource::Palette,
                        state,
                        net_manager,
                        command_registry,
                        &runtime_config.profiles,
                    )
                    .await;
                }
            }
            UiIntent::Event(event) => state.apply(AppEvent::Ui(event)),
            UiIntent::SubmitLine(line) => {
                if line.trim().is_empty() {
                    continue;
                }

                if line.trim_start().starts_with('/') {
                    execute_command_line(
                        &line,
                        CommandSource::Slash,
                        state,
                        net_manager,
                        command_registry,
                        &runtime_config.profiles,
                    )
                    .await;
                } else {
                    send_chat_line(
                        line,
                        state,
                        net_manager,
                        logger,
                        runtime_config.logging.enabled,
                    )
                    .await;
                }
            }
            UiIntent::ApplyTheme(name) => match load_theme(&paths.themes_dir, &name) {
                Ok(theme) => {
                    ui.set_theme(theme, name.clone());
                    runtime_config.ui.theme = name.clone();
                    if let Err(err) = save_config(&paths.config_file, runtime_config) {
                        state.set_status_for(
                            format!("failed to persist theme: {err}"),
                            Duration::from_millis(2_000),
                        );
                    } else {
                        state.set_status(format!("theme set: {name}"));
                    }

                    if let Ok(options) = available_themes(&paths.themes_dir) {
                        ui.set_theme_options(options, runtime_config.ui.theme.clone());
                    }
                }
                Err(err) => {
                    state.set_status_for(
                        format!("failed to load theme {name}: {err}"),
                        Duration::from_millis(2_000),
                    );
                }
            },
            UiIntent::ApplyKeymap(name) => match load_keymap(&paths.keymaps_dir, &name) {
                Ok(keymap) => {
                    ui.set_keymap(&keymap, name.clone());
                    runtime_config.keymap.active_map = name.clone();
                    if let Err(err) = save_config(&paths.config_file, runtime_config) {
                        state.set_status_for(
                            format!("failed to persist keymap: {err}"),
                            Duration::from_millis(2_000),
                        );
                    } else {
                        state.set_status(format!("keymap set: {name}"));
                    }

                    if let Ok(options) = available_keymaps(&paths.keymaps_dir) {
                        ui.set_keymap_options(options, runtime_config.keymap.active_map.clone());
                    }
                }
                Err(err) => {
                    state.set_status_for(
                        format!("failed to load keymap {name}: {err}"),
                        Duration::from_millis(2_000),
                    );
                }
            },
        }
    }

    open_config_editor
}

async fn send_chat_line(
    line: String,
    state: &mut tirc_core::AppState,
    net_manager: &mut NetManager,
    logger: &LogWriter,
    logging_enabled: bool,
) {
    let Some(focus) = state.focus.clone() else {
        state.set_status_for("No active buffer".to_owned(), Duration::from_millis(2_000));
        return;
    };

    if focus.network == "local" {
        state.set_status_for(
            "Cannot send without an active network buffer".to_owned(),
            Duration::from_millis(2_000),
        );
        return;
    }

    let command = NetCommand::Privmsg {
        target: focus.name.clone(),
        body: line.clone(),
    };

    if let Err(err) = net_manager.send_command(&focus.network, command).await {
        state.set_status_for(format!("send failed: {err}"), Duration::from_millis(2_000));
        return;
    }

    let message_event = NetworkEvent::Message {
        network: focus.network.clone(),
        buffer: focus.name.clone(),
        sender: None,
        body: line.clone(),
        tags: BTreeMap::new(),
        is_private: !focus.name.starts_with('#'),
        is_mention: false,
        ts: Utc::now(),
    };

    state.apply(AppEvent::Network(message_event.clone()));

    if logging_enabled
        && let NetworkEvent::Message {
            network,
            buffer,
            sender,
            body,
            tags,
            ts,
            ..
        } = message_event
    {
        let record = LogRecord {
            ts,
            network,
            buffer,
            sender,
            tags,
            kind: "self".to_owned(),
            body,
        };

        if let Err(err) = logger.append(&record).await {
            warn!("failed to log outbound message: {err}");
        }
    }
}

async fn execute_command_line(
    line: &str,
    source: CommandSource,
    state: &mut tirc_core::AppState,
    net_manager: &mut NetManager,
    command_registry: &CommandRegistry,
    profiles: &[ServerProfile],
) {
    let output = command_registry.execute_line(state, source, line);

    for event in output.events {
        if let AppEvent::Command(CommandEvent::Failed { command, error }) = &event {
            state.set_status_for(format!("{command}: {error}"), Duration::from_millis(2_000));
        }
        state.apply(event);
    }

    for outbound in output.outbound {
        match outbound {
            tirc_core::OutboundCommand::Raw { network, line } => {
                if let Err(err) = net_manager
                    .send_command(&network, NetCommand::Raw(line))
                    .await
                {
                    state.set_status_for(
                        format!("send failed: {err}"),
                        Duration::from_millis(2_000),
                    );
                }
            }
            tirc_core::OutboundCommand::Join { network, channel } => {
                if let Err(err) = net_manager
                    .send_command(&network, NetCommand::Join(channel))
                    .await
                {
                    state.set_status_for(
                        format!("join failed: {err}"),
                        Duration::from_millis(2_000),
                    );
                }
            }
            tirc_core::OutboundCommand::Part { network, channel } => {
                if let Err(err) = net_manager
                    .send_command(&network, NetCommand::Part(channel))
                    .await
                {
                    state.set_status_for(
                        format!("part failed: {err}"),
                        Duration::from_millis(2_000),
                    );
                }
            }
            tirc_core::OutboundCommand::Msg {
                network,
                target,
                body,
            } => {
                if let Err(err) = net_manager
                    .send_command(&network, NetCommand::Privmsg { target, body })
                    .await
                {
                    state
                        .set_status_for(format!("msg failed: {err}"), Duration::from_millis(2_000));
                }
            }
            tirc_core::OutboundCommand::Nick { network, nick } => {
                if let Err(err) = net_manager
                    .send_command(&network, NetCommand::Nick(nick))
                    .await
                {
                    state.set_status_for(
                        format!("nick failed: {err}"),
                        Duration::from_millis(2_000),
                    );
                }
            }
            tirc_core::OutboundCommand::Connect { profile } => {
                if let Some(profile_cfg) =
                    profiles.iter().find(|candidate| candidate.name == profile)
                {
                    if let Err(err) = net_manager.connect_profile(profile_cfg.clone()).await {
                        state.set_status_for(
                            format!("connect failed: {err}"),
                            Duration::from_millis(2_000),
                        );
                    } else {
                        state
                            .networks
                            .entry(profile_cfg.name.clone())
                            .or_insert_with(|| {
                                tirc_core::NetworkState::new(
                                    profile_cfg.name.clone(),
                                    format!("{}:{}", profile_cfg.host, profile_cfg.port),
                                )
                            });
                        state.push_message(
                            "local",
                            "status",
                            tirc_core::Message {
                                ts: Utc::now(),
                                sender: None,
                                body: format!(
                                    "Connecting to {}:{} as {}",
                                    profile_cfg.host, profile_cfg.port, profile_cfg.nick
                                ),
                                mention: false,
                                tags: BTreeMap::new(),
                            },
                        );
                        state.set_status_for(
                            format!(
                                "Connecting to {}:{} ({})",
                                profile_cfg.host, profile_cfg.port, profile_cfg.name
                            ),
                            Duration::from_millis(2_000),
                        );
                    }
                } else {
                    state.set_status_for(
                        format!("profile not found: {profile}"),
                        Duration::from_millis(2_000),
                    );
                }
            }
            tirc_core::OutboundCommand::Disconnect { network } => {
                if let Err(err) = net_manager.disconnect(&network).await {
                    state.set_status_for(
                        format!("disconnect failed: {err}"),
                        Duration::from_millis(2_000),
                    );
                }
            }
        }
    }
}

fn build_palette_items(
    state: &tirc_core::AppState,
    command_registry: &CommandRegistry,
    profiles: &[ServerProfile],
) -> Vec<String> {
    let mut items = command_registry
        .specs()
        .iter()
        .map(|spec| spec.id.to_owned())
        .collect::<Vec<_>>();

    items.extend(
        state
            .buffers
            .keys()
            .map(|buffer| format!("buffer {} {}", buffer.network, buffer.name)),
    );

    items.extend(
        profiles
            .iter()
            .map(|profile| format!("connect {}", profile.name)),
    );
    items.push("edit config".to_owned());

    items.sort();
    items.dedup();
    items
}

fn palette_to_command_line(selected: &str) -> Option<String> {
    if let Some(rest) = selected.strip_prefix("buffer ") {
        return Some(format!("/buffer {rest}"));
    }

    if let Some(rest) = selected.strip_prefix("connect ") {
        return Some(format!("/connect {rest}"));
    }

    Some(format!("/{selected}"))
}

fn open_config_in_editor(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    config_path: &Path,
) -> Result<()> {
    restore_terminal(terminal)?;
    let edit_result = run_editor(config_path);
    setup_terminal_in_place(terminal)?;
    edit_result
}

fn reload_runtime_config(
    paths: &AppPaths,
    runtime_config: &mut tirc_config::AppConfig,
    ui: &mut UiEngine,
) -> Result<()> {
    let loaded = load_config(paths, Some(&paths.config_file))?;
    *runtime_config = loaded.config;

    let theme = load_theme(&paths.themes_dir, &runtime_config.ui.theme)?;
    ui.set_theme(theme, runtime_config.ui.theme.clone());

    let keymap = load_keymap(&paths.keymaps_dir, &runtime_config.keymap.active_map)?;
    ui.set_keymap(&keymap, runtime_config.keymap.active_map.clone());

    let theme_options = available_themes(&paths.themes_dir)?;
    ui.set_theme_options(theme_options, runtime_config.ui.theme.clone());

    let keymap_options = available_keymaps(&paths.keymaps_dir)?;
    ui.set_keymap_options(keymap_options, runtime_config.keymap.active_map.clone());

    Ok(())
}

fn run_editor(config_path: &Path) -> Result<()> {
    let editor = env::var("EDITOR").unwrap_or_else(|_| "vi".to_owned());
    let mut parts = editor.split_whitespace();
    let program = parts.next().unwrap_or("vi");
    let mut command = Command::new(program);
    command.args(parts);
    command.arg(config_path);

    let status = command.status().with_context(|| {
        format!(
            "spawn editor '{program}' for {}",
            config_path.to_string_lossy()
        )
    })?;
    if !status.success() {
        anyhow::bail!("editor exited with status {status}");
    }

    Ok(())
}

async fn restore_session(path: &Path, state: &mut tirc_core::AppState) -> Result<()> {
    let session = load_session(path).await?;

    for buffer in session.open_buffers {
        state.ensure_buffer(&buffer.network, &buffer.name);
    }

    state.focus = session.focused_buffer;

    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn setup_terminal_in_place(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    enable_raw_mode().context("enable raw mode")?;
    execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    )?;
    terminal.clear()?;
    Ok(())
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode().context("disable raw mode")?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

pub fn setup_tracing(debug: bool) {
    let level = if debug { "debug" } else { "info" };

    if let Err(err) = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level)),
        )
        .try_init()
    {
        error!("failed to initialize tracing subscriber: {err}");
    }
}
