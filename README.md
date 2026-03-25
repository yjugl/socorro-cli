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

## Usage

### Crash Command

Fetch details about a specific crash by ID or URL:

```bash
# Using crash ID
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120

# Using full Socorro URL (copy-paste from browser)
socorro-cli crash https://crash-stats.mozilla.org/report/index/247653e8-7a18-4836-97d1-42a720260120

# Get full crash data without omissions
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --full

# Limit stack trace depth
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --depth 5

# Different output formats
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --format markdown
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --format json
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
CRASH 247653e8-7a18-4836-97d1-42a720260120
sig: mozilla::AudioDecoderInputTrack::EnsureTimeStretcher
reason: SIGSEGV / SEGV_MAPERR @ 0x0000000000000000
moz_reason: MOZ_RELEASE_ASSERT(mTimeStretcher->Init())
product: Fenix 147.0.1 (Android 36, SM-S918B 36 (REL))
build: 20260116091309
channel: release

stack[GraphRunner]:
  #0 mozilla::AudioDecoderInputTrack::EnsureTimeStretcher() @ ...AudioDecoderInputTrack.cpp:...:624
  #1 mozilla::AudioDecoderInputTrack::AppendTimeStretchedDataToSegment(...) @ ...AudioDecoderInputTrack.cpp:...:423
```

### JSON
Full structured data for programmatic processing.

### Markdown
Formatted output for documentation and chat interfaces.

## Options

### Global Options
- `--format <FORMAT>`: Output format (compact, json, markdown) [default: compact]
- `--version`/`-V`: Print version

### Crash Options
- `--depth <N>`: Stack trace depth [default: 10]
- `--full`: Output complete crash data without omissions (forces JSON format)
- `--all-threads`: Show stacks from all threads (useful for diagnosing deadlocks)
- `--modules <MODE>`: Which modules to list: `none`, `stack` (modules in displayed frames), `full` (all loaded modules), `third-party` (Windows only: not signed by Mozilla or Microsoft) [default: stack]

### Bugs Options
- `--signature <SIG>`: Crash signature(s) to look up bugs for (repeatable)
- `--bug-id <ID>`: Bugzilla bug ID(s) to look up signatures for (repeatable)

Note: `--signature` and `--bug-id` are mutually exclusive. At least one must be provided.

### Crash Pings Options
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
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120

# Output:
# CRASH 247653e8-7a18-4836-97d1-42a720260120
# sig: mozilla::AudioDecoderInputTrack::EnsureTimeStretcher
# reason: SIGSEGV / SEGV_MAPERR @ 0x0000000000000000
# moz_reason: MOZ_RELEASE_ASSERT(mTimeStretcher->Init())
# product: Fenix 147.0.1 (Android 36, SM-S918B 36 (REL))
# build: 20260116091309
# channel: release
#
# stack[GraphRunner]:
#   #0 mozilla::AudioDecoderInputTrack::EnsureTimeStretcher() @ git:github.com/.../AudioDecoderInputTrack.cpp:...:624
#   #1 mozilla::AudioDecoderInputTrack::AppendTimeStretchedDataToSegment(...) @ git:github.com/.../AudioDecoderInputTrack.cpp:...:423
#   ...

# Copy-paste URL directly from browser
socorro-cli crash https://crash-stats.mozilla.org/report/index/247653e8-7a18-4836-97d1-42a720260120

# Show only top 3 frames for quick overview
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --depth 3
```

### Deadlock and Multi-threading Issues

```bash
# Show all thread stacks (useful for diagnosing deadlocks, race conditions)
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --all-threads --depth 5

# Output shows all threads with the crashing thread marked:
# stack[thread 0:la.firefox:tab7]:
#   #0 ???
#   ...
#
# stack[thread 49:GraphRunner [CRASHING]]:
#   #0 mozilla::AudioDecoderInputTrack::EnsureTimeStretcher() @ ...
#   #1 mozilla::AudioDecoderInputTrack::AppendTimeStretchedDataToSegment(...) @ ...
#   ...
#
# stack[thread 50:MediaDecoderSta]:
#   #0 mozilla::SharedBuffer::Create(...) @ ...
#   ...

# All threads with minimal depth for overview
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --all-threads --depth 2
```

### Output Formats

```bash
# Markdown format for documentation or bug reports
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --format markdown

# JSON for programmatic processing
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --format json | jq '.signature'

# Full JSON dump without any omissions (includes all metadata)
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --full
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
socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --depth 15 --format markdown > crash-analysis.md

# Quick signature search to find related crashes
socorro-cli search --signature "~SpinEventLoopUntil" --days 30 --limit 10

# Check if a crash affects multiple versions
socorro-cli search --signature "OOM | small" --facet version --days 30

# Check if there are existing Bugzilla bugs for a crash
socorro-cli bugs --signature "OOM | small"

# Deadlock investigation workflow
# 1. Get crash with all threads
socorro-cli crash b7c998c8-d033-4cc7-a1fe-ce4240260224 --all-threads --depth 10 > deadlock-stacks.txt
# 2. Review all thread stacks to identify lock holders and waiters

# Check crash distribution across platforms
socorro-cli search --signature "OOM | small" --facet platform --days 7
```

## Data and Privacy

socorro-cli processes only **publicly available data** from Mozilla's crash reporting systems:

- **Crash command**: Fetches processed crash data via the [Socorro API](https://crash-stats.mozilla.org/api/). The tool's data model (`ProcessedCrash`) only deserializes public fields — signature, product, version, OS, stack traces, and crash metadata. [Protected data](https://crash-stats.mozilla.org/documentation/protected_data_access/) fields (user comments, email addresses, URLs from annotations, exploitability ratings) are not captured even if the API returns them. When JSON output is requested (`--full` or `--format json`), the API token is intentionally skipped so the server strips all protected fields server-side — this is a defense-in-depth measure against human error (e.g., accidentally creating a token with `view_pii` permission) that prevents raw `json_dump` sub-fields (registers, mac_boot_args, etc.) from leaking through. **The primary safeguard is ensuring your token has no permissions** — always verify at [API Tokens](https://crash-stats.mozilla.org/api/tokens/).
- **Search command**: Requests only public columns (uuid, date, signature, product, version, platform, build_id, release_channel, platform_version).
- **Bugs command**: Queries Socorro's public bug association endpoints, which map Bugzilla bugs to crash signatures.
- **Correlations command**: Fetches pre-computed correlation data from a public CDN, not the Socorro API.
- **Crash pings command**: Fetches opt-out crash ping telemetry from [crash-pings.mozilla.org](https://crash-pings.mozilla.org/), which contains no protected data.

When using socorro-cli — whether manually or through an AI agent — only provide data from **publicly accessible crash report fields** (stack traces, signatures, module lists, release information). Do not pass [protected crash report data](https://crash-stats.mozilla.org/documentation/protected_data_access/) (such as user comments, email addresses, or URLs from crash annotations) to AI tools analyzing crash reports.

For Mozilla's policies on using AI tools in development, see [AI and Coding](https://firefox-source-docs.mozilla.org/contributing/ai-coding.html). For contribution guidelines, see [CONTRIBUTING.md](CONTRIBUTING.md).

## License

This project is licensed under the [Mozilla Public License 2.0](LICENSE).
