// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use clap::{Parser, Subcommand};
use socorro_cli::{ModulesMode, OutputFormat, Result, SocorroClient};

const LONG_ABOUT: &str = "\
Query Mozilla's Socorro crash reporting system (https://crash-stats.mozilla.org).

Socorro collects and analyzes crash reports from Firefox, Fenix, Thunderbird,
and other Mozilla products. This tool fetches crash details and searches crash
data, with output optimized for LLM agents.

EXAMPLES:
    # Fetch a specific crash by ID
    socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120

    # Fetch a crash using a Socorro URL (copy-paste from browser)
    socorro-cli crash https://crash-stats.mozilla.org/report/index/247653e8-...

    # Search for crashes by signature
    socorro-cli search --signature \"OOM | small\"

    # Search Firefox crashes from last 30 days, aggregate by version
    socorro-cli search --product Firefox --days 30 --facet version

    # Search a specific date or date range
    socorro-cli search --signature \"OOM | small\" --date 2026-02-20
    socorro-cli search --signature \"OOM | small\" --from 2026-02-10 --to 2026-02-20

    # List top crash signatures by volume (like the Top Crashers web UI)
    socorro-cli search --facet signature

    # Show correlations for a crash signature
    socorro-cli correlations --signature \"OOM | small\"

    # Top crash signatures from yesterday's opt-out crash pings
    socorro-cli crash-pings

    # Crash pings filtered by channel and OS
    socorro-cli crash-pings --channel release --os Windows

API TOKEN:
    For higher rate limits, humans can run 'socorro-cli auth login' to store a token.
    Humans can create tokens at: https://crash-stats.mozilla.org/api/tokens/
    IMPORTANT: Tokens MUST have NO permissions (provides rate limit benefits
    only) to ensure there is no chance that the server returns protected data.

UPDATE CHECK:
    On each run, socorro-cli checks crates.io for a newer version (cached daily,
    5-second timeout). If an update is available, a notice is printed to stderr
    after the command output. Set MOZTOOLS_UPDATE_CHECK=0 to disable.

    Important: the update notice goes to stderr only. If your environment merges
    stderr into stdout (e.g. shell 2>&1 redirects), it may corrupt JSON output.
    In such cases, either redirect stderr separately or set MOZTOOLS_UPDATE_CHECK=0.

WORKFLOW:
    # 1. Find top crash signatures
    socorro-cli search --facet signature

    # 2. Examine a specific signature
    socorro-cli search --signature \"OOM | small\"

    # 3. Check if there are existing Bugzilla bugs for this crash
    socorro-cli bugs --signature \"OOM | small\"

    # 4. Debug a specific crash instance
    socorro-cli crash <crash_id from step 2>

    # 5. Find patterns across all crashes with this signature
    socorro-cli correlations --signature \"OOM | small\"";

#[derive(Parser)]
#[command(name = "socorro-cli")]
#[command(version)]
#[command(about = "Query Mozilla's Socorro crash reporting system")]
#[command(long_about = LONG_ABOUT)]
#[command(
    after_help = "Use 'socorro-cli <command> --help' for more information on a specific command."
)]
struct Cli {
    /// Output format: compact (default, token-efficient), json, or markdown. Note: json skips the API token for crash fetches (see 'crash --help')
    #[arg(long, value_enum, default_value = "compact", global = true)]
    format: OutputFormat,

    #[command(subcommand)]
    command: Commands,
}

const CRASH_ABOUT: &str = "\
Fetch details about a specific crash from Socorro.

The crash ID can be:
  - A bare UUID: 247653e8-7a18-4836-97d1-42a720260120
  - A full Socorro URL: https://crash-stats.mozilla.org/report/index/247653e8-...

EXAMPLES:
    # Basic crash lookup (compact output, includes modules from stack)
    socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120

    # Show more stack frames
    socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --depth 20

    # Show all threads (useful for deadlock analysis)
    socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --all-threads

    # Hide modules section
    socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --modules none

    # Show all loaded modules (for compatibility/environment analysis)
    socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --modules full

    # Get full JSON data
    socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120 --full

MODULES:
    --modules controls which loaded modules are listed in the output.
    Default is 'stack' which lists modules referenced by the displayed stack
    frames, with debug info (debug_file, debug_id, code_id, version). This
    provides exactly what 'symdis disasm' needs for disassembly.
    Use 'full' to list ALL loaded modules (can be 38-481 depending on platform).
    Use 'none' to omit the modules section entirely.
    --modules only applies to compact and markdown output; --full already
    dumps everything as raw JSON.

