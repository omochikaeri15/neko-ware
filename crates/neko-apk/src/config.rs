use crate::io::{load_local, save_local};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::io::{Write, stdin, stdout};
use tracing::{error, info};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct AppConfig {
    pub app_name: String,
    pub package_suffix: String,
    pub region: String,
    pub patch_dir: String,
    pub loose_dir: String,
    pub icons_dir: String,
    pub code_dir: String,
    pub output_dir: String,
    pub pem_file: Option<String>,
    pub architecture: Option<String>,
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
            code_dir: String::from("mod/code"),
            output_dir: String::from("apk"),
            pem_file: None,
            architecture: None,
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

    pub fn reset(show_ui: bool) {
        let fresh_config = Self::default();
        fresh_config.save();
        if show_ui {
            println!(
                "\n  {} {} has been reset to defaults.\n",
                "✓".green(),
                "config.json".cyan()
            );
        }
        info!("Config reset to defaults");
    }

    pub fn create(show_ui: bool) {
        if !show_ui {
            error!("Interactive config loading requires standard UI mode.");
            std::process::exit(1);
        }

        let mut active_config = Self::load();
        println!("\n--- BCC-APK Configuration Wizard ---");

        let user_name_input = request_user_input("Enter App Name: ");
        if !user_name_input.is_empty() {
            active_config.app_name = user_name_input;
        }

        let user_package_input = request_user_input("Enter Package Suffix: ");
        if !user_package_input.is_empty() {
            let sanitized_package: String = user_package_input
                .chars()
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
            _ => println!("{}", "Invalid choice. Keeping current region.".red()),
        }

        println!("\nSelect Architecture:");
        println!("1. arm64-v8a\n2. armeabi-v7a\n3. x86\n4. x86_64\n5. None");
        let arch_selection = request_user_input("Choice (1-5) [leave blank to skip]: ");

        match arch_selection.as_str() {
            "1" => active_config.architecture = Some(String::from("arm64-v8a")),
            "2" => active_config.architecture = Some(String::from("armeabi-v7a")),
            "3" => active_config.architecture = Some(String::from("x86")),
            "4" => active_config.architecture = Some(String::from("x86_64")),
            "5" => active_config.architecture = None,
            "" => {}
            _ => println!("{}", "Invalid choice. Keeping current architecture.".red()),
        }

        let patch_input = request_user_input("\nEnter Patch Directory: ");
        if !patch_input.is_empty() {
            active_config.patch_dir = patch_input;
        }

        let loose_input = request_user_input("Enter Loose Directory: ");
        if !loose_input.is_empty() {
            active_config.loose_dir = loose_input;
        }

        let icons_input = request_user_input("Enter Icons Directory: ");
        if !icons_input.is_empty() {
            active_config.icons_dir = icons_input;
        }

        let output_input = request_user_input("Enter Output Directory: ");
        if !output_input.is_empty() {
            active_config.output_dir = output_input;
        }

        let code_input = request_user_input("Enter Code Directory: ");
        if !code_input.is_empty() {
            active_config.code_dir = code_input;
        }

        let pem_input = request_user_input("Enter custom PEM identity file: ");
        if !pem_input.is_empty() {
            active_config.pem_file = Some(pem_input);
        } else {
            active_config.pem_file = None;
        }

        active_config.save();
        println!("\n  {} Configuration saved to {}\n", "✓".green(), "config.json".cyan());
    }
}

fn request_user_input(prompt_message: &str) -> String {
    print!("{prompt_message}");
    let _flush_result = stdout().flush();
    let mut captured_input = String::new();
    let _read_result = stdin().read_line(&mut captured_input);
    captured_input.trim().to_string()
}