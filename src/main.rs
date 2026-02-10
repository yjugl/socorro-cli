use clap::{Parser, Subcommand};
use socorro_cli::{Result, SocorroClient, OutputFormat};

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
    channel     - Release channel (release, beta, nightly, esr)
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
    socorro-cli search --product Firefox --cpu-arch aarch64

SIGNATURE PATTERNS:
    Exact match:  --signature \"OOM | small\"
    Contains:     --signature \"~AudioDecoder\" (use ~ prefix)

PRODUCTS:
    Firefox, Fenix, Thunderbird, Firefox Focus, etc.

PLATFORMS:
    Windows, Linux, Mac OS X, Android

CPU ARCHITECTURES:
    amd64, x86, aarch64, arm

OUTPUT FIELDS:
    crash_id    - Full crash UUID (usable with 'socorro-cli crash')
    product     - Product name and version
    platform    - Operating system name
    channel     - Release channel (release, beta, nightly, esr)
    build_id    - Mozilla build ID timestamp (YYYYMMDDHHMMSS)
    signature   - Crash signature";

#[derive(Subcommand)]
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

        /// Filter by CPU architecture (amd64, x86, aarch64, arm)
        #[arg(long)]
        cpu_arch: Option<String>,

        /// Search crashes from the last N days
        #[arg(long, default_value = "7")]
        days: u32,

        /// Maximum number of results to return
        #[arg(long, default_value = "10")]
        limit: usize,

        /// Aggregate results by field (can be repeated: --facet version --facet platform)
        #[arg(long)]
        facet: Vec<String>,

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
        Commands::Crash { crash_id, depth, full, all_threads, modules } => {
            let client = SocorroClient::new("https://crash-stats.mozilla.org/api".to_string());
            socorro_cli::commands::crash::execute(&client, &crash_id, depth, full, all_threads, modules, cli.format)?;
        }
        Commands::Search { signature, product, version, platform, cpu_arch, days, limit, facet, sort } => {
            let client = SocorroClient::new("https://crash-stats.mozilla.org/api".to_string());
            let params = socorro_cli::models::SearchParams {
                signature,
                product,
                version,
                platform,
                cpu_arch,
                days,
                limit,
                facets: facet,
                sort,
            };
            socorro_cli::commands::search::execute(&client, params, cli.format)?;
        }
    }

    Ok(())
}