RATE LIMITS:
    --full and --format json skip the API token so the server strips protected
    fields from the response. This is a defense-in-depth measure against human
    error (e.g., accidentally creating a token with view_pii permission). The
    primary safeguard is ensuring your token has NO permissions — humans can
    verify this at https://crash-stats.mozilla.org/api/tokens/. These modes use
    unauthenticated rate limits even when a token is configured. Compact and
    markdown formats are unaffected and still benefit from the token's higher
    rate limits.

OUTPUT FIELDS:
    sig         - Crash signature (identifies the crash type; often the crashing function, but can also be a category like \"OOM | small\" or \"shutdownhang | ...\")
    reason      - OS-level crash type and address (SIGSEGV, EXCEPTION_ACCESS_VIOLATION, etc.)
    moz_reason  - MOZ_CRASH/MOZ_RELEASE_ASSERT message (Mozilla code intentionally triggered the crash)
    abort       - C/C++ abort() message (third-party or stdlib code called abort)
    product     - Product name and version (Firefox 120.0, Fenix 147.0.1, etc.)
    build       - Mozilla build ID timestamp (YYYYMMDDHHMMSS)
    channel     - Release channel (release, beta, nightly, esr, aurora, default)
    stack       - Stack trace of the crashing thread
    modules     - Loaded modules with debug info (controlled by --modules)";

const SEARCH_ABOUT: &str = "\
Search and aggregate crashes from Socorro.

Searches the Super Search API for crashes matching the specified filters.
Use --facet to aggregate results by field (can be repeated).

EXAMPLES:
    # Find crashes with a specific signature
    socorro-cli search --signature \"OOM | small\"

    # Search Fenix crashes from last 14 days
    socorro-cli search --product Fenix --days 14

    # Search a specific date
    socorro-cli search --signature \"OOM | small\" --date 2026-02-20

    # Search a date range
    socorro-cli search --signature \"OOM | small\" --from 2026-02-10 --to 2026-02-20

    # Aggregate by platform and version
    socorro-cli search --product Firefox --facet platform --facet version

    # Find Windows crashes for a specific version
    socorro-cli search --product Firefox --platform Windows --version 147.0.4

    # Find crashes on ARM64 architecture
    socorro-cli search --product Firefox --cpu-arch arm64

    # Find nightly crashes only
    socorro-cli search --product Firefox --channel nightly

    # Find content process crashes
    socorro-cli search --signature \"OOM | small\" --process-type content

    # Break down a signature by process type
    socorro-cli search --signature \"OOM | small\" --facet process_type

    # Break down crashes by OS version
    socorro-cli search --signature \"OOM | small\" --facet platform_version

    # Filter to a specific Windows build
    socorro-cli search --signature \"OOM | small\" --platform-version \"~10.0.26100\"

NOTE: The crash-pings command uses a different data source with different flag
    names and values. Do not assume the same flags or values work across both
    commands — check 'socorro-cli crash-pings --help'.

TOP CRASHERS:
    To list the top crash signatures by volume (like the Socorro web UI's
    Top Crashers page), use --facet signature:

    # Top 50 Firefox crashers in the last 7 days (default facets size)
    socorro-cli search --facet signature

    # Top 20 nightly crashers in the last 14 days
    socorro-cli search --channel nightly --days 14 --facet signature --facets-size 20

    # Top 100 Fenix crashers on Android
    socorro-cli search --product Fenix --facet signature --facets-size 100

    When --facet is used, individual crash rows are hidden by default
    (only aggregated counts are shown). Use --limit 10 to also show
    individual crashes alongside the aggregations.
    --facets-size controls how many top signatures are returned (default: 50).

DATE RANGES:
    By default, searches the last 7 days. Use --days N for a different window,
    --date for a single day, or --from/--to for an arbitrary range.
    Both --from and --to are inclusive (--from 02-10 --to 02-12 includes all
    three days). --from without --to defaults --to to today.
    --date D is equivalent to --from D --to D.
    These flags are mutually exclusive.

FILTER OPERATORS:
    Exact match:  --signature \"OOM | small\" (default)
    Contains:     --signature \"~AudioDecoder\" (use ~ prefix)
    The --signature, --proto-signature, --platform-version, and --process-type
    flags support operator prefixes (~, $, ^, !, @, etc.).
    See https://crash-stats.mozilla.org/documentation/supersearch/ for details.

PROTO SIGNATURE:
    The proto signature is the raw, unsymbolicated crash signature before
    Socorro's signature generation. Use --proto-signature to search by it.
    Contains:     --proto-signature \"~libxul.so@0x123abc\"

PRODUCTS:
    Firefox, Fenix, Thunderbird, Firefox Focus, etc.

PLATFORMS:
    Windows, Linux, Mac OS X, Android

CPU ARCHITECTURES:
    amd64, x86, arm64, arm

RELEASE CHANNELS:
    release, beta, nightly, esr, aurora, default
    NOTE: \"aurora\" is the channel used by Firefox Developer Edition.
    NOTE: Linux distro builds often report channel as \"default\" instead
    of \"release\". To find all release-like crashes, run two searches:
      socorro-cli search --channel release ...
      socorro-cli search --channel default ...

PROCESS TYPES:
    parent, content, gpu, rdd, utility, socket, gmplugin, plugin
    NOTE: \"parent\" is the main/browser process.

PLATFORM VERSIONS:
    Values are OS version strings from the crash report, e.g.:
      Windows: \"10.0.19045\", \"10.0.26100\"
      macOS:   \"15.7.3 24G419\", \"10.13.6 17G14042\"
      Android: \"28\", \"36\" (API levels)
    Use --facet platform_version to see which OS builds are affected.
    Use --platform-version \"~10.0.26100\" to filter (~ prefix for contains match).

FACET / SORT FIELDS:
    signature, proto_signature, product, version, platform, cpu_arch,
    release_channel, platform_version, platform_pretty_version, process_type,
    plugin_filename, dom_ipc_enabled, adapter_vendor_id, adapter_device_id,
    build_id, date, reason, address, cpu_info, cpu_count, uptime
    Use -field for descending sort (e.g., --sort -date).

FILTER LOGIC:
    Multiple filters are combined with AND logic.
    Example: --platform Windows --channel nightly returns only
    crashes that are both Windows AND nightly.

OUTPUT FIELDS:
    crash_id    - Full crash UUID (usable with 'socorro-cli crash')
    product     - Product name and version
    platform    - Operating system name and version (e.g., Windows NT 10.0.19045)
    channel     - Release channel (release, beta, nightly, esr, aurora, default)
    build_id    - Mozilla build ID timestamp (YYYYMMDDHHMMSS)
    signature   - Crash signature";

const CRASH_PINGS_ABOUT: &str = "\
Query Firefox crash pings from crash-pings.mozilla.org.

Crash pings are opt-out telemetry (~1.7M/day), representing the actual crash
experience of the Firefox user base. Unlike Socorro crash reports (opt-in,
~40K/day), crash pings are not biased toward users who click 'submit'.

Data is a daily sample (~5000 pings per OS/process-type for release; more for
beta/nightly), available ~04:00 UTC for the previous day.

Downloaded data is cached locally so repeated queries for the same date are
instant.

EXAMPLES:
    # Top crash signatures from yesterday's pings
    socorro-cli crash-pings

    # Specify date
    socorro-cli crash-pings --date 2026-02-12

    # Query a date range
    socorro-cli crash-pings --from 2026-02-10 --to 2026-02-12

    # Query the last 7 days
    socorro-cli crash-pings --days 7

    # Filter by channel, OS, process type, version
    socorro-cli crash-pings --channel release --os Windows
    socorro-cli crash-pings --process main --version 147.0.3

    # Filter by signature (exact or contains with ~ prefix)
    socorro-cli crash-pings --signature \"OOM | small\"
    socorro-cli crash-pings --signature \"~AudioDecoder\"

    # Aggregate by a field instead of signature
    socorro-cli crash-pings --signature \"OOM | small\" --facet os
    socorro-cli crash-pings --facet process

    # Fetch symbolicated stack for a specific crash ping
    socorro-cli crash-pings --stack b343be53-8ec1-4849-98eb-ca6739a45645 --date 2026-02-23

NOTE: The search command uses a different data source with different flag
    names and values. Do not assume the same flags or values work across both
    commands — check 'socorro-cli search --help'.

DATE RANGES:
    By default, crash-pings queries yesterday's data. Use --date for a specific
    day, --days N for the last N days, or --from/--to for an arbitrary range.
    Both --from and --to are inclusive (--from 02-10 --to 02-12 includes all
    three days). --from without --to defaults to yesterday. Data for each date
    is cached locally, so repeated queries are fast. Progress is shown on stderr
    for multi-date queries.

FILTERING:
    Filters are matched client-side. Only exact match and ~ (contains) are
    supported. The full Super Search operator prefixes (used in 'search')
    do not apply here.

FACET FIELDS:
    signature, channel, os, process, version, arch, osversion, build_id,
    ipc_actor, reason, type

CRASH PINGS VS CRASH REPORTS:
    | Feature         | crash-pings (this)    | crash (Socorro)          |
    |-----------------|-----------------------|--------------------------|
    | Opt-in/out      | Opt-out (all users)   | Opt-in (user clicks)     |
    | Volume          | ~1.7M/day             | ~40K/day                 |
    | Bias            | Representative sample | Biased toward submitters |
    | Detail          | Signature + metadata  | Full minidump + stack    |
    | Stack traces    | Symbolicated stacks   | Full native stacks       |

    Use crash-pings for volume/trend analysis; use crash for deep debugging.";

const BUGS_ABOUT: &str = "\
Look up Bugzilla bugs associated with crash signatures, or find signatures
associated with specific bug IDs.

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
    to Socorro's Bugzilla API token are not included.

NOTE: --signature and --bug-id are mutually exclusive. At least one must be provided.";

const CORRELATIONS_ABOUT: &str = "\
Show attributes that are statistically over-represented in crashes with a given
signature compared to the overall crash population.

Correlation data is pre-computed daily for the top ~200 signatures per channel
and published to a CDN. Signatures outside the top ~200 will return a 'not found'
error. No API token is needed.

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
    prior       - Conditional: percentages when another attribute is also present

LIMITATIONS:
    - Only available for the top ~200 signatures per channel
    - Data is refreshed daily; may be up to 24 hours stale
    - Channels: release, beta, nightly, esr";

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
            "Run 'socorro-cli auth status' to check if a token is stored."
        } else {
            "Set SOCORRO_API_TOKEN_PATH to a file containing your API token."
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

        /// Number of stack frames to show per thread
        #[arg(long, default_value = "10")]
        depth: usize,

        /// Output complete crash data without omissions (forces JSON; skips API token for privacy, may lower rate limits)
        #[arg(long)]
        full: bool,

        /// Show stacks from all threads, not just the crashing thread (useful for diagnosing deadlocks)
        #[arg(long)]
        all_threads: bool,

        /// Which modules to list: none, stack (modules in displayed frames), or full (all loaded modules)
        #[arg(long, value_enum, default_value = "stack")]
        modules: ModulesMode,
    },

    /// Query Firefox crash pings (opt-out telemetry, representative sample)
    #[command(long_about = CRASH_PINGS_ABOUT)]
    CrashPings {
        /// Date to query (YYYY-MM-DD), defaults to yesterday (UTC)
        #[arg(long, conflicts_with_all = ["days", "from", "to"])]
        date: Option<String>,

        /// Query the last N days (ending at yesterday)
        #[arg(long, conflicts_with_all = ["date", "from", "to"])]
        days: Option<u32>,

        /// Start of date range, inclusive (YYYY-MM-DD)
        #[arg(long, conflicts_with_all = ["date", "days"])]
        from: Option<String>,

        /// End of date range, inclusive (YYYY-MM-DD), defaults to yesterday if only --from given
        #[arg(long, conflicts_with_all = ["date", "days"], requires = "from")]
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

        /// Fetch symbolicated stack for a crash ping ID (IDs appear in crash-pings aggregation output)
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

        /// Filter by product name
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
        #[arg(long, conflicts_with_all = ["days", "from", "to"])]
        date: Option<String>,

        /// Search crashes from the last N days
        #[arg(long, conflicts_with_all = ["date", "from", "to"])]
        days: Option<u32>,

        /// Start of date range, inclusive (YYYY-MM-DD)
        #[arg(long, conflicts_with_all = ["date", "days"])]
        from: Option<String>,

        /// End of date range, inclusive (YYYY-MM-DD), defaults to today if only --from given
        #[arg(long, conflicts_with_all = ["date", "days"], requires = "from")]
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
    let version_checker =
        moz_cli_version_check::VersionChecker::new("socorro-cli", env!("CARGO_PKG_VERSION"));
    version_checker.check_async();

    let result = run();

    version_checker.print_warning();

    result
}

fn run() -> Result<()> {
    let cli = Cli::parse();
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
        } => {
            let client = SocorroClient::new("https://crash-stats.mozilla.org/api".to_string());
            socorro_cli::commands::crash::execute(
                &client,
                &crash_id,
                depth,
                full,
                all_threads,
                modules,
                cli.format,
            )?;
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
