# AGENTS.md - AI Agent Guidelines for TIRC

## Project Overview

TIRC is a terminal IRC client written in Rust. Multi-crate workspace architecture separating concerns.

## Crate Responsibilities

| Crate | Purpose | Key Files |
|-------|---------|-----------|
| `tirc-core` | Domain model, commands, events, types | `model.rs`, `command.rs`, `types.rs`, `event.rs` |
| `tirc-net` | IRC protocol, connection management | `lib.rs` |
| `tirc-ui` | Terminal rendering, input handling | `engine.rs`, `render.rs`, `input.rs` |
| `tirc-config` | Configuration schema, paths, themes | `schema.rs`, `keymap.rs` |
| `tirc-app` | Application bootstrap, main loop | `app.rs`, `cli.rs` |
| `tirc-platform` | OS-specific functionality | `lib.rs` |

## Architecture Rules

1. **Dependency direction:** `tirc-app` → `tirc-ui` → `tirc-core` ← `tirc-net`. Core has no dependencies on other crates.
2. **Event-driven:** Components communicate via `mpsc` channels. Never block the UI thread.
3. **Newtype wrappers:** Use `ProfileName`, `NetworkName`, `ChannelName` from `types.rs` for domain identifiers.

## Code Patterns

### Do This

```rust
// Validate newtypes on construction
let profile = ProfileName::new(input)?;

// Handle channel send failures explicitly
if tx.send(event).await.is_err() {
    debug!("channel closed");
    return; // Graceful shutdown
}

// Use dirty flags for UI updates
self.dirty = true;
```

### Don't Do This

```rust
// BAD: Silent send failures
let _ = tx.send(event).await;

// BAD: Bypass validation with new_unchecked from untrusted input
let profile = ProfileName::new_unchecked(user_input);

// BAD: #[serde(transparent)] without custom Deserialize validation
#[serde(transparent)]
pub struct ValidatedType(String);  // Bypasses new() on deserialize!
```

## Known Pitfalls

1. **Serde + newtypes:** Using `#[serde(transparent)]` bypasses validation. Use `#[serde(try_from = "String")]` with `TryFrom` impl instead.

2. **Quote parsing:** `parse_command_args` in `command.rs` has no escape handling. If adding string parsing, handle backslash escapes.

3. **IRC RFC compliance:** Channel names can start with `#`, `&`, `+`, or `!` per RFC 2812. Channel names cannot contain NUL, BELL, CR, LF, SPACE, COMMA, or COLON.

4. **UI regions:** Click handling in `engine.rs` uses if-else ordering. Overlapping regions will match first condition only.

## Testing

- Unit tests live in `#[cfg(test)] mod tests` within each file
- Integration tests for `tirc-net` in `crates/tirc-net/tests/`
- Run: `cargo test --workspace`
- Always test edge cases: empty input, max length, invalid characters

## Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build
cargo test --workspace         # All tests
cargo clippy --workspace       # Lint
cargo fmt --all                # Format
./target/debug/tirc            # Run
```

## File Conventions

- Max line length: ~100 chars (soft limit)
- Use `tracing` macros (`debug!`, `info!`, `error!`) not `println!`
- Error types per crate in `error.rs`
- Public API exports via `lib.rs`

## When Modifying

1. **Adding commands:** Edit `command.rs`, add to `CommandRegistry::default_registry()`
2. **Adding events:** Edit `event.rs`, handle in `app.rs` main loop
3. **Adding config options:** Edit `config/schema.rs`, update defaults
4. **Adding UI keybinds:** Edit `engine.rs` `handle_key()` method

## Performance

- UI uses dirty tracking to avoid per-frame rebuilds
- Nicklist filtering is cached
- Sidebar items are rebuilt only on model changes
- Use `Cow<str>` or `&str` when possible to avoid allocations

## Current Tech Debt

See `QA_REPORT.md` for open issues. Key items:
- Quote parser needs escape handling
- Newtype serde validation bypass
- Dead `send_event` helper in tirc-net
