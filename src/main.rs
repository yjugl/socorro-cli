// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use clap::{Parser, Subcommand};
use socorro_cli::{ModulesMode, OutputFormat, Result, SocorroClient};

fn parse_date(value: &str) -> std::result::Result<String, String> {
    match chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d") {
        Ok(date) if date.format("%Y-%m-%d").to_string() == value => Ok(value.to_string()),
        _ => Err(format!("invalid date '{value}': expected YYYY-MM-DD")),
    }
}

const LONG_ABOUT: &str = "\
Query Mozilla's Socorro crash reporting system (https://crash-stats.mozilla.org).

Socorro collects and analyzes crash reports from Firefox, Fenix, Thunderbird,
and other Mozilla products. This tool fetches crash details and searches crash
data, with output optimized for LLM agents.

WORKFLOW:
    # 1. Find top crash signatures (like the Top Crashers web UI)
    socorro-cli search --facet signature

    # 2. Examine a specific signature
    socorro-cli search --signature \"OOM | small\"

    # 3. Check if there are existing Bugzilla bugs for this crash
    socorro-cli bugs --signature \"OOM | small\"

    # 4. Debug a specific crash instance (ID or Socorro URL from step 2)
    socorro-cli crash <CRASH_ID> --annotations

    # 5. Find patterns across all crashes with this signature
    socorro-cli correlations --signature \"OOM | small\"

    # 6. Measure the signature's volume in opt-out crash pings
    socorro-cli crash-pings --signature \"OOM | small\"

API TOKEN:
    For higher rate limits, humans can run 'socorro-cli auth login' to store a token.
    Humans can create tokens at: https://crash-stats.mozilla.org/api/tokens/
    IMPORTANT: Tokens MUST have NO permissions (provides rate limit benefits
    only) to ensure there is no chance that the server returns protected data.

UPDATE CHECK:
    Each run checks crates.io for a newer version (cached daily) and, if there
    is one, prints a notice to stderr after the output. Where stderr is merged
    into stdout (2>&1) the notice can corrupt JSON output; redirect stderr
    separately or set MOZTOOLS_UPDATE_CHECK=0 to disable the check.

CACHE:
    'crash-pings' caches downloaded ping data (~10 MB per day) under the OS
    cache directory (~/.cache/socorro-cli on Linux). Set SOCORRO_CACHE_DIR to
    relocate it; the value is used as-is, with no 'socorro-cli' subdirectory.";

#[derive(Parser)]
#[command(name = "socorro-cli")]
#[command(version)]
#[command(about = "Query Mozilla's Socorro crash reporting system")]
#[command(long_about = LONG_ABOUT)]
#[command(
    after_help = "Use 'socorro-cli <command> --help' for more information on a specific command."
)]
struct Cli {
    /// Output format (compact is the token-efficient default)
    #[arg(long, value_enum, default_value = "compact", global = true)]
    format: OutputFormat,

    #[command(subcommand)]
    command: Commands,
}

const CRASH_ABOUT: &str = "\
Fetch details about a specific crash from Socorro.

The crash ID can be:
  - A bare UUID, e.g. b98bbb81-3ff6-4825-991f-6a0b30260901
  - A full Socorro URL: https://crash-stats.mozilla.org/report/index/<CRASH_ID>

EXAMPLES:
    # Basic crash lookup (compact output, includes modules from stack)
    socorro-cli crash <CRASH_ID>

    # Show all threads, grouping threads that share a stack (deadlock analysis)
    socorro-cli crash <CRASH_ID> --all-threads

    # Cheapest overview of what every thread is doing
    socorro-cli crash <CRASH_ID> --all-threads --depth 1

    # Show only third-party modules (Windows crashes only)
    socorro-cli crash <CRASH_ID> --modules third-party

    # Diagnose a shutdown hang: annotations plus a deeper stack
    socorro-cli crash <CRASH_ID> --annotations --depth 20

    # Get full JSON data (the API response verbatim)
    socorro-cli crash <CRASH_ID> --full

