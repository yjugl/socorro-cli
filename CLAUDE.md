# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Personal, machine-local notes (not checked in) live in `CLAUDE.local.md`:

@CLAUDE.local.md

## Overview

socorro-cli is a Rust CLI tool for querying Mozilla's Socorro crash reporting system. It's optimized for LLM coding agents with token-efficient output formats. The tool provides six main commands: `crash` (fetch individual crash details), `search` (search and aggregate crashes), `bugs` (look up Bugzilla bugs for crash signatures or vice versa), `correlations` (show over-represented attributes for a signature), `crash-pings` (query opt-out crash ping telemetry from crash-pings.mozilla.org), and `auth` (manage API token storage).

The `crash` command's compact output always includes a `type:` line (report type, process type, uptime, thread count, and `startup` for a startup crash). `--annotations` opts into an additional section of crash annotations — shutdown blockers, app notes, proto signature and more — in compact and markdown output. `--all-threads` shows every thread, folding threads whose displayed frames are identical into a single block that names every member, and lowering the default `--depth` from 10 to 5; the two together took a 64-thread crash from 80,736 to 17,545 bytes. `--full` / `--format json` emit the API response verbatim.

**Example crash IDs in the docs expire.** Socorro drops processed crashes after roughly six months, but the exact cutoff moves and is not something you can compute from the date embedded in the UUID. Do not guess: as of 2026-09-01 an ID from mid-January 2026 had gone, while `b7c998c8-…-260224` — only five weeks younger — still resolved. Before editing or relying on any example invocation, verify each ID by fetching it:

```bash
curl -s -o /dev/null -w "%{http_code}\n" \
  "https://crash-stats.mozilla.org/api/ProcessedCrash/?crash_id=<id>"
```

Replace only the IDs that actually return 404. Note that expired IDs legitimately remain in offline unit-test fixtures (URL parsing, deserialization) across `src/`, where liveness is irrelevant — leave those alone; only documentation examples and live invocations need a working ID.

## Build & Development Commands

```bash
# Build the project
cargo build

# Build optimized release
cargo build --release

# Run locally without installing
cargo run -- crash b98bbb81-3ff6-4825-991f-6a0b30260901
cargo run -- crash b98bbb81-3ff6-4825-991f-6a0b30260901 --modules none
cargo run -- crash b98bbb81-3ff6-4825-991f-6a0b30260901 --modules full
cargo run -- crash b98bbb81-3ff6-4825-991f-6a0b30260901 --annotations
cargo run -- crash 5ec89bc3-404d-4689-a5f3-54fb00260318 --modules third-party
cargo run -- crash b98bbb81-3ff6-4825-991f-6a0b30260901 --all-threads
cargo run -- crash b98bbb81-3ff6-4825-991f-6a0b30260901 --all-threads --depth 2
cargo run -- search --signature "OOM | small"
cargo run -- search --signature "OOM | small" --date 2026-02-20
cargo run -- search --signature "OOM | small" --from 2026-02-10 --to 2026-02-20
cargo run -- bugs --signature "OOM | small"
cargo run -- bugs --bug-id 1234567
cargo run -- correlations --signature "OOM | small"
cargo run -- crash-pings --channel release --os Windows
cargo run -- crash-pings --days 7 --signature "OOM | small"
cargo run -- crash-pings --from 2026-02-10 --to 2026-02-15

# Install locally
cargo install --path .

# Run with specific subcommand
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901
socorro-cli search --signature "OOM | small"

# API token is managed via keychain or token file (see Authentication section)

# Run tests
cargo test

# Format code
cargo fmt

# Run linter
cargo clippy
```

**Important — after every code change, run the full check sequence before committing:**

1. **Update documentation**: Update `--help` text (clap attributes in `src/main.rs`), `README.md`, and this `CLAUDE.md` file to reflect any new or changed commands, flags, or behaviors.
2. **Format**: `cargo fmt`
3. **Lint**: `cargo clippy` — fix any warnings.
4. **Test**: `cargo test` — all tests must pass.
5. **Verify packaging `include` is complete**: After any significant change (new source file outside `src/`, new top-level asset referenced at build time such as a `build.rs`/`include_str!`/`include_bytes!` target, new test/bench/example directory, or renamed top-level file), run `cargo package --allow-dirty` and confirm the verification step compiles successfully against the extracted tarball in `target/package/`. The `include` directive in `Cargo.toml` is a strict allowlist — anything not listed is silently dropped from the published crate, and missing files will only surface as a build failure on the first downstream user. Add new required paths to `include` before publishing.

