# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

socorro-cli is a Rust CLI tool for querying Mozilla's Socorro crash reporting system. It's optimized for LLM coding agents with token-efficient output formats. The tool provides four main commands: `crash` (fetch individual crash details), `search` (search and aggregate crashes), `correlations` (show over-represented attributes for a signature), and `auth` (manage API token storage).

## Build & Development Commands

```bash
# Build the project
cargo build

# Build optimized release
cargo build --release

# Run locally without installing
cargo run -- crash <crash-id>
cargo run -- search --signature "SomeSignature"
cargo run -- correlations --signature "OOM | small"

# Install locally
cargo install --path .

# Run with specific subcommand
socorro-cli crash <crash-id>
socorro-cli search --signature "term"

# API token is managed via keychain or token file (see Authentication section)

# Run tests
cargo test

# Format code
cargo fmt

# Run linter
cargo clippy
```

**Important**: Run `cargo clippy` after any Rust code changes to keep the codebase tidy. Fix any warnings before committing.

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
  - Automatically retrieves auth token from keychain via `get_auth_header()`
- **src/commands/**: Command implementations
  - **auth.rs**: Handles `auth login/logout/status` subcommands
  - **crash.rs**: Handles crash fetching and output formatting
  - **search.rs**: Handles crash search and aggregation
  - **correlations.rs**: Fetches correlation data from CDN (not Socorro API), computes signature hash, handles CDN HTTP requests
- **src/models/**: Data structures for Socorro API responses
  - **processed_crash.rs**: `ProcessedCrash`, `Thread`, `CrashSummary` - crash data models
  - **search.rs**: `SearchResponse`, `SearchParams` - search data models
  - **correlations.rs**: `CorrelationsTotals`, `CorrelationsResponse`, `CorrelationsSummary` - correlation data models
  - **common.rs**: Shared types like `StackFrame`
- **src/output/**: Output formatters
  - **compact.rs**: Token-optimized plain text (default, LLM-friendly)
  - **json.rs**: Full JSON output
  - **markdown.rs**: Human-readable markdown

### Data Flow

1. CLI parses arguments → creates `SocorroClient` (token retrieved automatically from keychain/file)
2. Command dispatcher calls appropriate command module
3. Command module:
   - For crash: extracts crash ID from URL if needed → `client.get_crash()` → converts `ProcessedCrash` to `CrashSummary` → formats output
   - For search: builds `SearchParams` → `client.search()` → formats `SearchResponse`
   - For correlations: builds reqwest client with gzip → fetches totals + per-signature data from CDN → converts `CorrelationsResponse` to `CorrelationsSummary` → formats output
4. Output formatter generates final text based on selected format

### Key Design Decisions

**Crash ID Extraction**: `crash` command accepts both bare IDs and full Socorro URLs (e.g., `https://crash-stats.mozilla.org/report/index/<uuid>`). The `extract_crash_id()` function extracts the UUID from URLs.

**Two-Stage Model Conversion**: Raw API responses are deserialized into `ProcessedCrash`, then converted to `CrashSummary` which contains only display-relevant data at the requested depth. This separation keeps formatting logic simple and avoids processing unused data.

**Thread Handling**: Crash data includes multiple threads. The tool identifies the crashing thread via:
1. `crashing_thread` field
2. `crash_info.crashing_thread` field
3. `json_dump.crashing_thread` field

With `--all-threads`, it formats all threads (marking the crashing one), useful for deadlock analysis.

**Stack Frame Depth**: By default shows 10 frames. Configurable via `--depth` to control output size vs detail.

**Compact Format**: Default output format is designed to minimize tokens while preserving essential crash information. Uses abbreviations (sig, moz_reason) and omits field labels when clear from context.

**Facet-aware `--limit` default**: When `--facet` is used, `--limit` defaults to 0 (only aggregations shown). Without `--facet`, it defaults to 10. Users can override with `--limit N` to show individual crash rows alongside aggregations. `--facets-size` controls how many buckets each facet returns (e.g., top N signatures).

**Error Handling**: Uses `thiserror` for structured errors. Specific handling for:
- 404 → `NotFound` error with crash ID
- 429 → `RateLimited` error suggesting API token usage
- Parse errors include response preview (first 200 chars)

## Socorro API Details

**Base URL**: `https://crash-stats.mozilla.org/api`

**Endpoints Used**:
- `/ProcessedCrash/` - fetch individual crash by ID
- `/SuperSearch/` - search/aggregate crashes

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

The test suite (64 tests) covers:
- **Crash ID extraction**: Bare IDs, full URLs, URLs with trailing slashes
- **ProcessedCrash model**: JSON deserialization, `to_summary()` conversion, crashing thread identification from multiple sources, depth limiting, all-threads mode
- **Search models**: SearchResponse/CrashHit deserialization, facets parsing
- **Correlations models**: Deserialization, `to_summary()` percentage calculations, `format_item_map()` for item display
- **Output formatters**: Compact and Markdown formatters for crash, search, and correlations output
- **Client validation**: Crash ID format validation (rejects invalid characters, potential injection attempts)
- **Auth token file**: Reading from `SOCORRO_API_TOKEN_PATH`, whitespace handling, missing file handling

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
