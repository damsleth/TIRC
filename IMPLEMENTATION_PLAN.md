# TIRC Refactoring Implementation Plan

This plan addresses architectural, security, and correctness issues identified in code review.

## Phase 1: Critical Safety Fixes

- [ ] **1.1 Bound `channel_list_cache`**
  - Add `max_channel_list_entries` config option (default: 10,000)
  - Check cache size before inserting in `apply_network_event`
  - Files: `model.rs`, `schema.rs`

- [ ] **1.2 Fix Nick Collision Loop**
  - Track collision count in `ConnectionRuntime`
  - Limit to 5 attempts, then disconnect with error
  - Truncate base nick if resulting nick exceeds 16 chars
  - Files: `tirc-net/src/lib.rs`

- [ ] **1.3 Create Action Enum**
  - Define `UiAction` enum replacing all magic strings
  - Update `UiEvent::Action` to use enum
  - Compiler catches missing cases
  - Files: `event.rs`, `engine.rs`, `model.rs`

- [ ] **1.4 Implement Secret Redaction**
  - Add redaction logic to `LogWriter`
  - Redact patterns: `/msg nickserv identify`, `AUTHENTICATE`
  - Files: `log.rs`, `app.rs`

## Phase 2: IRC Protocol Completeness

- [ ] **2.1 Add CTCP Handling**
  - Detect CTCP in PRIVMSG (body starts/ends with `\x01`)
  - Handle VERSION, PING, ACTION
  - Add `NetworkEvent::Action` variant
  - Files: `tirc-net/src/lib.rs`, `event.rs`

- [ ] **2.2 Handle QUIT Events**
  - Add `NetworkEvent::UserQuit` variant
  - Handle `Command::QUIT` in parser
  - Remove nick from nicklists
  - Files: `tirc-net/src/lib.rs`, `event.rs`, `model.rs`

- [ ] **2.3 Handle KICK Events**
  - Add `NetworkEvent::Kicked` variant
  - Handle `Command::KICK` in parser
  - Update buffer state if self kicked
  - Files: `tirc-net/src/lib.rs`, `event.rs`, `model.rs`

- [ ] **2.4 Basic MODE Support**
  - Add `NetworkEvent::Mode` variant
  - Parse `Command::MODE`
  - Track channel operator prefixes in nicklist
  - Files: `tirc-net/src/lib.rs`, `model.rs`

## Phase 3: Architecture Refactoring

- [ ] **3.1 Extract App Components**
  - Create `runtime.rs` for main loop
  - Create `handlers/` module for event dispatch
  - Target: `run()` under 100 lines
  - Files: `tirc-app/src/`

- [ ] **3.2 Fix Error Handling**
  - Replace `let _ = tx.send()` with proper handling
  - Log and gracefully shutdown on channel close
  - Files: `tirc-net/src/lib.rs`, `app.rs`

- [ ] **3.3 Add Backpressure Handling**
  - Use `try_send()` with priority
  - Skip non-critical events when full
  - Add dropped event counter
  - Files: `tirc-net/src/lib.rs`

- [ ] **3.4 Fix Session Save Race**
  - Use mutex to protect session save
  - Use atomic write (`.tmp` then rename)
  - Files: `app.rs`, `session.rs`

## Phase 4: Performance & Deduplication

- [ ] **4.1 Cache Connected Profiles**
  - Remove rebuild from `draw()`
  - Add `update_network_state()` method
  - Files: `engine.rs`

- [ ] **4.2 Remove Duplicate Cache**
  - Remove `channel_list_cache` from `UiEngine`
  - Use only `AppViewModel` data
  - Files: `engine.rs`

- [ ] **4.3 Add Newtype Wrappers**
  - Create `ProfileName`, `NetworkName`, `ChannelName`
  - Add validation on construction
  - Files: new `types.rs`, update usages

- [ ] **4.4 Fix Focus Cycling Bug**
  - Remove early return when focus is None
  - Always clear unread after setting focus
  - Files: `model.rs`

- [ ] **4.5 Reduce Hot-Path Cloning**
  - Construct `LogRecord` directly
  - Use `Cow<str>` where appropriate
  - Files: `app.rs`

## Phase 5: Security & Configuration

- [ ] **5.1 TLS Configuration Options**
  - Add `TlsConfig` with verify options
  - Update `connect_stream()` to apply settings
  - Files: `schema.rs`, `tirc-net/src/lib.rs`

- [ ] **5.2 STATUSMSG Handling**
  - Parse prefixes from PRIVMSG target
  - Strip prefix for buffer name
  - Files: `tirc-net/src/lib.rs`

## Phase 6: Testing

- [ ] **6.1 Unit Tests for Command Registry**
  - Test all command variants
  - Test error cases and aliases
  - Files: `command.rs`

- [ ] **6.2 Unit Tests for UiEngine**
  - Test key handling
  - Test palette modes
  - Test scroll bounds
  - Files: `engine.rs`

- [ ] **6.3 Integration Tests for Network**
  - Mock IRC server
  - Test connection lifecycle
  - Files: `tirc-net/tests/`

---

## Progress Tracking

Started: 2026-02-13
