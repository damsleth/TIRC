use std::collections::BTreeMap;

use crate::{
    event::{AppEvent, CommandEvent, CommandSource, UiAction, UiEvent},
    model::AppState,
};

/// Command specification shared by slash and palette entry points.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
    pub args: &'static str,
    pub help: &'static str,
}

/// Command-side profile/network actions emitted by the reducer.
#[derive(Debug, Clone)]
pub enum OutboundCommand {
    Raw {
        network: String,
        line: String,
    },
    Join {
        network: String,
        channel: String,
    },
    Part {
        network: String,
        channel: String,
        reason: Option<String>,
    },
    Msg {
        network: String,
        target: String,
        body: String,
    },
    Nick {
        network: String,
        nick: String,
    },
    List {
        network: String,
    },
    Connect {
        profile: String,
    },
    Disconnect {
        network: String,
    },
}

/// Result of command execution.
#[derive(Debug, Clone, Default)]
pub struct CommandOutput {
    pub events: Vec<AppEvent>,
    pub outbound: Vec<OutboundCommand>,
}

/// Registry for commands and aliases.
#[derive(Debug, Clone)]
pub struct CommandRegistry {
    specs: Vec<CommandSpec>,
    aliases: BTreeMap<String, String>,
}

impl CommandRegistry {
    pub fn default_registry() -> Self {
        let specs = vec![
            CommandSpec {
                id: "help",
                aliases: &["h", "?"],
                args: "",
                help: "Toggle help screen",
            },
            CommandSpec {
                id: "join",
                aliases: &["j"],
                args: "<profile> <#channel>",
                help: "Join a channel",
            },
            CommandSpec {
                id: "part",
                aliases: &["leave"],
                args: "[profile] [target] [message...]",
                help: "Part a channel",
            },
            CommandSpec {
                id: "msg",
                aliases: &["m", "privmsg"],
                args: "[profile] <target> <message>",
                help: "Send a message",
            },
            CommandSpec {
                id: "query",
                aliases: &[],
                args: "[profile] <target>",
                help: "Open or focus a private-message buffer",
            },
            CommandSpec {
                id: "nick",
                aliases: &[],
                args: "<profile> <nick> | <nick>",
                help: "Change nick",
            },
            CommandSpec {
                id: "list",
                aliases: &["ls"],
                args: "[profile]",
                help: "List channels on a profile",
            },
            CommandSpec {
                id: "connect",
                aliases: &["conn"],
                args: "<profile>",
                help: "Connect using profile",
            },
            CommandSpec {
                id: "disconnect",
                aliases: &["dc"],
                args: "[profile]",
                help: "Disconnect from profile",
            },
            CommandSpec {
                id: "buffer",
                aliases: &["b"],
                args: "<profile> <name>",
                help: "Focus buffer",
            },
            CommandSpec {
                id: "theme",
                aliases: &["th"],
                args: "",
                help: "Open theme editor",
            },
            CommandSpec {
                id: "keymap",
                aliases: &["km"],
                args: "",
                help: "Open keymap editor",
            },
            CommandSpec {
                id: "quit",
                aliases: &["q"],
                args: "",
                help: "Quit TIRC",
            },
        ];

        let aliases = specs
            .iter()
            .flat_map(|spec| {
                spec.aliases
                    .iter()
                    .map(move |alias| (alias.to_string(), spec.id.to_string()))
            })
            .collect();

        Self { specs, aliases }
    }

    pub fn specs(&self) -> &[CommandSpec] {
        &self.specs
    }