ALL THREADS:
    --all-threads shows every thread, not just the crashing one, and lowers
    the default --depth from 10 to 5. Threads whose displayed frames are
    identical are folded into one block whose header names every member, e.g.
    'stack[5 threads: 11:StyleThread#1, 12:StyleThread#2, ...]', and a line
    such as 'threads: 64 total, 28 distinct stacks shown' reports the totals.
    The crashing thread is never folded. Grouping compares only the displayed
    frames, so a larger --depth can split groups.

ANNOTATIONS:
    --annotations adds a section of crash annotations to compact and markdown
    output; it is opt-in because it can roughly double the output. Fields
    shown, when present: shutdown (the async_shutdown_timeout blockers, with
    each condition's file:line and state), shutdown_progress, shutdown_reason,
    spin_event_loop, app_notes, last_error, crash_inconsistencies,
    topmost_filenames, modules_in_stack and proto_signature. With none present
    the section reads 'annotations: (none)'. Ignored with --full and
    --format json, which already contain every annotation.

MODULES:
    The default, 'stack', lists the modules referenced by the displayed frames
    with their debug info (debug_file, debug_id, code_id, version): exactly
    what 'symdis disasm' needs. 'third-party' lists modules not signed by
    Mozilla or Microsoft (e.g. injected antivirus or DRM DLLs); it relies on
    the Authenticode cert_subject, which only Windows crash reports carry.
    --modules is ignored with --full and --format json.

FULL OUTPUT:
    --full (and --format json) print the /ProcessedCrash/ response verbatim,
    pretty-printed, with keys sorted alphabetically. The set of keys varies
    from crash to crash: do not assume a fixed schema.

    These modes skip the API token so the server strips protected fields, as
    a defense in depth should a token wrongly carry permissions. They are
    therefore subject to unauthenticated rate limits; compact and markdown
    output still use the token.

OUTPUT FIELDS:
    sig         - Crash signature: often the crashing function, but can be a category like \"OOM | small\" or \"shutdownhang | ...\"
    reason      - OS-level crash type and address (SIGSEGV, EXCEPTION_ACCESS_VIOLATION, etc.)
    type        - Report type (crash/hang), process (parent/content/gpu/...), uptime, thread count, and 'startup' for a startup crash
    moz_reason  - MOZ_CRASH/MOZ_RELEASE_ASSERT message (Mozilla code intentionally triggered the crash)
    abort       - C/C++ abort() message (third-party or stdlib code called abort)
    build       - Mozilla build ID timestamp (YYYYMMDDHHMMSS)
    stack       - Stack trace of the crashing thread, or of a group of threads sharing one";

const SEARCH_ABOUT: &str = "\
Search and aggregate crashes from Socorro.

Searches the Super Search API for crashes matching all of the specified
filters (they combine with AND). Use --facet to aggregate results by field
(can be repeated).

EXAMPLES:
    # Find crashes with a specific signature
    socorro-cli search --signature \"OOM | small\"

    # Search Fenix crashes from last 14 days
    socorro-cli search --product Fenix --days 14

    # Find Windows crashes for a specific version
    socorro-cli search --product Firefox --platform Windows --version 147.0.4

    # Aggregate by platform and version
    socorro-cli search --product Firefox --facet platform --facet version

    # Break down a signature by process type
    socorro-cli search --signature \"OOM | small\" --facet process_type

    # Filter to a specific Windows build
    socorro-cli search --signature \"OOM | small\" --platform-version \"~10.0.26100\"

NOTE: 'crash-pings' uses a different data source, with different flag names and values.

TOP CRASHERS:
    --facet signature lists the top signatures by volume, like the Socorro web
    UI's Top Crashers page:

    socorro-cli search --channel nightly --days 14 --facet signature --facets-size 20

    With --facet, individual crash rows are hidden unless --limit N is given.

FILTER OPERATORS:
    Exact match:  --signature \"OOM | small\" (default)
    Contains:     --signature \"~AudioDecoder\" (use ~ prefix)
    The --signature, --proto-signature, --platform-version, and --process-type
    flags support operator prefixes (~, $, ^, !, @, etc.).
    See https://crash-stats.mozilla.org/documentation/supersearch/ for details.

VALUES:
    Channel \"aurora\" is Firefox Developer Edition. Linux distro builds often
    report \"default\" instead of \"release\": search both to cover release-like
    crashes. Process type \"parent\" is the main/browser process.
    --platform-version takes the OS version string from the crash report, e.g.
    Windows \"10.0.26100\", macOS \"15.7.3 24G419\", Android \"36\" (API level).

FACET / SORT FIELDS:
    signature, proto_signature, product, version, platform, cpu_arch,
    release_channel, platform_version, platform_pretty_version, process_type,
    plugin_filename, dom_ipc_enabled, adapter_vendor_id, adapter_device_id,
    build_id, date, reason, address, cpu_info, cpu_count, uptime
    Use -field for descending sort (e.g., --sort -date).

OUTPUT:
    Compact output has one row per crash:
    crash_id | date | product version | platform | channel | build_id | signature
    crash_id is usable with 'socorro-cli crash'; build_id is YYYYMMDDHHMMSS.";

const CRASH_PINGS_ABOUT: &str = "\
Query Firefox crash pings from crash-pings.mozilla.org.

Crash pings are opt-out telemetry (~1.7M/day), representing the actual crash
experience of the Firefox user base. Unlike Socorro crash reports (opt-in,
~40K/day), crash pings are not biased toward users who click 'submit'. Use
crash-pings for volume and trend analysis, and 'crash' for deep debugging.

Data is a daily sample (~5000 pings per OS/process-type for release; more for
beta/nightly), available ~04:00 UTC for the previous day.

EXAMPLES:
    # Top crash signatures from yesterday's pings
    socorro-cli crash-pings

    # Filter by channel, OS, process type, version
    socorro-cli crash-pings --channel release --os Windows
    socorro-cli crash-pings --process main --version 147.0.3

    # Filter by signature (exact or contains with ~ prefix)
    socorro-cli crash-pings --signature \"OOM | small\"
    socorro-cli crash-pings --signature \"~AudioDecoder\"

    # Aggregate by a field instead of signature, over the last 7 days
    socorro-cli crash-pings --signature \"OOM | small\" --facet os --days 7

    # Fetch the symbolicated stack of a ping ID listed in the output for a date
    socorro-cli crash-pings --stack <PING_ID> --date <DATE>

NOTE: 'search' uses a different data source, with different flag names and values.

DATE RANGES:
    By default, crash-pings queries yesterday's data. Use --date for a specific
    day, --days N for the last N days, or --from/--to for an inclusive range.
    Each day's data is cached locally, so repeated queries are fast. Progress
    is shown on stderr for multi-date queries.

FILTERING:
    Filters are matched client-side. Only exact match and ~ (contains) are
    supported. The full Super Search operator prefixes (used in 'search')
    do not apply here.

FACET FIELDS:
    signature, channel, os, process, version, arch, osversion, build_id,
    ipc_actor, reason, type";

const BUGS_ABOUT: &str = "\
Look up Bugzilla bugs associated with crash signatures, or find signatures
associated with specific bug IDs. Pass either --signature or --bug-id.

Socorro maintains a mapping between Bugzilla bugs and crash signatures (from
the cf_crash_signature field in Bugzilla). This command queries that mapping.

When looking up by --signature, the API also returns 'related' bugs: if bug 123
is linked to signatures A and B, querying for signature A will return both
{123, A} and {123, B}.

EXAMPLES:
    # Find bugs for a crash signature
    socorro-cli bugs --signature \"OOM | small\"

    # Find bugs for multiple signatures
    socorro-cli bugs --signature \"OOM | small\" --signature \"OOM | large\"

    # Find signatures associated with a Bugzilla bug
    socorro-cli bugs --bug-id 1234567

    # Look up multiple bugs at once
    socorro-cli bugs --bug-id 1234567 --bug-id 9876543

OUTPUT:
    Results are grouped by bug ID, with Bugzilla links and associated signatures.
    Use --format json for the raw API response.

LIMITATIONS:
    Security-restricted Bugzilla bugs may not appear in results. Socorro syncs
    bug associations from Bugzilla periodically, and bugs that are not visible
    to Socorro's Bugzilla API token are not included.";

const CORRELATIONS_ABOUT: &str = "\
Show attributes that are statistically over-represented in crashes with a given
signature compared to the overall crash population.

Correlation data is pre-computed daily (so it may be up to 24 hours stale) for
the top ~200 signatures per channel and published to a CDN. Signatures outside
the top ~200 will return a 'not found' error. No API token is needed.

EXAMPLES:
    # Show correlations for a signature on the release channel (default)
    socorro-cli correlations --signature \"UiaNode::ProviderInfo::~ProviderInfo\"

    # Show correlations on the nightly channel
    socorro-cli correlations --signature \"OOM | small\" --channel nightly

    # Get raw JSON data
    socorro-cli correlations --signature \"OOM | small\" --format json

OUTPUT FIELDS:
    sig_%       - Percentage of crashes with this signature that have this attribute
    ref_%       - Percentage of all crashes on the channel that have this attribute
    attribute   - The over-represented attribute (module, OS version, GPU, etc.)
    prior       - Conditional: percentages when another attribute is also present";

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Manage API token stored in system keychain
    #[command(
        long_about = if cfg!(any(target_os = "windows", target_os = "macos", feature = "secret-service")) {
"\
Manage the API token stored in the system keychain.

Tokens provide higher rate limits (no extra permissions needed).
Humans can create tokens at: https://crash-stats.mozilla.org/api/tokens/
IMPORTANT: Tokens MUST have NO permissions attached.

EXAMPLES:
    # Store a token (prompts for input, hidden)
    socorro-cli auth login

    # Check if a token is stored
    socorro-cli auth status

    # Remove stored token
    socorro-cli auth logout"
        } else {
"\
Check API token status.

This build was compiled without system keychain support. Authentication
is available via the SOCORRO_API_TOKEN_PATH environment variable, which
should point to a file containing your API token.

Tokens provide higher rate limits (no extra permissions needed).
Humans can create tokens at: https://crash-stats.mozilla.org/api/tokens/
IMPORTANT: Tokens MUST have NO permissions attached.

EXAMPLES:
    # Check token status
    socorro-cli auth status"
        },
        after_help = if cfg!(any(target_os = "windows", target_os = "macos", feature = "secret-service")) {
            None
        } else {
            Some("Set SOCORRO_API_TOKEN_PATH to a file containing your API token.")
        }
    )]
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },

    /// Fetch details about a specific crash
    #[command(long_about = CRASH_ABOUT)]
    Crash {
        /// Crash ID (UUID) or full Socorro URL
        crash_id: String,

        /// Number of stack frames to show per thread (default: 10, or 5 with --all-threads)
        #[arg(long)]
        depth: Option<usize>,

        /// Output complete crash data without omissions (forces JSON; skips API token for privacy, may lower rate limits)
        #[arg(long)]
        full: bool,

        /// Show stacks from all threads, not just the crashing thread (useful for diagnosing deadlocks; lowers the default --depth to 5 to keep the output readable)
        #[arg(long)]
        all_threads: bool,

        /// Which modules to list: none, stack, full (all platforms), or third-party (Windows only — filters out modules signed by Mozilla or Microsoft; errors on non-Windows crashes)
        #[arg(long, value_enum, default_value = "stack")]
        modules: ModulesMode,

        /// Show extra crash annotations: shutdown blockers, app notes, proto signature (adds output; ignored with --full or --format json, which already include them)
        #[arg(long)]
        annotations: bool,
    },

    /// Query Firefox crash pings (opt-out telemetry, representative sample)
    #[command(long_about = CRASH_PINGS_ABOUT)]
    CrashPings {
        /// Date to query (YYYY-MM-DD), defaults to yesterday (UTC)
        #[arg(long, value_parser = parse_date, conflicts_with_all = ["days", "from", "to"])]
        date: Option<String>,

        /// Query the last N days (ending at yesterday)
        #[arg(long, conflicts_with_all = ["date", "from", "to"])]
        days: Option<u32>,

        /// Start of date range, inclusive (YYYY-MM-DD)
        #[arg(long, value_parser = parse_date, conflicts_with_all = ["date", "days"])]
        from: Option<String>,

        /// End of date range, inclusive (YYYY-MM-DD), defaults to yesterday if only --from given
        #[arg(long, value_parser = parse_date, conflicts_with_all = ["date", "days"], requires = "from")]
        to: Option<String>,

        /// Filter by release channel (release, beta, nightly)
        #[arg(long)]
        channel: Option<String>,

        /// Filter by OS (Windows, Linux, Mac, Android)
        #[arg(long)]
        os: Option<String>,

        /// Filter by process type (main, content, gpu, rdd, utility, socket, gmplugin)
        #[arg(long)]
        process: Option<String>,

        /// Filter by product version (e.g., "147.0.3")
        #[arg(long)]
        version: Option<String>,

        /// Filter by crash signature (use ~ prefix for contains match)
        #[arg(long)]
        signature: Option<String>,

        /// Filter by CPU architecture (x86_64, aarch64, x86, arm)
        #[arg(long)]
        arch: Option<String>,

        /// Aggregate by field instead of signature
        #[arg(long, default_value = "signature")]
        facet: String,

        /// Number of top entries to show
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Fetch symbolicated stack for a crash ping ID (IDs appear in aggregation output; --date must be the date it was listed under)
        #[arg(long, conflicts_with_all = ["days", "from", "to"])]
        stack: Option<String>,
    },

    /// Look up Bugzilla bugs for crash signatures (or signatures for bugs)
    #[command(long_about = BUGS_ABOUT)]
    Bugs {
        /// Crash signature(s) to look up bugs for (repeatable)
        #[arg(long, conflicts_with = "bug_id")]
        signature: Vec<String>,

        /// Bugzilla bug ID(s) to look up signatures for (repeatable)
        #[arg(long, conflicts_with = "signature")]
        bug_id: Vec<u64>,
    },

    /// Show over-represented attributes for a crash signature
    #[command(long_about = CORRELATIONS_ABOUT)]
    Correlations {
        /// Crash signature (exact match)
        #[arg(long)]
        signature: String,

        /// Release channel (release, beta, nightly, esr)
        #[arg(long, default_value = "release")]
        channel: String,
    },

    /// Search and aggregate crashes
    #[command(long_about = SEARCH_ABOUT)]
    Search {
        /// Filter by crash signature (use ~ prefix for contains match)
        #[arg(long)]
        signature: Option<String>,

        /// Filter by proto signature (raw unsymbolicated signature; use ~ prefix for contains match)
        #[arg(long)]
        proto_signature: Option<String>,

        /// Filter by product name (Firefox, Fenix, Thunderbird, Firefox Focus, ...)
        #[arg(long, default_value = "Firefox")]
        product: String,

        /// Filter by product version (e.g., "120.0")
        #[arg(long)]
        version: Option<String>,

        /// Filter by platform (Windows, Linux, Mac OS X, Android)
        #[arg(long)]
        platform: Option<String>,

        /// Filter by CPU architecture (amd64, x86, arm64, arm)
        #[arg(long)]
        cpu_arch: Option<String>,

        /// Filter by release channel (release, beta, nightly, esr, aurora, default)
        #[arg(long)]
        channel: Option<String>,

        /// Filter by OS version string (e.g., "10.0.19045"; use ~ prefix for contains match)
        #[arg(long)]
        platform_version: Option<String>,

        /// Filter by process type (parent, content, gpu, rdd, utility, socket, gmplugin, plugin)
        #[arg(long)]
        process_type: Option<String>,

        /// Single date to search (YYYY-MM-DD)
        #[arg(long, value_parser = parse_date, conflicts_with_all = ["days", "from", "to"])]
        date: Option<String>,

        /// Search crashes from the last N days (default: 7)
        #[arg(long, conflicts_with_all = ["date", "from", "to"])]
        days: Option<u32>,

        /// Start of date range, inclusive (YYYY-MM-DD)
        #[arg(long, value_parser = parse_date, conflicts_with_all = ["date", "days"])]
        from: Option<String>,

        /// End of date range, inclusive (YYYY-MM-DD), defaults to today if only --from given
        #[arg(long, value_parser = parse_date, conflicts_with_all = ["date", "days"], requires = "from")]
        to: Option<String>,

        /// Maximum number of individual crash results to return (default: 10, or 0 when --facet is used)
        #[arg(long)]
        limit: Option<usize>,

        /// Aggregate results by field (can be repeated: --facet version --facet platform)
        #[arg(long)]
        facet: Vec<String>,

        /// Number of facet buckets to return, default 50 (e.g., top N signatures)
        #[arg(long)]
        facets_size: Option<usize>,

        /// Sort field (prefix with - for descending, e.g., -date)
        #[arg(long, default_value = "-date")]
        sort: String,
    },
}

