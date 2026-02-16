# QA Report — TIRC Code Review

**Reviewer posture:** Senior Rust developer, adversarial review.
**Date:** 2026-02-16
**Scope:** Full workspace (`tirc-core`, `tirc-net`, `tirc-ui`, `tirc-config`, `tirc-app`, `tirc-platform`)

---

## Severity Legend

| Label | Meaning |
|-------|---------|
| **CRITICAL** | Security hole or data-loss risk |
| **HIGH** | Bug that will bite users in normal operation |
| **MEDIUM** | Design smell that makes the codebase harder to maintain or extend |
| **LOW** | Nit or missed optimization |

---

## 1. CRITICAL — Silent `let _ =` on channel sends throughout `tirc-net`

**Files:** `crates/tirc-net/src/lib.rs` (dozens of occurrences)

The AGENTS.md **explicitly** says:

> **Don't Do This:** `let _ = tx.send(event).await;`

And yet almost every `events_tx.send(...)` call in `tirc-net/src/lib.rs` does exactly that:

```rust
let _ = events_tx
    .send(NetworkEvent::Parted { ... })
    .await;
```

This pattern silently drops events when the receiver is gone. If the app side drops the `mpsc::Receiver` (e.g. during shutdown, a panic, or a race), the network task will happily continue running, sending IRC messages into the void while the user sees a frozen UI. The *one* place that correctly checks `.is_err()` and returns is `NetworkEvent::Connected` at the top of the reconnect loop — every other send is fire-and-forget.

**Fix:** Propagate send failures upward or at minimum `return` from the task when the channel is closed, exactly as the `Connected` send already does. A helper like:

```rust
async fn send_or_bail(tx: &mpsc::Sender<NetworkEvent>, event: NetworkEvent) -> Result<(), ()> {
    tx.send(event).await.map_err(|_| ())
}
```

called with `send_or_bail(&events_tx, event).await?;` would catch every instance.

---

## 2. CRITICAL — SASL credentials logged / exposed in memory as plain `String`

**Files:** `crates/tirc-config/src/schema.rs`, `crates/tirc-net/src/lib.rs`

`SecretRef.password` and `SecretRef.sasl_password` are `Option<String>`. They are:

- Deserialized from TOML with no zeroization
- Cloned freely (`ServerProfile` derives `Clone`)
- Passed through `format!()` to build base64 payloads
- Kept alive for the entire process lifetime

An IRC client holding plaintext passwords in ordinary heap `String`s that are never zeroed is a textbook credential-leak vector. A core dump, `/proc/<pid>/mem` read, or swap-file recovery trivially recovers them.

**Fix:** Use `secrecy::SecretString` (or `zeroize::Zeroizing<String>`) for password fields. Implement `Deserialize` manually so the secret is wrapped on load and exposed only through a short-lived `expose_secret()` borrow.

---

## 3. CRITICAL — No IRC message length validation

**Files:** `crates/tirc-net/src/lib.rs` — `write_raw_line`, `write_command`

IRC RFC 2812 §2.3 limits messages to **512 bytes** including the trailing `\r\n`. Neither `write_raw_line` nor `write_command` enforces this. A user typing a long `/msg` or a long `/raw` line will send an over-length message that the server may silently truncate or drop. Worse, a malicious server could send arbitrarily long lines back — `read_until(b'\n')` will happily allocate unbounded memory.

**Fix:**
- Outbound: truncate or split messages that exceed 510 bytes (512 − CRLF).
- Inbound: use `read_until` with a capped buffer or `take()` adapter to limit reads to e.g. 8 KiB per line.

---

## 4. HIGH — `BufferId` is raw `String` pairs with no validation — newtypes are a lie

**Files:** `crates/tirc-core/src/model.rs`, AGENTS.md

AGENTS.md claims the project uses `ProfileName`, `NetworkName`, `ChannelName` newtypes from `types.rs`. **That file does not exist.** `BufferId` is just:

```rust
pub struct BufferId {
    pub network: String,
    pub name: String,
}
```

Both fields are public, constructed with `impl Into<String>`, and never validated. Any code can create a `BufferId` with an empty string, a string containing NUL/CR/LF/SPACE (all illegal in IRC identifiers), or a network name that doesn't match any configured profile. The claims in AGENTS.md about newtype safety are aspirational documentation, not reality.

**Fix:** Actually create the newtypes with validation on construction, make `BufferId` fields private, and enforce invariants. Or at minimum remove the lies from AGENTS.md so the next developer isn't misled.