    pub fn execute_line(
        &self,
        state: &mut AppState,
        source: CommandSource,
        line: &str,
    ) -> CommandOutput {
        let mut out = CommandOutput::default();
        let trimmed = line.trim();

        if trimmed.is_empty() {
            return out;
        }

        let stripped = trimmed.strip_prefix('/').unwrap_or(trimmed);

        // Extract command name with simple whitespace split first.
        let (cmd_str, rest) = stripped
            .split_once(char::is_whitespace)
            .map(|(c, r)| (c, r.trim_start()))
            .unwrap_or((stripped, ""));

        if cmd_str.is_empty() {
            return out;
        }

        let command = self.resolve_command(cmd_str);

        // Commands with freeform trailing text (chat messages, part reasons)
        // use simple whitespace splitting to preserve literal quotes and backslashes.
        // Commands that benefit from quoted arguments (profile names with spaces)
        // use the quote-aware parser.
        let uses_raw_args = matches!(
            command.as_str(),
            "msg" | "part" | "help" | "theme" | "keymap" | "quit"
        );
        let parts_owned = if uses_raw_args || rest.is_empty() {
            rest.split_whitespace().map(String::from).collect::<Vec<_>>()
        } else {
            match parse_command_args(rest) {
                Ok(parts) => parts,
                Err(err) => {
                    out.events.push(failed(
                        cmd_str,
                        &format!("failed to parse command arguments: {err}"),
                    ));
                    return out;
                }
            }
        };

        let args_refs: Vec<&str> = parts_owned.iter().map(String::as_str).collect();
        let args = args_refs.as_slice();

        match command.as_str() {
            "help" => {
                out.events
                    .push(AppEvent::Ui(UiEvent::Action(UiAction::HelpToggle)));
                out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                    command,
                    detail: "Help toggled".to_owned(),
                }));
            }
            "join" => {
                if let Some((profile, channel)) = resolve_join_args(state, args) {
                    out.outbound.push(OutboundCommand::Join {
                        network: profile.clone(),
                        channel: channel.clone(),
                    });
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Joining {profile}/{channel}"),
                    }));
                } else {
                    out.events.push(failed(
                        "join",
                        "usage: /join [profile] <#channel> (profile defaults to focused)",
                    ));
                }
            }
            "part" => {
                if let Some((profile, channel, reason)) = resolve_part_args(state, args) {
                    out.outbound.push(OutboundCommand::Part {
                        network: profile.clone(),
                        channel: channel.clone(),
                        reason,
                    });
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Parting {profile}/{channel}"),
                    }));
                } else {
                    out.events.push(failed(
                        "part",
                        "usage: /part [profile] [target] [message...] (defaults to focused profile/target)",
                    ));
                }
            }
            "msg" => {
                if let Some((profile, target, body)) = resolve_msg_args(state, args) {
                    out.outbound.push(OutboundCommand::Msg {
                        network: profile,
                        target,
                        body,
                    });
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: "Message queued".to_owned(),
                    }));
                } else {
                    out.events.push(failed(
                        "msg",
                        "usage: /msg [profile] <target> <message> (profile defaults to focused profile)",
                    ));
                }
            }
            "query" => {
                if let Some((profile, target)) = resolve_query_args(state, args) {
                    let id = state.ensure_buffer(&profile, &target);
                    state.focus = Some(id);
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Opened PM buffer {profile}/{target}"),
                    }));
                } else {
                    out.events.push(failed(
                        "query",
                        "usage: /query [profile] <target> (profile defaults to focused profile)",
                    ));
                }
            }
            "nick" => match args {
                [profile, nick] => {
                    out.outbound.push(OutboundCommand::Nick {
                        network: (*profile).to_owned(),
                        nick: (*nick).to_owned(),
                    });
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Nick change requested: {nick}"),
                    }));
                }
                [nick] => {
                    if let Some(profile) = infer_single_profile(state) {
                        out.outbound.push(OutboundCommand::Nick {
                            network: profile,
                            nick: (*nick).to_owned(),
                        });
                        out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                            command,
                            detail: format!("Nick change requested: {nick}"),
                        }));
                    } else {
                        out.events.push(failed("nick", &nick_usage(state)));
                    }
                }
                _ => {
                    out.events.push(failed("nick", &nick_usage(state)));
                }
            },
            "list" => match args {
                [] => {
                    if let Some(profile) = infer_single_profile(state) {
                        out.outbound
                            .push(OutboundCommand::List { network: profile });
                        out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                            command,
                            detail: "Requesting channel list".to_owned(),
                        }));
                    } else {
                        out.events.push(failed(
                            "list",
                            "usage: /list [profile] (omit profile only when one connected profile exists)",
                        ));
                    }
                }
                [profile] => {
                    out.outbound.push(OutboundCommand::List {
                        network: (*profile).to_owned(),
                    });
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Listing channels on profile {profile}"),
                    }));
                }
                _ => out.events.push(failed("list", "usage: /list [profile]")),
            },
            "connect" => {
                if let [profile] = args {
                    out.outbound.push(OutboundCommand::Connect {
                        profile: (*profile).to_owned(),
                    });
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Connecting profile: {profile}"),
                    }));
                } else {
                    out.events
                        .push(failed("connect", "usage: /connect <profile>"));
                }
            }
            "disconnect" => match args {
                [profile] => {
                    out.outbound.push(OutboundCommand::Disconnect {
                        network: (*profile).to_owned(),
                    });
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Disconnecting profile: {profile}"),
                    }));
                }
                [] => {
                    if let Some(profile) = infer_single_profile(state) {
                        out.outbound.push(OutboundCommand::Disconnect {
                            network: profile.clone(),
                        });
                        out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                            command,
                            detail: format!("Disconnecting profile: {profile}"),
                        }));
                    } else {
                        out.events.push(failed(
                            "disconnect",
                            "usage: /disconnect [profile] (omit profile only when one connected profile exists)",
                        ));
                    }
                }
                _ => out
                    .events
                    .push(failed("disconnect", "usage: /disconnect [profile]")),
            },
            "buffer" => {
                if let [profile, name] = args {
                    let id = state.ensure_buffer(profile, name);
                    state.focus = Some(id);
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Focused {profile}/{name}"),
                    }));
                } else {
                    out.events
                        .push(failed("buffer", "usage: /buffer <profile> <name>"));
                }
            }
            "theme" => {
                out.events
                    .push(AppEvent::Ui(UiEvent::Action(UiAction::ThemeEditorToggle)));
                out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                    command,
                    detail: "Theme editor opened".to_owned(),
                }));
            }
            "keymap" => {
                out.events
                    .push(AppEvent::Ui(UiEvent::Action(UiAction::KeymapEditorToggle)));
                out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                    command,
                    detail: "Keymap editor opened".to_owned(),
                }));
            }
            "quit" => {
                state.should_quit = true;
                out.events.push(AppEvent::Ui(UiEvent::QuitRequested));
                out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                    command,
                    detail: "Bye".to_owned(),
                }));
            }
            _ => {
                out.events.push(AppEvent::Command(CommandEvent::Run {
                    source,
                    line: line.to_owned(),
                }));
                out.events.push(failed(&command, "unknown command"));
            }
        }

        out
    }

    pub fn palette_items(&self) -> Vec<String> {
        self.specs
            .iter()
            .map(|spec| format!("{} {} - {}", spec.id, spec.args, spec.help))
            .collect()
    }

    fn resolve_command(&self, raw: &str) -> String {
        if self.specs.iter().any(|spec| spec.id == raw) {
            return raw.to_owned();
        }
        self.aliases
            .get(raw)
            .cloned()
            .unwrap_or_else(|| raw.to_owned())
    }
}