**License header requirement:** Every new `.rs` source file must include the MPL 2.0 header as the very first lines:

```rust
// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.
```

followed by a blank line before any code. Do not omit this header from any new file.

**License compatibility**: When adding, updating, or enabling a new feature on a dependency, run `cargo tree --format "{p} {l}" --prefix none` and verify that no new license is incompatible with MPL-2.0. In particular, reject GPL/LGPL/AGPL dependencies. All current dependencies are permissive (MIT, Apache-2.0, ISC, BSD-3-Clause, BSL-1.0, Zlib, etc.) — keep it that way. If in doubt, flag the license for review before committing.

## Architecture

### Module Structure

- **src/main.rs**: CLI entry point using `clap` for argument parsing
- **src/lib.rs**: Library re-exports and error types
- **src/auth.rs**: Keychain operations for secure token storage
  - `get_token()`: Retrieves token from keychain, falls back to file at `SOCORRO_API_TOKEN_PATH`
  - `store_token()`: Stores token in system keychain
  - `delete_token()`: Removes token from system keychain
- **src/client.rs**: `SocorroClient` - HTTP client for Socorro API
  - `get_crash()`: Fetches processed crash data by ID
  - `search()`: Queries SuperSearch API with filters
  - `get_bugs()`: Queries Bugs API for bug associations by signature
  - `get_signatures_by_bugs()`: Queries SignaturesByBugs API for signatures by bug ID
  - Automatically retrieves auth token from keychain via `get_auth_header()`