---

## 5. HIGH — `view_model()` clones the entire world on every frame

**File:** `crates/tirc-core/src/model.rs` — `AppState::view_model()`

```rust
pub fn view_model(&self) -> AppViewModel {
    // clones: all networks, all buffers, all nicklists, ALL messages, all warnings,
    // the entire palette state, the entire channel_list_cache
}
```

This clones every `Message` in every buffer, every nick, the full channel list cache, etc. — **on every render frame**. For a user in 20 channels with 5,000 messages each, that's 100,000 `Message` clones per frame. At 4 Hz idle that's 400k clones/sec doing absolutely nothing.

The `ActiveBufferView.lines` field is `Vec<Message>` — a full deep clone of the focused buffer's `VecDeque<Message>` via `.iter().cloned().collect()`.

**Fix:** Either:
- Return `&AppState` directly and let the renderer borrow (the renderer already takes `&AppViewModel`), or
- Use `Cow` / `Arc` for messages so cloning is O(1), or
- Only clone the *active* buffer's visible slice instead of the entire message history.

---

## 6. HIGH — `InputEditor` is O(n) on every keystroke

**File:** `crates/tirc-ui/src/input.rs`

Every single character insertion, deletion, or cursor movement in `InputEditor` does:

```rust
let mut chars = self.text.chars().collect::<Vec<_>>();
chars.insert(self.cursor, c);
self.text = chars.into_iter().collect();
```

This is O(n) where n is the length of the input line (*three* allocations: Vec, then String, plus the iteration). For an IRC input line this is fine in practice (inputs are short), but `kill_to_end`, `kill_to_start`, and `kill_word` all do the same `chars().collect::<Vec<_>>()` dance. The cursor is tracked as a **character index**, but byte operations on the `String` use byte indices — this impedance mismatch is a latent bug if you ever add multi-byte cursor positioning.

**Low-priority fix:** Use `String` byte indices directly with `char_indices()`, or use a rope/gap buffer library. At minimum, document that `cursor` is a *char offset*, not a byte offset.

---

## 7. HIGH — `resolve_part_args` is an inscrutable heuristic nightmare

**File:** `crates/tirc-core/src/command.rs` — `resolve_part_args`

This function is ~60 lines of nested conditionals guessing what the user meant. It's completely untested (no unit tests for any of the `resolve_*_args` helpers except indirectly through the `join` test). The heuristics are fragile:

```rust
[only] => {
    if only.starts_with('#') {
        // It's a channel
    } else if state.networks.contains_key(*only) {
        // It's a profile
    } else {
        // It's... a part reason?? For the focused buffer??
    }
}
```

So `/part goodbye` is interpreted as "part the focused channel with reason 'goodbye'" — but `/part freenode` where `freenode` happens to be a network name means "part the focused channel on the freenode network". This is wildly unintuitive. A user who types `/part I'm leaving` will have `I'm` interpreted as... a channel? A network? A reason? Nobody can predict this without reading the source.

**Fix:** Stop guessing. Make the grammar unambiguous: `/part [#channel] [reason...]` in the focused profile, `/part <profile> <#channel> [reason...]` for explicit. Require `#` prefix for channels.

---

## 8. HIGH — Channel name validation ignores RFC 2812 entirely

**Files:** Multiple — `resolve_join_args`, `build_sidebar_items`, `close_focused_buffer`

The code checks `starts_with('#')` in many places to detect channels, but IRC channels can start with `#`, `&`, `+`, or `!` per RFC 2812. `build_sidebar_items` in `engine.rs` does check `#` and `&` but not `+` or `!`. Meanwhile `resolve_part_args` only checks `#`. This inconsistency means:

- A `+channel` buffer will be classified as a "chat" in the sidebar
- `/part +channel` won't work correctly (it hits the "is it a network?" branch)
- `close_focused_buffer` only sends PART for `#`-prefixed buffers

