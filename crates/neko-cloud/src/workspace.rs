use crate::config::AppConfig;
use crate::identity::ServerIdentity;
use crate::io::get_local_dir;
use std::fs;
use tracing::debug;
use colored::Colorize;

pub fn init(show_ui: bool) -> std::io::Result<()> {
    debug!("Initializing default workspace configurations...");
    let active_config = AppConfig::default();
    active_config.save();

    let active_identity = ServerIdentity::default();
    active_identity.save();

    let base_directory = get_local_dir();
    let mut files_directory = base_directory.clone();
    files_directory.push(&active_config.output_dir);

    if !files_directory.exists() {
        fs::create_dir_all(&files_directory)?;
    }

    if show_ui {
        println!("\n  {} Workspace initialized, Created config files and directories\n", "✓".green());
    }

    Ok(())
}

pub fn repair(show_ui: bool) -> std::io::Result<()> {
    debug!("Repairing default workspace configurations...");

    AppConfig::repair(show_ui);

    let active_config = AppConfig::load();
    let identity_path = get_local_dir().join("identity.json");
    if !identity_path.exists() {
        debug!("Identity file missing, generating defaults.");
        ServerIdentity::default().save();
    }

    let base_directory = get_local_dir();
    let mut files_directory = base_directory.clone();
    files_directory.push(&active_config.output_dir);

    if !files_directory.exists() {
        fs::create_dir_all(&files_directory)?;
    }

    if show_ui {
        println!("\n  ✓ Workspace repaired successfully\n");
    }

    Ok(())
}