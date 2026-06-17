use crate::io::{load_local, save_local};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use std::io::{stdin, stdout, Write};
use tracing::{debug, error, info};

#[derive(Clone, Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct AppConfig {
    pub output_dir: String,
    pub search_index_range: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            output_dir: String::from("files"),
            search_index_range: Some(String::from("0-50")),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        load_local("config.json").unwrap_or_default()
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
        println!("\n--- Neko-Cloud Configuration Wizard ---");

        let output_input = request_user_input("Enter Output Directory (default 'files'): ");
        if !output_input.is_empty() {
            active_config.output_dir = output_input;
        }

        let range_input = request_user_input("Enter Search Index Range (default '0-350'): ");
        if !range_input.is_empty() {
            if range_input.contains('-') {
                active_config.search_index_range = Some(range_input);
            } else {
                println!("  {} Invalid range format. Please use 'start-end' (e.g., '0-350'). Retaining previous value.", "⚠".yellow());
            }
        }

        active_config.save();
        println!(
            "\n  {} Configuration saved to {}\n",
            "✓".green(),
            "config.json".cyan()
        );
    }

    pub fn repair(_show_ui: bool) {
        let config_path = crate::io::get_exe_dir().join("config.json");
        if !config_path.exists() {
            Self::default().save();
            debug!("Config missing entirely. Recreated with defaults.");
        }
    }
}

pub fn request_user_input(prompt_message: &str) -> String {
    print!("{prompt_message}");
    let _ = stdout().flush();
    let mut captured_input = String::new();
    let _ = stdin().read_line(&mut captured_input);
    captured_input.trim().to_string()
}