- **src/commands/**: Command implementations
  - **auth.rs**: Handles `auth login/logout/status` subcommands
  - **crash.rs**: Handles crash fetching and output formatting. Takes a `CrashParams` struct (`crash_id`, `depth: Option<usize>`, `full`, `all_threads`, `modules_mode`, `annotations`) rather than positional arguments, mirroring `search::execute(client, params, format)`; `format` stays a separate argument because `--format` is a global CLI option, not a `crash` flag. Also home to `resolve_depth()` (with `DEFAULT_DEPTH = 10` and `DEFAULT_ALL_THREADS_DEPTH = 5`), to `should_use_auth()` — the pure function deciding whether the API token is sent — and to the raw-JSON passthrough path
  - **search.rs**: Handles crash search and aggregation
  - **bugs.rs**: Handles `bugs` command, dispatches to `get_bugs()` or `get_signatures_by_bugs()` based on flags
  - **correlations.rs**: Fetches correlation data from CDN (not Socorro API), computes signature hash, handles CDN HTTP requests
  - **crash_pings.rs**: Fetches crash ping data from crash-pings.mozilla.org, client-side filtering/aggregation, stack trace fetching
- **src/cache.rs**: Generic file cache module using OS cache directory (`dirs::cache_dir()`)
  - `cache_dir()`: Returns/creates the cache directory
  - `read_cached()`: Read cached data by key
  - `write_cache()`: Write data to cache by key
- **src/models/**: Data structures for Socorro API responses
  - **processed_crash.rs**: `ProcessedCrash`, `Thread`, `ThreadSummary`, `ThreadRef`, `CrashSummary` - crash data models. `ThreadSummary` (which derives `Default`) carries `identical_threads: Vec<ThreadRef>`, the threads folded into it by `to_summary()`; `ThreadRef` is a `thread_index` plus an optional `thread_name`. Both are re-exported from `src/models/`. `CrashSummary` includes `modules: Vec<ModuleInfo>` extracted from `json_dump.modules`. Also carries the annotation fields: always-rendered ones (`report_type`, `process_type`, `uptime`, `startup_crash`, `thread_count`) and the opt-in ones behind `--annotations` (`async_shutdown_timeout`, `shutdown_progress`, `shutdown_reason`, `xpcom_spin_event_loop_stack`, `app_notes`, `last_error_value`, `crash_inconsistencies`, `topmost_filenames`, `modules_in_stack`, `proto_signature`)
  - **annotations.rs**: `AsyncShutdownTimeout` (a `Parsed`/`Raw` enum), `AsyncShutdownTimeoutData` (phase + conditions), `ShutdownCondition` (name, `filename`, `line_number`, free-form `state`). Also home to the two lenient scalar deserializers, `deserialize_optional_u64` and `deserialize_optional_bool`, used by `ProcessedCrash`
  - **search.rs**: `SearchResponse`, `SearchParams`, `CrashHit`, `FacetBucket` - search data models. `SearchParams` includes filters: signature, proto_signature, product, version, platform, cpu_arch, release_channel, platform_version, process_type, date_from, date_to, limit, facets, facets_size, sort. `CrashHit` includes build_id, release_channel, and platform_version fields
  - **bugs.rs**: `BugsResponse`, `BugHit`, `BugsSummary`, `BugGroup` - bug association data models. `BugsResponse` is the raw API response; `BugsSummary` groups hits by bug ID with sorted signatures
  - **correlations.rs**: `CorrelationsTotals`, `CorrelationsResponse`, `CorrelationsSummary` - correlation data models
  - **crash_pings.rs**: `CrashPingsResponse`, `CrashPingStackResponse`, `CrashPingsSummary`, `CrashPingStackSummary` - crash ping data models (struct-of-arrays with string deduplication). `CrashPingsSummary` uses `date_from`/`date_to` fields for date range support. `CrashPingsItem` includes `example_ids: Vec<String>` (up to 3 crash ping IDs per bucket, usable with `--stack`)
  - **common.rs**: Shared types like `StackFrame` and `ModuleInfo` (includes `cert_subject` for Authenticode signer and `is_third_party()` method). `StackFrame` derives `PartialEq, Eq` so that truncated frame lists can be compared for thread grouping
- **src/output/**: Output formatters
  - **compact.rs**: Token-optimized plain text (default, LLM-friendly)
  - **json.rs**: Full JSON output
  - **markdown.rs**: Human-readable markdown

### Data Flow

1. CLI parses arguments → creates `SocorroClient` (token retrieved automatically from keychain/file)
2. Command dispatcher calls appropriate command module
3. Command module:
   - For crash: extracts crash ID from URL if needed → resolves the frame depth with `resolve_depth(depth, all_threads)` (an explicit `--depth` always wins, including when it equals the default it replaces; otherwise 5 under `--all-threads` and 10 without) → `client.get_crash()` → for `--full`/`--format json`, print the raw response body verbatim (pretty-printed) and stop; otherwise convert `ProcessedCrash` to `CrashSummary` (including modules from `json_dump.modules`, parsing `async_shutdown_timeout`'s embedded JSON, and — under `--all-threads` only — grouping threads with identical truncated frame lists into `ThreadSummary::identical_threads`, all in `to_summary()`) → format output with the `--modules` mode (none/stack/full/third-party) and the `--annotations` flag
   - For search: resolves date params (`--date`, `--days`, `--from`/`--to`) into `date_from`/`date_to` → builds `SearchParams` → `client.search()` → formats `SearchResponse`
   - For bugs: calls `client.get_bugs()` or `client.get_signatures_by_bugs()` → converts `BugsResponse` to `BugsSummary` (grouped by bug ID) → formats output
   - For correlations: builds reqwest client with gzip → fetches totals + per-signature data from CDN → converts `CorrelationsResponse` to `CorrelationsSummary` → formats output
   - For crash-pings: resolves date params (`--date`, `--days`, `--from`/`--to`) into a date range → builds reqwest client with gzip → fetches each day's ping data from crash-pings.mozilla.org (cached locally, skips 404/202 with warning) → aggregates across all dates → formats `CrashPingsSummary`; or fetches individual stack trace → formats `CrashPingStackSummary`
4. Output formatter generates final text based on selected format

### Key Design Decisions

**Crash ID Extraction**: `crash` command accepts both bare IDs and full Socorro URLs (e.g., `https://crash-stats.mozilla.org/report/index/<uuid>`). The `extract_crash_id()` function extracts the UUID from URLs.

**Two-Stage Model Conversion**: For compact and markdown output, raw API responses are deserialized into `ProcessedCrash`, then converted to `CrashSummary` which contains only display-relevant data at the requested depth. This separation keeps formatting logic simple and avoids processing unused data. JSON output bypasses both stages entirely (see "Raw JSON Passthrough" below), so `ProcessedCrash` is no longer the bottleneck it once was for `--full`.

**Raw JSON Passthrough**: `--full` and `--format json` print the `/ProcessedCrash/` response body verbatim, pretty-printed, rather than re-serializing the `ProcessedCrash` struct. The struct is a *filter* — it declares far fewer fields than the API returns — so re-serializing silently dropped most of the response: originally 16 of the 85 top-level keys survived, losing exactly the ones shutdown-hang investigation needs (`async_shutdown_timeout`, `app_notes`, `proto_signature`, `thread_count`, `uptime`, `telemetry_environment`). Measured on `b98bbb81-3ff6-4825-991f-6a0b30260901`, the passthrough now emits **85 keys / 768,791 bytes**. Two consequences to keep in mind:

- **The key set is per-crash, not a fixed schema.** The reference Windows crash yields 85 keys; two Linux nightly crashes yielded 81 and 77. Never hard-code a key list.
- **Key order is alphabetical**, not the server's order, because `serde_json` is built without its `preserve_order` feature (its `Map` is a `BTreeMap`). No key is lost, only reordered.

Because JSON output no longer goes through the typed model, options that depend on typed fields cannot be enforced on that path: `--modules third-party` on a non-Windows crash exits 0 and emits JSON (the flag is meaningless there) while compact and markdown still error with `UnsupportedOption`.

**`async_shutdown_timeout` Parsing**: The API delivers this annotation as a JSON document embedded in a JSON *string*. `ProcessedCrash` keeps it as a plain `Option<String>` — deliberately, so the field round-trips untouched and the raw passthrough is unaffected — and `to_summary()` calls `AsyncShutdownTimeout::parse()` at the model boundary. `parse()` never fails: it requires a JSON *object* (serde will otherwise happily deserialize the struct from a JSON array by position, turning junk into a plausible-looking parse) and falls back to `AsyncShutdownTimeout::Raw(String)` for anything unexpected, so a malformed annotation is printed verbatim rather than dropped. Note that `AsyncShutdownTimeout` derives only `Debug, Clone`; it has no `Serialize`/`Deserialize` impl of its own.

**Lenient Scalar Deserializers**: `deserialize_optional_u64` and `deserialize_optional_bool` (in `src/models/annotations.rs`) accept a value the API may send as a number, a numeric string, or (for bools) `true`/`false`/`yes`/`no`/`0`/`1` in any case. An unrecognized value yields `None` instead of failing. This matters because these deserializers sit on `ProcessedCrash`: a single oddly-typed annotation would otherwise abort the whole crash fetch rather than degrade one line of output.

**Thread Handling**: Crash data includes multiple threads. The tool identifies the crashing thread via:
1. `crashing_thread` field
2. `crash_info.crashing_thread` field
3. `json_dump.crashing_thread` field

With `--all-threads`, it formats all threads (marking the crashing one), useful for deadlock analysis. See "Thread Stack Grouping" below for how identical stacks are folded.

**Stack Frame Depth**: By default shows 10 frames, or 5 under `--all-threads`, where the per-thread cost is multiplied by the thread count. `resolve_depth(depth: Option<usize>, all_threads: bool)` in `src/commands/crash.rs` picks between `DEFAULT_DEPTH` and `DEFAULT_ALL_THREADS_DEPTH`; an explicit `--depth` always wins, including when it happens to equal the default it replaces. Because the default is computed rather than declared to clap, `crash --help` no longer prints `[default: 10]` for `--depth` — both defaults are stated in the flag's doc text instead.

**Thread Stack Grouping**: Under `--all-threads`, `to_summary()` folds threads whose frame lists are identical into one `ThreadSummary`, recording the others in its `identical_threads: Vec<ThreadRef>`. The representative is the lowest-index member and groups are ordered by lowest member index, so output order is stable and roughly the original thread order. This is what makes `--all-threads` affordable: on `b98bbb81-3ff6-4825-991f-6a0b30260901` (64 threads) compact output went from 80,736 bytes to 17,545 with 28 distinct stacks, comfortably inside the ~30–40 KB tool-output cap of the LLM agents this tool serves — the old output overran it silently, so the tail of the thread list was invisible. Three non-obvious properties:

- **Grouping compares the *truncated* frame list**, not the full one. This is deliberate: the reader only ever sees the truncated list, so two threads that differ solely below the cut would look identical anyway. The consequence is that `--depth` changes the grouping — threads agreeing on their first N frames group at `depth == N` and split at a larger depth. On the crash above, depth 5 reports 28 distinct stacks and depth 10 reports 30 (41,234 bytes).
- **The crashing thread is never folded into a group and never accepts members**, so its `[CRASHING]` marker is unambiguous. A consequence worth knowing: whenever a crashing thread *is* identified, any crash with ≥2 threads has ≥2 distinct entries, so `2 total, 1 distinct` does not occur in normal output. It is not structurally impossible, though — `crashing_thread_idx` is an `Option` fed by the three fallbacks above, and if all three are absent no thread is marked crashing and two identical-stack threads would fold to `2 total, 1 distinct`.
- **The count line's numbers are derived by the formatters from the same `identical_threads` data that produces the member lists** (distinct = `all_threads.len()`, total = the sum of `1 + identical_threads.len()`), *not* from the `thread_count` annotation. That is load-bearing, not incidental: it means the count line cannot over-report what it names. Mutate the grouping so that it folds threads but drops the member records, and the line necessarily reads `28 total, 28 distinct` rather than falsely claiming 64 — it degrades to honest under-reporting instead of a confident lie. Sourcing the total from `thread_count` would have lost that property.

Both formatters report the grouping. Compact prints a count line and a blank line before the first stack block, and gives a group a header naming every member on one unwrapped line:

```
threads: 64 total, 28 distinct stacks shown

stack[thread 0:MainThread [CRASHING]]:
  #0 Abort(char const*) @ ...

stack[5 threads: 11:StyleThread#1, 12:StyleThread#2, 13:StyleThread#3, 14:StyleThread#4, 15:StyleThread#5]:
  #0 ZwWaitForAlertByThreadId
```

Markdown prints a total/distinct line under `## All Threads`, a `+ N identical` suffix on the group's heading, and an `Identical stacks:` line naming only the folded threads:

```
64 threads total, 28 distinct stacks shown.

### Thread 2 (COM MTA) + 16 identical

Identical stacks: 10 (HTML5 Parser), 16 (BHMgr Monitor), ...
```

Both count lines and the compact `type:` line are pluralized — 1 is singular, everything else plural (`threads: 1 total, 1 distinct stack shown`, `1 thread total, 1 distinct stack shown.`, `type: crash | parent | uptime 13s | 1 thread`).

**Compact Format**: Default output format is designed to minimize tokens while preserving essential crash information. Uses abbreviations (sig, moz_reason) and omits field labels when clear from context.

**Always-On Crash Type Line**: Compact output includes a single `type:` line combining `report_type`, `process_type`, `uptime`, `thread_count` and (only when `startup_crash` is true) `startup` — e.g. `type: hang | parent | uptime 2175s | 64 threads`. Markdown renders the same data as four bullets under `## Details`. This is unconditional because the cost is small and the information changes how every other line is read: measured on `b98bbb81-3ff6-4825-991f-6a0b30260901`, compact grew 2,617 → 2,665 bytes (+48) and markdown 2,871 → 2,972 (+101). Absent fields are omitted, so the line shrinks rather than printing placeholders.

**Annotations Are Opt-In**: Everything else annotation-derived sits behind `--annotations`, which costs substantially more: 2,665 → 4,834 bytes on the same crash, most of it `proto_signature` and `modules_in_stack`. Compact prints an `annotations:` header followed by two-space-indented `key: value` lines in a fixed order (`shutdown`, `shutdown_progress`, `shutdown_reason`, `spin_event_loop`, `app_notes`, `last_error`, `crash_inconsistencies`, `topmost_filenames`, `modules_in_stack`, `proto_signature`), omitting absent fields, and prints exactly `annotations: (none)` when none are present. The flag is silently ignored (not an error) with `--full`/`--format json`, which already contain every annotation.

**JSON Crash Output Skips Auth Token**: When `crash` output will be JSON (`--full` or `--format json`), the API token is not sent. Without a token, the server strips all protected fields (registers, mac_boot_args, etc. inside `json_dump`) server-side. This is a defense-in-depth measure against human error (e.g., accidentally creating a token with `view_pii` permission) — the primary safeguard is that users must create tokens with no permissions. Compact/markdown output is safe because `to_summary()` only extracts public sub-fields, so those formats still use the token for higher rate limits.

This invariant is now load-bearing rather than incidental: since JSON output is a raw passthrough, skipping the token is the *only* thing keeping protected fields out of it. The decision is isolated in the pure function `should_use_auth(full: bool, format: OutputFormat) -> bool` in `src/commands/crash.rs`, and a test exercises its full input matrix. Any change to that function or to `get_crash`'s `use_auth` argument is a security-relevant change.

**Facet-aware `--limit` default**: When `--facet` is used, `--limit` defaults to 0 (only aggregations shown). Without `--facet`, it defaults to 10. Users can override with `--limit N` to show individual crash rows alongside aggregations. `--facets-size` controls how many buckets each facet returns (e.g., top N signatures).

**Version Checking**: On startup, `moz-cli-version-check` asynchronously checks for newer releases on crates.io. If a newer version is found, a warning is printed to stderr after the command completes. Environments that merge stderr into stdout (e.g. shell `2>&1` redirects) should either redirect stderr separately or set `MOZTOOLS_UPDATE_CHECK=0` to avoid corrupting JSON output.

**Error Handling**: Uses `thiserror` for structured errors. The `Error` enum variants:
- `Http` — wraps `reqwest::Error` for network/HTTP failures
- `Json` — wraps `serde_json::Error` for deserialization failures
- `NotFound` — 404 responses, with context (crash ID or date)
- `RateLimited` — 429 responses, suggests using an API token
- `ParseError` — parse failures with response preview (first 200 chars)
- `InvalidCrashId` — crash ID contains invalid characters (injection protection)
- `Keyring` — keychain/credential storage errors

### Field Naming Differences: `search` vs `crash-pings`

The `search` and `crash-pings` commands query different data sources (Socorro API vs crash-pings.mozilla.org) that use different naming conventions. **Flag names, accepted values, and facet field names differ between the two commands.** The CLI uses each source's native vocabulary so that filter values match what appears in output. Always check the `--help` for the specific command being used — do not assume flags or values are interchangeable.

## Socorro API Details

**Base URL**: `https://crash-stats.mozilla.org/api`

**Endpoints Used**:
- `/ProcessedCrash/` - fetch individual crash by ID
- `/SuperSearch/` - search/aggregate crashes
- `/Bugs/` - look up Bugzilla bugs for crash signatures (returns related bugs too)
- `/SignaturesByBugs/` - look up crash signatures for Bugzilla bug IDs

**Authentication**: Optional `Auth-Token` header for higher rate limits. Token is retrieved in order:
1. System keychain (via `socorro-cli auth login`)
2. File at path specified by `SOCORRO_API_TOKEN_PATH` environment variable (fallback for CI/headless)

**Security Note**: The API token is stored in the OS keychain and is never printed to output or written to files. This prevents AI agents from accessing the token value while allowing the CLI to use it for authenticated requests.

**CI Fallback**: The `SOCORRO_API_TOKEN_PATH` environment variable points to a file containing the token, for environments without a system keychain (Docker, TaskCluster, headless servers). The file should be stored in a location that AI agents cannot read (e.g., outside the project directory, with restricted permissions like `chmod 600`). Interactive users should use `auth login` instead.

## Testing

Run tests with:
```bash
cargo test
```

The test suite (223 tests) covers:
- **Crash ID extraction**: Bare IDs, full URLs, URLs with trailing slashes
- **ProcessedCrash model**: JSON deserialization, `to_summary()` conversion, crashing thread identification from multiple sources, depth limiting, all-threads mode, module extraction from `json_dump.modules`, annotation field extraction, and the lenient scalar deserializers (`deserialize_optional_u64`/`deserialize_optional_bool` accepting numbers, numeric strings and bool-ish strings, and yielding `None` rather than erroring on junk)
- **Annotations model**: `AsyncShutdownTimeout::parse()` — phase and condition extraction, `filename`/`lineNumber` handling, object vs. string vs. missing `state`, polymorphic `stack` field, zero-condition payloads, and the `Raw` fallback for malformed or unexpectedly-shaped input (including a JSON array, which serde would otherwise deserialize positionally)
- **Search models**: SearchResponse/CrashHit deserialization, facets parsing
- **Bugs models**: Deserialization, `to_summary()` grouping by bug ID, signature sorting, empty response handling
- **Correlations models**: Deserialization, `to_summary()` percentage calculations, `format_item_map()` for item display
- **Crash pings models**: IndexedStrings/NullableIndexedStrings deserialization, accessor methods, filter matching (channel, OS, process, version, signature exact/contains, arch, combined), facet value resolution, stack response deserialization
- **Crash pings command**: Aggregation by signature/OS, filtering, limit, percentage calculations, frame formatting, multi-response aggregation, date range generation
- **Cache module**: Cache directory creation, read/write roundtrip, empty cache handling
- **Thread stack grouping in `to_summary()`**: identical stacks folded under `--all-threads`, the crashing thread never folded and never accepting members, groups ordered by lowest member index, comparison against the *truncated* frame list (so the same threads group at one `--depth` and split at a larger one), and a no-thread-lost accounting check that every thread appears exactly once as a representative or a member
- **Depth resolution**: `resolve_depth()`'s four cases (explicit or default × with or without `--all-threads`), plus the case where an explicit `--depth` equal to the default it replaces still wins
- **Output formatters**: Compact and Markdown formatters for crash (including `--modules` none/stack/full/third-party modes and the always-on `type:` line), search, bugs, correlations, and crash pings output
- **Thread group rendering**: the compact count line (present even when nothing was grouped), the group header being one unwrapped line that lists every member including unnamed ones, a grouped representative that is also the crashing thread, and the markdown equivalents (total/distinct line, `+ N identical` heading suffix, `Identical stacks:` member list, and an ungrouped heading left unchanged)
- **Pluralization**: both count lines and the compact `type:` line in singular and plural form
- **Annotations formatters**: The compact `annotations:` section — fixed field order, omission of absent fields, the `annotations: (none)` empty case, multi-line shutdown-condition rendering, and the markdown equivalent
- **Module filtering**: `is_third_party()` cert_subject classification (Mozilla, Microsoft, third-party, unsigned)
- **Client validation**: Crash ID format validation (rejects invalid characters, potential injection attempts)
- **Auth token file**: Reading from `SOCORRO_API_TOKEN_PATH`, whitespace handling, missing file handling
- **JSON auth invariant**: The full input matrix of `should_use_auth(full, format)`, asserting the token is skipped for `--full` and `--format json` and sent for compact/markdown. This guards the security invariant above, which the raw JSON passthrough makes load-bearing

Note: HTTP-level tests (404, 429, network errors) would require mocking the reqwest client and are not currently implemented.

## Future Improvements

Features inspired by [crashstats-tools](https://github.com/mozilla-services/crashstats-tools) that could benefit AI agents (all use public API, no special permissions required):

1. **`--supersearch-url` parameter**: Accept a Socorro web UI search URL directly, parse supported parameters, and warn about unsupported ones. Allows humans to share search URLs with AI agents.

2. **`--modules-in-stack` filter**: Find crashes where a specific module appears in the stack. Supports wildcards (e.g., `--modules-in-stack='^libgallium_dri.so'`).

3. **`--columns` selection**: Specify which fields to return in search results, reducing token output (e.g., `--columns uuid,signature,build_id`).

4. **Histogram aggregations**: Get crash counts per day broken down by a field (`--histogram-date=product`). Useful for trend analysis.

5. **Cardinality queries**: Count distinct values of a field (`--facet=_cardinality.build_id`). Example: "how many different build IDs have this crash?"

6. **Nested aggregations**: Multi-level drill-downs (`--aggs=product.version.release_channel`) for deeper analysis.

7. **`--crash-report-keys` filter**: Find crashes containing specific annotations that may not be searchable yet. Useful when investigating newly-added Firefox annotations.
