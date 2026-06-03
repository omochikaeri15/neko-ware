use std::fs;
use std::path::{Path, PathBuf};
use zip::ZipArchive;

use crate::keys::UserKeys;
use crate::patch::modify;
use crate::patch::pack;
use crate::patch::sign;

pub fn execute_patch(
    input_apk_path: &Path,
    patch_directory: &Path,
    icons_directory: &Path,
    loose_directory: &Path,
    output_directory_path: &Path,
    target_app_title: &str,
    target_package_suffix: &str,
    target_region: &str,
    force_action: Option<String>,
) -> Result<(String, String), String> {
    
    let current_keys = UserKeys::load();
    let valid_region_key = current_keys.get_validated_region_key(target_region).map_err(|error| {
        eprintln!("\n\x1b[31m  ✗ ERROR: {}\x1b[0m", error);
        error
    })?;

    let application_directory = PathBuf::from("app_workspace");
    let temporary_binary_directory = application_directory.join("binaries");
    let temporary_assets_directory = application_directory.join("assets");

    let _removal_result = fs::remove_dir_all(&application_directory);
    let _creation_result = fs::create_dir_all(&temporary_binary_directory);
    let _creation_result = fs::create_dir_all(&temporary_assets_directory);
    
    let source_apk_file = fs::File::open(input_apk_path).map_err(|error| {
        let out = format!("Failed to open APK: {}", error);
        eprintln!("\n\x1b[31m  ✗ ERROR: {}\x1b[0m", out);
        out
    })?;
    let mut zip_archive = ZipArchive::new(source_apk_file).map_err(|error| {
        let out = format!("Failed to read APK archive: {}", error);
        eprintln!("\n\x1b[31m  ✗ ERROR: {}\x1b[0m", out);
        out
    })?;

    let manifest_extraction_path = temporary_binary_directory.join("AndroidManifest.xml");
    let resource_extraction_path = temporary_binary_directory.join("resources.arsc");
    let mut extracted_resource_table = false;

    for archive_index in 0..zip_archive.len() {
        let mut inner_file = match zip_archive.by_index(archive_index) {
            Ok(file) => file,
            Err(_) => continue,
        };

        let inner_file_name = match inner_file.name() {
            Ok(name_cow) => name_cow.to_string(),
            Err(_) => continue,
        };

        if inner_file_name == "AndroidManifest.xml" {
            let mut output_manifest_file = fs::File::create(&manifest_extraction_path).map_err(|error| error.to_string())?;
            let _copy_result = std::io::copy(&mut inner_file, &mut output_manifest_file);
        } else if inner_file_name == "resources.arsc" {
            let mut output_resource_file = fs::File::create(&resource_extraction_path).map_err(|error| error.to_string())?;
            let _copy_result = std::io::copy(&mut inner_file, &mut output_resource_file);
            extracted_resource_table = true;
        }
    }
    drop(zip_archive);

    let optional_resource_path = if extracted_resource_table { Some(resource_extraction_path.as_path()) } else { None };

    let mut apk_manifest_editor = modify::ApkEditor::from_paths(&manifest_extraction_path, optional_resource_path)
        .map_err(|error| {
            let out = format!("Failed to parse APK binaries: {}", error);
            eprintln!("\n\x1b[31m  ✗ ERROR: {}\x1b[0m", out);
            out
        })?;
    
    let target_package_full = format!("jp.co.ponos.battlecats{}", target_package_suffix.trim());
    let current_package = apk_manifest_editor.get_current_package().unwrap_or_default();

    let is_update = match force_action.as_deref() {
        Some("update") | Some("u") => true,
        Some("create") | Some("c") => false,
        _ => current_package == target_package_full,
    };

    println!();
    if force_action.is_some() {
        println!("  \x1b[33m!\x1b[0m Bypassed identity check via \x1b[36mforce\x1b[0m flag");
    } else {
        println!("  \x1b[32m✓\x1b[0m Analyzed APK identity");
    }

    if !is_update {
        apk_manifest_editor.apply_patches(target_package_suffix, target_app_title)
            .map_err(|error| {
                let out = format!("Patch Error: {}", error);
                eprintln!("\x1b[31m  ✗ ERROR: {}\x1b[0m", out);
                out
            })?;

        apk_manifest_editor.save_to_paths(&manifest_extraction_path, optional_resource_path)
            .map_err(|error| {
                let out = format!("Failed to save patched binaries: {}", error);
                eprintln!("\x1b[31m  ✗ ERROR: {}\x1b[0m", out);
                out
            })?;
    }
    
    let packed_files_count = pack::stream_pack_and_list(
        patch_directory,
        &temporary_assets_directory,
        "DownloadLocal",
        valid_region_key
    ).map_err(|error| {
        eprintln!("\x1b[31m  ✗ ERROR: {}\x1b[0m", error);
        error
    })?;
    eprintln!("  \x1b[32m✓\x1b[0m Packaged \x1b[36m{}\x1b[0m files into a pack", packed_files_count);
    
    let unsigned_apk_path = application_directory.join("unsigned_final.apk");

    let injected_file_count = modify::inject_and_build_apk(
        input_apk_path,
        &unsigned_apk_path,
        &temporary_assets_directory,
        icons_directory,
        loose_directory,
        if is_update { None } else { Some(manifest_extraction_path.as_path()) },
        if is_update || !extracted_resource_table { None } else { Some(resource_extraction_path.as_path()) }
    ).map_err(|error| {
        eprintln!("\x1b[31m  ✗ ERROR: {}\x1b[0m", error);
        error
    })?;
    println!("  \x1b[32m✓\x1b[0m Rebuilt modified APK");
    println!("  \x1b[32m✓\x1b[0m Injected \x1b[36m{}\x1b[0m additional assets", injected_file_count);
    
    let normalized_apk_path = application_directory.join("normalized_final.apk");
    modify::normalize_apk(&unsigned_apk_path, &normalized_apk_path, input_apk_path).map_err(|error| {
        eprintln!("\x1b[31m  ✗ ERROR: {}\x1b[0m", error);
        error
    })?;
    println!("  \x1b[32m✓\x1b[0m Normalized binaries");
    
    sign::sign_apk_file(&normalized_apk_path).map_err(|error| {
        let out = error.to_string();
        eprintln!("\x1b[31m  ✗ ERROR: {}\x1b[0m", out);
        out
    })?;
    println!("  \x1b[32m✓\x1b[0m Signed APK");
    
    let (action_verb, destination_file_path) = if is_update {
        ("Updated".to_string(), input_apk_path.to_path_buf())
    } else {
        let _ = fs::create_dir_all(output_directory_path);

        let sanitized_title = target_app_title
            .trim()
            .replace("/", "_")
            .replace("\\", "_");

        let base_title = if sanitized_title.is_empty() { "modded_aligned".to_string() } else { sanitized_title };

        let mut chosen_filename = format!("{}.apk", base_title);

        if output_directory_path.join(&chosen_filename).exists() {
            let mut file_index = 1;
            loop {
                let candidate_name = format!("{}{}.apk", base_title, file_index);
                if !output_directory_path.join(&candidate_name).exists() {
                    chosen_filename = candidate_name;
                    break;
                }
                file_index += 1;
            }
        }

        ("Created".to_string(), output_directory_path.join(chosen_filename))
    };

    fs::copy(&normalized_apk_path, &destination_file_path).map_err(|error| {
        let out = format!("Failed to copy to output target: {}", error);
        eprintln!("\x1b[31m  ✗ ERROR: {}\x1b[0m", out);
        out
    })?;

    let _cleanup_result = fs::remove_dir_all(&application_directory);

    let output_display_name = destination_file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    Ok((action_verb, output_display_name))
}