fn failed(command: &str, error: &str) -> AppEvent {
    AppEvent::Command(CommandEvent::Failed {
        command: command.to_owned(),
        error: error.to_owned(),
    })
}

fn infer_single_profile(state: &AppState) -> Option<String> {
    let connected = state
        .networks
        .iter()
        .filter_map(|(name, profile_state)| profile_state.connected.then_some(name.clone()))
        .collect::<Vec<_>>();
    if connected.len() == 1 {
        return connected.first().cloned();
    }

    if state.networks.len() == 1 {
        return state.networks.keys().next().cloned();
    }

    None
}

fn infer_profile_from_focus_or_single(state: &AppState) -> Option<String> {
    if let Some(focus) = state.focus.as_ref()
        && focus.network != "local"
    {
        return Some(focus.network.clone());
    }
    infer_single_profile(state)
}

fn infer_target_from_focus(state: &AppState) -> Option<String> {
    state.focus.as_ref().and_then(|focus| {
        if focus.network == "local" {
            return None;
        }
        if focus.name == "status" {
            return None;
        }
        Some(focus.name.clone())
    })
}

/// Resolve join command arguments: /join [profile] <#channel>
fn resolve_join_args(state: &AppState, args: &[&str]) -> Option<(String, String)> {
    match args {
        [channel] => {
            // Single arg: infer profile from focused buffer
            let profile = infer_profile_from_focus_or_single(state)?;
            Some((profile, (*channel).to_owned()))
        }
        [profile, channel] => {
            // Explicit profile and channel
            Some(((*profile).to_owned(), (*channel).to_owned()))
        }
        _ => None,
    }
}

