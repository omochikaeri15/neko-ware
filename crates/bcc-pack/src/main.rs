mod io;
mod keys;
mod scanner;
mod decrypt;
pub mod workspace;

use clap::{CommandFactory, Parser, Subcommand};
use keys::UserKeys;
use std::process::Command as ProcessCommand;
use colored::Colorize;
use tracing::{error, Level};
use tracing_subscriber::fmt;

#[derive(Parser)]
#[command(name = "bcc-pack", version, about = "BCC Standalone Pack Utility", long_about = None)]
struct Cli {
    #[arg(short, long, global = true, help = "Enable verbose debug logging")]
    verbose: bool,
    #[arg(short = 't', long, global = true, help = "Enable maximum trace-level logging")]
    trace: bool,
    #[arg(short, long, global = true, help = "Output logs in structured JSON format")]
    json: bool,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Manage and view decryption keys")]
    Keys {
        #[command(subcommand)]
        action: KeysAction,
    },
    #[command(about = "Initialize workspace or set everything to default")]
    Init,
    #[command(about = "Decrypt game files from various formats")]
    Decrypt {
        #[arg(value_name = "PACK | LIST | APK | DIR")]
        input: String,
        #[arg(short, long, help = "Force decryption and skip key validation prompts")]
        force: bool,
        #[arg(short, long, help = "Override the default output directory")]
        output: Option<String>,
    },
}

#[derive(Subcommand)]
enum KeysAction {
    #[command(about = "Print current keys and validate them")]
    Print,
    #[command(about = "Initialize the keys.json creation wizard")]
    Load,
    #[command(about = "Show required environment variables for headless configuration")]
    Env,
}

fn main() {
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
        Some(Commands::Init) => {
            if let Err(err) = workspace::init() {
                if show_ui {
                    println!("\n  {} Failed to initialize workspace: {}\n", "✗".red(), err);
                }
                error!("Failed to initialize workspace: {}", err);
            } else {
                if show_ui {
                    println!("\n  {} Workspace initialized! Created empty keys.json and decrypted directory\n", "✓".green());
                }
            }
        }
        Some(Commands::Keys { action }) => match action {
            KeysAction::Print => {
                let keys = UserKeys::load();
                keys.print_status(show_ui);
            }
            KeysAction::Load => {
                UserKeys::prompt_interactive_load(show_ui);
            }
            KeysAction::Env => {
                UserKeys::print_env_template(show_ui);
            }
        },
        Some(Commands::Decrypt { input, force, output }) => {
            decrypt::execute(&input, show_ui, force, output.as_deref());
        }
        None => {
            let mut cmd = Cli::command();
            let _ = cmd.print_help();

            if cfg!(target_os = "windows") {
                let _ = ProcessCommand::new("cmd.exe").status();
            } else {
                let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());
                let _ = ProcessCommand::new(shell).status();
            }
        }
    }
}