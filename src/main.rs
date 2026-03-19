mod add;
mod client;
mod config;
mod models;
mod search;
mod setup;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "clh", about = "Fuzzy-search your command history via clh-server")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Fuzzy-search history and print selected command to stdout (default)
    Search,
    /// Record a command to the server (called from zsh precmd hook)
    Add {
        #[arg(long)]
        hostname: String,
        #[arg(long)]
        pwd: String,
        #[arg(long)]
        command: String,
    },
    /// Print zsh integration script to stdout (eval with: eval "$(clh setup)")
    Setup,
    /// Manage configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current config path and contents
    Show,
    /// Create a new config file interactively
    Init {
        /// Server URL (e.g. https://clh.example.com)
        #[arg(long)]
        url: String,
        /// Basic auth username
        #[arg(long)]
        user: Option<String>,
        /// Basic auth password
        #[arg(long)]
        password: Option<String>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None | Some(Commands::Search) => {
            let cfg = config::Config::load()?;
            search::run_search(&cfg)?;
        }
        Some(Commands::Add {
            hostname,
            pwd,
            command,
        }) => {
            add::run_add(&hostname, &pwd, &command)?;
        }
        Some(Commands::Setup) => {
            setup::print_setup();
        }
        Some(Commands::Config { action }) => match action {
            ConfigAction::Show => config::Config::show()?,
            ConfigAction::Init {
                url,
                user,
                password,
            } => {
                config::Config::init(&url, user.as_deref(), password.as_deref())?;
            }
        },
    }

    Ok(())
}
