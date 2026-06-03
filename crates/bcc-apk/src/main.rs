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

#[derive(Parser)]
#[command(name = "bcc-apk", version, about = "BCC Standalone APK Patcher", long_about = None)]
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
    #[command(about = "Initialize the \x1b[36mkeys.json\x1b[0m creation wizard")]
    Load,
}

#[derive(Subcommand)]
enum ConfigAction {
    #[command(about = "Reset \x1b[36mconfig.json\x1b[0m to factory defaults")]
    Reset,
    #[command(about = "Interactive configuration wizard for \x1b[36mconfig.json\x1b[0m")]
    Create,
}

#[derive(Subcommand)]
enum PemAction {
    #[command(about = "Generate a new custom debug.pem identity file")]
    Generate,
}

fn main() {
    let cli_arguments = Cli::parse();

    match cli_arguments.command {
        Some(Commands::Init) => handle_init_command(),
        Some(Commands::Pem { action }) => handle_pem_command(action),
        Some(Commands::Keys { action }) => handle_keys_command(action),
        Some(Commands::Config { action }) => handle_config_command(action),
        Some(Commands::Patch { apk_path, patch_dir, icons_dir, loose_dir, output_dir, app_name, package_suffix, region, force_action, pem_file }) => {
            handle_patch_command(apk_path, patch_dir, icons_dir, loose_dir, output_dir, app_name, package_suffix, region, force_action, pem_file)
        }
        None => handle_fallback_shell(),
    }
}

fn handle_pem_command(action_type: PemAction) {
    match action_type {
        PemAction::Generate => {
            println!("\n  \x1b[33m!\x1b[0m Generating new RSA-2048 Identity");
            match pem::generate_pem() {
                Ok(new_pem) => match pem::save_pem(&new_pem) {
                    Ok(_) => println!("  \x1b[32m✓\x1b[0m Successfully created and saved \x1b[36mdebug.pem\x1b[0m!\n"),
                    Err(e) => println!("  \x1b[31m✗\x1b[0m Failed to save PEM: {}\n", e),
                },
                Err(e) => println!("  \x1b[31m✗\x1b[0m Failed to generate PEM: {}\n", e),
            }
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
        println!("\n\x1b[31m  ✗ Invalid Region: '{}'. Must be JP, EN, TW, or KR.\x1b[0m\n", final_region);
        return;
    }

    let final_force_action = override_force.map(|action_string| action_string.to_lowercase());
    if let Some(ref selected_action) = final_force_action {
        if !["update", "u", "create", "c"].contains(&selected_action.as_str()) {
            println!("\n\x1b[31m  ✗ Invalid Force Flag: '{}'. Must be 'update' (u) or 'create' (c)\x1b[0m\n", selected_action);
            return;
        }
    }

    let resolved_apk_path = PathBuf::from(target_apk);
    if !resolved_apk_path.exists() {
        println!("\n\x1b[31m  ✗ APK file not found at specified path\x1b[0m\n");
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
    ) {
        Ok((action_verb, output_filename)) => {
            println!("\nSUCCESS: {} {}!\n", action_verb, output_filename);
        },
        Err(_) => {
            eprintln!("\nFAILURE: Couldnt patch APK!\n");
            std::process::exit(1);
        }
    }
}

fn handle_init_command() {
    match workspace::init() {
        Ok(_) => println!("\n\x1b[32m  ✓ Workspace initialized! Created config files and directories.\x1b[0m\n"),
        Err(init_error) => println!("\n\x1b[31m  ✗ Failed to initialize workspace: {}\x1b[0m\n", init_error),
    }
}

fn handle_keys_command(action_type: KeysAction) {
    match action_type {
        KeysAction::Print => {
            let current_keys = UserKeys::load();
            current_keys.print_status();
        }
        KeysAction::Load => {
            let _loaded_keys = UserKeys::prompt_interactive_load();
        }
    }
}

fn handle_config_command(action_type: ConfigAction) {
    match action_type {
        ConfigAction::Reset => AppConfig::reset(),
        ConfigAction::Create => AppConfig::create(),
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