fn resolve_part_args(state: &AppState, args: &[&str]) -> Option<(String, String, Option<String>)> {
    match args {
        [] => {
            let profile = infer_profile_from_focus_or_single(state)?;
            let target = infer_target_from_focus(state)?;
            Some((profile, target, None))
        }
        [only] => {
            if only.starts_with('#') {
                let profile = infer_profile_from_focus_or_single(state)?;
                Some((profile, (*only).to_owned(), None))
            } else if state.networks.contains_key(*only) {
                let target = infer_target_from_focus(state)?;
                Some(((*only).to_owned(), target, None))
            } else {
                let profile = infer_profile_from_focus_or_single(state)?;
                let target = infer_target_from_focus(state)?;
                Some((profile, target, Some((*only).to_owned())))
            }
        }
        [first, second] => {
            if state.networks.contains_key(*first) {
                if second.starts_with('#') || !second.contains(' ') {
                    Some(((*first).to_owned(), (*second).to_owned(), None))
                } else {
                    let target = infer_target_from_focus(state)?;
                    Some(((*first).to_owned(), target, Some((*second).to_owned())))
                }
            } else {
                let profile = infer_profile_from_focus_or_single(state)?;
                if first.starts_with('#') {
                    Some((profile, (*first).to_owned(), Some((*second).to_owned())))
                } else {
                    let target = infer_target_from_focus(state)?;
                    Some((profile, target, Some(format!("{first} {second}"))))
                }
            }
        }
        _ => {
            let first = args[0];
            let second = args[1];
            let rest = args[2..].join(" ");
            if state.networks.contains_key(first) {
                Some((first.to_owned(), second.to_owned(), Some(rest)))
            } else {
                let profile = infer_profile_from_focus_or_single(state)?;
                if first.starts_with('#') {
                    Some((profile, first.to_owned(), Some(args[1..].join(" "))))
                } else {
                    let target = infer_target_from_focus(state)?;
                    Some((profile, target, Some(args.join(" "))))
                }
            }
        }
    }
}

fn resolve_msg_args(state: &AppState, args: &[&str]) -> Option<(String, String, String)> {
    if args.len() < 2 {
        return None;
    }
    if state.networks.contains_key(args[0]) {
        if args.len() < 3 {
            return None;
        }
        return Some((args[0].to_owned(), args[1].to_owned(), args[2..].join(" ")));
    }
    let profile = infer_profile_from_focus_or_single(state)?;
    Some((profile, args[0].to_owned(), args[1..].join(" ")))
}

fn resolve_query_args(state: &AppState, args: &[&str]) -> Option<(String, String)> {
    match args {
        [target] => {
            let profile = infer_profile_from_focus_or_single(state)?;
            Some((profile, (*target).to_owned()))
        }
        [profile, target] if state.networks.contains_key(*profile) => {
            Some(((*profile).to_owned(), (*target).to_owned()))
        }
        _ => None,
    }
}

fn nick_usage(state: &AppState) -> String {
    let available = state
        .networks
        .keys()
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if available.is_empty() {
        "usage: /nick <profile> <nick> | /nick <nick> (when exactly one connected profile exists). <profile> is your profile name (e.g. default).".to_owned()
    } else {
        format!(
            "usage: /nick <profile> <nick> | /nick <nick> (single-profile default). <profile> is your profile name. available profiles: {available}"
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CommandArgParseError {
    UnclosedQuote { quote: char },
    TrailingEscape,
}

impl std::fmt::Display for CommandArgParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnclosedQuote { quote } => {
                write!(f, "unclosed quote: {quote}")
            }
            Self::TrailingEscape => write!(f, "trailing escape character"),
        }
    }
}

impl std::error::Error for CommandArgParseError {}

