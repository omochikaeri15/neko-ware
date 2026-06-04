use std::fs;
use std::path::{Path, PathBuf};
use zip::ZipArchive;
use colored::Colorize;
use tracing::{info, warn, error, debug};

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
    pem_file: Option<String>,
    show_ui: bool,
) -> Result<(String, String), String> {
    debug!(target = %input_apk_path.display(), "Initiating APK patch cycle");

    let current_keys = UserKeys::load();
    let valid_region_key = current_keys.get_validated_region_key(target_region).map_err(|err| {
        if show_ui { println!("\n  {} ERROR: {}", "✗".red(), err); }
        error!(error = %err, "Failed to retrieve validated region key");
        err
    })?;

    let application_directory = PathBuf::from("app_workspace");
    let temporary_binary_directory = application_directory.join("binaries");
    let temporary_assets_directory = application_directory.join("assets");

    let _removal_result = fs::remove_dir_all(&application_directory);
    let _creation_result = fs::create_dir_all(&temporary_binary_directory);
    let _creation_result = fs::create_dir_all(&temporary_assets_directory);

    let source_apk_file = fs::File::open(input_apk_path).map_err(|err| {
        let out = format!("Failed to open APK: {}", err);
        if show_ui { println!("\n  {} ERROR: {}", "✗".red(), out); }
        error!(error = %err, "Failed to open source APK file");
        out
    })?;
    let mut zip_archive = ZipArchive::new(source_apk_file).map_err(|err| {
        let out = format!("Failed to read APK archive: {}", err);
        if show_ui { println!("\n  {} ERROR: {}", "✗".red(), out); }
        error!(error = %err, "Failed to read APK archive");
        out
    })?;

    let manifest_extraction_path = temporary_binary_directory.join("AndroidManifest.xml");
    let resource_extraction_path = temporary_binary_directory.join("resources.arsc");
    let mut extracted_resource_table = false;

    debug!("Extracting core manifest and resource tables");
    for archive_index in 0..zip_archive.len() {
        let mut inner_file = match zip_archive.by_index(archive_index) {
            Ok(file) => file,
            Err(_) => continue,
        };

        let inner_file_name = inner_file.name().to_string();

        if inner_file_name == "AndroidManifest.xml" {
            let mut output_manifest_file = fs::File::create(&manifest_extraction_path).map_err(|err| err.to_string())?;
            let _copy_result = std::io::copy(&mut inner_file, &mut output_manifest_file);
        } else if inner_file_name == "resources.arsc" {
            let mut output_resource_file = fs::File::create(&resource_extraction_path).map_err(|err| err.to_string())?;
            let _copy_result = std::io::copy(&mut inner_file, &mut output_resource_file);
            extracted_resource_table = true;
        }
    }
    drop(zip_archive);

    let optional_resource_path = if extracted_resource_table { Some(resource_extraction_path.as_path()) } else { None };

    let mut apk_manifest_editor = modify::ApkEditor::from_paths(&manifest_extraction_path, optional_resource_path)
        .map_err(|err| {
            let out = format!("Failed to parse APK binaries: {}", err);
            if show_ui { println!("\n  {} ERROR: {}", "✗".red(), out); }
            error!(error = %err, "Failed to parse APK binaries");
            out
        })?;

    let target_package_full = format!("jp.co.ponos.battlecats{}", target_package_suffix.trim());
    let current_package = apk_manifest_editor.get_current_package().unwrap_or_default();

    let is_update = match force_action.as_deref() {
        Some("update") | Some("u") => true,
        Some("create") | Some("c") => false,
        _ => current_package == target_package_full,
    };

    if show_ui { println!(); }
    if force_action.is_some() {
        if show_ui { println!("  {} Bypassed identity check via {} flag", "!".yellow(), "force".cyan()); }
        warn!("Bypassed identity check via force flag");
    } else {
        if show_ui { println!("  {} Analyzed APK identity", "✓".green()); }
        info!("Analyzed APK identity");
    }

    if !is_update {
        debug!("Applying XML modifications and patching manifest");
        apk_manifest_editor.apply_patches(target_package_suffix, target_app_title)
            .map_err(|err| {
                let out = format!("Patch Error: {}", err);
                if show_ui { println!("  {} ERROR: {}", "✗".red(), out); }
                error!(error = %err, "Patch application failed");
                out
            })?;

        apk_manifest_editor.save_to_paths(&manifest_extraction_path, optional_resource_path)
            .map_err(|err| {
                let out = format!("Failed to save patched binaries: {}", err);
                if show_ui { println!("  {} ERROR: {}", "✗".red(), out); }
                error!(error = %err, "Failed to save patched binaries");
                out
            })?;
    }

    debug!("Compressing user modifications into DownloadLocal pack stream");
    let packed_files_count = pack::stream_pack_and_list(
        patch_directory,
        &temporary_assets_directory,
        "DownloadLocal",
        valid_region_key
    ).map_err(|err| {
        if show_ui { println!("  {} ERROR: {}", "✗".red(), err); }
        error!(error = %err, "Failed to pack patch files");
        err
    })?;

    if show_ui { println!("  {} Packaged {} files into a pack", "✓".green(), packed_files_count.to_string().cyan()); }
    info!(packed_files = packed_files_count, "Successfully built modification pack");

    let unsigned_apk_path = application_directory.join("unsigned_final.apk");

    debug!("Injecting modifications into unaligned APK clone");
    let injected_file_count = modify::inject_and_build_apk(
        input_apk_path,
        &unsigned_apk_path,
        &temporary_assets_directory,
        icons_directory,
        loose_directory,
        if is_update { None } else { Some(manifest_extraction_path.as_path()) },
        if is_update || !extracted_resource_table { None } else { Some(resource_extraction_path.as_path()) }
    ).map_err(|err| {
        if show_ui { println!("  {} ERROR: {}", "✗".red(), err); }
        error!(error = %err, "APK injection and build failed");
        err
    })?;

    if show_ui {
        println!("  {} Rebuilt modified APK", "✓".green());
        println!("  {} Injected {} additional assets", "✓".green(), injected_file_count.to_string().cyan());
    }
    info!(injected_assets = injected_file_count, "Rebuilt APK with new injections");

    let normalized_apk_path = application_directory.join("normalized_final.apk");
    debug!("Normalizing structural zip alignment");
    modify::normalize_apk(&unsigned_apk_path, &normalized_apk_path, input_apk_path).map_err(|err| {
        if show_ui { println!("  {} ERROR: {}", "✗".red(), err); }
        error!(error = %err, "APK alignment and normalization failed");
        err
    })?;

    if show_ui { println!("  {} Normalized binaries", "✓".green()); }
    info!("Successfully restored storage alignment semantics");

    debug!("Executing cryptographic signature schema");
    sign::sign_apk_file(&normalized_apk_path, pem_file).map_err(|err| {
        let out = err.to_string();
        if show_ui { println!("  {} ERROR: {}", "✗".red(), out); }
        error!(error = %out, "Cryptographic signing failed");
        out
    })?;

    if show_ui { println!("  {} Signed APK", "✓".green()); }
    info!("Successfully signed binary");

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

    fs::copy(&normalized_apk_path, &destination_file_path).map_err(|err| {
        let out = format!("Failed to copy to output target: {}", err);
        if show_ui { println!("  {} ERROR: {}", "✗".red(), out); }
        error!(error = %err, "Failed to move APK to output location");
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