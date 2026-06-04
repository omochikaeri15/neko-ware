mod config;
mod io;
mod keys;
pub mod patch;
pub mod workspace;
pub mod pem;

use clap::{CommandFactory, Parser, Subcommand};
use config::AppConfig;
use keys::UserKeys;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use colored::Colorize;
use tracing::Level;
use tracing_subscriber::fmt;

#[derive(Parser)]
#[command(name = "bcc-apk", version, about = "BCC Standalone APK Patcher", long_about = None)]
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
    Patch {
        #[arg(help = "Path to the target APK file")]
        apk_path: String,
        #[arg(short = 'p', long = "patch", help = "Override default patch directory")]
        patch_dir: Option<String>,
        #[arg(short = 'i', long = "icons", help = "Override default icons directory")]
        icons_dir: Option<String>,
        #[arg(short = 'l', long = "loose", help = "Override default loose directory")]
        loose_dir: Option<String>,
        #[arg(short = 'o', long = "output", help = "Override default APK creation directory")]
        output_dir: Option<String>,
        #[arg(short = 'n', long = "name", help = "Override application name")]
        app_name: Option<String>,
        #[arg(short = 'k', long = "package", alias = "pkg", help = "Override package suffix")]
        package_suffix: Option<String>,
        #[arg(short = 'r', long = "region", help = "Override target region (JP, EN, TW, KR)")]
        region: Option<String>,
        #[arg(short = 'f', long = "force", help = "Force 'update' (u) or 'create' (c) action")]
        force_action: Option<String>,
        #[arg(short = 'm', long = "pem", help = "Override default PEM identity file")]
        pem_file: Option<String>,
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
        Some(Commands::Patch { apk_path, patch_dir, icons_dir, loose_dir, output_dir, app_name, package_suffix, region, force_action, pem_file }) => {
            handle_patch_command(apk_path, patch_dir, icons_dir, loose_dir, output_dir, app_name, package_suffix, region, force_action, pem_file, show_ui)
        }
        None => handle_fallback_shell(),
    }
}

fn handle_pem_command(action_type: PemAction, show_ui: bool) {
    match action_type {
        PemAction::Generate => {
            if show_ui { println!("\n  {} Generating new RSA-2048 Identity", "!".yellow()); }
            tracing::info!("Generating new RSA-2048 Identity");

            match pem::generate_pem() {
                Ok(new_pem) => match pem::save_pem(&new_pem) {
                    Ok(_) => {
                        if show_ui { println!("  {} Successfully created and saved {}!\n", "✓".green(), "debug.pem".cyan()); }
                        tracing::info!("Successfully created and saved debug.pem");
                    },
                    Err(e) => {
                        if show_ui { println!("  {} Failed to save PEM: {}\n", "✗".red(), e); }
                        tracing::error!(error = %e, "Failed to save PEM");
                    },
                },
                Err(e) => {
                    if show_ui { println!("  {} Failed to generate PEM: {}\n", "✗".red(), e); }
                    tracing::error!(error = %e, "Failed to generate PEM");
                },
            }
        },
        PemAction::Env => {
            pem::print_env_template(show_ui);
        }
    }
}

fn handle_patch_command(
    target_apk: String,
    override_patch: Option<String>,
    override_icons: Option<String>,
    override_loose: Option<String>,
    override_output: Option<String>,
    override_name: Option<String>,
    override_package_suffix: Option<String>,
    override_region: Option<String>,
    override_force: Option<String>,
    override_pem: Option<String>,
    show_ui: bool,
) {
    let base_config = AppConfig::load();

    let final_patch_dir = override_patch.unwrap_or(base_config.patch_dir);
    let final_icons_dir = override_icons.unwrap_or(base_config.icons_dir);
    let final_loose_dir = override_loose.unwrap_or(base_config.loose_dir);
    let final_output_dir = override_output.unwrap_or(base_config.output_dir);
    let final_app_name = override_name.unwrap_or(base_config.app_name);
    let final_pem_file = override_pem.or(base_config.pem_file);

    let final_package_suffix = override_package_suffix
        .unwrap_or(base_config.package_suffix)
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>()
        .to_lowercase();

    let final_region = override_region
        .unwrap_or(base_config.region)
        .trim()
        .to_uppercase();

    let valid_regions = ["JP", "EN", "TW", "KR"];
    if !valid_regions.contains(&final_region.as_str()) {
        if show_ui { println!("\n  {} Invalid Region: '{}'. Must be JP, EN, TW, or KR.\n", "✗".red(), final_region.cyan()); }
        tracing::error!(region = %final_region, "Invalid region provided");
        return;
    }

    let final_force_action = override_force.map(|action_string| action_string.to_lowercase());
    if let Some(ref selected_action) = final_force_action {
        if !["update", "u", "create", "c"].contains(&selected_action.as_str()) {
            if show_ui { println!("\n  {} Invalid Force Flag: '{}'. Must be 'update' (u) or 'create' (c)\n", "✗".red(), selected_action.cyan()); }
            tracing::error!(flag = %selected_action, "Invalid force flag provided");
            return;
        }
    }

    let resolved_apk_path = PathBuf::from(&target_apk);
    if !resolved_apk_path.exists() {
        if show_ui { println!("\n  {} APK file not found at specified path\n", "✗".red()); }
        tracing::error!(path = %target_apk, "APK file not found");
        return;
    }

    match patch::apk::execute_patch(
        &resolved_apk_path,
        &PathBuf::from(final_patch_dir),
        &PathBuf::from(final_icons_dir),
        &PathBuf::from(final_loose_dir),
        &PathBuf::from(final_output_dir),
        &final_app_name,
        &final_package_suffix,
        &final_region,
        final_force_action,
        final_pem_file,
        show_ui,
    ) {
        Ok((action_verb, output_filename)) => {
            if show_ui { println!("\nSUCCESS: {} {}!\n", action_verb, output_filename.cyan()); }
            tracing::info!(action = %action_verb, file = %output_filename, "APK Patching complete");
        },
        Err(_) => {
            if show_ui { println!("\nFAILURE: Couldnt patch APK!\n"); }
            tracing::error!("Failed to patch APK");
            std::process::exit(1);
        }
    }
}

fn handle_init_command(show_ui: bool) {
    match workspace::init(show_ui) {
        Ok(_) => {
            if show_ui { println!("\n  {} Workspace initialized! Created config files and directories.\n", "✓".green()); }
            tracing::info!("Workspace initialized successfully");
        }
        Err(err) => {
            if show_ui { println!("\n  {} Failed to initialize workspace: {}\n", "✗".red(), err); }
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