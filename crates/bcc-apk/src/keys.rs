use crate::io::{load_local, save_local};
use colored::{ColoredString, Colorize};
use serde::{Deserialize, Serialize};
use std::io::{Write, stdin, stdout};
use tracing::{error, info};

pub const EXPECTED_HASHES: [(&str, &str); 4] = [
    ("bac299d3cf278544782427ff7c71ef58", "6910fae125547fd957a505c67e1c72bd"),
    ("b9e48b02312e5b3dd60194a03157d70c", "45cad482726268e341f5759230ce8cff"),
    ("264a0ffd5f69d257284b93ae881ce2b6", "213cecb58af008964303ecb2cf0f5373"),
    ("3d22eafdcc4fc2a1379b103970b36217", "4cacdb0839634116caaf0b966638865b"),
];

#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct RegionKey {
    pub key: String,
    pub iv: String,
}

#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Debug)]
pub struct UserKeys {
    #[serde(alias = "jp")]
    pub ja: RegionKey,
    pub en: RegionKey,
    pub tw: RegionKey,
    #[serde(alias = "kr")]
    pub ko: RegionKey,
}

impl UserKeys {
    pub fn load() -> Self {
        let mut current_keys: Self = if let Some(json_keys) = load_local("keys.json") {
            json_keys
        } else {
            load_local("keys").unwrap_or_default()
        };

        if let Ok(env_key) = std::env::var("BCC_KEY_JP") {
            current_keys.ja.key = env_key;
        }
        if let Ok(env_iv) = std::env::var("BCC_IV_JP") {
            current_keys.ja.iv = env_iv;
        }

        if let Ok(env_key) = std::env::var("BCC_KEY_EN") {
            current_keys.en.key = env_key;
        }
        if let Ok(env_iv) = std::env::var("BCC_IV_EN") {
            current_keys.en.iv = env_iv;
        }

        if let Ok(env_key) = std::env::var("BCC_KEY_TW") {
            current_keys.tw.key = env_key;
        }
        if let Ok(env_iv) = std::env::var("BCC_IV_TW") {
            current_keys.tw.iv = env_iv;
        }

        if let Ok(env_key) = std::env::var("BCC_KEY_KR") {
            current_keys.ko.key = env_key;
        }
        if let Ok(env_iv) = std::env::var("BCC_IV_KR") {
            current_keys.ko.iv = env_iv;
        }

        current_keys
    }

    pub fn save(&self) {
        save_local("keys.json", self);
    }

    pub fn print_status(&self, show_ui: bool) {
        if !show_ui {
            info!(keys = ?self, "Current user decryption keys");
            return;
        }

        let validation_results = self.validate();

        println!("=================================================================================");
        println!("{:<6} | {:<34} | {:<34}", "REGION", "KEY", "IV");
        println!("-------+------------------------------------+------------------------------------");

        print_region_row("JP", &self.ja.key, &self.ja.iv, validation_results[0]);
        print_region_row("EN", &self.en.key, &self.en.iv, validation_results[1]);
        print_region_row("TW", &self.tw.key, &self.tw.iv, validation_results[2]);
        print_region_row("KR", &self.ko.key, &self.ko.iv, validation_results[3]);

        println!("=================================================================================");
    }

    pub fn print_env_template(show_ui: bool) {
        if !show_ui {
            info!(
                msg = "Environment variable configuration requirements",
                required_vars =
                    "BCC_KEY_JP, BCC_IV_JP, BCC_KEY_EN, BCC_IV_EN, BCC_KEY_TW, BCC_IV_TW, BCC_KEY_KR, BCC_IV_KR"
            );
            return;
        }

        println!("\n=================================================================================");
        println!("                   BCC HEADLESS ENVIRONMENT VARIABLES                            ");
        println!("=================================================================================");
        println!("To bypass 'keys.json', export the following hexadecimal keys into your system:\n");

        println!(
            "  {:<15} : Hex-encoded decryption key for the Japanese region",
            "BCC_KEY_JP".cyan().bold()
        );
        println!(
            "  {:<15} : 16-byte initialization vector for the Japanese region",
            "BCC_IV_JP".cyan().bold()
        );
        println!("---------------------------------------------------------------------------------");
        println!(
            "  {:<15} : Hex-encoded decryption key for the English region",
            "BCC_KEY_EN".cyan().bold()
        );
        println!(
            "  {:<15} : 16-byte initialization vector for the English region",
            "BCC_IV_EN".cyan().bold()
        );
        println!("---------------------------------------------------------------------------------");
        println!(
            "  {:<15} : Hex-encoded decryption key for the Taiwanese region",
            "BCC_KEY_TW".cyan().bold()
        );
        println!(
            "  {:<15} : 16-byte initialization vector for the Taiwanese region",
            "BCC_IV_TW".cyan().bold()
        );
        println!("---------------------------------------------------------------------------------");
        println!(
            "  {:<15} : Hex-encoded decryption key for the Korean region",
            "BCC_KEY_KR".cyan().bold()
        );
        println!(
            "  {:<15} : 16-byte initialization vector for the Korean region",
            "BCC_IV_KR".cyan().bold()
        );
        println!("=================================================================================");

        println!(
            "\n{}: Example configuration inside a bash script or ecosystem file:",
            "TIP".green().bold()
        );
        println!(
            "{}",
            "  export BCC_KEY_EN=\"0123456789abcdef0123456789abcdef\"".bright_black()
        );
        println!(
            "{}",
            "  export BCC_IV_EN=\"abcdef0123456789abcdef0123456789\"".bright_black()
        );
        println!();
    }

