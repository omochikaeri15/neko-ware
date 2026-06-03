mod io;
mod keys;
mod scanner;
mod decrypt;
pub mod workspace;

use clap::{CommandFactory, Parser, Subcommand};
use keys::UserKeys;
use std::process::Command as ProcessCommand;

#[derive(Parser)]
#[command(name = "bcc-pack", version, about = "BCC Standalone Pack Utility", long_about = None)]
struct Cli {
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
    },
}

#[derive(Subcommand)]
enum KeysAction {
    #[command(about = "Print current keys and validate them")]
    Print,
    #[command(about = "Initialize the \x1b[36mkeys.json\x1b[0m creation wizard")]
    Load,
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Init) => {
            if let Err(error) = workspace::init() {
                println!("\n\x1b[31m  ✗ Failed to initialize workspace: {}\x1b[0m\n", error);
            } else {
                println!("\n\x1b[32m  ✓ Workspace initialized! Created empty keys.json and decrypted directory\x1b[0m\n");
            }
        }
        Some(Commands::Keys { action }) => match action {
            KeysAction::Print => {
                let keys = UserKeys::load();
                keys.print_status();
            }
            KeysAction::Load => {
                UserKeys::prompt_interactive_load();
            }
        },
        Some(Commands::Decrypt { input }) => {
            decrypt::execute(&input);
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