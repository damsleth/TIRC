# TIRC Refactoring Implementation Plan

This plan addresses architectural, security, and correctness issues identified in code review.

## Phase 1: Critical Safety Fixes ✅

- [x] **1.1 Bound `channel_list_cache`**
  - Add `max_channel_list_entries` config option (default: 10,000)
  - Check cache size before inserting in `apply_network_event`
  - Files: `model.rs`, `schema.rs`

- [x] **1.2 Fix Nick Collision Loop**
  - Track collision count in `ConnectionRuntime`
  - Limit to 5 attempts, then disconnect with error
  - Use underscore suffix pattern for collisions
  - Files: `tirc-net/src/lib.rs`

- [x] **1.3 Create Action Enum**
  - Define `UiAction` enum replacing all magic strings
  - Update `UiEvent::Action` to use enum
  - Compiler catches missing cases
  - Files: `event.rs`, `engine.rs`, `model.rs`

- [x] **1.4 Implement Secret Redaction**
  - Add redaction logic to `LogWriter`
  - Redact patterns: `/msg nickserv identify`, `AUTHENTICATE`
  - Files: `log.rs`, `app.rs`

## Phase 2: IRC Protocol Completeness ✅

- [x] **2.1 Add CTCP Handling**
  - Detect CTCP in PRIVMSG (body starts/ends with `\x01`)
  - Handle VERSION, PING, TIME, CLIENTINFO, SOURCE, ACTION
  - Add `NetworkEvent::Action` variant
  - Files: `tirc-net/src/lib.rs`, `event.rs`

- [x] **2.2 Handle QUIT Events**
  - Add `NetworkEvent::UserQuit` variant
  - Handle `Command::QUIT` in parser
  - Remove nick from nicklists
  - Files: `tirc-net/src/lib.rs`, `event.rs`, `model.rs`

- [x] **2.3 Handle KICK Events**
  - Add `NetworkEvent::Kicked` variant with `was_self` flag
  - Handle `Command::KICK` in parser
  - Update buffer state if self kicked
  - Files: `tirc-net/src/lib.rs`, `event.rs`, `model.rs`

- [x] **2.4 Basic MODE Support**
  - Add `NetworkEvent::ModeChange` variant
  - Parse `Command::ChannelMODE` and `Command::UserMODE`
  - Track channel operator prefixes in nicklist
  - Files: `tirc-net/src/lib.rs`, `model.rs`

## Phase 3: Architecture Refactoring (Partial)

- [x] **3.1 Decouple Config from Model**
  - Remove unused `tirc-config` dependency from `tirc-core`
  - Files: `tirc-core/Cargo.toml`

- [x] **3.2 Safe Input History**
  - Add `InputHistory` wrapper struct
  - Safe `prev()`/`next()` methods prevent out-of-bounds
  - Files: `input.rs`, `engine.rs`

- [ ] **3.3 Add Newtype Wrappers** (deferred)
  - Create `ProfileName`, `NetworkName`, `ChannelName`
  - Add validation on construction

- [ ] **3.4 Consistent Error Handling** (deferred)
  - Replace `let _ = tx.send()` with proper handling
  - Log and gracefully shutdown on channel close

## Phase 4: Performance & Deduplication (Not Started)

- [ ] **4.1 Cache Connected Profiles**
  - Remove rebuild from `draw()`
  - Add `update_network_state()` method

- [ ] **4.2 Remove Duplicate Cache**
  - Remove `channel_list_cache` from `UiEngine`
  - Use only `AppViewModel` data

- [ ] **4.3 Reduce Hot-Path Allocations**
  - Use `Cow<str>` where appropriate
  - Pre-allocate vectors with capacity

- [ ] **4.4 Smarter Dirty Flags**
  - Track which parts of state changed
  - Skip re-rendering unchanged widgets

## Phase 5: Security Hardening

- [x] **5.1 TLS by Default**
  - Change default TLS setting to `true`
  - Add warning when connecting without TLS
  - Files: `schema.rs`, `tirc-net/src/lib.rs`

- [ ] **5.2 STATUSMSG Prefix Handling**
  - Check for @/+ prefix on channel messages
  - Only display to appropriate users

## Phase 6: Test Coverage (Not Started)

- [ ] **6.1 Unit Tests**
  - Add tests for CTCP parsing
  - Add tests for MODE parsing
  - Add tests for nick collision handling

- [ ] **6.2 Integration Tests**
  - Add tests for QUIT/KICK handling
  - Add tests for history navigation

---

## Progress Tracking

**Started:** 2026-02-13

### Completed (11 items)
- Phase 1: Critical Safety Fixes (4/4)
- Phase 2: IRC Protocol Completeness (4/4)
- Phase 3: Architecture Refactoring (2/4)
- Phase 5: Security Hardening (1/2)

### Commits
- `4a191f1` fix: bound channel_list_cache to prevent memory exhaustion
- `17bc3f2` fix: limit nick collision retries to prevent infinite loop
- `0793015` refactor: replace magic action strings with UiAction enum
- `7a7d34b` feat(log): implement secret redaction for sensitive data
- `fe9530f` feat(net): implement CTCP support
- `e665dad` feat(net): implement proper QUIT handling
- `d1d9c4c` feat(net): implement proper KICK handling
- `8a7380d` feat(net): implement MODE tracking
- `15f9217` refactor(core): remove unused tirc-config dependency
- `1e021f1` refactor(ui): create InputHistory wrapper for safe history navigation
- `b2bc838` feat(security): enforce TLS by default with plaintext warning
