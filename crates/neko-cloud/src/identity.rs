use crate::config::request_user_input;
use crate::io::{load_local, save_local};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use tracing::{error, info};

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(default)]
pub struct ServerIdentity {
    pub key_pair_id: Option<String>,
    pub rsa_private_key: Option<String>,
    pub ponos_server_url: Option<String>,
}

impl ServerIdentity {
    pub fn load() -> Self {
        load_local("identity.json").unwrap_or_default()
    }

    pub fn save(&self) {
        save_local("identity.json", self);
    }

    pub fn reset(show_ui: bool) {
        let fresh_identity = Self::default();
        fresh_identity.save();
        if show_ui {
            println!(
                "\n  {} {} has been reset to defaults.\n",
                "✓".green(),
                "identity.json".cyan()
            );
        }
        info!("Identity reset to defaults");
    }

    pub fn create(show_ui: bool) {
        if !show_ui {
            error!("Interactive identity loading requires standard UI mode.");
            std::process::exit(1);
        }

        let mut active_identity = Self::load();
        println!("\n--- Neko-Fetch Identity Wizard ---");

        let key_id_input = request_user_input("Enter Key Pair ID: ");
        if !key_id_input.is_empty() {
            active_identity.key_pair_id = Some(key_id_input);
        }

        println!("Enter RSA Private Key (Empty line to finish):");
        let mut private_key_lines = Vec::new();
        loop {
            let line = request_user_input("");
            if line.is_empty() {
                break;
            }
            private_key_lines.push(line);
        }

        if !private_key_lines.is_empty() {
            active_identity.rsa_private_key = Some(private_key_lines.join("\n"));
        }

        let url_input = request_user_input("Enter PONOS Server URL:");
        if !url_input.is_empty() {
            active_identity.ponos_server_url = Some(url_input);
        }

        active_identity.save();
        println!(
            "\n  {} Identity saved to {}\n",
            "✓".green(),
            "identity.json".cyan()
        );
    }
}