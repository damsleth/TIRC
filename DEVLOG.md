# DEVLOG
- (pending) feat(config,ui): add startup.showWelcomeMessage and startup.welcomeMessage config
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

## QA Report — Full Fix Implementation

Implemented all 25 findings from QA_REPORT.md. Summary of changes:

### CRITICAL (3/3 fixed)
- **#1** Silent `let _ =` channel sends → `send_event()` helper with explicit error handling (tirc-net)
- **#2** SASL credentials in plain `String` → `Zeroizing<String>` with custom serde (tirc-config)
- **#3** No IRC message length validation → 510-byte outbound cap, 8192-byte inbound limit (tirc-net)

### HIGH (6/6 fixed)
- **#4** BufferId fields public → private with `network()`/`name()` accessors + `new()` constructor
- **#5** view_model clones all messages → `Arc<Vec<Message>>` shared ownership
- **#6** InputEditor cursor semantics undocumented → comprehensive doc comment
- **#7** resolve_part_args ambiguous grammar → clean 4-arm match + 7 unit tests
- **#8** Channel detection scattered `starts_with('#')` → centralized `is_channel_name()` (RFC 2812)
- **#9** NAMREPLY strips mode prefixes → preserved in nick list

### MEDIUM (9/9 fixed)
- **#10** AppRuntime struct scaffolding for future extraction
- **#11** Busy-poll main loop → `tokio::select!` with crossterm EventStream
- **#12** UiEngine subsystem extraction: sidebar/nicklist/chat/layout state grouped into structs
- **#13** UserQuit broadcasts to all channels → only relevant channels via membership tracking
- **#14** No rate limiting → token-bucket rate limiter (5 burst, 2/sec refill)
- **#15** Double join/part events → eliminated premature channel events
- **#16** AppState derives Serialize/Deserialize needlessly → removed
- **#17** Channel list linear scan → O(1) HashMap index
- **#18** Dead mouse.rs module → deleted

### LOW (7/7 addressed)
- **#19** Clippy disabled (`all = "allow"`) → `pedantic = warn` with selective allows, 0 warnings
- **#20** Fixed reconnect delay → proportional jitter, cap at attempt 7
- **#21** CTCP SOURCE URL pointed to wrong repo → corrected
- **#22** QA report claimed `macos-notify` feature missing — already exists in tirc-platform/Cargo.toml (no change needed)
- **#23+#25** Unused nucleo-matcher + naive fuzzy_score → proper Pattern/Matcher API with 3 tests
- **#24** Brittle redaction patterns → stricter service-keyword matching (no false positives), all-occurrence handling, PASS/AUTHENTICATE coverage, 16 tests

### Verification
- `cargo build` — clean
- `cargo clippy --workspace` — 0 warnings
- `cargo test --workspace` — 57 tests pass (46 core, 2 config, 8 ui, 1 platform)

### New dependencies
- `zeroize` (workspace) — credential zeroing
- `futures` (tirc-app) — StreamExt for EventStream
- crossterm `event-stream` feature — async terminal events
