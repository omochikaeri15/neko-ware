use crate::io::{load_local, save_local};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::io::{Write, stdin, stdout};
use tracing::{error, info, debug};

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum OutputBehavior {
    Automatic,
    Replace,
    Create,
}

impl Default for OutputBehavior {
    fn default() -> Self {
        Self::Create
    }
}

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct AppConfig {
    pub app_name: String,
    pub package_suffix: String,
    pub region: String,
    pub architecture: Option<String>,
    pub output_behavior: OutputBehavior,
    pub pem_file: Option<String>,
    pub patch_dir: String,
    pub loose_dir: String,
    pub icons_dir: String,
    pub code_dir: String,
    pub output_dir: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            app_name: String::from("The Modded Cats"),
            package_suffix: String::from("mod"),
            region: String::from("EN"),
            architecture: None,
            output_behavior: OutputBehavior::default(),
            pem_file: None,
            patch_dir: String::from("mod/patch"),
            loose_dir: String::from("mod/loose"),
            icons_dir: String::from("mod/icons"),
            code_dir: String::from("mod/code"),
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

    pub fn repair(show_ui: bool) {
        let config_path = crate::io::get_exe_dir().join("config.json");
        let default_config = Self::default();
        let mut repaired = false;

        match std::fs::read_to_string(&config_path) {
            Ok(content) => {
                if let Ok(parsed) = serde_json::from_str::<Self>(&content) {
                    let serialized_test = serde_json::to_string_pretty(&parsed).unwrap_or_default();
                    if content.replace("\r\n", "\n").trim() != serialized_test.replace("\r\n", "\n").trim() {
                        parsed.save();
                        repaired = true;
                        debug!("Config structure/ordering differed. Re-ordered to defaults.");
                    }
                } else {
                    let mut salvaged_json = serde_json::to_value(&default_config).unwrap();

                    for line in content.lines() {
                        let trimmed = line.trim();
                        let Some((key_part, value_part)) = trimmed.split_once(':') else { continue; };

                        let key = key_part.trim().trim_matches('"');
                        let mut val_str = value_part.trim();
                        if val_str.ends_with(',') {
                            val_str = &val_str[..val_str.len() - 1];
                        }
                        val_str = val_str.trim();

                        let Some(target) = salvaged_json.get_mut(key) else { continue; };

                        if val_str.starts_with('"') && val_str.ends_with('"') {
                            *target = serde_json::Value::String(val_str.trim_matches('"').to_string());
                        } else if val_str == "true" {
                            *target = serde_json::Value::Bool(true);
                        } else if val_str == "false" {
                            *target = serde_json::Value::Bool(false);
                        } else if let Ok(num) = val_str.parse::<serde_json::Number>() {
                            *target = serde_json::Value::Number(num);
                        }
                    }

                    let final_config = serde_json::from_value::<Self>(salvaged_json).unwrap_or(default_config);
                    final_config.save();
                    repaired = true;
                    debug!("Config file partially corrupted. Salvaged successfully.");
                }
            }
            Err(_) => {
                default_config.save();
                repaired = true;
                debug!("Config missing entirely. Recreated with defaults.");
            }
        }

        if show_ui && repaired {
            println!("  {} Repaired {}", "✓".green(), "config.json".cyan());
        }
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
        println!("\n--- Neko-Apk Configuration Wizard ---");

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

        println!("\nSelect Output Behavior:");
        println!("1. Create (always create a new APK in output dir)");
        println!("2. Replace (always overwrite original APK)");
        println!("3. Automatic (scan APK identity to determine action)");
        let behavior_selection = request_user_input("Choice (1-3) [leave blank to skip]: ");

        match behavior_selection.as_str() {
            "1" => active_config.output_behavior = OutputBehavior::Create,
            "2" => active_config.output_behavior = OutputBehavior::Replace,
            "3" => active_config.output_behavior = OutputBehavior::Automatic,
            "" => {}
            _ => println!("{}", "Invalid choice. Keeping current output behavior.".red()),
        }

        let pem_input = request_user_input("Enter custom PEM identity file: ");
        if !pem_input.is_empty() {
            active_config.pem_file = Some(pem_input);
        } else {
            active_config.pem_file = None;
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