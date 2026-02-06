use clap::{Parser, Subcommand};
use socorro_cli::{Result, SocorroClient, OutputFormat};

#[derive(Parser)]
#[command(name = "socorro-cli")]
#[command(about = "Query Mozilla's Socorro crash reporting system", long_about = None)]
struct Cli {
    #[arg(long, value_enum, default_value = "compact", global = true)]
    format: OutputFormat,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage API token stored in system keychain
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// Fetch details about a specific crash
    Crash {
        crash_id: String,

        #[arg(long, default_value = "10")]
        depth: usize,

        #[arg(long, help = "Output full crash data without omissions (forces JSON format)")]
        full: bool,

        #[arg(long, help = "Show stacks from all threads (useful for diagnosing deadlocks)")]
        all_threads: bool,

        #[arg(long)]
        modules: bool,
    },
    /// Search and aggregate crashes
    Search {
        #[arg(long)]
        signature: Option<String>,

        #[arg(long, default_value = "Firefox")]
        product: String,

        #[arg(long)]
        version: Option<String>,

        #[arg(long)]
        platform: Option<String>,

        #[arg(long, default_value = "7")]
        days: u32,

        #[arg(long, default_value = "10")]
        limit: usize,

        #[arg(long)]
        facet: Vec<String>,

        #[arg(long, default_value = "-date")]
        sort: String,
    },
}

#[derive(Subcommand)]
enum AuthAction {
    /// Store API token in system keychain
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
        Commands::Search { signature, product, version, platform, days, limit, facet, sort } => {
            let client = SocorroClient::new("https://crash-stats.mozilla.org/api".to_string());
            let params = socorro_cli::models::SearchParams {
                signature,
                product,
                version,
                platform,
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
