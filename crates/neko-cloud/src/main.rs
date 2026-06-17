mod config;
mod fetch;
mod identity;
mod io;
mod workspace;

use clap::{CommandFactory, Parser, Subcommand};
use colored::Colorize;
use std::process::Command as ProcessCommand;
use tracing::Level;
use tracing_subscriber::fmt;

#[derive(Parser)]
#[command(name = "neko-cloud", version, about = "Standalone Battle Cats Server Fetcher", long_about = None)]
struct Cli {
    #[arg(short, long, global = true)]
    verbose: bool,
    #[arg(short = 't', long, global = true)]
    trace: bool,
    #[arg(short, long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    #[command(alias = "id")]
    Identity {
        #[command(subcommand)]
        action: IdentityAction,
    },
    Fetch {
        #[arg(short = 'u', long = "update", value_name = "VERSION", required = true)]
        update: String,
    },
}

#[derive(Subcommand)]
enum WorkspaceAction {
    Init,
    Repair,
}

#[derive(Subcommand)]
enum ConfigAction {
    Create,
    Reset,
}

#[derive(Subcommand)]
enum IdentityAction {
    Create,
    Reset,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let show_ui = !cli.json && !cli.verbose && !cli.trace;

    if cli.json {
        colored::control::set_override(false);
        let max_level = if cli.trace {
            Level::TRACE
        } else if cli.verbose {
            Level::DEBUG
        } else {
            Level::INFO
        };
        fmt()
            .json()
            .with_file(true)
            .with_line_number(true)
            .with_max_level(max_level)
            .init();
    } else if cli.trace {
        fmt()
            .with_file(true)
            .with_line_number(true)
            .with_max_level(Level::TRACE)
            .init();
    } else if cli.verbose {
        fmt()
            .with_file(true)
            .with_line_number(true)
            .with_max_level(Level::DEBUG)
            .init();
    }

    match cli.command {
        Some(Commands::Workspace { action }) => handle_workspace_command(action, show_ui),
        Some(Commands::Config { action }) => handle_config_command(action, show_ui),
        Some(Commands::Identity { action }) => handle_identity_command(action, show_ui),
        Some(Commands::Fetch { update }) => {
            if let Err(error) = fetch::execute::execute_fetch(&update, show_ui).await {
                tracing::error!(error = %error, "Fetch operation failed");
                if show_ui {
                    println!("\n  {} Fetch failed: {}\n", "✗".red(), error);
                }
                std::process::exit(1);
            }
        }
        None => handle_fallback_shell(),
    }
}

fn handle_workspace_command(action_type: WorkspaceAction, show_ui: bool) {
    match action_type {
        WorkspaceAction::Init => {
            if let Err(error) = workspace::init(show_ui) {
                tracing::error!(error = %error, "Workspace init failed");
            }
        }
        WorkspaceAction::Repair => {
            if let Err(error) = workspace::repair(show_ui) {
                tracing::error!(error = %error, "Workspace repair failed");
            }
        }
    }
}

fn handle_config_command(action_type: ConfigAction, show_ui: bool) {
    match action_type {
        ConfigAction::Create => config::AppConfig::create(show_ui),
        ConfigAction::Reset => config::AppConfig::reset(show_ui),
    }
}

fn handle_identity_command(action_type: IdentityAction, show_ui: bool) {
    match action_type {
        IdentityAction::Create => identity::ServerIdentity::create(show_ui),
        IdentityAction::Reset => identity::ServerIdentity::reset(show_ui),
    }
}

fn handle_fallback_shell() {
    let mut command_instance = Cli::command();
    let _ = command_instance.print_help();

    if cfg!(target_os = "windows") {
        let _ = ProcessCommand::new("cmd.exe").status();
        return;
    }

    let fallback_shell = std::env::var("SHELL").unwrap_or_else(|_| String::from("sh"));
    let _ = ProcessCommand::new(fallback_shell).status();
}