#![allow(clippy::multiple_crate_versions)]
mod add;
mod client;
mod config;
mod models;
mod search;
mod setup;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "clh",
    about = "Fuzzy-search your command history via clh-server"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Fuzzy-search history and print selected command to stdout (default)
    Search {
        /// Filter results to this working directory
        #[arg(long)]
        pwd: Option<String>,
    },
    /// Record a command to the server (called from zsh precmd hook)
    Add {
        #[arg(long)]
        hostname: String,
        #[arg(long)]
        pwd: String,
        #[arg(long)]
        command: String,
    },
    /// Print shell integration script to stdout
    ///
    /// zsh:  eval "$(clh setup)"
    /// bash: eval "$(clh setup)"
    /// fish: clh setup | source
    Setup {
        /// Shell to generate integration for (default: auto-detect from $SHELL)
        #[arg(long, value_name = "SHELL")]
        shell: Option<String>,
    },
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
        /// Server URL (e.g. `<https://clh.example.com>`)
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
        None | Some(Commands::Search { pwd: None }) => {
            let cfg = config::Config::load()?;
            search::run_search(&cfg)?;
        }
        Some(Commands::Search { pwd: Some(pwd) }) => {
            let cfg = config::Config::load()?;
            search::run_search_with_pwd(&cfg, &pwd)?;
        }
        Some(Commands::Add {
            hostname,
            pwd,
            command,
        }) => {
            add::run_add(&hostname, &pwd, &command)?;
        }
        Some(Commands::Setup { shell }) => {
            setup::print_setup(shell.as_deref())?;
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
