use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use crate::io::get_local_dir;
use crate::keys::UserKeys;
use crate::scanner::scan_and_resolve;
use nyanko::pack::cryptology::{self, check_integrity};

pub fn execute(input_target: &str) {
    let input_path = Path::new(input_target);
    let keys = UserKeys::load();
    let validations = keys.validate();
    let all_valid = validations.iter().all(|&(key, iv)| key && iv);

    if !all_valid {
        print!("\n\x1b[33mWARNING: Invalid or missing keys detected in 'keys' file, continue anyways? [Y/n]: \x1b[0m");
        let _ = std::io::stdout().flush();
        let mut choice = String::new();
        let _ = std::io::stdin().read_line(&mut choice);

        if choice.trim().to_lowercase() != "y" {
            println!("\nFAILURE: Session aborted!\n");
            return;
        }
        println!("\x1b[33mNOTE: You can create a 'keys.json' file by running the 'bcc-pack init' command.\x1b[0m");
    }

    let nyanko_keys = match keys.to_nyanko_keys() {
        Ok(valid_keys) => valid_keys,
        Err(error) => {
            println!("\x1b[31mERROR: Failed to parse keys for decryption: {}\x1b[0m", error);
            return;
        }
    };

    let pairs = match scan_and_resolve(input_path) {
        Ok(resolved_pairs) => resolved_pairs,
        Err(error) => {
            println!("\x1b[31m{}\x1b[0m", error);
            return;
        }
    };

    let mut output_base = get_local_dir();
    output_base.push("decrypted");
    let mut total_extracted_count = 0;

    for pair in pairs {
        let Ok(list_data) = fs::read(&pair.list_path) else {
            println!("\x1b[31m  ✗ Failed to extract files from {} (Could not read .list file)\x1b[0m", pair.name);
            continue;
        };

        let decoded_list_content = match cryptology::decrypt_list(&list_data) {
            Ok(content) => content,
            Err(_) => {
                println!("\x1b[31m  ✗ Failed to extract files from {} (List decryption failed)\x1b[0m", pair.name);
                continue;
            }
        };

        if decoded_list_content.trim().is_empty() {
            println!("\x1b[33m  ✗ No files found in {}\x1b[0m", pair.name);
            continue;
        }

        let mut pack_file = match fs::File::open(&pair.pack_path) {
            Ok(file) => file,
            Err(_) => {
                println!("\x1b[31m  ✗ Failed to extract files from {} (Could not open .pack file)\x1b[0m", pair.name);
                continue;
            }
        };

        let pack_output_dir = output_base.join(&pair.name);
        let mut extracted_count = 0;
        let mut corrupted_count = 0;

        for line in decoded_list_content.lines() {
            if line.trim().is_empty() { continue; }

            let parts: Vec<&str> = line.split(',').collect();
            if parts.len() < 3 { continue; }

            let asset_name = parts[0].trim();
            let Ok(offset): Result<u64, _> = parts[1].trim().parse() else { continue; };
            let Ok(size): Result<usize, _> = parts[2].trim().parse() else { continue; };

            if size == 0 { continue; }

            let memory_aligned_size = if size % 16 == 0 { size } else { ((size / 16) + 1) * 16 };
            let mut encrypted_buffer = vec![0u8; memory_aligned_size];

            if pack_file.seek(SeekFrom::Start(offset)).is_err() { continue; }
            if pack_file.read_exact(&mut encrypted_buffer).is_err() { continue; }

            let (decrypted_data, _) = cryptology::decrypt_chunk(&encrypted_buffer, asset_name, &nyanko_keys);
            let strict_limit = std::cmp::min(size, decrypted_data.len());
            let clean_data = &decrypted_data[..strict_limit];

            if !check_integrity(clean_data, asset_name) {
                corrupted_count += 1;
                continue;
            }

            if extracted_count == 0 {
                let _ = fs::create_dir_all(&pack_output_dir);
            }

            let final_path = pack_output_dir.join(asset_name);
            if let Some(parent_dir) = final_path.parent() {
                let _ = fs::create_dir_all(parent_dir);
            }

            if fs::write(&final_path, clean_data).is_ok() {
                extracted_count += 1;
            }
        }

        drop(pack_file);
        total_extracted_count += extracted_count;

        if extracted_count > 0 {
            if corrupted_count > 0 {
                println!("\x1b[31m  ✗ Skipped {} corrupted files in {}\x1b[0m", corrupted_count, pair.name);
            }
            println!("\x1b[32m  ✓ Extracted {} files to decrypted/{}/\x1b[0m", extracted_count, pair.name);
        } else if corrupted_count > 0 {
            println!("\x1b[31m  ✗ Skipped corrupted pack {}\x1b[0m", pair.name);
        } else {
            println!("\x1b[33m  ✗ No files found in {}\x1b[0m", pair.name);
        }
    }

    let mut temp_apk_dir = get_local_dir();
    temp_apk_dir.push("apk");

    if temp_apk_dir.exists() {
        if let Err(error) = fs::remove_dir_all(&temp_apk_dir) {
            println!("\n\x1b[33m  ⚠ ERROR: Could not delete temporary 'apk' directory: {}\x1b[0m", error);
        } else {
            println!("\x1b[32m  ✓ Cleaned up temporary APK files\x1b[0m");
        }
    }

    if total_extracted_count > 0 {
        println!("\nSUCCESS: Decrypted {} files!\n", total_extracted_count);
    } else {
        println!("\nFAILURE: Decrypted no files!\n");
    }
}