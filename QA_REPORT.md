# QA Report: Code Review for Commit e404d22

**Date:** 2026-02-16
**Commit:** `feat: implement command argument parsing with quote support and add newtype wrappers for profile, network, and channel names`

---

## Critical Issues

### 1. Argument Parser Mangles Freeform Text Commands

**File:** `crates/tirc-core/src/command.rs` (`parse_command_args`)

The escape/quote parser is applied uniformly to **all** commands, including `/msg`, `/me`, and `/raw` where trailing arguments are freeform user text. This causes:

- `/msg #rust check C:\Users\path` → sends `check C:Userspath` (backslashes eaten)
- `/msg #rust "hello world"` → sends `hello world` (quotes stripped)
- `/me says "wow"` → sends `says wow`

IRC users are not in a shell. Backslashes and quotes appear constantly in chat. The parser is modeled after shell semantics but applied to a chat context.

**Impact:** Data corruption of user messages. Users will not understand why their text is being mangled.

**Recommendation:** Per-command argument semantics. Commands like `/connect` and `/join` benefit from quoted args. Commands like `/msg`, `/me`, `/raw` need raw trailing text after the first N positional args (similar to IRC protocol's `:trailing` parameter).

---

### 2. Newtype Wrappers Are Dead Code (513 Lines, Zero Consumers)

**File:** `crates/tirc-core/src/types.rs`

`ProfileName`, `NetworkName`, and `ChannelName` are defined and exported from `lib.rs` but used by **nothing**. Not `command.rs`, not `engine.rs`, not `tirc-net`. Every `OutboundCommand` still passes raw `String`. Every `SidebarItem` still stores raw `String`.

This is 513 lines of speculative code with full serde roundtrips, `TryFrom` impls, and `new_unchecked` escape hatches — for types that don't participate in the data model.

**Impact:** Dead code increases maintenance burden, review surface, and compile time for zero benefit.

**Recommendation:** Ship the types in the commit that actually integrates them into the data model. Don't land unused abstractions.

---

### 3. `new_unchecked` Undermines Newtype Guarantees Before They're Even Used

**File:** `crates/tirc-core/src/types.rs`

All three newtypes expose `pub fn new_unchecked()`. The entire point of validated newtypes is to make invalid states unrepresentable. A public unchecked constructor defeats that guarantee on day one — any downstream consumer can bypass validation trivially.

**Recommendation:** At minimum, restrict to `pub(crate)`. Better: don't add it until there's a proven need with a concrete caller.

---

## Medium Issues

### 4. Disconnect Logic Has TOCTOU Race and Leaks Tasks

**File:** `crates/tirc-net/src/lib.rs` (lines 123-127)

```rust
if handle.tx.send(NetCommand::Disconnect).await.is_err() {
    debug!(network, "disconnect command channel already closed");
} else if !handle.task.is_finished() {
    handle.task.abort();
}
```

Between `send` succeeding and the `is_finished()` check, the task could finish processing the disconnect. More importantly: the `JoinHandle` is dropped without being joined or aborted in some paths. Dropped `JoinHandle`s in tokio **detach** the task — it keeps running as a zombie.

The old code was blunt (unconditional `abort()`) but deterministic. The new code has a time-of-check/time-of-use window and doesn't ensure cleanup.

**Recommendation:** Either `abort()` unconditionally (simple and correct), or `.await` the task after sending disconnect to ensure clean shutdown.

---

### 5. `name.len()` Validates Byte Length, Not Character Length

**File:** `crates/tirc-core/src/types.rs` (`validate_profile_like_name`)

```rust
if name.len() > max_len {
```

But the allowed character set includes `c.is_alphanumeric()`, which matches non-ASCII Unicode. A profile name like `"日本語のプロファイル"` (10 chars) is 30 bytes and would fail the 64-"character" check at ~22 characters. The `MAX_LEN` constant implies a character limit but enforces a byte limit.

**Recommendation:** Either restrict to ASCII (`c.is_ascii_alphanumeric()`) or use `.chars().count()`. Be explicit about what "length" means.

---

### 6. `validate_channel_name` Has Redundant Empty Checks

**File:** `crates/tirc-core/src/types.rs` (lines 96-141)

The function checks `name.is_empty()`, then pattern-matches `chars.next()` against `None` (same condition), then checks `rest.is_empty()`. Three separate empty-ish checks in 30 lines, each returning the same `Empty` error kind but for subtly different conditions ("no input" vs "no prefix" vs "prefix only").

**Recommendation:** Consolidate. Check length/prefix once, clearly.

---

### 7. `CommandArgParseError` Missing `impl std::error::Error`

**File:** `crates/tirc-core/src/command.rs`

`Display` is implemented but `std::error::Error` is not. Meanwhile, `NameValidationError` in the same commit *does* implement `Error`. This inconsistency means `CommandArgParseError` won't work with `?` propagation into `anyhow::Error`, `Box<dyn Error>`, or similar patterns.

**Recommendation:** Add `impl std::error::Error for CommandArgParseError {}`.

---

### 8. Sidebar Dual Data Structure Sync Hazard

**File:** `crates/tirc-ui/src/engine.rs`

`sidebar_items` and `sidebar_buffer_indices` must stay perfectly in sync. Both are rebuilt every frame in `draw()`, but nothing structurally enforces this — they're two independent `Vec`s on `UiEngine`. If any code path mutates `sidebar_items` without rebuilding `sidebar_buffer_indices` (or vice versa), you get silent wrong-buffer-selected bugs or index panics.

Additionally, `navigate_focused_pane` does a linear scan of `sidebar_buffer_indices` to find `current_pos` on every keypress.

**Recommendation:** Either encapsulate both in a single struct with a single rebuild method, or compute `buffer_indices` lazily on navigation rather than eagerly every frame.

---

## Minor Issues

### 9. Sidebar `build_sidebar_items` Clones Profile Name Per Item

**File:** `crates/tirc-ui/src/engine.rs` (`build_sidebar_items`)

Every `SidebarItem` variant stores an owned `String` for `profile`. A single profile with 10 channels and 5 chats clones the profile name 15+ times. This runs every frame.

**Recommendation:** Use `Rc<str>` or store profile indices instead of redundant owned strings.

---

### 10. Formatting Changes Mixed with Logic Changes

A significant portion of this diff is whitespace cleanup, brace style reformatting, and line rewrapping (especially in `tirc-net/src/lib.rs` and `engine.rs`). This makes the diff harder to review and pollutes `git blame`.

**Recommendation:** Run `rustfmt` in CI. Ship formatting fixes as a separate commit.

---

## Summary

| Severity | Count |
|----------|-------|
| Critical | 3 |
| Medium   | 5 |
| Minor    | 2 |

### Resolved From Previous Review

The following issues from the prior QA report have been addressed in this commit:

- Quote parser now supports `\` escape sequences and reports unclosed quotes as errors
- `NetworkName` validation decoupled from `ProfileName` (uses shared helper with own `MAX_LEN`)
- Serde now uses `#[serde(try_from = "String")]` instead of `#[serde(transparent)]` — validation runs on deserialize
- Channel validation updated to RFC 2812 (`+`, `!` prefixes; colon forbidden)
- Dead `send_event` helper removed
- `# Safety` doc removed from non-unsafe `new_unchecked`
- `SidebarItem::Status` restored as its own variant
- Sidebar navigation no longer O(n) worst-case (uses pre-computed buffer indices)
- `new()` now takes `&str` to avoid allocation on validation failure
- `TryFrom<String>` and `TryFrom<&str>` implemented for all newtypes
- Tests added for escaped quotes, unclosed quotes, `+`/`!` channel prefixes, colon rejection, serde round-trips
