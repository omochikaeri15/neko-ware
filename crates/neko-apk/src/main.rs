mod config;
mod io;
mod keys;
mod patch;
pub mod pem;
pub mod workspace;

use clap::{Args, CommandFactory, Parser, Subcommand};
use colored::Colorize;
use config::{AppConfig, OutputBehavior};
use keys::UserKeys;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use tracing::Level;
use tracing_subscriber::fmt;

#[derive(Parser)]
#[command(name = "neko-apk", version, about = "Standalone Battle Cats APK Patcher", long_about = None)]
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

#[derive(Args, Debug)]
pub struct PatchArgs {
    #[arg(help = "Path to the target APK file")]
    pub apk_path: String,
    #[arg(short = 'p', long = "patch", help = "Override default patch directory")]
    pub patch_dir: Option<String>,
    #[arg(short = 'i', long = "icons", help = "Override default icons directory")]
    pub icons_dir: Option<String>,
    #[arg(short = 'l', long = "loose", help = "Override default loose directory")]
    pub loose_dir: Option<String>,
    #[arg(short = 'c', long = "code", help = "Override default code directory")]
    pub code_dir: Option<String>,
    #[arg(short = 'o', long = "output", help = "Override default APK creation directory")]
    pub output_dir: Option<String>,
    #[arg(short = 'n', long = "name", help = "Override application name")]
    pub app_name: Option<String>,
    #[arg(short = 'k', long = "package", alias = "pkg", help = "Override package suffix")]
    pub package_suffix: Option<String>,
    #[arg(short = 'r', long = "region", help = "Override target region (JP, EN, TW, KR)")]
    pub region: Option<String>,
    #[arg(short = 'f', long = "force", help = "Force 'update' (u), 'create' (c), or 'automatic' (a) action")]
    pub force_action: Option<String>,
    #[arg(short = 'm', long = "pem", help = "Override default PEM identity file")]
    pub pem_file: Option<String>,
    #[arg(short = 'u', long = "architecture", help = "Override target architecture")]
    pub architecture: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Manage and view decryption keys")]
    Keys {
        #[command(subcommand)]
        action: KeysAction,
    },
    #[command(about = "Manage app configuration settings")]
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    #[command(about = "Initialize workspace or set everything to default")]
    Init,
    #[command(about = "Manage PEM identity files")]
    Pem {
        #[command(subcommand)]
        action: PemAction,
    },
    #[command(about = "Patch a specified APK file")]
    Patch(Box<PatchArgs>),
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

#[derive(Subcommand)]
enum ConfigAction {
    #[command(about = "Reset config.json to factory defaults")]
    Reset,
    #[command(about = "Interactive configuration wizard for config.json")]
    Create,
}

#[derive(Subcommand)]
enum PemAction {
    #[command(about = "Generate a new custom debug.pem identity file")]
    Generate,
    #[command(about = "Show required environment variables for headless configuration")]
    Env,
}

fn main() {
    #[cfg(windows)]
    let _ = colored::control::set_virtual_terminal(true);

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
        Some(Commands::Init) => handle_init_command(show_ui),
        Some(Commands::Pem { action }) => handle_pem_command(action, show_ui),
        Some(Commands::Keys { action }) => handle_keys_command(action, show_ui),
        Some(Commands::Config { action }) => handle_config_command(action, show_ui),
        Some(Commands::Patch(args)) => handle_patch_command(*args, show_ui),
        None => handle_fallback_shell(),
    }
}

fn handle_pem_command(action_type: PemAction, show_ui: bool) {
    match action_type {
        PemAction::Generate => {
            if show_ui {
                println!("\n  {} Generating new RSA-2048 Identity", "!".yellow());
            }
            tracing::info!("Generating new RSA-2048 Identity");

            match pem::generate_pem() {
                Ok(new_pem) => match pem::save_pem(&new_pem) {
                    Ok(_) => {
                        if show_ui {
                            println!(
                                "  {} Successfully created and saved {}!\n",
                                "✓".green(),
                                "debug.pem".cyan()
                            );
                        }
                        tracing::info!("Successfully created and saved debug.pem");
                    }
                    Err(err) => {
                        if show_ui {
                            println!("  {} Failed to save PEM: {}\n", "✗".red(), err);
                        }
                        tracing::error!(error = %err, "Failed to save PEM");
                    }
                },
                Err(err) => {
                    if show_ui {
                        println!("  {} Failed to generate PEM: {}\n", "✗".red(), err);
                    }
                    tracing::error!(error = %err, "Failed to generate PEM");
                }
            }
        }
        PemAction::Env => {
            pem::print_env_template(show_ui);
        }
    }
}

