use crate::keys::RegionKey;
use nyanko::pack::cryptology;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::Path;
use tracing::{debug, trace};

pub fn stream_pack_and_list(
    source_directory: &Path,
    destination_directory: &Path,
    target_pack_name: &str,
    region_cryptology_key: &RegionKey,
) -> Result<usize, String> {
    let mut valid_files_with_sizes = Vec::new();

    if let Ok(directory_entries) = fs::read_dir(source_directory) {
        for entry_result in directory_entries.flatten() {
            let file_path = entry_result.path();
            if file_path.is_file()
                && let Ok(file_metadata) = fs::metadata(&file_path)
            {
                valid_files_with_sizes.push((file_path, file_metadata.len() as usize));
            }
        }
    }

    let total_files_count = valid_files_with_sizes.len();
    if total_files_count == 0 {
        return Err("No files found in the patch directory.".to_string());
    }

    let lowercase_pack_name = target_pack_name.to_lowercase();
    let resolved_pack_type = if lowercase_pack_name.contains("imagedatalocal") {
        cryptology::PackType::ImageData
    } else if lowercase_pack_name.contains("server") {
        cryptology::PackType::Server
    } else {
        cryptology::PackType::Standard
    };

    let parsed_standard_keys = if resolved_pack_type == cryptology::PackType::Standard {
        let decoded_key_bytes =
            hex::decode(&region_cryptology_key.key).map_err(|_| "Invalid Region Key Hex".to_string())?;
        let decoded_iv_bytes =
            hex::decode(&region_cryptology_key.iv).map_err(|_| "Invalid Region IV Hex".to_string())?;

        if decoded_key_bytes.len() != 16 || decoded_iv_bytes.len() != 16 {
            return Err("Region Key/IV length is incorrect. Ensure they are 32 hex characters.".to_string());
        }

        let standard_key_array: [u8; 16] = decoded_key_bytes.try_into().map_err(|_| "Failed to map key array")?;
        let standard_iv_array: [u8; 16] = decoded_iv_bytes.try_into().map_err(|_| "Failed to map IV array")?;

        Some((standard_key_array, standard_iv_array))
    } else {
        None
    };

    let pack_output_path = destination_directory.join(format!("{}.pack", target_pack_name));
    let list_output_path = destination_directory.join(format!("{}.list", target_pack_name));

    let generated_pack_file =
        File::create(&pack_output_path).map_err(|error| format!("Failed to create pack stream file: {}", error))?;
    let mut buffered_pack_writer = BufWriter::new(generated_pack_file);

    let mut cumulative_list_string = format!("{}\n", total_files_count);
    let mut current_byte_address = 0;

    for (active_file_path, _file_size) in valid_files_with_sizes.iter() {
        let extracted_filename = active_file_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let mut raw_file_data =
            fs::read(active_file_path).map_err(|error| format!("Failed to read {}: {}", extracted_filename, error))?;

        let (active_cipher_key, active_cipher_iv) = match &parsed_standard_keys {
            Some((resolved_key_array, resolved_iv_array)) => (Some(resolved_key_array), Some(resolved_iv_array)),
            None => (None, None),
        };

        raw_file_data =
            cryptology::encrypt_chunk(&raw_file_data, resolved_pack_type, active_cipher_key, active_cipher_iv)
                .map_err(|error| format!("Encryption failed for {}: {}", extracted_filename, error))?;

        buffered_pack_writer
            .write_all(&raw_file_data)
            .map_err(|error| format!("Failed to write to pack buffer: {}", error))?;

        let newly_encrypted_size = raw_file_data.len();
        cumulative_list_string.push_str(&format!(
            "{},{},{}\n",
            extracted_filename, current_byte_address, newly_encrypted_size
        ));
        current_byte_address += newly_encrypted_size;

        trace!(file = %extracted_filename, size = newly_encrypted_size, "Encrypted and packed file into output stream");
    }

    buffered_pack_writer
        .flush()
        .map_err(|error| format!("Failed to flush pack stream to disk: {}", error))?;
    debug!("Successfully flushed multi-gigabyte pack stream buffer to physical disk");

    let encrypted_list_bytes = cryptology::encrypt_list(&cumulative_list_string)
        .map_err(|error| format!("Failed to encrypt list file: {}", error))?;

    fs::write(list_output_path, encrypted_list_bytes)
        .map_err(|error| format!("Failed to write list file: {}", error))?;

    Ok(total_files_count)
}
