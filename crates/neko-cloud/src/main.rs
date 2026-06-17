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
#[command(name = "neko-cloud", version, about = "Battle Cats Server Fetcher", long_about = None)]
struct Cli {
    #[arg(short, long, global = true, help = "Enable verbose debug logging")]
    verbose: bool,

    #[arg(short = 't', long, global = true, help = "Enable execution trace logging")]
    trace: bool,

    #[arg(short, long, global = true, help = "Output logs in JSON format")]
    json: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Manage the local application workspace")]
    Workspace {
        #[command(subcommand)]
        action: WorkspaceAction,
    },
    #[command(about = "Manage application configuration settings")]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    #[command(alias = "id", about = "Manage server identity credentials")]
    Identity {
        #[command(subcommand)]
        action: IdentityAction,
    },
    #[command(about = "Execute download pipelines against game servers")]
    Fetch {
        #[command(subcommand)]
        action: FetchAction,
    },
}

#[derive(Subcommand)]
enum FetchAction {
    #[command(about = "Target the cloud asset server for raw payload files")]
    Payload {
        #[arg(
            help = "Target payload timestamp string or path to game binary"
        )]
        input: String,

        #[arg(
            short = 'r',
            long = "region",
            default_value = "en",
            help = "Target game region code (en, jp, kr, tw)"
        )]
        region: String,
    },
}

#[derive(Subcommand)]
enum WorkspaceAction {
    #[command(about = "Initialize a new workspace environment")]
    Init,
    #[command(about = "Repair a corrupted workspace environment")]
    Repair,
}

#[derive(Subcommand)]
enum ConfigAction {
    #[command(about = "Create a new default configuration file")]
    Create,
    #[command(about = "Reset the configuration file to default settings")]
    Reset,
}

#[derive(Subcommand)]
enum IdentityAction {
    #[command(about = "Create a new server identity profile")]
    Create,
    #[command(about = "Reset the server identity profile")]
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
        Some(Commands::Fetch { action }) => {
            match action {
                FetchAction::Payload { input, region } => {
                    if let Err(error) = fetch::execute::execute_fetch(&input, &region, show_ui).await {
                        tracing::error!(error = %error, "Fetch operation failed");
                        if show_ui {
                            println!("\n  {} Fetch failed: {}\n", "✗".red(), error);
                        }
                        std::process::exit(1);
                    }
                }
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