use colored::Colorize;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, error, info, trace};
use zip::ZipArchive;

use crate::config::OutputBehavior;
use crate::keys::UserKeys;
use crate::patch::modify;
use crate::patch::pack;
use crate::patch::sign;

pub struct PatchConfig {
    pub input_apk_path: PathBuf,
    pub patch_directory: PathBuf,
    pub icons_directory: PathBuf,
    pub loose_directory: PathBuf,
    pub code_directory: PathBuf,
    pub output_directory_path: PathBuf,
    pub target_app_title: String,
    pub target_package_suffix: String,
    pub target_region: String,
    pub output_behavior: OutputBehavior,
    pub force_inject: Option<PathBuf>,
    pub pem_file: Option<String>,
    pub target_architecture: Option<String>,
    pub show_ui: bool,
}

pub fn execute_patch(config: &PatchConfig) -> Result<(String, String), String> {
    debug!(target = %config.input_apk_path.display(), "Initiating APK mod cycle");

    let has_direct_files = |dir: &PathBuf| -> bool {
        trace!(dir = %dir.display(), "Checking directory for files");
        if !dir.exists() {
            return false;
        }
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Ok(file_type) = entry.file_type() {
                    if file_type.is_file() {
                        trace!(dir = %dir.display(), "Found valid file in directory");
                        return true;
                    }
                }
            }
        }
        false
    };

    if !has_direct_files(&config.patch_directory)
        && !has_direct_files(&config.icons_directory)
        && !has_direct_files(&config.loose_directory)
        && !has_direct_files(&config.code_directory)
    {
        let msg = "Found no files to patch";
        if config.show_ui {
            println!("\n  {} ERROR: {msg}", "✗".red());
        }
        error!("{msg}");
        return Err(msg.to_string());
    }

    trace!("Loading user keys...");
    let current_keys = UserKeys::load();
    let valid_region_key = current_keys
        .get_validated_region_key(&config.target_region)
        .map_err(|err| {
            if config.show_ui {
                println!("\n  {} ERROR: {err}", "✗".red());
            }
            error!(error = %err, "Failed to retrieve validated region key");
            err
        })?;

    trace!("Setting up application directories...");
    let application_directory = PathBuf::from("app");
    let temporary_binary_directory = application_directory.join("binaries");
    let temporary_assets_directory = application_directory.join("assets");

    std::fs::create_dir_all(&temporary_binary_directory).map_err(|err| err.to_string())?;
    std::fs::create_dir_all(&temporary_assets_directory).map_err(|err| err.to_string())?;

    let source_apk_file = fs::File::open(&config.input_apk_path).map_err(|err| {
        let out = format!("Failed to open APK: {err}");
        if config.show_ui {
            println!("\n  {} ERROR: {out}", "✗".red());
        }
        error!(error = %err, "Failed to open source APK file");
        out
    })?;

    let mut zip_archive = ZipArchive::new(source_apk_file).map_err(|err| {
        let out = format!("Failed to read APK archive: {err}");
        if config.show_ui {
            println!("\n  {} ERROR: {out}", "✗".red());
        }
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
            let mut output_manifest_file =
                fs::File::create(&manifest_extraction_path).map_err(|err| err.to_string())?;
            let _copy_result = std::io::copy(&mut inner_file, &mut output_manifest_file);
            trace!("Extracted AndroidManifest.xml");
        } else if inner_file_name == "resources.arsc" {
            let mut output_resource_file =
                fs::File::create(&resource_extraction_path).map_err(|err| err.to_string())?;
            let _copy_result = std::io::copy(&mut inner_file, &mut output_resource_file);
            extracted_resource_table = true;
            trace!("Extracted resources.arsc");
        }
    }
    drop(zip_archive);

    let optional_resource_path = if extracted_resource_table {
        Some(resource_extraction_path.as_path())
    } else {
        None
    };

    trace!("Initializing ApkEditor...");
    let mut apk_manifest_editor = modify::ApkEditor::from_paths(&manifest_extraction_path, optional_resource_path)
        .map_err(|err| {
            let out = format!("Failed to parse APK binaries: {err}");
            if config.show_ui {
                println!("\n  {} ERROR: {out}", "✗".red());
            }
            error!(error = %err, "Failed to parse APK binaries");
            out
        })?;

    let target_package_full = format!("jp.co.ponos.battlecats{}", config.target_package_suffix.trim());
    let current_package = apk_manifest_editor.get_current_package().unwrap_or_default();

    println!();
    trace!(current = %current_package, target = %target_package_full, "Comparing package identities");

    let is_update_patch = current_package == target_package_full;

    if config.show_ui {
        println!("  {} Analyzed APK identity", "✓".green());
    }
    info!("Analyzed APK identity");

    if !is_update_patch {
        debug!("Applying XML modifications and patching manifest");
        apk_manifest_editor
            .apply_patches(&config.target_package_suffix, &config.target_app_title)
            .map_err(|err| {
                let out = format!("Patch Error: {err}");
                if config.show_ui {
                    println!("  {} ERROR: {out}", "✗".red());
                }
                error!(error = %err, "Patch application failed");
                out
            })?;

        trace!("Saving modified manifest/resources...");
        apk_manifest_editor
            .save_to_paths(&manifest_extraction_path, optional_resource_path)
            .map_err(|err| {
                let out = format!("Failed to save patched binaries: {err}");
                if config.show_ui {
                    println!("  {} ERROR: {out}", "✗".red());
                }
                error!(error = %err, "Failed to save patched binaries");
                out
            })?;
    } else {
        debug!("Package identity matches target. Skipping manifest modifications.");
    }

    debug!("Compressing user modifications into DownloadLocal pack stream");
    let packed_files_count = pack::stream_pack_and_list(
        &config.patch_directory,
        &temporary_assets_directory,
        "DownloadLocal",
        valid_region_key,
    )
        .map_err(|err| {
            if config.show_ui {
                println!("  {} ERROR: {err}", "✗".red());
            }
            error!(error = %err, "Failed to pack mod files");
            err
        })?;

    if config.show_ui {
        println!(
            "  {} Packaged {} files into a pack",
            "✓".green(),
            packed_files_count.to_string().cyan()
        );
    }
    info!(
        packed_files = packed_files_count,
        "Successfully built modification pack"
    );

    let unsigned_apk_path = application_directory.join("unsigned_final.apk");

    debug!("Injecting modifications into unaligned APK clone");
    trace!("Starting injection and build process...");
    let injected_file_count = modify::inject_and_build_apk(
        &config.input_apk_path,
        &unsigned_apk_path,
        &temporary_assets_directory,
        &config.icons_directory,
        &config.loose_directory,
        &config.code_directory,
        if is_update_patch { None } else { Some(manifest_extraction_path.as_path()) },
        if is_update_patch || !extracted_resource_table { None } else { Some(resource_extraction_path.as_path()) },
        config.target_architecture.as_deref(),
        config.force_inject.as_deref(),
        config.show_ui,
    )
        .map_err(|err| {
            if config.show_ui {
                println!("  {} ERROR: {err}", "✗".red());
            }
            error!(error = %err, "APK injection and build failed");
            err.to_string()
        })?;

    if config.show_ui {
        println!("  {} Rebuilt modified APK", "✓".green());
        println!(
            "  {} Injected {} additional assets",
            "✓".green(),
            injected_file_count.to_string().cyan()
        );
    }

    info!(injected_assets = injected_file_count, "Rebuilt APK with new injections");

    let normalized_apk_path = application_directory.join("normalized_final.apk");
    debug!("Normalizing structural zip alignment");
    trace!("Starting APK normalization...");
    modify::normalize_apk(&unsigned_apk_path, &normalized_apk_path, &config.input_apk_path).map_err(|err| {
        if config.show_ui {
            println!("  {} ERROR: {err}", "✗".red());
        }
        error!(error = %err, "APK alignment and normalization failed");
        err.to_string()
    })?;

    if config.show_ui {
        println!("  {} Normalized binaries", "✓".green());
    }
    info!("Successfully restored storage alignment semantics");

    debug!("Executing cryptographic signature schema");
    trace!("Starting APK signing...");
    sign::sign_apk_file(&normalized_apk_path, config.pem_file.clone()).map_err(|err| {
        let out = err.to_string();
        if config.show_ui {
            println!("  {} ERROR: {out}", "✗".red());
        }
        error!(error = %out, "Cryptographic signing failed");
        out
    })?;

    if config.show_ui {
        println!("  {} Signed APK", "✓".green());
    }
    info!("Successfully signed binary");

    let get_incremental_path = |dir: &PathBuf, base_name: &str| -> PathBuf {
        let mut chosen_filename = format!("{}.apk", base_name);
        if dir.join(&chosen_filename).exists() {
            trace!(filename = %chosen_filename, "Filename exists, searching for alternative...");
            let mut file_index = 1;
            loop {
                let candidate_name = format!("{}{}.apk", base_name, file_index);
                trace!(candidate = %candidate_name, "Testing candidate filename");
                if !dir.join(&candidate_name).exists() {
                    chosen_filename = candidate_name;
                    break;
                }
                file_index += 1;
            }
        }
        dir.join(chosen_filename)
    };

    let (action_verb, destination_file_path) = match config.output_behavior {
        OutputBehavior::Replace => {
            ("Updated".to_string(), config.input_apk_path.to_path_buf())
        },
        OutputBehavior::Create => {
            let sanitized_title = config.target_app_title.trim().replace(['\\', '/'], "_");
            let base_title = if sanitized_title.is_empty() { "modded_aligned" } else { &sanitized_title };
            let _ = fs::create_dir_all(&config.output_directory_path);
            ("Created".to_string(), get_incremental_path(&config.output_directory_path, base_title))
        },
        OutputBehavior::Automatic => {
            if is_update_patch {
                ("Updated".to_string(), config.input_apk_path.to_path_buf())
            } else {
                let sanitized_title = config.target_app_title.trim().replace(['\\', '/'], "_");
                let base_title = if sanitized_title.is_empty() { "modded_aligned" } else { &sanitized_title };
                let _ = fs::create_dir_all(&config.output_directory_path);
                ("Created".to_string(), get_incremental_path(&config.output_directory_path, base_title))
            }
        }
    };

    trace!(destination = %destination_file_path.display(), "Copying final APK");
    fs::copy(&normalized_apk_path, &destination_file_path).map_err(|err| {
        let out = format!("Failed to copy to output target: {err}");
        if config.show_ui {
            println!("  {} ERROR: {out}", "✗".red());
        }
        error!(error = %err, "Failed to move APK to output location");
        out
    })?;

    trace!("Cleaning up temporary application directory...");
    let _cleanup_result = fs::remove_dir_all(&application_directory);

    let output_display_name = destination_file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    Ok((action_verb, output_display_name))
}