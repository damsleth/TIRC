# TIRC

TIRC ("turk") is a fast, opinionated, batteries-included terminal IRC client written in Rust with `ratatui`.

## Features (v1)

- Simultaneous multi-server IRC sessions.
- IRCv3 baseline support (CAP negotiation, SASL PLAIN, message tags, `server-time`, `echo-message`, `batch`).
- `Ctrl-P` command palette with fuzzy filtering.
- Unified slash + palette command execution.
- Theme editor (`Ctrl-T`, `/theme`) and keymap editor (`Ctrl-Y`, `/keymap`) overlays.
- Theme and keymap customization via TOML.
- Session restore + JSONL chat logging with retention.
- Terminal bell + macOS notifications.
- First-run config generation with a starter Libera profile.

## Install (development)

```bash
cargo run -p tirc-app -- --help
```

## CLI

```text
tirc
tirc --config <path>
tirc --profile <name>
tirc --no-restore
tirc --debug
```

## Configuration Paths

TIRC uses XDG-style paths:

- Config: `~/.config/tirc/config.toml`
- Themes: `~/.config/tirc/themes/*.toml`
- Keymaps: `~/.config/tirc/keymaps/*.toml`
- Session: `~/.local/state/tirc/session.json`
- Logs: `~/.local/state/tirc/logs/<profile>/<buffer>/<YYYY-MM-DD>.jsonl`
- Cache: `~/.cache/tirc`

## Security Notes

- Secrets are stored plaintext in config by design for v1.
- TIRC enforces strict file permissions (`0600`) for config and refuses to start with insecure permissions.

## Development

```bash
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## License

Licensed under either:

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.
