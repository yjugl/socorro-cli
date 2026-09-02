# socorro-cli

A Rust CLI tool for querying Mozilla's Socorro crash reporting system, optimized for LLM coding agents.

If you're a human user, you probably want
[crashstats-tools](https://github.com/mozilla-services/crashstats-tools)
instead. It's the official Python CLI maintained by Mozilla with more features
for interactive use.

| Feature | socorro-cli | crashstats-tools |
|---------|-------------|------------------|
| **Target audience** | AI agents | Humans |
| **Output format** | Token-optimized (compact) | Human-readable tables |
| **Token security** | Keychain storage (hidden from AI) | Environment variable |
| **Query interface** | Curated CLI options | Arbitrary Super Search fields |
| **Download raw data** | No | Yes (raw crashes, minidumps) |
| **Reprocess crashes** | No | Yes |
| **Super Search URL** | No | Yes (copy-paste from web UI) |

socorro-cli exists because AI agents benefit from:
- Compact output that minimizes token usage
- Secure token storage that prevents the AI from reading credentials
- Simplified options that reduce prompt complexity

## Installation

Pre-built binaries (fastest):

```bash
cargo binstall socorro-cli
```

From source:

```bash
cargo install socorro-cli
```

Or clone and build:

```bash
git clone https://github.com/yjugl/socorro-cli.git
cd socorro-cli
cargo install --path .
```

## Configuration

### API Token

For higher rate limits, API tokens can be used. Humans can create an API token
at https://crash-stats.mozilla.org/api/tokens/ (requires login). Tokens for use
by socorro-cli must be created **without any permission attached to them**,
which still provides rate limit benefits (and only that).

Whenever possible, tokens should not be directly shared with an AI agent nor
stored in a location that's easily accessible to an AI agent. We recommend
using:

```bash
# Store token securely (for humans, prompts for token, input is hidden)
socorro-cli auth login

# Check if a token is stored (for humans or AI agents)
socorro-cli auth status

# Remove stored token (for humans)
socorro-cli auth logout
```

In that case, the token is stored in the operating system's secure credential
storage:
- **macOS**: Keychain
- **Windows**: Credential Manager
- **Linux**: Secret Service (GNOME Keyring, KWallet, etc.)

### CI/Headless Environments

Some environments lack a system keychain (Docker containers, CI systems like
TaskCluster, SSH sessions, headless servers). For these cases, use the
`SOCORRO_API_TOKEN_PATH` environment variable to point to a file containing
the token:

```bash
# Create token file (outside project directory, restricted permissions)
echo "your_token_here" > ~/.socorro-token
chmod 600 ~/.socorro-token

# Set the environment variable to the file path
export SOCORRO_API_TOKEN_PATH=~/.socorro-token
```

**Security note**: The token file should be stored in a location that AI agents
cannot read. Recommended practices:
- Store outside the project directory (e.g., `~/.socorro-token`)
- Use restrictive file permissions (`chmod 600`)
- Never commit the token file or its path to version control
- Consider using a path outside directories typically allowed for AI agents

The CLI checks the keychain first, falling back to reading from the file
specified by `SOCORRO_API_TOKEN_PATH` only if the keychain is unavailable or
empty.

### Update Check

On each run, socorro-cli checks crates.io for a newer version (cached daily,
5-second timeout). If an update is available, a notice is printed to stderr
after the command output. To disable:

```bash
export MOZTOOLS_UPDATE_CHECK=0
```

### Cache Location

`crash-pings` caches each day of downloaded ping data on disk, so repeated
queries for the same date are instant. By default it lives in a
`socorro-cli` subdirectory of the OS-standard cache location, which is
`~/.cache/socorro-cli/` on Linux.

Set `SOCORRO_CACHE_DIR` to put it somewhere else — a larger disk, or a
scratch directory you can throw away:

```bash
export SOCORRO_CACHE_DIR=/mnt/scratch/socorro-cache
```

The value is used **verbatim** as the cache directory: no `socorro-cli`
component is appended, so the example above writes
`/mnt/scratch/socorro-cache/crash-pings-2026-08-31.json` directly. An unset or
blank value falls back to the default. One day of ping data is on the order of
10 MB, so point it at a directory you do not mind filling.

## Usage

### Crash Command

Fetch details about a specific crash by ID or URL:

```bash
# Using crash ID
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901

# Using full Socorro URL (copy-paste from browser)
socorro-cli crash https://crash-stats.mozilla.org/report/index/b98bbb81-3ff6-4825-991f-6a0b30260901

# Get the API response verbatim as JSON
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --full

# Add crash annotations (shutdown blockers, app notes, proto signature)
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --annotations

# Show every thread, grouping threads that share a stack (17,545 bytes for
# this 64-thread crash, down from 80,736 before grouping and the lower depth)
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --all-threads

# Limit stack trace depth
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --depth 5

# Different output formats
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --format markdown
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --format json
```

### Bugs Command

Look up Bugzilla bugs associated with crash signatures, or find signatures
associated with specific bug IDs:

```bash
# Find bugs for a crash signature
socorro-cli bugs --signature "OOM | small"

# Find bugs for multiple signatures
socorro-cli bugs --signature "OOM | small" --signature "OOM | large"

# Find signatures associated with a Bugzilla bug
socorro-cli bugs --bug-id 1234567

# Look up multiple bugs at once
socorro-cli bugs --bug-id 1234567 --bug-id 9876543
```

### Crash Pings Command

Query Firefox crash pings — opt-out telemetry that represents the actual crash
experience (~1.7M/day vs ~40K/day for opt-in Socorro reports):

```bash
# Top crash signatures from yesterday's pings
socorro-cli crash-pings

# Specify date or date range
socorro-cli crash-pings --date 2026-02-12
socorro-cli crash-pings --from 2026-02-10 --to 2026-02-12
socorro-cli crash-pings --days 7

# Filter by channel, OS, process type
socorro-cli crash-pings --channel release --os Windows
socorro-cli crash-pings --process main --version 147.0.3

# Filter by signature (exact or contains with ~ prefix)
socorro-cli crash-pings --signature "OOM | small"

# Aggregate by a field instead of signature
socorro-cli crash-pings --signature "OOM | small" --facet os
socorro-cli crash-pings --facet process

# Fetch symbolicated stack for a specific crash ping
socorro-cli crash-pings --stack b343be53-8ec1-4849-98eb-ca6739a45645 --date 2026-02-23

# Different output formats
socorro-cli crash-pings --format json
socorro-cli crash-pings --format markdown
```

### Correlations Command

Show attributes that are statistically over-represented in crashes with a given
signature compared to the overall crash population:

```bash
# Show correlations for a signature on the release channel (default)
socorro-cli correlations --signature "UiaNode::ProviderInfo::~ProviderInfo"

# Show correlations on the nightly channel
socorro-cli correlations --signature "OOM | small" --channel nightly

# Get raw JSON data
socorro-cli correlations --signature "OOM | small" --format json
```

### Search Command

Search and aggregate crashes with filters:

```bash
# Basic search
socorro-cli search --signature "OOM | small"

# Search with filters
socorro-cli search --product Firefox --platform Windows --days 30 --limit 20

# Search a specific date or date range
socorro-cli search --signature "OOM | small" --date 2026-02-20
socorro-cli search --signature "OOM | small" --from 2026-02-10 --to 2026-02-20

# Aggregate by fields
socorro-cli search --product Firefox --days 7 --facet platform --facet version

# Sort results
socorro-cli search --product Firefox --days 1 --sort -date --limit 10
```

## Output Formats

### Compact (default)
Token-optimized plain text format designed for LLMs:
```
CRASH b98bbb81-3ff6-4825-991f-6a0b30260901
sig: AsyncShutdownTimeout | profile-before-change | ASRouterStorage: flush pending writes,ServiceWorkerRegistrar: Flushing data,ShieldRecipeClient: Cleaning up
reason: EXCEPTION_BREAKPOINT @ 0x00007fffba3d2c6e
type: hang | parent | uptime 2175s | 64 threads
moz_reason: [Parent 36236, Main Thread] ###!!! ABORT: file checkouts\gecko\dom\serviceworkers\ServiceWorkerRegistrar.cpp:1566
abort: xpcom_runtime_abort(###!!! ABORT: file checkouts\gecko\dom\serviceworkers\ServiceWorkerRegistrar.cpp:1566)
product: Firefox 157.0a1 (Windows NT 10.0.26200)
build: 20260831193004
channel: nightly

stack[MainThread]:
  #0 Abort(char const*) @ git:github.com/mozilla-firefox/firefox:xpcom/base/nsDebugImpl.cpp:9b794146973d3e99d273c58f9f6a5cc1dcfc09cb:528
  #1 NS_DebugBreak(unsigned int, char const*, char const*, char const*, int) @ git:github.com/mozilla-firefox/firefox:xpcom/base/nsDebugImpl.cpp:9b794146973d3e99d273c58f9f6a5cc1dcfc09cb:511
  #2 nsDebugImpl::Abort(char const*, int) @ git:github.com/mozilla-firefox/firefox:xpcom/base/nsDebugImpl.cpp:9b794146973d3e99d273c58f9f6a5cc1dcfc09cb:127
  #3 XPTC__InvokebyIndex() @ /builds/worker/workspace/obj-build/toolkit/library/build/Z:/builds/worker/checkouts/gecko/xpcom/reflect/xptcall/md/win32/xptcinvoke_asm_x86_64.asm:97
  ...

modules:
  xul.dll 157.0.0.404 | xul.pdb | 87B0A0D5FAAC4E194C4C44205044422E1 | 6a960f5cacf2000
```

### JSON
For the `crash` command, `--full` and `--format json` print the
`/ProcessedCrash/` API response **verbatim**, pretty-printed — not a filtered
subset of it. Two things not to assume about the result:

- **The set of keys is per-crash, not a fixed schema.** The Windows crash above
  returns 85 top-level keys; two Linux nightly crashes returned 81 and 77. Do
  not hard-code a key list — test for the keys you need.
- **Key order is alphabetical**, not the order the server sent it in, because
  `serde_json` is built without its `preserve_order` feature (its map is a
  `BTreeMap`). No key is lost, only reordered.

### Markdown
Formatted output for documentation and chat interfaces.

## Options

### Global Options
- `--format <FORMAT>`: Output format (compact, json, markdown) [default: compact]
- `--version`/`-V`: Print version

### Crash Options
- `--depth <N>`: Stack trace depth [default: 10, or 5 with `--all-threads`]
- `--full`: Print the API response verbatim as pretty-printed JSON (forces JSON format)
- `--all-threads`: Show stacks from all threads (useful for diagnosing deadlocks). Threads whose displayed frames are identical are folded into a single block whose header names every member, and the default `--depth` drops from 10 to 5. Together those took the 64-thread crash above from 80,736 bytes to 17,545 — small enough to fit an LLM agent's tool-output budget, which the old output silently overran.
- `--annotations`: Add a crash annotations section — shutdown blockers, app notes, proto signature, and more. Opt-in because it costs extra output (2,665 → 4,834 bytes on the crash above). Silently ignored with `--full` or `--format json`, which already contain every annotation.
- `--modules <MODE>`: Which modules to list: `none`, `stack` (modules in displayed frames), `full` (all loaded modules), `third-party` (Windows only: not signed by Mozilla or Microsoft) [default: stack]

### Bugs Options
- `--signature <SIG>`: Crash signature(s) to look up bugs for (repeatable)
- `--bug-id <ID>`: Bugzilla bug ID(s) to look up signatures for (repeatable)

Note: `--signature` and `--bug-id` are mutually exclusive. At least one must be provided.

### Crash Pings Options

For both `search` and `crash-pings`, every `--date`, `--from`, and `--to`
value must be an exact, valid `YYYY-MM-DD` date. Malformed values produce a
graceful command-line usage error before any query or cache access. Date ranges
remain inclusive; `search --from` defaults its end to today, while
`crash-pings --from` defaults its end to yesterday.

- `--date <DATE>`: Date to query (YYYY-MM-DD) [default: yesterday UTC]
- `--days <N>`: Query the last N days (ending at yesterday)
- `--from <DATE>`: Start of date range, inclusive (YYYY-MM-DD)
- `--to <DATE>`: End of date range, inclusive (YYYY-MM-DD)
- `--channel <CH>`: Filter by release channel (release, beta, nightly)
- `--os <OS>`: Filter by OS (Windows, Linux, Mac, Android)
- `--process <PROC>`: Filter by process type (main, content, gpu, rdd, utility, socket, gmplugin)
- `--version <VER>`: Filter by product version
- `--signature <SIG>`: Filter by crash signature (use ~ prefix for contains match)
- `--arch <ARCH>`: Filter by CPU architecture (x86_64, aarch64, x86, arm)
- `--facet <FIELD>`: Aggregate by field [default: signature]
- `--limit <N>`: Number of top entries to show [default: 10]
- `--stack <ID>`: Fetch symbolicated stack for a specific crash ping

### Search Options

All search filters default to exact match. `--signature`, `--proto-signature`, `--platform-version`, and `--process-type` also support [Super Search operator prefixes](https://crash-stats.mozilla.org/documentation/supersearch/) like `~` for contains match.

- `--signature <SIG>`: Filter by crash signature
- `--proto-signature <SIG>`: Filter by proto signature (raw unsymbolicated signature)
- `--product <PROD>`: Filter by product [default: Firefox]
- `--version <VER>`: Filter by version
- `--platform <PLAT>`: Filter by platform (Windows, Linux, Mac OS X, Android)
- `--cpu-arch <ARCH>`: Filter by CPU architecture (amd64, x86, arm64, arm)
- `--channel <CH>`: Filter by release channel (release, beta, nightly, esr, aurora, default)
- `--platform-version <VER>`: Filter by OS version string (e.g., "10.0.19045")
- `--process-type <TYPE>`: Filter by process type (parent, content, gpu, rdd, utility, socket, gmplugin, plugin)
- `--date <DATE>`: Single date to search (YYYY-MM-DD)
- `--days <N>`: Search crashes from last N days [default: 7]
- `--from <DATE>`: Start of date range, inclusive (YYYY-MM-DD)
- `--to <DATE>`: End of date range, inclusive (YYYY-MM-DD), defaults to today if only --from given
- `--limit <N>`: Maximum individual crash results to return [default: 10, or 0 when --facet is used]
- `--facet <FIELD>`: Aggregate by field (can be repeated)
- `--facets-size <N>`: Number of facet buckets to return [default: 50]
- `--sort <FIELD>`: Sort field [default: -date]

### Correlations Options
- `--signature <SIG>`: Crash signature (exact match, required)
- `--channel <CH>`: Release channel (release, beta, nightly, esr) [default: release]

## Examples

### Basic Crash Investigation

```bash
# Quick crash lookup (compact format, default)
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901

# Output:
# CRASH b98bbb81-3ff6-4825-991f-6a0b30260901
# sig: AsyncShutdownTimeout | profile-before-change | ASRouterStorage: flush pending writes,ServiceWorkerRegistrar: Flushing data,ShieldRecipeClient: Cleaning up
# reason: EXCEPTION_BREAKPOINT @ 0x00007fffba3d2c6e
# type: hang | parent | uptime 2175s | 64 threads
# moz_reason: [Parent 36236, Main Thread] ###!!! ABORT: file checkouts\gecko\dom\serviceworkers\ServiceWorkerRegistrar.cpp:1566
# abort: xpcom_runtime_abort(###!!! ABORT: file checkouts\gecko\dom\serviceworkers\ServiceWorkerRegistrar.cpp:1566)
# product: Firefox 157.0a1 (Windows NT 10.0.26200)
# build: 20260831193004
# channel: nightly
#
# stack[MainThread]:
#   #0 Abort(char const*) @ git:github.com/mozilla-firefox/firefox:xpcom/base/nsDebugImpl.cpp:9b794146973d3e99d273c58f9f6a5cc1dcfc09cb:528
#   #1 NS_DebugBreak(unsigned int, char const*, char const*, char const*, int) @ git:github.com/mozilla-firefox/firefox:xpcom/base/nsDebugImpl.cpp:9b794146973d3e99d273c58f9f6a5cc1dcfc09cb:511
#   #2 nsDebugImpl::Abort(char const*, int) @ git:github.com/mozilla-firefox/firefox:xpcom/base/nsDebugImpl.cpp:9b794146973d3e99d273c58f9f6a5cc1dcfc09cb:127
#   #3 XPTC__InvokebyIndex() @ /builds/worker/workspace/obj-build/toolkit/library/build/Z:/builds/worker/checkouts/gecko/xpcom/reflect/xptcall/md/win32/xptcinvoke_asm_x86_64.asm:97
#   ...
#
# modules:
#   xul.dll 157.0.0.404 | xul.pdb | 87B0A0D5FAAC4E194C4C44205044422E1 | 6a960f5cacf2000

# Copy-paste URL directly from browser
socorro-cli crash https://crash-stats.mozilla.org/report/index/b98bbb81-3ff6-4825-991f-6a0b30260901

# Show only top 3 frames for quick overview
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --depth 3
```

### Deadlock and Multi-threading Issues

```bash
# Show all thread stacks (useful for diagnosing deadlocks, race conditions).
# Threads whose displayed frames are identical are folded into one block, and
# --all-threads lowers the default --depth from 10 to 5: 17,545 bytes with 28
# distinct stacks for this 64-thread crash, where the two changes together
# replaced 80,736 bytes of one-block-per-thread output.
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --all-threads

# The same crash at --depth 2 is 3,933 bytes and 14 distinct stacks. Output,
# with one group's member list and the last nine stacks elided:
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --all-threads --depth 2
# threads: 64 total, 14 distinct stacks shown
#
# stack[thread 0:MainThread [CRASHING]]:
#   #0 Abort(char const*) @ git:github.com/mozilla-firefox/firefox:xpcom/base/nsDebugImpl.cpp:9b794146973d3e99d273c58f9f6a5cc1dcfc09cb:528
#   #1 NS_DebugBreak(unsigned int, char const*, char const*, char const*, int) @ git:github.com/mozilla-firefox/firefox:xpcom/base/nsDebugImpl.cpp:9b794146973d3e99d273c58f9f6a5cc1dcfc09cb:511
#
# stack[2 threads: 1:BrokerEvent, 5:IPC I/O Parent]:
#   #0 NtRemoveIoCompletion
#   #1 GetQueuedCompletionStatus
#
# stack[42 threads: 2:COM MTA, 4:glean.dispatcher, 8:IPDL Background, 10:HTML5 Parser, ...]:
#   #0 ZwWaitForAlertByThreadId
#   #1 RtlWaitOnAddress
#
# stack[thread 3:Breakpad ExceptionHandler]:
#   #0 ZwGetContextThread
#   #1 0xfffffffffffffffe
#
# stack[3 threads: 6:Timer, 40:unknown, 58:unknown]:
#   #0 NtWaitForMultipleObjects
#   #1 WaitForMultipleObjectsEx
#
# ...and nine more distinct stacks

# The count line says how many threads exist and how many distinct stacks are
# shown, so folding is never silent. A group header names every member on one
# unwrapped line, so no thread disappears. The crashing thread is never folded
# into a group and never accepts members, so its [CRASHING] marker stays
# unambiguous. Grouping compares the *displayed* frames, so raising --depth
# splits groups that agree only on their first few frames: --depth 10 on this
# crash reports 30 distinct stacks instead of 28, for 41,234 bytes.

# All threads with minimal depth for overview (2,804 bytes for 64 threads)
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --all-threads --depth 1
```

### Crash Annotations

`--annotations` adds a section of crash annotations. It is opt-in because it
costs extra output — on this crash the compact form grows from 2,665 to 4,834
bytes — so it is worth reaching for when the default output does not explain
the crash, shutdown hangs being the obvious case.

```bash
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --annotations

# The default output, plus (values truncated here for brevity):
# annotations:
#   shutdown: phase profile-before-change, 3 conditions
#     - ServiceWorkerRegistrar: Flushing data
#       ..\..\..\..\checkouts\gecko\dom\serviceworkers\ServiceWorkerRegistrar.cpp:1566
#       saveDataRunnableDispatched=false shuttingDown=false
#     - ASRouterStorage: flush pending writes
#       resource:///modules/asrouter/ASRouterDefaultConfig.sys.mjs:50
#       pending=1
#     - ShieldRecipeClient: Cleaning up
#       resource://normandy/lib/CleanupManager.sys.mjs:39
#       (none)
#   shutdown_progress: profile-before-change
#   shutdown_reason: AppClose
#   spin_event_loop: default: AsyncShutdown Spinner for profile-before-change
#   app_notes: -L1000-W0000100-T1) DWrite? DWrite+ WR! WR+ xpcom_runtime_abort(###!!! ABORT: file checkouts\gecko\dom\serviceworkers\ServiceWorkerRegistrar.cpp:1566)
#   last_error: ERROR_SUCCESS
#   topmost_filenames: git:github.com/mozilla-firefox/firefox:mfbt/Assertions.h:9b794146973d3e99d273c58f9f6a5cc1dcfc09cb
#   modules_in_stack: firefox.exe/77DFC624CE9E472E4C4C44205044422E1;kernel32.dll/1BFECEF3ECC283476A31E4461A4AD4F61;...
#   proto_signature: MOZ_Crash | Abort | NS_PrintStackTrace | NS_DebugBreak | nsDebugImpl::Abort | XPTC__InvokebyIndex | ...
```

The `shutdown:` entry comes from the `async_shutdown_timeout` annotation, which
the API delivers as a JSON document embedded in a JSON string. socorro-cli
parses it into the shutdown phase plus one entry per blocking condition, each
with its `file:line` and state; if it does not parse, the raw string is printed
verbatim rather than dropped.

Fields are printed in a fixed order and absent ones are omitted:
`shutdown`, `shutdown_progress`, `shutdown_reason`, `spin_event_loop`,
`app_notes`, `last_error`, `crash_inconsistencies`, `topmost_filenames`,
`modules_in_stack`, `proto_signature`. When a crash has none of them the
section reads `annotations: (none)`.

`--annotations` is silently ignored (not an error) with `--full` or
`--format json`, because those already contain every annotation the API
returned.

### Output Formats

```bash
# Markdown format for documentation or bug reports
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --format markdown

# JSON for programmatic processing
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --format json | jq '.signature'

# The API response verbatim, pretty-printed (85 keys for this crash)
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --full
```

### Search and Aggregation

```bash
# Find recent crashes with specific signature
socorro-cli search --signature "mozilla::gmp::GMPLoader::Load" --days 30

# Output:
# FOUND 19785 crashes
#
# abc12345-aab0-4a25-8c78-4e0070260210 | Firefox 148.0 | Windows NT 10.0.26100 | release | 20260210191108 | mozilla::gmp::GMPLoader::Load
# def67890-d5e6-4427-8ecb-be9f00260210 | Firefox 148.0 | Windows NT 10.0.19045 | release | 20260210191108 | mozilla::gmp::GMPLoader::Load
# ...

# Aggregate crashes by platform and version (only aggregations shown)
socorro-cli search --product Firefox --days 7 --facet platform --facet version

# Output:
# FOUND 69146 crashes
#
# AGGREGATIONS:
#
# version:
#   146.0.1 (407)
#   147.0.1 (179)
#   ...
#
# platform:
#   Windows NT (45000)
#   Linux (12000)
#   ...

# Show 5 individual crashes alongside aggregations
socorro-cli search --product Firefox --days 7 --facet platform --facet version --limit 5

# Find crashes on specific platform and version
socorro-cli search --product Firefox --platform Windows --version 147.0.1 --days 14

# Top 20 crash signatures by volume
socorro-cli search --product Firefox --days 7 --facet signature --facets-size 20

# Recent Android crashes
socorro-cli search --product Fenix --platform Android --days 3 --limit 20
```

### Bug Lookup

```bash
# Find bugs associated with a crash signature
socorro-cli bugs --signature "OOM | small"

# Output:
# Bug 1234567 — https://bugzilla.mozilla.org/show_bug.cgi?id=1234567
#   OOM | small
#   OOM | large
#
# Bug 9876543 — https://bugzilla.mozilla.org/show_bug.cgi?id=9876543
#   OOM | small

# Find signatures associated with a specific bug
socorro-cli bugs --bug-id 1234567
```

### Common Workflows

```bash
# Investigate a crash from triage
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --depth 15 --format markdown > crash-analysis.md

# Quick signature search to find related crashes
socorro-cli search --signature "~SpinEventLoopUntil" --days 30 --limit 10

# Check if a crash affects multiple versions
socorro-cli search --signature "OOM | small" --facet version --days 30

# Check if there are existing Bugzilla bugs for a crash
socorro-cli bugs --signature "OOM | small"

# Deadlock investigation workflow
# 1. Get crash with all threads (41 threads, 20 distinct stacks, 12,333 bytes;
#    --depth 10 would buy 2 more distinct stacks for 27,320 bytes, so start here
#    and only raise --depth if a group's members turn out to diverge below it)
socorro-cli crash b7c998c8-d033-4cc7-a1fe-ce4240260224 --all-threads > deadlock-stacks.txt
# 2. Review all thread stacks to identify lock holders and waiters. Threads
#    sharing a stack are folded into one block that names every member, so the
#    idle pool threads collapse and the distinctive stacks stand out.

# Shutdown hang investigation workflow
# 1. Read the annotations to see which components blocked shutdown, and in which phase
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --annotations
# 2. Each shutdown condition names a file:line — read that code to see what it waits on
# 3. Confirm the blocker on the main thread's stack
socorro-cli crash b98bbb81-3ff6-4825-991f-6a0b30260901 --depth 30

# Check crash distribution across platforms
socorro-cli search --signature "OOM | small" --facet platform --days 7
```

## Data and Privacy

socorro-cli processes only **publicly available data** from Mozilla's crash reporting systems:

- **Crash command**: Fetches processed crash data via the [Socorro API](https://crash-stats.mozilla.org/api/). In compact and markdown output the tool's data model (`ProcessedCrash`) only deserializes public fields — signature, product, version, OS, stack traces, and crash metadata — so [protected data](https://crash-stats.mozilla.org/documentation/protected_data_access/) fields (user comments, email addresses, URLs from annotations, exploitability ratings) are not captured even if the API returns them. JSON output (`--full` or `--format json`) is a verbatim passthrough of the API response and so does **not** filter fields itself; instead the API token is intentionally skipped for those two modes, so the server strips all protected fields server-side before they ever reach the tool. This is a defense-in-depth measure against human error (e.g., accidentally creating a token with `view_pii` permission) that prevents raw `json_dump` sub-fields (registers, mac_boot_args, etc.) from leaking through. **The primary safeguard is ensuring your token has no permissions** — always verify at [API Tokens](https://crash-stats.mozilla.org/api/tokens/).
- **Search command**: Requests only public columns (uuid, date, signature, product, version, platform, build_id, release_channel, platform_version).
- **Bugs command**: Queries Socorro's public bug association endpoints, which map Bugzilla bugs to crash signatures.
- **Correlations command**: Fetches pre-computed correlation data from a public CDN, not the Socorro API.
- **Crash pings command**: Fetches opt-out crash ping telemetry from [crash-pings.mozilla.org](https://crash-pings.mozilla.org/), which contains no protected data.

When using socorro-cli — whether manually or through an AI agent — only provide data from **publicly accessible crash report fields** (stack traces, signatures, module lists, release information). Do not pass [protected crash report data](https://crash-stats.mozilla.org/documentation/protected_data_access/) (such as user comments, email addresses, or URLs from crash annotations) to AI tools analyzing crash reports.

For Mozilla's policies on using AI tools in development, see [AI and Coding](https://firefox-source-docs.mozilla.org/contributing/ai-coding.html). For contribution guidelines, see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

This project is licensed under the [Mozilla Public License 2.0](LICENSE).
