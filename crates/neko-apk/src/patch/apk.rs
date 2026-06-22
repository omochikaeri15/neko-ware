use colored::Colorize;
use std::fs;
use std::path::PathBuf;
use tracing::{debug, error, info, instrument, trace};
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

#[instrument(skip_all, fields(target_apk = %config.input_apk_path.display(), region = %config.target_region))]
pub fn execute_patch(config: &PatchConfig) -> Result<(String, String), String> {
    debug!("Initiating APK mod cycle");

    let has_direct_files = |directory_path: &PathBuf| -> bool {
        trace!(dir = %directory_path.display(), "Checking directory for files");
        if !directory_path.exists() {
            return false;
        }

        let Ok(directory_entries) = fs::read_dir(directory_path) else {
            return false;
        };

        for directory_entry in directory_entries.flatten() {
            let Ok(file_type) = directory_entry.file_type() else {
                continue;
            };

            if file_type.is_file() {
                trace!(dir = %directory_path.display(), "Found valid file in directory");
                return true;
            }
        }
        false
    };

    if !has_direct_files(&config.patch_directory)
        && !has_direct_files(&config.icons_directory)
        && !has_direct_files(&config.loose_directory)
        && !has_direct_files(&config.code_directory)
    {
        let error_message = "Found no files to patch";
        if config.show_ui {
            println!("\n  {} ERROR: {error_message}", "✗".red());
        }
        error!("{error_message}");
        return Err(error_message.to_string());
    }

    trace!("Loading user keys...");
    let current_keys = UserKeys::load();
    let valid_region_key = current_keys
        .get_validated_region_key(&config.target_region)
        .map_err(|error| {
            if config.show_ui {
                println!("\n  {} ERROR: {error}", "✗".red());
            }
            error!(error = %error, "Failed to retrieve validated region key");
            error
        })?;

    trace!("Setting up application directories...");
    let application_directory = PathBuf::from("app");
    let temporary_binary_directory = application_directory.join("binaries");
    let temporary_assets_directory = application_directory.join("assets");

    fs::create_dir_all(&temporary_binary_directory).map_err(|error| error.to_string())?;
    fs::create_dir_all(&temporary_assets_directory).map_err(|error| error.to_string())?;

    let source_apk_file = fs::File::open(&config.input_apk_path).map_err(|error| {
        let error_output = format!("Failed to open APK: {error}");
        if config.show_ui {
            println!("\n  {} ERROR: {error_output}", "✗".red());
        }
        error!(error = %error, "Failed to open source APK file");
        error_output
    })?;

    let mut zip_archive = ZipArchive::new(source_apk_file).map_err(|error| {
        let error_output = format!("Failed to read APK archive: {error}");
        if config.show_ui {
            println!("\n  {} ERROR: {error_output}", "✗".red());
        }
        error!(error = %error, "Failed to read APK archive");
        error_output
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
                fs::File::create(&manifest_extraction_path).map_err(|error| error.to_string())?;
            let _copy_result = std::io::copy(&mut inner_file, &mut output_manifest_file);
            trace!("Extracted AndroidManifest.xml");
        } else if inner_file_name == "resources.arsc" {
            let mut output_resource_file =
                fs::File::create(&resource_extraction_path).map_err(|error| error.to_string())?;
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
        .map_err(|error| {
            let error_output = format!("Failed to parse APK binaries: {error}");
            if config.show_ui {
                println!("\n  {} ERROR: {error_output}", "✗".red());
            }
            error!(error = %error, "Failed to parse APK binaries");
            error_output
        })?;

    let apk_version_info = apk_manifest_editor.get_version_info();

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
            .map_err(|error| {
                let error_output = format!("Patch Error: {error}");
                if config.show_ui {
                    println!("  {} ERROR: {error_output}", "✗".red());
                }
                error!(error = %error, "Patch application failed");
                error_output
            })?;

        trace!("Saving modified manifest/resources...");
        apk_manifest_editor
            .save_to_paths(&manifest_extraction_path, optional_resource_path)
            .map_err(|error| {
                let error_output = format!("Failed to save patched binaries: {error}");
                if config.show_ui {
                    println!("  {} ERROR: {error_output}", "✗".red());
                }
                error!(error = %error, "Failed to save patched binaries");
                error_output
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
        .map_err(|error| {
            if config.show_ui {
                println!("  {} ERROR: {error}", "✗".red());
            }
            error!(error = %error, "Failed to pack mod files");
            error
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
        .map_err(|error| {
            if config.show_ui {
                println!("  {} ERROR: {error}", "✗".red());
            }
            error!(error = %error, "APK injection and build failed");
            error.to_string()
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
    modify::normalize_apk(&unsigned_apk_path, &normalized_apk_path, &config.input_apk_path).map_err(|error| {
        if config.show_ui {
            println!("  {} ERROR: {error}", "✗".red());
        }
        error!(error = %error, "APK alignment and normalization failed");
        error.to_string()
    })?;

    if config.show_ui {
        println!("  {} Normalized binaries", "✓".green());
    }
    info!("Successfully restored storage alignment semantics");

    debug!("Executing cryptographic signature schema");
    trace!("Starting APK signing...");
    sign::sign_apk_file(&normalized_apk_path, config.pem_file.clone()).map_err(|error| {
        let error_output = error.to_string();
        if config.show_ui {
            println!("  {} ERROR: {error_output}", "✗".red());
        }
        error!(error = %error_output, "Cryptographic signing failed");
        error_output
    })?;

    if config.show_ui {
        println!("  {} Signed APK", "✓".green());
    }
    info!("Successfully signed binary");

    let get_incremental_path = |directory_path: &PathBuf, base_name: &str| -> PathBuf {
        let mut chosen_filename = format!("{}.apk", base_name);

        if directory_path.join(&chosen_filename).exists() {
            trace!(filename = %chosen_filename, "Filename exists, searching for alternative...");
            let mut file_index = 1;
            loop {
                let candidate_name = format!("{}{}.apk", base_name, file_index);
                trace!(candidate = %candidate_name, "Testing candidate filename");
                if !directory_path.join(&candidate_name).exists() {
                    chosen_filename = candidate_name;
                    break;
                }
                file_index += 1;
            }
        }
        directory_path.join(chosen_filename)
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
    fs::copy(&normalized_apk_path, &destination_file_path).map_err(|error| {
        let error_output = format!("Failed to copy to output target: {error}");
        if config.show_ui {
            println!("  {} ERROR: {error_output}", "✗".red());
        }
        error!(error = %error, "Failed to move APK to output location");
        error_output
    })?;

    trace!("Cleaning up temporary application directory...");
    let _cleanup_result = fs::remove_dir_all(&application_directory);

    if config.show_ui {
        if let Some((version_code, version_name)) = apk_version_info {
            if version_code <= 1401010 {
                println!();
                println!("  {} Legacy game version {version_name} detected", "!".truecolor(255, 165, 0));
                println!("  {} Legacy versions are known to crash on load", "!".truecolor(255, 165, 0));
                println!("  {} Please update to a more stable app version", "!".truecolor(255, 165, 0));
            }
        }
    }

    let output_display_name = destination_file_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    Ok((action_verb, output_display_name))
}