#[derive(Subcommand)]
enum AuthAction {
    /// Store API token in system keychain (prompts for token)
    #[cfg_attr(
        not(any(target_os = "windows", target_os = "macos", feature = "secret-service")),
        command(hide = true)
    )]
    Login,
    /// Remove API token from system keychain
    #[cfg_attr(
        not(any(target_os = "windows", target_os = "macos", feature = "secret-service")),
        command(hide = true)
    )]
    Logout,
    /// Check if API token is stored
    Status,
}

fn main() -> Result<()> {
    let version_checker = moz_cli_version_check::VersionChecker::new(
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION"),
    );
    version_checker.check_async();

    match Cli::try_parse() {
        Ok(cli) => {
            let result = run(cli);
            version_checker.print_warning();
            result
        }
        Err(e) => {
            let _ = e.print();
            version_checker.print_warning();
            std::process::exit(e.exit_code());
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Auth { action } => match action {
            AuthAction::Login => socorro_cli::commands::auth::login()?,
            AuthAction::Logout => socorro_cli::commands::auth::logout()?,
            AuthAction::Status => socorro_cli::commands::auth::status()?,
        },
        Commands::CrashPings {
            date,
            days,
            from,
            to,
            channel,
            os,
            process,
            version,
            signature,
            arch,
            facet,
            limit,
            stack,
        } => {
            let yesterday = || {
                let y = chrono::Utc::now() - chrono::Duration::days(1);
                y.format("%Y-%m-%d").to_string()
            };
            let (date_from, date_to) = if let Some(d) = date {
                (d.clone(), d)
            } else if let Some(n) = days {
                let end = chrono::Utc::now() - chrono::Duration::days(1);
                let start = end - chrono::Duration::days(n as i64 - 1);
                (
                    start.format("%Y-%m-%d").to_string(),
                    end.format("%Y-%m-%d").to_string(),
                )
            } else if let Some(f) = from {
                let t = to.unwrap_or_else(yesterday);
                if f > t {
                    return Err(socorro_cli::Error::ParseError(format!(
                        "--from date ({}) is after --to date ({})",
                        f, t
                    )));
                }
                (f, t)
            } else {
                let y = yesterday();
                (y.clone(), y)
            };
            let filters = socorro_cli::models::crash_pings::CrashPingFilters {
                channel,
                os,
                process,
                version,
                signature,
                arch,
            };
            socorro_cli::commands::crash_pings::execute(
                &date_from,
                &date_to,
                filters,
                &facet,
                limit,
                stack.as_deref(),
                cli.format,
            )?;
        }
        Commands::Bugs { signature, bug_id } => {
            if signature.is_empty() && bug_id.is_empty() {
                return Err(socorro_cli::Error::ParseError(
                    "Provide at least one --signature or --bug-id".to_string(),
                ));
            }
            let client = SocorroClient::new("https://crash-stats.mozilla.org/api".to_string());
            socorro_cli::commands::bugs::execute(&client, &signature, &bug_id, cli.format)?;
        }
        Commands::Correlations { signature, channel } => {
            socorro_cli::commands::correlations::execute(&signature, &channel, cli.format)?;
        }
        Commands::Crash {
            crash_id,
            depth,
            full,
            all_threads,
            modules,
            annotations,
        } => {
            let client = SocorroClient::new("https://crash-stats.mozilla.org/api".to_string());
            let params = socorro_cli::commands::crash::CrashParams {
                crash_id,
                depth,
                full,
                all_threads,
                modules_mode: modules,
                annotations,
            };
            socorro_cli::commands::crash::execute(&client, params, cli.format)?;
        }
        Commands::Search {
            signature,
            proto_signature,
            product,
            version,
            platform,
            cpu_arch,
            channel,
            platform_version,
            process_type,
            date,
            days,
            from,
            to,
            limit,
            facet,
            facets_size,
            sort,
        } => {
            let today = || chrono::Utc::now().format("%Y-%m-%d").to_string();
            let (date_from, date_to) = if let Some(d) = date {
                (d.clone(), Some(d))
            } else if let Some(n) = days {
                let start = chrono::Utc::now() - chrono::Duration::days(n as i64);
                (start.format("%Y-%m-%d").to_string(), None)
            } else if let Some(f) = from {
                let t = to.unwrap_or_else(today);
                if f > t {
                    return Err(socorro_cli::Error::ParseError(format!(
                        "--from date ({}) is after --to date ({})",
                        f, t
                    )));
                }
                (f, Some(t))
            } else {
                let start = chrono::Utc::now() - chrono::Duration::days(7);
                (start.format("%Y-%m-%d").to_string(), None)
            };
            let client = SocorroClient::new("https://crash-stats.mozilla.org/api".to_string());
            let limit = limit.unwrap_or(if facet.is_empty() { 10 } else { 0 });
            let params = socorro_cli::models::SearchParams {
                signature,
                proto_signature,
                product,
                version,
                platform,
                cpu_arch,
                release_channel: channel,
                platform_version,
                process_type,
                date_from,
                date_to,
                limit,
                facets: facet,
                facets_size,
                sort,
            };
            socorro_cli::commands::search::execute(&client, params, cli.format)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn search_rejects_malformed_date_arguments() {
        assert_malformed_dates("search");
    }

    #[test]
    fn crash_pings_rejects_malformed_date_arguments() {
        assert_malformed_dates("crash-pings");
    }

    #[test]
    fn search_accepts_valid_leap_day_arguments() {
        assert_valid_leap_day_dates("search");
    }

    #[test]
    fn crash_pings_accepts_valid_leap_day_arguments() {
        assert_valid_leap_day_dates("crash-pings");
    }

    fn assert_malformed_dates(command: &str) {
        for flag in ["--date", "--from", "--to"] {
            let mut args = vec!["socorro-cli", command];
            if flag == "--to" {
                args.extend(["--from", "2024-02-01"]);
            }
            args.extend([flag, "not-a-date"]);

            let error = match Cli::try_parse_from(args) {
                Ok(_) => panic!("malformed date should be rejected"),
                Err(error) => error,
            };
            assert_eq!(error.kind(), ErrorKind::ValueValidation);
            assert!(
                error.to_string().contains("YYYY-MM-DD"),
                "error for {command} {flag} did not explain the format: {error}"
            );
        }
    }

    fn assert_valid_leap_day_dates(command: &str) {
        for args in [
            vec!["socorro-cli", command, "--date", "2024-02-29"],
            vec![
                "socorro-cli",
                command,
                "--from",
                "2024-02-29",
                "--to",
                "2024-02-29",
            ],
        ] {
            Cli::try_parse_from(args).expect("valid leap day should be accepted");
        }
    }
}