**Fix:** Centralize channel detection into a single `is_channel_name()` function (or use the `irc_proto` crate's `ChannelExt` trait which is already a dependency) and use it everywhere.

---

## 9. HIGH — RPL_NAMREPLY strips mode prefixes, losing operator/voice status

**File:** `crates/tirc-net/src/lib.rs` — `Response::RPL_NAMREPLY` handler

```rust
let nicks = args[3]
    .split_whitespace()
    .map(|nick| nick.trim_start_matches(['@', '+', '%', '&', '~']))
    .map(ToOwned::to_owned)
    .collect::<Vec<_>>();
```

This strips all mode prefixes on NAMES reply, so the nicklist sent to the UI has no operator/voice indicators. But the `ModeChange` handler in `model.rs` tries to *add* prefixes (`@`, `+`, etc.) to the nicklist. So the initial nicklist has no prefixes, then mode changes start adding them, creating an inconsistent state where some users have prefixes and some don't depending on whether they had the mode before or after the NAMES reply.

**Fix:** Preserve the mode prefix from RPL_NAMREPLY. The nicklist should be `["@op_user", "+voiced_user", "regular_user"]` from the start.

---

## 10. MEDIUM — `app.rs::run()` is a 450-line god function

**File:** `crates/tirc-app/src/app.rs`

The `run()` function is the entire application lifecycle crammed into one `async fn`. It sets up config, creates state, builds the UI, connects profiles, runs the event loop, handles UI intents, executes commands, sends messages, manages sessions, cleans up logs, persists config, opens external editors, and tears down the terminal. It takes `&mut` borrows of ~10 different pieces of state and passes them through helper functions that each take 5-8 arguments.

This is a textbook Single Responsibility Principle violation. You cannot test any part of the main loop without spinning up a real terminal, real network connections, and real file I/O.

**Fix:** Extract an `AppRuntime` struct that owns the state, net manager, logger, etc. and provides methods like `process_network_events()`, `handle_intents()`, `tick()`. This makes each piece testable in isolation.

---

## 11. MEDIUM — The main loop is a busy-poll with `event::poll(20ms)`

**File:** `crates/tirc-app/src/app.rs`

```rust
loop {
    while let Ok(network_event) = net_event_rx.try_recv() { ... }
    if event::poll(Duration::from_millis(20))? { ... }
    if ui.should_draw_heartbeat() { ... }
    // ...
}
```

This is busy-polling: even when nothing is happening, the app wakes up every 20ms to check for terminal events and drains the network channel with `try_recv()`. The correct approach is `tokio::select!` over the network channel, a terminal event stream (crossterm supports async with the `event-stream` feature), and a heartbeat timer. The current approach wastes CPU and makes the app unsuitable for battery-operated devices.

**Fix:** Use `crossterm::event::EventStream` and `tokio::select!` to sleep until something actually happens.

---

## 12. MEDIUM — `UiEngine` has 40+ fields and `#[allow(clippy::too_many_arguments)]`

**File:** `crates/tirc-ui/src/engine.rs`

`UiEngine::new()` takes 8 arguments (and suppresses the clippy lint about it). The struct has 40+ fields including layout regions, scroll state, sidebar state, palette state, filter state, editor state, history, and cached data from the model. This is a "God object" that owns rendering, input handling, palette logic, composer workflows, sidebar interaction, nicklist filtering, scroll tracking, and dirty-flag caching.

**Fix:** Extract subsystems: `SidebarState`, `NicklistState`, `ScrollState`, `ComposerState`. Each gets its own `handle_key()` and `draw()` methods. The `UiEngine` becomes a thin coordinator.

---

## 13. MEDIUM — `UserQuit` broadcasts to ALL joined channels, not just the quitting user's channels

**File:** `crates/tirc-net/src/lib.rs` — `Command::QUIT` handler

```rust
Command::QUIT(reason) => {
    if let Some(nick) = source_nick {
        // Note: We don't track other users' channel membership, so we send to all joined channels
        let channels: Vec<String> = joined_channels.iter().cloned().collect();
        let _ = events_tx.send(NetworkEvent::UserQuit {
            network: profile.name.clone(),
            nick,
            reason,
            channels,
        }).await;
    }
}
```

This sends a quit event for every channel *we* are in, not just the ones the quitting user was in. The nicklist removal in `model.rs` then scans all those buffers, which is wasted work. More importantly, the quit message "bob has quit" will appear in channels bob was never in, because `model.rs` posts the message to every mentioned channel's buffer.

Actually wait — `model.rs` only posts the message if the buffer exists AND the user is found in the nicklist, so the nicklist check prevents ghost quit messages. But the code comment says "The UI can filter based on nick list tracking" — this is working by accident, not by design. And the quit message **is** posted to every channel buffer regardless:

```rust
for channel in &channels {
    if let Some(buffer) = self.buffers.get_mut(&id) {
        buffer.messages.push_back(Message { body: quit_msg.clone(), ... });
    }
}
```

The message push is unconditional. The `retain` call removes the nick (which is a no-op for channels the user wasn't in), but the quit message still gets posted.

**Fix:** Track per-channel membership in `tirc-net` (maintain a `HashMap<String, HashSet<String>>` of channel→nicks), or at minimum only post the quit message to buffers where the user was actually removed from the nicklist.

---

## 14. MEDIUM — No flood protection on outbound messages

**File:** `crates/tirc-net/src/lib.rs`

There is zero rate limiting on outbound IRC commands. A user can paste 50 lines into the input, and TIRC will blast them all at once. Most IRC servers will kill the connection for "Excess Flood" within seconds.

**Fix:** Implement a token-bucket or sliding-window rate limiter on `write_command` / `write_raw_line`. Standard practice is ~2 messages/second with a burst of 5.

---

## 15. MEDIUM — `handle_server_line` conflates our JOINs with server-confirmed JOINs

**File:** `crates/tirc-net/src/lib.rs`

When we send a `NetCommand::Join`, `handle_command` immediately fires a `NetworkEvent::Joined` event **before** the server confirms the join:

```rust
NetCommand::Join(channel) => {
    joined_channels.insert(channel.clone());
    write_command(writer, Command::JOIN(channel.clone(), None, None)).await?;
    let _ = events_tx.send(NetworkEvent::Joined { ... }).await;  // <- premature
}
```

Then when the server echoes back `JOIN`, the handler fires *another* `NetworkEvent::Joined`:

```rust
Command::JOIN(channel, _, _) => {
    if source_nick.as_deref() == Some(runtime.current_nick.as_str()) {
        joined_channels.insert(channel.clone());  // already inserted
        let _ = events_tx.send(NetworkEvent::Joined { ... }).await;  // duplicate
    }
}
```

This causes **double join events**, double focus changes, and double "Joined #channel" status messages. The same issue exists for PART.

**Fix:** Don't emit `Joined`/`Parted` from `handle_command`. Let the server echo be the single source of truth.

---

## 16. MEDIUM — `Serialize`/`Deserialize` on `AppState` is a footgun

**File:** `crates/tirc-core/src/model.rs`

`AppState` derives `Serialize, Deserialize`. This struct contains:
- `VecDeque<Message>` (potentially thousands of messages per buffer)
- `BTreeMap` of network states with internal runtime flags
- Ephemeral UI state like `input`, `help_open`, `palette` state

Nobody should ever serialize the full `AppState`. The session system correctly uses `SessionState` (a subset), but having `Serialize` on `AppState` invites someone to accidentally `serde_json::to_string(&state)` and serialize 50 MB of chat history. The `#[serde(skip)]` on two fields proves this derive is awkward — you're fighting serde to not serialize things you don't want serialized.

**Fix:** Remove `Serialize, Deserialize` from `AppState`. Use purpose-built DTOs for any serialization needs.

---

## 17. MEDIUM — `ChannelListItem` uses linear scan on every item

**File:** `crates/tirc-core/src/model.rs` — `apply_network_event`

```rust
NetworkEvent::ChannelListItem { ... } => {
    if let Some(existing) = entries.iter_mut().find(|entry| entry.channel == channel) {
        existing.users = users;
        existing.topic = topic;
    } else if entries.len() < self.max_channel_list_entries {
        entries.push(ChannelListEntry::new(channel, users, topic));
    }
}
```

For every incoming channel list entry, we do a linear scan of all existing entries. On a large network (Libera has ~25,000 channels), this is O(n²) total: 25,000 entries × 25,000 scans = 625 million comparisons. The `max_channel_list_entries` default is 10,000, so at best it's O(10,000 × 10,000) = 100 million string comparisons.

**Fix:** Use a `HashMap<String, usize>` index from channel name to position in the Vec, or just use a `BTreeMap<String, ChannelListEntry>`.

---

## 18. MEDIUM — `mouse.rs` exists but is dead code

**File:** `crates/tirc-ui/src/mouse.rs`

This file declares `LayoutRegions` and `MouseContext` with `#![allow(dead_code)]` and a comment: "NOTE: Currently unused — mouse handling is inline in engine.rs." The `engine.rs` duplicates all this logic inline. This is a half-finished refactoring that now creates two places to keep in sync, neither of which knows about the other. The AGENTS.md notes this as a known issue about click handling ordering.

**Fix:** Either finish the extraction or delete `mouse.rs`. Don't leave zombie modules with `#![allow(dead_code)]`.

---

## 19. MEDIUM — `clippy::all = "allow"` in workspace lints

**File:** `Cargo.toml`

```toml
[workspace.lints.clippy]
all = "allow"
```

You've disabled **all** clippy lints workspace-wide. This means clippy catches nothing — no `unwrap()` in library code, no redundant clones, no suspicious arithmetic, no missing error handling. The one lint you do enforce (`unsafe_code = "forbid"`) is good, but suppressing everything else defeats the purpose of having a linter.

**Fix:** Remove `all = "allow"`. Enable at least `clippy::pedantic` as warn, and selectively `#[allow]` specific lints where justified.

---

## 20. LOW — Reconnect jitter uses full second of randomness

**File:** `crates/tirc-net/src/lib.rs`

```rust
fn reconnect_delay(attempt: u32) -> Duration {
    let exp = 2_u64.saturating_pow(attempt.min(10));
    let base = Duration::from_secs(exp).min(Duration::from_secs(120));
    let jitter = rand::rng().random_range(0..1_000_u64);
    base + Duration::from_millis(jitter)
}
```

At attempt 1, the base is 2 seconds and jitter is 0–1 second (50% of base). At attempt 10, base is 120 seconds and jitter is 0–1 second (0.8% of base). The jitter should scale with the base delay to be effective. Also, `2^10 = 1024` which gets clamped to 120 — the cap kicks in at attempt 7 (`2^7 = 128 → 120`), so attempts 7–10 are identical.

**Fix:** Use `jitter = rng.random_range(0..base.as_millis())` for proportional jitter and cap at attempt 7 instead of 10.

---

## 21. LOW — CTCP SOURCE contains a placeholder URL

**File:** `crates/tirc-net/src/lib.rs`

```rust
"SOURCE" => {
    let reply = format!(
        "{CTCP_DELIM}SOURCE https://github.com/user/tirc{CTCP_DELIM}"
    );
```

This links to `github.com/user/tirc` which is presumably a placeholder. The repo is `damsleth/TIRC`.

---

## 22. LOW — `notify-rust` dependency declared but gated behind nonexistent feature

**File:** `crates/tirc-platform/src/lib.rs`

```rust
#[cfg(all(target_os = "macos", feature = "macos-notify"))]
{
    notify_rust::Notification::new()
```

The `tirc-platform/Cargo.toml` doesn't define a `macos-notify` feature, so this code is never compiled and the `notify-rust` dependency in `workspace.dependencies` is dead weight.

---

## 23. LOW — `nucleo-matcher` dependency declared but never used

**File:** `Cargo.toml`

`nucleo-matcher = "0.3.1"` is listed in workspace dependencies but no crate actually depends on it. The palette uses a hand-rolled `fuzzy_score` function instead.

---

## 24. LOW — Redaction patterns in `log.rs` are brittle and incomplete

**File:** `crates/tirc-core/src/log.rs`

The `redact_sensitive_content` function uses substring matching on lowercased text. Problems:

- It only redacts the first occurrence of each pattern
- "identify" appearing in normal chat ("I identify with that") triggers false-positive redaction
- The patterns are case-insensitive but the replacement uses the original text's byte positions on the lowercased version — this is correct but fragile
- SASL `AUTHENTICATE` with multi-line payloads (400+ bytes get split) won't be caught

---

## 25. LOW — `PaletteEngine::fuzzy_score` is naive

**File:** `crates/tirc-ui/src/palette.rs`

The scoring function penalizes `candidate.len() - query.len()` and uses byte-position offsets for gap penalties. This produces poor results for multi-word queries and doesn't reward word-boundary matches. The workspace already has `nucleo-matcher` listed as a dependency (see issue #23), which is a battle-tested fuzzy matcher. Use it instead of rolling your own.

---

## Summary

| Severity | Count |
|----------|-------|
| CRITICAL | 3 |
| HIGH | 6 |
| MEDIUM | 9 |
| LOW | 7 |

The architecture (workspace crates, event-driven model, dirty tracking) is solid in concept. The problems are in execution: raw strings where newtypes are claimed, silent failures in the network layer, a god function for the main loop, unnecessary cloning, missing IRC protocol compliance, and disabled linting. The AGENTS.md is more aspirational spec than truthful documentation — it describes newtypes and patterns that don't actually exist in the code.

Fix the critical items first (silent sends, credential handling, message length validation), then work through the high-severity protocol correctness issues. The medium items are quality-of-life for maintainability.