    pub fn prompt_interactive_load(show_ui: bool) -> Self {
        if !show_ui {
            error!("Interactive key loading requires standard UI mode.");
            std::process::exit(1);
        }

        let mut updated_keys = Self::load();
        println!("\n--- BCC Key Configuration Wizard ---");
        println!("Paste your Hex keys and IVs below. Leave blank to skip a field.\n");

        let input_ja_key = prompt_for_field("Enter JP Key: ");
        if !input_ja_key.is_empty() {
            updated_keys.ja.key = input_ja_key;
        }

        let input_ja_iv = prompt_for_field("Enter JP IV : ");
        if !input_ja_iv.is_empty() {
            updated_keys.ja.iv = input_ja_iv;
        }

        let input_en_key = prompt_for_field("Enter EN Key: ");
        if !input_en_key.is_empty() {
            updated_keys.en.key = input_en_key;
        }

        let input_en_iv = prompt_for_field("Enter EN IV : ");
        if !input_en_iv.is_empty() {
            updated_keys.en.iv = input_en_iv;
        }

        let input_tw_key = prompt_for_field("Enter TW Key: ");
        if !input_tw_key.is_empty() {
            updated_keys.tw.key = input_tw_key;
        }

        let input_tw_iv = prompt_for_field("Enter TW IV : ");
        if !input_tw_iv.is_empty() {
            updated_keys.tw.iv = input_tw_iv;
        }

        let input_ko_key = prompt_for_field("Enter KR Key: ");
        if !input_ko_key.is_empty() {
            updated_keys.ko.key = input_ko_key;
        }

        let input_ko_iv = prompt_for_field("Enter KR IV : ");
        if !input_ko_iv.is_empty() {
            updated_keys.ko.iv = input_ko_iv;
        }

        updated_keys.save();
        println!("\nSUCCESS: Configuration saved to neighboring 'keys.json' file.\n");
        updated_keys
    }

    pub fn validate(&self) -> [(bool, bool); 4] {
        [
            (
                validate_hash(&self.ja.key, EXPECTED_HASHES[0].0),
                validate_hash(&self.ja.iv, EXPECTED_HASHES[0].1),
            ),
            (
                validate_hash(&self.en.key, EXPECTED_HASHES[1].0),
                validate_hash(&self.en.iv, EXPECTED_HASHES[1].1),
            ),
            (
                validate_hash(&self.tw.key, EXPECTED_HASHES[2].0),
                validate_hash(&self.tw.iv, EXPECTED_HASHES[2].1),
            ),
            (
                validate_hash(&self.ko.key, EXPECTED_HASHES[3].0),
                validate_hash(&self.ko.iv, EXPECTED_HASHES[3].1),
            ),
        ]
    }

    pub fn get_validated_region_key(&self, target_region: &str) -> Result<&RegionKey, String> {
        let (region_key, expected_hash) = match target_region {
            "JP" => (&self.ja, EXPECTED_HASHES[0]),
            "EN" => (&self.en, EXPECTED_HASHES[1]),
            "TW" => (&self.tw, EXPECTED_HASHES[2]),
            "KR" => (&self.ko, EXPECTED_HASHES[3]),
            _ => return Err(format!("Unknown region identifier: {}", target_region)),
        };

        if !validate_hash(&region_key.key, expected_hash.0) {
            return Err(format!("{} Region Key is invalid or missing", target_region));
        }
        if !validate_hash(&region_key.iv, expected_hash.1) {
            return Err(format!("{} Region IV is invalid or missing", target_region));
        }

        Ok(region_key)
    }
}

fn print_region_row(region_name: &str, key_value: &str, iv_value: &str, is_valid: (bool, bool)) {
    let formatted_key = format_table_cell(key_value, is_valid.0);
    let formatted_iv = format_table_cell(iv_value, is_valid.1);
    println!("{:<6} | {} | {}", region_name, formatted_key, formatted_iv);
}

fn format_table_cell(cell_value: &str, is_valid: bool) -> ColoredString {
    let quoted_string = format!("\"{}\"", cell_value);
    let padded_string = format!("{:<34}", quoted_string);

    if is_valid {
        padded_string.green()
    } else {
        padded_string.red()
    }
}

fn prompt_for_field(label_message: &str) -> String {
    print!("{}", label_message);
    if stdout().flush().is_err() {
        return String::new();
    }

    let mut user_input = String::new();
    if stdin().read_line(&mut user_input).is_err() {
        return String::new();
    }

    user_input.retain(|character| !character.is_whitespace());
    user_input
}

fn validate_hash(input_value: &str, expected_hash: &str) -> bool {
    let cleaned_value = input_value.trim();
    if cleaned_value.is_empty() {
        return false;
    }

    let computed_hash = format!("{:x}", md5::compute(cleaned_value.as_bytes()));
    computed_hash == expected_hash
}
