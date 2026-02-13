use clap::{Parser, Subcommand};
use socorro_cli::{OutputFormat, Result, SocorroClient};

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

    # List top crash signatures by volume (like the Top Crashers web UI)
    socorro-cli search --facet signature

API TOKEN:
    For higher rate limits, run 'socorro-cli auth login' to store a token.
    Create tokens at: https://crash-stats.mozilla.org/api/tokens/
    Tokens should have NO permissions (provides rate limit benefits only).";

#[derive(Parser)]
#[command(name = "socorro-cli")]
#[command(about = "Query Mozilla's Socorro crash reporting system")]
#[command(long_about = LONG_ABOUT)]
#[command(after_help = "Use 'socorro-cli <command> --help' for more information on a specific command.")]
struct Cli {
    /// Output format: compact (default, token-efficient), json, or markdown
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
    # Basic crash lookup (compact output)
    socorro-cli crash 247653e8-7a18-4836-97d1-42a720260120

    # Show more stack frames
    socorro-cli crash <crash-id> --depth 20

    # Show all threads (useful for deadlock analysis)
    socorro-cli crash <crash-id> --all-threads

    # Get full JSON data
    socorro-cli crash <crash-id> --full

OUTPUT FIELDS:
    sig         - Crash signature (function where crash occurred)
    reason      - Crash type (SIGSEGV, EXCEPTION_ACCESS_VIOLATION, etc.)
    moz_reason  - Mozilla assertion message if applicable
    product     - Product name and version (Firefox 120.0, Fenix 147.0.1, etc.)
    build       - Mozilla build ID timestamp (YYYYMMDDHHMMSS)
    channel     - Release channel (release, beta, nightly, esr, aurora, default)
    stack       - Stack trace of the crashing thread";

const SEARCH_ABOUT: &str = "\
Search and aggregate crashes from Socorro.

Searches the Super Search API for crashes matching the specified filters.
Use --facet to aggregate results by field (can be repeated).

EXAMPLES:
    # Find crashes with a specific signature
    socorro-cli search --signature \"mozilla::AudioDecoderInputTrack\"

    # Search Fenix crashes from last 14 days
    socorro-cli search --product Fenix --days 14

    # Aggregate by platform and version
    socorro-cli search --product Firefox --facet platform --facet version

    # Find Windows crashes for a specific version
    socorro-cli search --product Firefox --platform Windows --version 120.0

    # Find crashes on ARM64 architecture
    socorro-cli search --product Firefox --cpu-arch arm64

    # Find nightly crashes only
    socorro-cli search --product Firefox --channel nightly

    # Break down crashes by OS version
    socorro-cli search --signature \"OOM | small\" --facet platform_version

    # Filter to a specific Windows build
    socorro-cli search --signature \"OOM | small\" --platform-version \"~10.0.26100\"

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

SIGNATURE PATTERNS:
    Exact match:  --signature \"OOM | small\"
    Contains:     --signature \"~AudioDecoder\" (use ~ prefix)

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

PLATFORM VERSIONS:
    Values are OS version strings from the crash report, e.g.:
      Windows: \"10.0.19045\", \"10.0.26100\"
      macOS:   \"15.7.3 24G419\", \"10.13.6 17G14042\"
      Android: \"28\", \"36\" (API levels)
    Use --facet platform_version to see which OS builds are affected.
    Use --platform-version \"~10.0.26100\" to filter (~ prefix for contains match).

FACET / SORT FIELDS:
    signature, product, version, platform, cpu_arch, release_channel,
    platform_version, platform_pretty_version, process_type, plugin_filename,
    dom_ipc_enabled, adapter_vendor_id, adapter_device_id
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
    #[command(after_help = "Run 'socorro-cli auth status' to check if a token is stored.")]
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

        /// Output complete crash data without omissions (forces JSON format)
        #[arg(long)]
        full: bool,

        /// Show stacks from all threads, not just the crashing thread (useful for diagnosing deadlocks)
        #[arg(long)]
        all_threads: bool,

        /// Include loaded modules in output
        #[arg(long)]
        modules: bool,
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

        /// Filter by OS version string (e.g., "10.0.19045", "10.0.26100")
        #[arg(long)]
        platform_version: Option<String>,

        /// Search crashes from the last N days
        #[arg(long, default_value = "7")]
        days: u32,

        /// Maximum number of individual crash results to return (default: 10, or 0 when --facet is used)
        #[arg(long)]
        limit: Option<usize>,

        /// Aggregate results by field (can be repeated: --facet version --facet platform)
        #[arg(long)]
        facet: Vec<String>,

        /// Number of facet buckets to return (e.g., top N signatures)
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
    Login,
    /// Remove API token from system keychain
    Logout,
    /// Check if API token is stored
    Status,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Auth { action } => {
            match action {
                AuthAction::Login => socorro_cli::commands::auth::login()?,
                AuthAction::Logout => socorro_cli::commands::auth::logout()?,
                AuthAction::Status => socorro_cli::commands::auth::status()?,
            }
        }
        Commands::Correlations { signature, channel } => {
            socorro_cli::commands::correlations::execute(&signature, &channel, cli.format)?;
        }
        Commands::Crash { crash_id, depth, full, all_threads, modules } => {
            let client = SocorroClient::new("https://crash-stats.mozilla.org/api".to_string());
            socorro_cli::commands::crash::execute(&client, &crash_id, depth, full, all_threads, modules, cli.format)?;
        }
        Commands::Search { signature, product, version, platform, cpu_arch, channel, platform_version, days, limit, facet, facets_size, sort } => {
            let client = SocorroClient::new("https://crash-stats.mozilla.org/api".to_string());
            let limit = limit.unwrap_or(if facet.is_empty() { 10 } else { 0 });
            let params = socorro_cli::models::SearchParams {
                signature,
                product,
                version,
                platform,
                cpu_arch,
                release_channel: channel,
                platform_version,
                days,
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