/// Parse command arguments supporting quoted strings and `\` escapes.
///
/// Handles both single and double quotes. Unquoted arguments are split by whitespace.
/// Backslash escapes the next character both inside and outside quotes.
/// Examples:
/// - `profile "My Server"` -> ["profile", "My Server"]
/// - `"My Profile" #channel` -> ["My Profile", "#channel"]
/// - `simple args` -> ["simple", "args"]
fn parse_command_args(input: &str) -> Result<Vec<String>, CommandArgParseError> {
    let mut args = Vec::new();
    let mut current = String::new();
    let mut in_quotes: Option<char> = None;
    let mut escaped = false;
    let mut token_started = false;

    for c in input.chars() {
        if escaped {
            current.push(c);
            token_started = true;
            escaped = false;
            continue;
        }

        if c == '\\' {
            escaped = true;
            token_started = true;
            continue;
        }

        match in_quotes {
            Some(quote) if c == quote => {
                in_quotes = None;
            }
            Some(_) => {
                current.push(c);
                token_started = true;
            }
            None if c == '"' || c == '\'' => {
                in_quotes = Some(c);
                token_started = true;
            }
            None if c.is_whitespace() => {
                if token_started {
                    args.push(std::mem::take(&mut current));
                    token_started = false;
                }
            }
            None => {
                current.push(c);
                token_started = true;
            }
        }
    }

    if escaped {
        return Err(CommandArgParseError::TrailingEscape);
    }
    if let Some(quote) = in_quotes {
        return Err(CommandArgParseError::UnclosedQuote { quote });
    }

    if token_started {
        args.push(current);
    }

    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppState, BufferId};

    #[test]
    fn alias_resolution_runs_join() {
        let registry = CommandRegistry::default_registry();
        let mut state = AppState::new(100, 1000);
        let output = registry.execute_line(&mut state, CommandSource::Slash, "/j net #rust");

        assert_eq!(output.outbound.len(), 1);
        match &output.outbound[0] {
            OutboundCommand::Join { network, channel } => {
                assert_eq!(network, "net");
                assert_eq!(channel, "#rust");
            }
            other => {
                panic!("unexpected outbound: {other:?}");
            }
        }
    }

    #[test]
    fn join_infers_profile_from_focus() {
        let registry = CommandRegistry::default_registry();
        let mut state = AppState::new(100, 1000);
        // Set up a focused buffer in "myprofile"
        state.ensure_buffer("myprofile", "#existing");
        state.focus = Some(BufferId::new("myprofile", "#existing"));
        state.networks.insert(
            "myprofile".to_owned(),
            crate::model::NetworkState::new("myprofile", "irc.example.com:6667"),
        );

        // Join with only channel name - should infer profile from focus
        let output = registry.execute_line(&mut state, CommandSource::Slash, "/join #newchan");

        assert_eq!(output.outbound.len(), 1);
        match &output.outbound[0] {
            OutboundCommand::Join { network, channel } => {
                assert_eq!(network, "myprofile");
                assert_eq!(channel, "#newchan");
            }
            other => panic!("unexpected outbound: {other:?}"),
        }
    }

    #[test]
    fn quit_command_sets_quit_flag() {
        let registry = CommandRegistry::default_registry();
        let mut state = AppState::new(100, 1000);
        let output = registry.execute_line(&mut state, CommandSource::Slash, "/quit");

        assert!(state.should_quit);
        assert!(
            output
                .events
                .iter()
                .any(|event| { matches!(event, AppEvent::Ui(UiEvent::QuitRequested)) })
        );
    }

    #[test]
    fn theme_command_opens_editor() {
        let registry = CommandRegistry::default_registry();
        let mut state = AppState::new(100, 1000);
        let output = registry.execute_line(&mut state, CommandSource::Slash, "/theme");

        assert!(output.events.iter().any(|event| {
            matches!(
                event,
                AppEvent::Ui(UiEvent::Action(UiAction::ThemeEditorToggle))
            )
        }));
    }

    #[test]
    fn nick_defaults_to_single_profile() {
        let registry = CommandRegistry::default_registry();
        let mut state = AppState::new(100, 1000);
        state.networks.insert(
            "default".to_owned(),
            crate::model::NetworkState {
                name: "default".to_owned(),
                server: "irc.homelien.no:6666".to_owned(),
                connected: true,
                channels: std::collections::BTreeSet::new(),
                reconnect_attempts: 0,
                last_error: None,
            },
        );

        let output = registry.execute_line(&mut state, CommandSource::Slash, "/nick mynick");
        assert!(matches!(
            output.outbound.first(),
            Some(OutboundCommand::Nick { network, nick })
                if network == "default" && nick == "mynick"
        ));
    }

    #[test]
    fn disconnect_defaults_to_single_profile() {
        let registry = CommandRegistry::default_registry();
        let mut state = AppState::new(100, 1000);
        state.networks.insert(
            "default".to_owned(),
            crate::model::NetworkState {
                name: "default".to_owned(),
                server: "irc.homelien.no:6666".to_owned(),
                connected: true,
                channels: std::collections::BTreeSet::new(),
                reconnect_attempts: 0,
                last_error: None,
            },
        );

        let output = registry.execute_line(&mut state, CommandSource::Slash, "/disconnect");
        assert!(matches!(
            output.outbound.first(),
            Some(OutboundCommand::Disconnect { network }) if network == "default"
        ));
    }

    #[test]
    fn parse_command_args_simple() {
        let args = parse_command_args("connect myprofile").unwrap();
        assert_eq!(args, vec!["connect", "myprofile"]);
    }

    #[test]
    fn parse_command_args_quoted() {
        let args = parse_command_args(r#"connect "My Profile""#).unwrap();
        assert_eq!(args, vec!["connect", "My Profile"]);
    }

    #[test]
    fn parse_command_args_single_quoted() {
        let args = parse_command_args("connect 'Profile With Spaces'").unwrap();
        assert_eq!(args, vec!["connect", "Profile With Spaces"]);
    }

    #[test]
    fn parse_command_args_multiple_quoted() {
        let args = parse_command_args("\"My Profile\" #test message").unwrap();
        assert_eq!(args, vec!["My Profile", "#test", "message"]);
    }

    #[test]
    fn parse_command_args_escaped_quote() {
        let args = parse_command_args(r#"msg #rust "Hello \"World\"""#).unwrap();
        assert_eq!(args, vec!["msg", "#rust", "Hello \"World\""]);
    }

    #[test]
    fn parse_command_args_escaped_single_quote() {
        let args = parse_command_args(r"msg #rust 'test\'s'").unwrap();
        assert_eq!(args, vec!["msg", "#rust", "test's"]);
    }

    #[test]
    fn parse_command_args_unclosed_quote() {
        let err = parse_command_args(r#"msg #rust "unclosed"#).unwrap_err();
        assert_eq!(err, CommandArgParseError::UnclosedQuote { quote: '"' });
    }

    #[test]
    fn parse_command_args_trailing_escape() {
        let err = parse_command_args(r#"msg #rust test\"#).unwrap_err();
        assert_eq!(err, CommandArgParseError::TrailingEscape);
    }

    #[test]
    fn execute_line_reports_parse_error() {
        let registry = CommandRegistry::default_registry();
        let mut state = AppState::new(100, 1000);
        // Use /connect which goes through the quote-aware parser
        let output =
            registry.execute_line(&mut state, CommandSource::Slash, r#"/connect "oops"#);

        assert!(matches!(
            output.events.first(),
            Some(AppEvent::Command(CommandEvent::Failed { command, .. }))
                if command == "connect"
        ));
    }

    #[test]
    fn msg_preserves_literal_quotes_and_backslashes() {
        let registry = CommandRegistry::default_registry();
        let mut state = AppState::new(100, 1000);
        state.networks.insert(
            "net".to_owned(),
            crate::model::NetworkState::new("net", "irc.example.com:6667"),
        );
        state.focus = Some(BufferId::new("net", "#rust"));

        // Quotes should be preserved literally in message body
        let output = registry.execute_line(
            &mut state,
            CommandSource::Slash,
            r#"/msg #rust "hello world""#,
        );
        assert!(matches!(
            &output.outbound[0],
            OutboundCommand::Msg { body, .. } if body == r#""hello world""#
        ));

        // Backslashes should be preserved literally
        let output = registry.execute_line(
            &mut state,
            CommandSource::Slash,
            r"/msg #rust C:\Users\path",
        );
        assert!(matches!(
            &output.outbound[0],
            OutboundCommand::Msg { body, .. } if body == r"C:\Users\path"
        ));
    }

    #[test]
    fn connect_with_quoted_profile() {
        let registry = CommandRegistry::default_registry();
        let mut state = AppState::new(100, 1000);

        let output = registry.execute_line(
            &mut state,
            CommandSource::Slash,
            r#"/connect "My Profile Name""#,
        );

        assert_eq!(output.outbound.len(), 1);
        match &output.outbound[0] {
            OutboundCommand::Connect { profile } => {
                assert_eq!(profile, "My Profile Name");
            }
            other => panic!("unexpected outbound: {other:?}"),
        }
    }
}
