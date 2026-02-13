use std::collections::BTreeMap;

use crate::{
    event::{AppEvent, CommandEvent, CommandSource, UiEvent},
    model::{AppState, BufferId},
};

/// Command specification shared by slash and palette entry points.
#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub id: &'static str,
    pub aliases: &'static [&'static str],
    pub args: &'static str,
    pub help: &'static str,
}

/// Command-side network actions emitted by the reducer.
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
                args: "<network> <#channel>",
                help: "Join a channel",
            },
            CommandSpec {
                id: "part",
                aliases: &["leave"],
                args: "<network> <#channel>",
                help: "Part a channel",
            },
            CommandSpec {
                id: "msg",
                aliases: &["m", "privmsg"],
                args: "<network> <target> <message>",
                help: "Send a message",
            },
            CommandSpec {
                id: "nick",
                aliases: &[],
                args: "<network> <nick>",
                help: "Change nick",
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
                args: "<network>",
                help: "Disconnect from network",
            },
            CommandSpec {
                id: "buffer",
                aliases: &["b"],
                args: "<network> <name>",
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
        let parts = stripped.split_whitespace().collect::<Vec<_>>();
        if parts.is_empty() {
            return out;
        }

        let command = self.resolve_command(parts[0]);
        let args = &parts[1..];

        match command.as_str() {
            "help" => {
                out.events
                    .push(AppEvent::Ui(UiEvent::Action("help.toggle".to_owned())));
                out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                    command,
                    detail: "Help toggled".to_owned(),
                }));
            }
            "join" => {
                if let [network, channel] = args {
                    out.outbound.push(OutboundCommand::Join {
                        network: (*network).to_owned(),
                        channel: (*channel).to_owned(),
                    });
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Joining {channel}"),
                    }));
                } else {
                    out.events
                        .push(failed("join", "usage: /join <network> <#channel>"));
                }
            }
            "part" => {
                if let [network, channel] = args {
                    out.outbound.push(OutboundCommand::Part {
                        network: (*network).to_owned(),
                        channel: (*channel).to_owned(),
                    });
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Parting {channel}"),
                    }));
                } else {
                    out.events
                        .push(failed("part", "usage: /part <network> <#channel>"));
                }
            }
            "msg" => {
                if args.len() >= 3 {
                    let network = args[0].to_owned();
                    let target = args[1].to_owned();
                    let body = args[2..].join(" ");
                    out.outbound.push(OutboundCommand::Msg {
                        network,
                        target,
                        body,
                    });
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: "Message queued".to_owned(),
                    }));
                } else {
                    out.events
                        .push(failed("msg", "usage: /msg <network> <target> <message>"));
                }
            }
            "nick" => {
                if let [network, nick] = args {
                    out.outbound.push(OutboundCommand::Nick {
                        network: (*network).to_owned(),
                        nick: (*nick).to_owned(),
                    });
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Nick change requested: {nick}"),
                    }));
                } else {
                    out.events
                        .push(failed("nick", "usage: /nick <network> <nick>"));
                }
            }
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
            "disconnect" => {
                if let [network] = args {
                    out.outbound.push(OutboundCommand::Disconnect {
                        network: (*network).to_owned(),
                    });
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Disconnecting: {network}"),
                    }));
                } else {
                    out.events
                        .push(failed("disconnect", "usage: /disconnect <network>"));
                }
            }
            "buffer" => {
                if let [network, name] = args {
                    state.focus = Some(BufferId::new((*network).to_owned(), (*name).to_owned()));
                    out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                        command,
                        detail: format!("Focused {network}/{name}"),
                    }));
                } else {
                    out.events
                        .push(failed("buffer", "usage: /buffer <network> <name>"));
                }
            }
            "theme" => {
                out.events.push(AppEvent::Ui(UiEvent::Action(
                    "theme.editor.toggle".to_owned(),
                )));
                out.events.push(AppEvent::Command(CommandEvent::Succeeded {
                    command,
                    detail: "Theme editor opened".to_owned(),
                }));
            }
            "keymap" => {
                out.events.push(AppEvent::Ui(UiEvent::Action(
                    "keymap.editor.toggle".to_owned(),
                )));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::AppState;

    #[test]
    fn alias_resolution_runs_join() {
        let registry = CommandRegistry::default_registry();
        let mut state = AppState::new(100);
        let output = registry.execute_line(&mut state, CommandSource::Slash, "/j net #rust");

        assert_eq!(output.outbound.len(), 1);
        match &output.outbound[0] {
            OutboundCommand::Join { network, channel } => {
                assert_eq!(network, "net");
                assert_eq!(channel, "#rust");
            }
            other => panic!("unexpected outbound: {other:?}"),
        }
    }

    #[test]
    fn quit_command_sets_quit_flag() {
        let registry = CommandRegistry::default_registry();
        let mut state = AppState::new(100);
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
        let mut state = AppState::new(100);
        let output = registry.execute_line(&mut state, CommandSource::Slash, "/theme");

        assert!(output.events.iter().any(|event| {
            matches!(
                event,
                AppEvent::Ui(UiEvent::Action(action)) if action == "theme.editor.toggle"
            )
        }));
    }
}
