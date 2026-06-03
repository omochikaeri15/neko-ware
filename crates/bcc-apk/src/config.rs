use crate::io::{load_local, save_local};
use serde::{Deserialize, Serialize};
use std::io::{stdin, stdout, Write};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AppConfig {
    pub app_name: String,
    pub package_suffix: String,
    pub region: String,
    pub patch_dir: String,
    pub loose_dir: String,
    pub icons_dir: String,
    pub output_dir: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            app_name: String::from("The Modded Cats"),
            package_suffix: String::from("mod"),
            region: String::from("EN"),
            patch_dir: String::from("mod/patch"),
            loose_dir: String::from("mod/loose"),
            icons_dir: String::from("mod/icons"),
            output_dir: String::from("apk"),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        if let Some(loaded_config) = load_local("config.json") {
            return loaded_config;
        }
        Self::default()
    }

    pub fn save(&self) {
        save_local("config.json", self);
    }

    pub fn reset() {
        let fresh_config = Self::default();
        fresh_config.save();
        println!("\n\x1b[32m  ✓ config.json has been reset to defaults.\x1b[0m\n");
    }

    pub fn create() {
        let mut active_config = Self::load();
        println!("\n--- BCC-APK Configuration Wizard ---");

        let user_name_input = request_user_input("Enter App Name: ");
        if !user_name_input.is_empty() { active_config.app_name = user_name_input; }

        let user_package_input = request_user_input("Enter Package Suffix: ");
        if !user_package_input.is_empty() {
            let sanitized_package: String = user_package_input.chars()
                .filter(|character| !character.is_whitespace())
                .collect();
            active_config.package_suffix = sanitized_package.to_lowercase();
        }

        println!("\nSelect Region:");
        println!("1. JP\n2. EN\n3. TW\n4. KR");
        let region_selection = request_user_input("Choice (1-4) [leave blank to skip]: ");

        match region_selection.as_str() {
            "1" => active_config.region = String::from("JP"),
            "2" => active_config.region = String::from("EN"),
            "3" => active_config.region = String::from("TW"),
            "4" => active_config.region = String::from("KR"),
            "" => {}
            _ => println!("\x1b[31mInvalid choice. Keeping current region.\x1b[0m"),
        }

        let patch_input = request_user_input("\nEnter Patch Directory: ");
        if !patch_input.is_empty() { active_config.patch_dir = patch_input; }

        let loose_input = request_user_input("Enter Loose Directory: ");
        if !loose_input.is_empty() { active_config.loose_dir = loose_input; }

        let icons_input = request_user_input("Enter Icons Directory: ");
        if !icons_input.is_empty() { active_config.icons_dir = icons_input; }

        let output_input = request_user_input("Enter Output Directory: ");
        if !output_input.is_empty() { active_config.output_dir = output_input; }

        active_config.save();
        println!("\n  \x1b[32m✓\x1b[0m Configuration saved to \x1b[36mconfig.json\x1b[0m\n");
    }
}

fn request_user_input(prompt_message: &str) -> String {
    print!("{}", prompt_message);
    let _flush_result = stdout().flush();
    let mut captured_input = String::new();
    let _read_result = stdin().read_line(&mut captured_input);
    captured_input.trim().to_string()
}