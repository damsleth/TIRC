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

## Phase 3: Architecture Refactoring ✅

- [x] **3.1 Decouple Config from Model**
  - Remove unused `tirc-config` dependency from `tirc-core`
  - Files: `tirc-core/Cargo.toml`

- [x] **3.2 Safe Input History**
  - Add `InputHistory` wrapper struct
  - Safe `prev()`/`next()` methods prevent out-of-bounds
  - Files: `input.rs`, `engine.rs`

- [x] **3.3 Add Newtype Wrappers**
  - Create `ProfileName`, `NetworkName`, `ChannelName` in `types.rs`
  - Add validation on construction (length limits, character restrictions)
  - Include `new()`, `new_unchecked()`, `as_str()`, `into_inner()` methods
  - Files: `tirc-core/src/types.rs`, `tirc-core/src/lib.rs`

- [x] **3.4 Consistent Error Handling**
  - Add `send_event()` helper for logging on channel close
  - Replace `let _ = tx.send()` with proper logging in disconnect handlers
  - Document pattern for future migration of remaining call sites
  - Files: `tirc-net/src/lib.rs`

## Phase 4: Performance & Deduplication ✅

- [x] **4.1 Cache Connected Profiles with Dirty Tracking**
  - Track `last_network_count`, `last_connected_count`, `last_buffer_count`
  - Only rebuild caches when counts change
  - Pre-allocate vectors with capacity
  - Files: `engine.rs`

- [x] **4.2 Remove Duplicate Rebuilds**
  - Remove redundant `server_names`/`buffer_refs` rebuilds in draw()
  - Keep only selection bounds clamping

- [x] **4.3 Smart Channel List Cache**
  - Track `last_channel_list_len`
  - Only clone when cache size changes

## Phase 5: Security Hardening ✅

- [x] **5.1 TLS by Default**
  - Change default TLS setting to `true`
  - Add warning when connecting without TLS
  - Files: `schema.rs`, `tirc-net/src/lib.rs`

- [x] **5.2 STATUSMSG Prefix Handling**
  - Strip @/+/% prefixes before is_channel_name() check
  - Properly route to channel buffer
  - Apply to both PRIVMSG and CTCP ACTION
  - Files: `tirc-net/src/lib.rs`

## Phase 6: Test Coverage ✅

- [x] **6.1 Unit Tests**
  - Add tests for bounded channel_list_cache
  - Add tests for channel list entry updates
  - Add tests for QUIT/KICK removing users from nicklist
  - Add tests for InputHistory navigation and bounds
  - Add tests for InputHistory deduplication
  - Files: `model.rs`, `input.rs`

---

## Progress Tracking

**Started:** 2026-02-13
**Completed:** 2026-02-16

### Completed (All 18 items)
- Phase 1: Critical Safety Fixes (4/4) ✅
- Phase 2: IRC Protocol Completeness (4/4) ✅
- Phase 3: Architecture Refactoring (4/4) ✅
- Phase 4: Performance & Deduplication (3/3) ✅
- Phase 5: Security Hardening (2/2) ✅
- Phase 6: Test Coverage (1/1) ✅

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
- `2308e38` perf(ui): add dirty tracking to avoid per-frame rebuilds
- `95217c4` feat(net): add STATUSMSG prefix support for channel messages
- `9097a34` test(core,ui): add unit tests for model and input history
- (pending) feat(core): add newtype wrappers for ProfileName, NetworkName, ChannelName
- (pending) refactor(net): add consistent error handling with send_event helper
