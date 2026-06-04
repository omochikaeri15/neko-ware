use std::fs;
use crate::io::get_local_dir;
use crate::keys::UserKeys;
use crate::config::AppConfig;
use tracing::debug;

const README_CONTENT: &[u8] = include_bytes!("../README.md");

pub fn init(_show_ui: bool) -> std::io::Result<()> {
    debug!("Initializing default workspace configurations...");
    let default_keys = UserKeys::default();
    default_keys.save();

    let active_config = AppConfig::default();
    active_config.save();

    let mut readme_path = get_local_dir();
    readme_path.push("README.md");
    fs::write(readme_path, README_CONTENT)?;

    let base_directory = get_local_dir();

    let mut mod_directory = base_directory.clone();
    mod_directory.push("mod");

    if mod_directory.exists() {
        debug!("Purging pre-existing mod directory.");
        let _removal_result = fs::remove_dir_all(&mod_directory);
    }

    let required_folder_names = vec![
        "mod/loose",
        "mod/patch",
        "mod/icons",
        "apk",
    ];

    debug!("Creating fresh workspace directories.");
    for target_folder in required_folder_names {
        let mut directory_path = base_directory.clone();
        directory_path.push(target_folder);
        fs::create_dir_all(&directory_path)?;
    }

    Ok(())
}