fn handle_patch_command(args: PatchArgs, show_ui: bool) {
    let base_config = AppConfig::load();

    let final_patch_dir = args.patch_dir.unwrap_or(base_config.patch_dir);
    let final_icons_dir = args.icons_dir.unwrap_or(base_config.icons_dir);
    let final_loose_dir = args.loose_dir.unwrap_or(base_config.loose_dir);
    let final_code_dir = args.code_dir.unwrap_or(base_config.code_dir);
    let final_output_dir = args.output_dir.unwrap_or(base_config.output_dir);
    let final_app_name = args.app_name.unwrap_or(base_config.app_name);
    let final_pem_file = args.pem_file.or(base_config.pem_file);
    let final_architecture = args.architecture.or(base_config.architecture);

    let final_package_suffix = args
        .package_suffix
        .unwrap_or(base_config.package_suffix)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_lowercase();

    let final_region = args.region.unwrap_or(base_config.region).trim().to_uppercase();

    let valid_regions = ["JP", "EN", "TW", "KR"];
    if !valid_regions.contains(&final_region.as_str()) {
        if show_ui {
            println!(
                "\n  {} Invalid Region: '{}' Must be JP, EN, TW, or KR\n",
                "✗".red(),
                final_region.cyan()
            );
        }
        tracing::error!(region = %final_region, "Invalid region provided");
        return;
    }

    let final_force_action = args.force_action.map(|action_string| action_string.to_lowercase());

    let final_behavior = if let Some(ref selected_action) = final_force_action {
        match selected_action.as_str() {
            "update" | "u" => OutputBehavior::Replace,
            "create" | "c" => OutputBehavior::Create,
            "automatic" | "auto" | "a" => OutputBehavior::Automatic,
            _ => {
                if show_ui {
                    println!(
                        "\n  {} Invalid Force Flag: '{}' Must be 'update' (u), 'create' (c) or 'automatic' (a)\n",
                        "✗".red(),
                        selected_action.cyan()
                    );
                }
                tracing::error!(flag = %selected_action, "Invalid force flag provided");
                return;
            }
        }
    } else {
        base_config.output_behavior
    };

    let resolved_apk_path = PathBuf::from(&args.apk_path);
    if !resolved_apk_path.exists() {
        if show_ui {
            println!("\n  {} APK file not found at specified path\n", "✗".red());
        }
        tracing::error!(path = %resolved_apk_path.display(), "APK file not found");
        return;
    }

    let patch_config = patch::apk::PatchConfig {
        input_apk_path: resolved_apk_path,
        patch_directory: PathBuf::from(&final_patch_dir),
        icons_directory: PathBuf::from(&final_icons_dir),
        loose_directory: PathBuf::from(&final_loose_dir),
        code_directory: PathBuf::from(&final_code_dir),
        output_directory_path: PathBuf::from(&final_output_dir),
        target_app_title: final_app_name,
        target_package_suffix: final_package_suffix,
        target_region: final_region,
        output_behavior: final_behavior,
        pem_file: final_pem_file,
        target_architecture: final_architecture,
        show_ui,
    };

    match patch::apk::execute_patch(&patch_config) {
        Ok((action_verb, output_filename)) => {
            if show_ui {
                println!("\nSUCCESS: {} {}!\n", action_verb, output_filename.cyan());
            }
            tracing::info!(action = %action_verb, file = %output_filename, "APK Patching complete");
        }
        Err(_) => {
            if show_ui {
                println!("\nFAILURE: Couldnt mod APK!\n");
            }
            tracing::error!("Failed to mod APK");
            std::process::exit(1);
        }
    }
}

fn handle_init_command(show_ui: bool) {
    match workspace::init(show_ui) {
        Ok(_) => {
            if show_ui {
                println!(
                    "\n  {} Workspace initialized, Created config files and directories\n",
                    "✓".green()
                );
            }
            tracing::info!("Workspace initialized successfully");
        }
        Err(err) => {
            if show_ui {
                println!("\n  {} Failed to initialize workspace: {}\n", "✗".red(), err);
            }
            tracing::error!(error = %err, "Failed to initialize workspace");
        }
    }
}

fn handle_keys_command(action_type: KeysAction, show_ui: bool) {
    match action_type {
        KeysAction::Print => {
            let current_keys = UserKeys::load();
            current_keys.print_status(show_ui);
        }
        KeysAction::Load => {
            let _loaded_keys = UserKeys::prompt_interactive_load(show_ui);
        }
        KeysAction::Env => {
            UserKeys::print_env_template(show_ui);
        }
    }
}

fn handle_config_command(action_type: ConfigAction, show_ui: bool) {
    match action_type {
        ConfigAction::Reset => AppConfig::reset(show_ui),
        ConfigAction::Create => AppConfig::create(show_ui),
    }
}

fn handle_fallback_shell() {
    let mut command_instance = Cli::command();
    let _help_print_result = command_instance.print_help();

    if cfg!(target_os = "windows") {
        let _process_result = ProcessCommand::new("cmd.exe").status();
        return;
    }

    let fallback_shell = std::env::var("SHELL").unwrap_or_else(|_environment_error| String::from("sh"));
    let _process_result = ProcessCommand::new(fallback_shell).status();
}