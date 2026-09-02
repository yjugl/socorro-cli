# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### New Features

- **`--annotations` flag for `crash`**: Adds a section of crash annotations to
  compact and markdown output — the `async_shutdown_timeout` blocker list (with
  each blocking condition's `file:line` and state), `shutdown_progress`,
  `shutdown_reason`, `xpcom_spin_event_loop_stack`, `app_notes`,
  `last_error_value`, `crash_inconsistencies`, `topmost_filenames`,
  `modules_in_stack` and `proto_signature`. Absent fields are omitted; a crash
  with none of them prints `annotations: (none)`. The flag is opt-in because it
  roughly doubles the output (2,665 to 4,834 bytes on
  `b98bbb81-3ff6-4825-991f-6a0b30260901`), and it is silently ignored with
  `--full` or `--format json`, which already contain every annotation.
- **Crash type line in default `crash` output**: Compact output gains one
  always-on line combining report type, process type, uptime, thread count and
  a `startup` marker for startup crashes, e.g.
  `type: hang | parent | uptime 2175s | 64 threads`. Markdown gains the
  equivalent bullets under `## Details`. This information changes how the rest
  of the report reads and costs 48 bytes (2,617 to 2,665) on the crash above.
- **`--all-threads` groups threads that share a stack.** Threads whose
  displayed frames are identical are folded into one block whose header names
  every member (`stack[8 threads: 21:TaskController #0, 22:TaskController #1,
  ...]`), and a count line above the first stack reports how many threads exist
  against how many distinct stacks are shown (`threads: 64 total, 28 distinct
  stacks shown`), so folding is never silent. Markdown gains the equivalent: a
  total/distinct line under `## All Threads`, a `+ N identical` heading suffix,
  and an `Identical stacks:` line. The crashing thread is never folded and never
  accepts members, so its `[CRASHING]` marker stays unambiguous. Grouping
  compares the displayed frames rather than the full ones, so `--depth` changes
  the grouping: on `b98bbb81-3ff6-4825-991f-6a0b30260901` (64 threads) the
  default reports 28 distinct stacks and `--depth 10` reports 30.

### Fixes

- **`--full` and `--format json` no longer drop most of the API response.**
  Both re-serialized the internal `ProcessedCrash` struct, which declared only
  16 of the 85 top-level keys the `/ProcessedCrash/` endpoint returns — so 69
  keys were silently discarded, including `async_shutdown_timeout`, `app_notes`,
  `proto_signature`, `thread_count`, `uptime` and `telemetry_environment`. JSON
  output is now a verbatim passthrough of the response body, pretty-printed.
  Two caveats: the key set is per-crash rather than a fixed schema (85 keys for
  a Windows crash, 81 and 77 for two Linux ones), and key order is alphabetical
  rather than the server's, because `serde_json` is built without its
  `preserve_order` feature. No key is lost.

### Behavior Changes

- **`--all-threads` now defaults to `--depth 5` instead of 10.** With one stack
  per thread the per-thread cost is multiplied by the thread count, and the
  result overran the ~30–40 KB tool-output cap of the LLM agents this tool
  exists to serve — so the tail of the thread list was silently invisible to
  them. On `b98bbb81-3ff6-4825-991f-6a0b30260901` the lower depth and the
  grouping above together took compact `--all-threads` from **80,736 bytes and
  64 stack blocks to 17,545 bytes and 28**, and markdown from 80,361 to 18,114.
  An explicit `--depth` always wins, so `--all-threads --depth 10` is still
  available (41,234 bytes, 30 distinct stacks) and `--all-threads --depth 1` is
  now 2,804 bytes rather than 5,062. Default `crash` output, without
  `--all-threads`, is byte-identical at 2,665. Because the default is now
  computed rather than declared, `crash --help` no longer prints
  `[default: 10]` for `--depth`; both defaults are stated in the flag's help
  text.
- **`--modules third-party` on a non-Windows crash no longer fails when the
  output is JSON.** Combined with `--full` or `--format json` it previously
  exited 1 with `UnsupportedOption`; it now exits 0 and emits JSON. The OS
  check reads a typed field that the raw passthrough does not populate, and
  `--modules` is meaningless for JSON output in any case. Compact and markdown
  output still report the error as before.

### Internal

- Added `src/models/annotations.rs`: `AsyncShutdownTimeout` (a `Parsed`/`Raw`
  enum whose `parse()` never fails, falling back to the verbatim string for a
  malformed or unexpectedly-shaped payload), `AsyncShutdownTimeoutData`,
  `ShutdownCondition`, and two lenient scalar deserializers that tolerate a
  number arriving as a string rather than failing the whole crash fetch.
- Added regression tests for the invariant that JSON crash output skips the API
  token, covering the full input matrix of `should_use_auth()`. The invariant
  previously had no test coverage, and the raw passthrough makes it the only
  thing keeping protected fields out of JSON output.
- Removed the dead struct-serializing `json::format_crash` (a `pub` item
  removed from the library crate).
- `commands::crash::execute` now takes a `CrashParams` struct instead of eight
  positional arguments, mirroring `search::execute(client, params, format)`;
  `#[allow(clippy::too_many_arguments)]` is gone. `format` stays a separate
  argument because `--format` is a global option rather than a `crash` flag.
- Added `ThreadRef` and `ThreadSummary::identical_threads` to
  `src/models/processed_crash.rs`, and derived `PartialEq, Eq` on `StackFrame`
  so truncated frame lists can be compared.

## [0.6.0] - 2026-04-22

### New Features

- **`--modules third-party` mode**: New mode for `crash --modules` that uses
  the Authenticode `cert_subject` field to show only modules not signed by
  Mozilla or Microsoft. Useful for identifying injected DLLs (antivirus, DRM,
  printer drivers, etc.) in Windows crash reports. Windows only — returns an
  error for non-Windows crashes since `cert_subject` is not available on
  Linux/macOS.

### Internal

- Switched `Cargo.toml` from `exclude` to `include` for packaging — the
  published crate tarball now uses an explicit allowlist of shipped files.
- Fixed Rust 1.95.0 clippy warnings.
- Bumped `clap`, `moz-cli-version-check`, `sha1`, and `tempfile`.

## [0.5.2] - 2026-03-13

### Fixes

- Fixed release CI failing on Windows: `cargo-about` rejects shell output
  redirection (`>`) in PowerShell; switched to its built-in `-o` flag.

## [0.5.1] - 2026-03-13

### Improvements

- Release archives now include a `THIRD-PARTY-LICENSES.txt` file with
  attribution for all third-party dependencies (generated by `cargo-about`).
- Version update notices now appear even when running `--help` or `--version`
  (previously, `Cli::parse()` called `process::exit` before `print_warning`
  could run; now uses `try_parse()` to regain control).
- README now documents the `bugs` command, `--modules` flag, date range flags
  (`--date`, `--days`, `--from`/`--to`), `--proto-signature`, and includes
  more usage examples and workflow recipes.

### Internal

- Added CHANGELOG.md and wired it into the GitHub release workflow for
  automatic release notes.
- Added license compatibility check to the CLAUDE.md change checklist.

## [0.5.0] - 2026-03-05

### New Features

- **`bugs` command**: Look up Bugzilla bugs associated with crash signatures,
  or find signatures associated with specific bug IDs. Queries Socorro's
  `/api/Bugs/` and `/api/SignaturesByBugs/` endpoints.
  - `socorro-cli bugs --signature "OOM | small"` — find bugs for a signature
  - `socorro-cli bugs --bug-id 1234567` — find signatures for a bug
  - Both flags are repeatable; results are grouped by bug ID. Compact, JSON,
    and Markdown output formats are supported.

### New Platforms

- Linux ARM64 (`aarch64-unknown-linux-gnu`)
- Linux musl (`x86_64-unknown-linux-musl`) — statically linked, no system
  keychain support (use `SOCORRO_API_TOKEN_PATH` for auth)
- Windows ARM64 (`aarch64-pc-windows-msvc`)

### CI & Build Improvements

- Clippy and tests now run on all three OSes (Linux, Windows, macOS) instead
  of Linux only.
- Replaced deprecated `actions/create-release@v1` and
  `upload-release-asset@v1` with artifact-based release flow using
  `softprops/action-gh-release@v2`.
- Release is only created after all platform builds succeed.
- Upgraded to `actions/cache@v4`; install `cross` from git instead of
  crates.io.
- Dropped version from release archive filenames for stable `cargo binstall`
  URLs.

### Internal

- Auth module refactored with conditional compilation: keychain support is
  available on Windows and macOS unconditionally, while Linux requires the
  `secret-service` feature (D-Bus). Builds without keychain support (e.g.,
  musl) show a clear message directing users to `SOCORRO_API_TOKEN_PATH`.

## [0.4.0] - 2026-03-04

### New Features

- **`--modules` flag for `crash` command**: Show loaded module debug info
  (`debug_file`, `debug_id`, `code_id`, `version`) alongside crash output. Three
  modes: `stack` (default, only modules appearing in displayed frames), `full`
  (all loaded modules), `none` (omit section). Useful for invoking symdis
  without needing `--full`.
- **Example crash ping IDs in aggregation output**: Each `crash-pings`
  aggregation bucket now shows up to 3 example crash ping IDs, so you can use
  them directly with `--stack` without visiting crash-pings.mozilla.org.
- **Crash date in search compact output**: Search results now include the
  crash timestamp, so agents can see timing without fetching each crash
  individually.

### Improvements

- Documented additional search facets: `reason`, `address`, `cpu_info`,
  `cpu_count`, `uptime` (these already worked but weren't listed in `--help`).
- Documented that `search` and `crash-pings` use different flag names and
  field values.
- Added warning about merging stdout and stderr (version check output can
  corrupt JSON).

### Internal

- Switched to Rust edition 2024.
- Bumped `reqwest` from 0.12 to 0.13 (`native-tls` to `rustls`).

## [0.3.0] - 2026-02-25

### Bug Fixes

- Fix `--signature` doing word-level match instead of exact match (#1).
  Search filters on string fields now correctly default to exact match.
  Use `~` prefix for contains match (e.g. `--signature "~AudioDecoder"`).
- Fix `--facet build_id` failing due to API returning integer terms.

### New Features

- Add `--date`, `--from`, `--to` date flags to `search` and `crash-pings`
  commands (#2). Both commands now support flexible date ranges with inclusive
  bounds. `crash-pings` supports multi-day queries with per-day caching.
- Add `--proto-signature` filter to `search` command.

## [0.2.1] - 2026-02-23

### Fixes

- Fix `cargo binstall` on Windows by adding zip format override.

## [0.2.0] - 2026-02-20

### New Commands

- **`correlations`**: Show attributes that are statistically over-represented
  in crashes with a given signature compared to the overall crash population.
  Data comes from a pre-computed CDN (no API token needed).
- **`crash-pings`**: Query Firefox opt-out crash ping telemetry from
  crash-pings.mozilla.org (~1.7M/day vs ~40K/day for opt-in Socorro reports).
  Supports filtering by channel, OS, process type, version, signature, and
  architecture. Downloaded data is cached locally. Can also fetch symbolicated
  stacks for individual crash pings.
- **`auth`**: Manage API tokens via `auth login`, `auth logout`, and
  `auth status`. Tokens are stored in the OS keychain (macOS Keychain, Windows
  Credential Manager, Linux Secret Service), keeping them hidden from AI
  agents. Falls back to `SOCORRO_API_TOKEN_PATH` for CI/headless environments.

### New Search Filters

- `--cpu-arch` — Filter by CPU architecture (amd64, x86, arm64, arm)
- `--channel` — Filter by release channel (release, beta, nightly, esr,
  aurora, default)
- `--platform-version` — Filter by OS version string (supports `~` prefix for
  contains match)
- `--process-type` — Filter by process type (parent, content, gpu, rdd,
  utility, socket, gmplugin, plugin)
- `--facets-size` — Control how many facet buckets are returned (e.g., top N
  signatures)

### Improved Search Output

- Search results now show full UUIDs, build ID, release channel, and platform
  version.
- `--limit` defaults to 0 when `--facet` is used (show only aggregations).

### Security

- JSON crash output (`--full` or `--format json`) now skips the API token so
  the server strips all protected fields server-side — defense-in-depth against
  accidental `view_pii` token permissions.

### Other

- Added `--version` / `-V` flag.
- Comprehensive `--help` text with examples for all commands.
- 96 unit tests (up from 0).
- Added MPL 2.0 license file and source headers.
- Added CONTRIBUTING.md and Data and Privacy section to README.

### Breaking Changes

- Removed unimplemented `--modules` flag from the `crash` command.

## [0.1.1] - 2026-02-06

Initial release on crates.io.

- **`crash` command**: Fetch processed crash details by ID or full Socorro URL.
  Compact (token-optimized), JSON, and Markdown output formats. `--depth` to
  control stack trace depth, `--full` for complete JSON dump, `--all-threads`
  for multi-thread analysis (deadlock debugging).
- **`search` command**: Query Socorro SuperSearch with filters (`--signature`,
  `--product`, `--version`, `--platform`, `--days`, `--sort`, `--limit`) and
  facet aggregations (`--facet`).
- Cross-platform binaries for Linux x86_64, macOS (x86_64 + ARM64), and
  Windows x86_64. Installable via `cargo install` or `cargo binstall`.
