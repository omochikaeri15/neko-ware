use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use tracing::{debug, error, info, instrument, trace, warn};
use zip::{ZipArchive, ZipWriter};

use resand::{
    res_value::{ResValue, ResValueType},
    string_pool::StringPoolHandler,
    table::{ResTable, ResTableEntryValue},
    xmltree::{XMLTree, XMLTreeNode},
};

#[derive(Debug, thiserror::Error)]
pub enum ResError {
    #[error("File operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Manifest parse error: {0}")]
    Manifest(String),
    #[error("Missing required element: {0}")]
    MissingElement(&'static str),
}

pub struct ApkEditor {
    pub manifest: XMLTree,
    pub res_table: Option<ResTable>,
}

impl ApkEditor {
    #[instrument(skip_all, fields(manifest = %manifest_path.display()))]
    pub fn from_paths(manifest_path: &Path, table_path: Option<&Path>) -> Result<Self, ResError> {
        debug!("Parsing Manifest from paths");
        let mut manifest_file = fs::File::open(manifest_path)?;
        let manifest = XMLTree::read(&mut manifest_file).map_err(|error| {
            error!("Failed to parse Manifest: {}", error);
            ResError::Manifest(error.to_string())
        })?;
        trace!("Successfully parsed Manifest");

        let res_table = match table_path {
            Some(target_path) if target_path.exists() => {
                debug!("Parsing resources.arsc from {:?}", target_path);
                let mut table_file = fs::File::open(target_path)?;
                Some(ResTable::read_all(&mut table_file).map_err(|error| {
                    error!("Failed to parse resources.arsc: {}", error);
                    ResError::Manifest(error.to_string())
                })?)
            }
            Some(target_path) => {
                warn!("resources.arsc path provided but file does not exist: {:?}", target_path);
                None
            }
            _ => {
                trace!("No resources.arsc path provided.");
                None
            }
        };

        Ok(Self { manifest, res_table })
    }

    #[instrument(skip_all)]
    pub fn get_version_info(&self) -> Option<(u32, String)> {
        trace!("Extracting version information from XML tree");
        let root_element = self.manifest.root.get_element(&["manifest"], &self.manifest.string_pool)?;

        let version_code_attribute = root_element.get_attribute("versionCode", &self.manifest.string_pool)?;
        let version_name_attribute = root_element.get_attribute("versionName", &self.manifest.string_pool)?;

        let extracted_version_code = match &version_code_attribute.typed_value.data {
            ResValueType::IntDec(decimal_value) => {
                trace!(value = decimal_value, "Extracted raw decimal versionCode");
                *decimal_value
            },
            ResValueType::IntHex(hex_value) => {
                trace!(value = hex_value, "Extracted raw hex versionCode");
                *hex_value
            },
            ResValueType::String(string_reference) => {
                let resolved = string_reference.resolve(&self.manifest.string_pool)?;
                let parsed = resolved.parse::<u32>().ok()?;
                trace!(value = parsed, "Parsed string-based versionCode");
                parsed
            }
            fallback_data => {
                let data_string = format!("{:?}", fallback_data);
                let parsed = data_string.chars().filter(|character| character.is_ascii_digit()).collect::<String>().parse::<u32>().ok()?;
                trace!(value = parsed, "Fell back to regex-style parsing for versionCode");
                parsed
            }
        };

        let extracted_version_name = match &version_name_attribute.typed_value.data {
            ResValueType::String(string_reference) => {
                let resolved = string_reference.resolve(&self.manifest.string_pool)?.to_string();
                trace!(name = %resolved, "Extracted string-based versionName");
                resolved
            }
            _ => {
                trace!("Failed to extract valid versionName");
                return None;
            }
        };

        Some((extracted_version_code, extracted_version_name))
    }

    #[instrument(skip_all)]
    pub fn save_to_paths(self, manifest_path: &Path, table_path: Option<&Path>) -> Result<(), ResError> {
        debug!("Saving patched Manifest to {:?}", manifest_path);
        let mut manifest_output_file = fs::File::create(manifest_path)?;
        self.manifest
            .write(&mut manifest_output_file)
            .map_err(|error| {
                error!("Failed to write Manifest: {}", error);
                ResError::Manifest(error.to_string())
            })?;

        if let (Some(target_path), Some(resource_table)) = (table_path, self.res_table) {
            debug!("Saving patched resources.arsc to {:?}", target_path);
            let mut table_output_file = fs::File::create(target_path)?;
            resource_table
                .write_all(&mut table_output_file)
                .map_err(|error| {
                    error!("Failed to write resources.arsc: {}", error);
                    ResError::Manifest(error.to_string())
                })?;
        }
        Ok(())
    }

    #[instrument(skip_all)]
    pub fn get_current_package(&mut self) -> Option<String> {
        trace!("Attempting to retrieve current package from Manifest");
        let root_element = self
            .manifest
            .root
            .get_element_mut(&["manifest"], &self.manifest.string_pool)?;
        let package_attribute = root_element.get_attribute_mut("package", &self.manifest.string_pool)?;

        match package_attribute.typed_value.data {
            ResValueType::String(ref string_value) => {
                let resolved = string_value.resolve(&self.manifest.string_pool).map(|resolved_string| resolved_string.to_string());
                trace!(package = ?resolved, "Retrieved package identifier");
                resolved
            },
            _ => None,
        }
    }

    #[instrument(skip_all, fields(suffix = %target_package_suffix, title = %app_title))]
    pub fn apply_patches(&mut self, target_package_suffix: &str, app_title: &str) -> Result<(), ResError> {
        info!("Applying Manifest patches");

        let root_element = self
            .manifest
            .root
            .get_element_mut(&["manifest"], &self.manifest.string_pool)
            .ok_or_else(|| {
                error!("Could not find root <manifest> element.");
                ResError::MissingElement("manifest root")
            })?;

        let initial_children_count = root_element.children.len();
        root_element.children.retain(|child_node| {
            let resolved_name = child_node
                .element
                .name
                .resolve(&self.manifest.string_pool)
                .unwrap_or_default();
            resolved_name != "split"
        });

        if root_element.children.len() < initial_children_count {
            trace!("Removed ghost <split> tags from root");
        }

        root_element.element.attributes.retain(|attribute_node| {
            let Some(resolved_name) = attribute_node.name.resolve(&self.manifest.string_pool) else {
                return true;
            };
            resolved_name != "split" && resolved_name != "isFeatureSplit"
        });

        trace!("Injecting isFeatureSplit=false into manifest root");
        root_element.insert_attribute(
            "isFeatureSplit".into(),
            ResValue::new_bool(false),
            &mut self.manifest.string_pool,
            self.manifest.resource_map.as_mut(),
            Some(0x0101055b.into()),
        );

        let package_attribute = root_element
            .get_attribute_mut("package", &self.manifest.string_pool)
            .ok_or_else(|| {
                error!("Missing 'package' attribute on root manifest.");
                ResError::MissingElement("package attribute")
            })?;

        let original_package_name = match package_attribute.typed_value.data {
            ResValueType::String(ref string_value) => string_value
                .resolve(&self.manifest.string_pool)
                .unwrap_or_default()
                .to_string(),
            _ => {
                error!("Invalid package string format found in manifest.");
                return Err(ResError::MissingElement("Invalid package string format"));
            }
        };

        let mut package_parts: Vec<&str> = original_package_name.split('.').collect();
        if !package_parts.is_empty() {
            package_parts.pop();
        }
        let new_package_tail = format!("battlecats{}", target_package_suffix.trim());
        package_parts.push(&new_package_tail);
        let final_constructed_package_name = package_parts.join(".");

        debug!(original = %original_package_name, modified = %final_constructed_package_name, "Altered manifest package identifier");

        package_attribute.write_string(
            final_constructed_package_name.as_str().into(),
            &mut self.manifest.string_pool,
        );

        trace!("Initiating deep recursive package reference scrubbing...");
        let resource_table_reference = self.res_table.as_ref();
        replace_package_references(
            &mut self.manifest.root,
            &mut self.manifest.string_pool,
            resource_table_reference,
            &original_package_name,
            &final_constructed_package_name,
        );

        let Some(application_element) = self
            .manifest
            .root
            .get_element_mut(&["manifest", "application"], &self.manifest.string_pool)
        else {
            warn!("Could not find <application> element in Manifest!");
            return Ok(());
        };

        application_element.element.attributes.retain(|attribute_node| {
            let Some(resolved_name) = attribute_node.name.resolve(&self.manifest.string_pool) else {
                return true;
            };
            resolved_name != "extractNativeLibs" && resolved_name != "isSplitRequired"
        });

        let pre_vending_count = application_element.children.len();
        application_element.children.retain(|child_node| {
            let is_metadata_tag = child_node.element.name.resolve(&self.manifest.string_pool) == Some("meta-data");
            if !is_metadata_tag {
                return true;
            }

            let Some(name_attribute) = child_node.get_attribute("name", &self.manifest.string_pool) else {
                return true;
            };
            let ResValueType::String(ref string_value) = name_attribute.typed_value.data else {
                return true;
            };
            let Some(resolved_value) = string_value.resolve(&self.manifest.string_pool) else {
                return true;
            };

            !(resolved_value.contains("vending.splits") || resolved_value.contains("vending.derived.apk.id"))
        });

        if application_element.children.len() < pre_vending_count {
            trace!("Stripped vending split metadata tags");
        }

        application_element.insert_attribute(
            "extractNativeLibs".into(),
            ResValue::new_bool(true),
            &mut self.manifest.string_pool,
            self.manifest.resource_map.as_mut(),
            Some(0x010104ea.into()),
        );

        application_element.insert_attribute(
            "isSplitRequired".into(),
            ResValue::new_bool(false),
            &mut self.manifest.string_pool,
            self.manifest.resource_map.as_mut(),
            Some(0x01010591.into()),
        );

        if !app_title.trim().is_empty() {
            if let Some(label_attribute) = application_element.get_attribute_mut("label", &self.manifest.string_pool) {
                debug!("Overwriting app label with '{}'", app_title.trim());
                label_attribute.write_string(app_title.trim().into(), &mut self.manifest.string_pool);
            } else {
                debug!("Inserting new app label '{}'", app_title.trim());
                application_element.insert_attribute(
                    "label".into(),
                    ResValue::new_str(app_title.trim().into(), &mut self.manifest.string_pool),
                    &mut self.manifest.string_pool,
                    self.manifest.resource_map.as_mut(),
                    Some(0x01010001.into()),
                );
            }

            trace!("Scrubbing original labels from all activities to force application label inheritance...");
            strip_activity_labels(application_element, &mut self.manifest.string_pool);
        }

        if let Some(ref mut mutable_table) = self.res_table {
            if let Some(first_package) = mutable_table.packages.first_mut() {
                debug!("Updating resources.arsc package name to {}", final_constructed_package_name);
                first_package.name.clone_from(&final_constructed_package_name);
            }
        }

        info!("Patching complete. New identity: {}", final_constructed_package_name);
        Ok(())
    }
}

fn strip_activity_labels(node: &mut XMLTreeNode, pool: &mut StringPoolHandler) {
    let is_activity = node.element.name.resolve(pool).is_some_and(|name| name == "activity" || name == "activity-alias");

    if is_activity {
        node.element.attributes.retain(|attribute| {
            attribute.name.resolve(pool).is_none_or(|attr_name| attr_name != "label")
        });
    }

    for child in &mut node.children {
        strip_activity_labels(child, pool);
    }
}

fn replace_package_references(
    element_node: &mut XMLTreeNode,
    string_pool: &mut StringPoolHandler,
    resource_table: Option<&ResTable>,
    old_package_identity: &str,
    new_package_identity: &str,
) {
    let attributes_to_inspect = [
        "name",
        "authorities",
        "taskAffinity",
        "sharedUserId",
        "value",
        "scheme",
        "host",
    ];

    let tag_name = element_node.element.name.resolve(string_pool).unwrap_or_default().to_string();
    let is_component = matches!(tag_name.as_str(), "application" | "activity" | "activity-alias" | "service" | "receiver" | "provider");

    for attribute_name in attributes_to_inspect {
        let Some(attribute_node) = element_node.get_attribute_mut(attribute_name, string_pool) else {
            continue;
        };

        let mut resolved_string_value: Option<String> = None;

        match attribute_node.typed_value.data {
            ResValueType::String(ref string_value) => {
                if let Some(resolved_value) = string_value.resolve(string_pool) {
                    resolved_string_value = Some(resolved_value.to_string());
                }
            }
            ResValueType::Reference(ref table_reference) => {
                resolved_string_value = (|| -> Option<String> {
                    let active_table = resource_table?;
                    let active_package = active_table.packages.first()?;
                    let resource_value = active_package.resolve_ref(*table_reference)?;
                    let ResTableEntryValue::ResValue(ref actual_value) = resource_value.data else {
                        return None;
                    };
                    let ResValueType::String(ref string_reference) = actual_value.data.data else {
                        return None;
                    };
                    let resolved_reference_string = string_reference.resolve(&active_table.string_pool)?;
                    Some(resolved_reference_string.to_string())
                })();
            }
            _ => {}
        }

        if let Some(found_string) = resolved_string_value {
            if attribute_name == "name" && is_component {
                if found_string.starts_with('.') {
                    let new_val = format!("{}{}", old_package_identity, found_string);
                    attribute_node.write_string(new_val.into(), string_pool);
                } else if !found_string.contains('.') {
                    let new_val = format!("{}.{}", old_package_identity, found_string);
                    attribute_node.write_string(new_val.into(), string_pool);
                }
                continue;
            }

            if found_string.contains(old_package_identity) {
                trace!("Replaced deep reference in attribute '{}': {} -> {}", attribute_name, old_package_identity, new_package_identity);
                let replaced_value = found_string.replace(old_package_identity, new_package_identity);
                attribute_node.write_string(replaced_value.into(), string_pool);
            }
        }
    }

    for child_node in &mut element_node.children {
        replace_package_references(
            child_node,
            string_pool,
            resource_table,
            old_package_identity,
            new_package_identity,
        );
    }
}

#[instrument(skip_all)]
pub fn inject_and_build_apk(
    source_apk_path: &Path,
    output_apk_path: &Path,
    assets_directory: &Path,
    icons_directory: &Path,
    loose_directory: &Path,
    code_directory: &Path,
    patched_manifest_path: Option<&Path>,
    patched_arsc_path: Option<&Path>,
    target_architecture: Option<&str>,
    force_inject_path: Option<&Path>,
    show_ui: bool,
) -> Result<usize> {
    info!("Starting APK build & injection from {:?} to {:?}", source_apk_path, output_apk_path);
    let source_file = fs::File::open(source_apk_path).map_err(|error| {
        error!("Failed to open source APK: {}", error);
        error
    })?;
    let mut zip_archive = ZipArchive::new(source_file).map_err(|error| {
        error!("Failed to read source APK archive: {}", error);
        error
    })?;

    let destination_file = fs::File::create(output_apk_path).map_err(|error| {
        error!("Failed to create output APK: {}", error);
        error
    })?;
    let mut zip_writer = ZipWriter::new(destination_file);

    let mut successfully_injected_count = 0;
    let mut active_files_to_inject = HashSet::new();

    if patched_manifest_path.is_some() {
        active_files_to_inject.insert("AndroidManifest.xml".to_string());
    }
    if patched_arsc_path.is_some() {
        active_files_to_inject.insert("resources.arsc".to_string());
    }

    if assets_directory.exists() {
        let directory_entries = fs::read_dir(assets_directory)?;
        for entry_result in directory_entries.flatten() {
            if entry_result.path().is_file() {
                active_files_to_inject.insert(format!("assets/{}", entry_result.file_name().to_string_lossy()));
            }
        }
    }

    if loose_directory.exists() {
        let directory_entries = fs::read_dir(loose_directory)?;
        for entry_result in directory_entries.flatten() {
            if entry_result.path().is_file() {
                active_files_to_inject.insert(format!("assets/{}", entry_result.file_name().to_string_lossy()));
            }
        }
    }

    debug!("Identified {} files to inject or replace.", active_files_to_inject.len());

    let has_custom_icon = icons_directory.join("icon.png").exists();
    let has_custom_foreground_icon = icons_directory.join("icon_foreground.png").exists();
    let has_custom_push_icon = icons_directory.join("push_icon.png").exists();
    let fallback_foreground = has_custom_icon && !has_custom_foreground_icon;

    let mut pre_existing_resource_folders = HashSet::new();

    let mut force_inject_map = HashMap::new();
    if let Some(force_path) = force_inject_path {
        if force_path.is_file() {
            if let Some(name) = force_path.file_name() {
                force_inject_map.insert(name.to_string_lossy().into_owned(), force_path.to_path_buf());
            }
        } else if force_path.is_dir() {
            if let Ok(entries) = fs::read_dir(force_path) {
                for entry_result in entries.flatten() {
                    if !entry_result.path().is_file() {
                        continue;
                    }
                    let name = entry_result.file_name().to_string_lossy().into_owned();
                    force_inject_map.insert(name, entry_result.path());
                }
            }
        }
    }

    let mut custom_code_files = HashMap::new();
    if code_directory.exists() {
        let code_entries = fs::read_dir(code_directory)?;
        for entry_result in code_entries.flatten() {
            if entry_result.path().is_file() {
                let filename = entry_result.file_name().to_string_lossy().into_owned();
                custom_code_files.insert(filename, entry_result.path());
            }
        }
    }

    let mut discovered_code_zip_paths = Vec::new();
    let mut has_target_architecture_folder = false;

    let inject_local_file = |writer: &mut ZipWriter<fs::File>, count: &mut usize, local_file_path: &Path, internal_zip_path: &str, require_store: bool| -> Result<()> {
        if !local_file_path.exists() {
            return Ok(());
        }
        let raw_file_data = fs::read(local_file_path)?;
        let compression_method = if require_store {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        };
        let write_options = zip::write::SimpleFileOptions::default().compression_method(compression_method);

        writer.start_file(internal_zip_path, write_options)?;
        writer.write_all(&raw_file_data)?;
        *count += 1;
        trace!(file = %internal_zip_path, "Injected modified payload into APK stream");
        Ok(())
    };

    trace!("Scanning original APK zip contents...");
    for archive_index in 0..zip_archive.len() {
        let archive_file = zip_archive.by_index(archive_index)?;
        let internal_file_name = archive_file.name().to_string();

        let uppercase_file_name = internal_file_name.to_ascii_uppercase();
        if uppercase_file_name.starts_with("META-INF/") || uppercase_file_name.starts_with("META-INF\\") {
            if uppercase_file_name.ends_with(".SF")
                || uppercase_file_name.ends_with(".RSA")
                || uppercase_file_name.ends_with(".DSA")
                || uppercase_file_name.ends_with(".EC")
                || uppercase_file_name.ends_with("MANIFEST.MF")
                || uppercase_file_name.contains("STAMP-CERT")
            {
                trace!("Skipping original signature file: {}", internal_file_name);
                continue;
            }
        }

        if internal_file_name.starts_with("res/") {
            if let Some(parent_path) = Path::new(&internal_file_name).parent() {
                pre_existing_resource_folders.insert(parent_path.to_string_lossy().replace("\\", "/"));
            }
        }

        let short_file_name = Path::new(&internal_file_name)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();

        if let Some(forced_local_path) = force_inject_map.get(&short_file_name) {
            trace!(file = %internal_file_name, "Force overwriting file from --force flag directive");
            let require_store = archive_file.compression() == zip::CompressionMethod::Stored;
            inject_local_file(&mut zip_writer, &mut successfully_injected_count, forced_local_path, &internal_file_name, require_store)?;
            continue;
        }

        if internal_file_name.starts_with("lib/") {
            if let Some(target_arch) = target_architecture {
                if internal_file_name.starts_with(&format!("lib/{target_arch}/")) {
                    has_target_architecture_folder = true;
                    if custom_code_files.contains_key(&short_file_name) {
                        trace!(file = %internal_file_name, "Intercepted vanilla native library in archive; queuing for replacement");
                        discovered_code_zip_paths.push((internal_file_name.clone(), short_file_name));
                        continue;
                    }
                }
            }
        }

        if active_files_to_inject.contains(&internal_file_name) {
            continue;
        }

        if internal_file_name.starts_with("res/") {
            if short_file_name == "icon.png" && has_custom_icon {
                trace!(file = %internal_file_name, "Intercepted original icon.png");
                continue;
            }
            if short_file_name == "icon_foreground.png" && (has_custom_foreground_icon || fallback_foreground) {
                trace!(file = %internal_file_name, "Intercepted and dropped original icon_foreground.png");
                continue;
            }
            if short_file_name == "push_icon.png" && has_custom_push_icon {
                trace!(file = %internal_file_name, "Intercepted original push_icon.png");
                continue;
            }
        }

        zip_writer.raw_copy_file(archive_file)?;
    }

    debug!("Beginning to inject files...");

    if let Some(manifest_path) = patched_manifest_path {
        inject_local_file(&mut zip_writer, &mut successfully_injected_count, manifest_path, "AndroidManifest.xml", false)?;
    }
    if let Some(arsc_path) = patched_arsc_path {
        inject_local_file(&mut zip_writer, &mut successfully_injected_count, arsc_path, "resources.arsc", true)?;
    }

    if !custom_code_files.is_empty() {
        if let Some(target_arch) = target_architecture {
            if !has_target_architecture_folder {
                if show_ui {
                    use colored::Colorize;
                    println!("  {} Target architecture missing from APK", "!".truecolor(255, 165, 0));
                    println!("  {} Skipping code injection", "!".truecolor(255, 165, 0));
                }
                warn!("Target architecture missing from APK, skipping code injection");
            } else {
                debug!("Injecting modded native code payloads for architecture {}...", target_arch);
                let mut successfully_injected_keys = HashSet::new();

                for (zip_path, short_name) in discovered_code_zip_paths {
                    if let Some(local_path) = custom_code_files.get(&short_name) {
                        trace!(file = %zip_path, "Overwriting exact zip path with modded native library");
                        inject_local_file(&mut zip_writer, &mut successfully_injected_count, local_path, &zip_path, true)?;
                        successfully_injected_keys.insert(short_name.clone());
                    }
                }

                for key in successfully_injected_keys {
                    custom_code_files.remove(&key);
                }

                for (short_name, local_path) in custom_code_files {
                    let fallback_path = format!("lib/{target_arch}/{short_name}");
                    trace!(file = %fallback_path, "Injecting new native library into target architecture");
                    inject_local_file(&mut zip_writer, &mut successfully_injected_count, &local_path, &fallback_path, true)?;
                }
            }
        } else {
            if show_ui {
                use colored::Colorize;
                println!("  {} No architecture specified", "!".truecolor(255, 165, 0));
                println!("  {} Skipping code injection", "!".truecolor(255, 165, 0));
            }
            debug!("No architecture specified, skipping code injection");
        }
    }

    if assets_directory.exists() {
        let directory_entries = fs::read_dir(assets_directory)?;
        for entry_result in directory_entries.flatten() {
            if entry_result.path().is_file() {
                let generated_name = entry_result.file_name().to_string_lossy().to_string();
                let force_store = generated_name.ends_with(".pack") || generated_name.ends_with(".list");
                inject_local_file(&mut zip_writer, &mut successfully_injected_count, &entry_result.path(), &format!("assets/{generated_name}"), force_store)?;
            }
        }
    }

    if loose_directory.exists() {
        let directory_entries = fs::read_dir(loose_directory)?;
        for entry_result in directory_entries.flatten() {
            if entry_result.path().is_file() {
                let generated_name = entry_result.file_name().to_string_lossy().to_string();
                inject_local_file(&mut zip_writer, &mut successfully_injected_count, &entry_result.path(), &format!("assets/{generated_name}"), true)?;
            }
        }
    }

    if icons_directory.exists() {
        info!("Scaling and injecting custom icons...");
        let foreground_source = if fallback_foreground { "icon.png" } else { "icon_foreground.png" };

        let icon_blueprints = vec![
            ("icon.png", "icon.png", 192, 144, 96, has_custom_icon, false),
            ("icon_foreground.png", foreground_source, 432, 324, 216, has_custom_foreground_icon || fallback_foreground, fallback_foreground),
            ("push_icon.png", "push_icon.png", 96, 72, 48, has_custom_push_icon, false),
        ];

        for (blueprint_file_name, source_name, size_xxxhdpi, size_xxhdpi, size_xhdpi, asset_exists, is_fallback) in icon_blueprints {
            if !asset_exists {
                continue;
            }

            let source_image_path = icons_directory.join(source_name);
            let Ok(decoded_source_image) = image::open(&source_image_path) else {
                warn!("Failed to open or decode custom icon: {}", source_name);
                continue;
            };

            let target_resolutions = [
                ("drawable-xxxhdpi", size_xxxhdpi),
                ("drawable-xxhdpi", size_xxhdpi),
                ("drawable-xhdpi", size_xhdpi),
                ("drawable-xxxhdpi-v4", size_xxxhdpi),
                ("drawable-xxhdpi-v4", size_xxhdpi),
                ("drawable-xhdpi-v4", size_xhdpi),
                ("mipmap-xxxhdpi", size_xxxhdpi),
                ("mipmap-xxhdpi", size_xxhdpi),
                ("mipmap-xhdpi", size_xhdpi),
                ("mipmap-xxxhdpi-v4", size_xxxhdpi),
                ("mipmap-xxhdpi-v4", size_xxhdpi),
                ("mipmap-xhdpi-v4", size_xhdpi),
            ];

            for (target_folder_name, canvas_size) in target_resolutions {
                let formatted_resource_folder = format!("res/{target_folder_name}");

                if !pre_existing_resource_folders.contains(&formatted_resource_folder) {
                    continue;
                }

                let final_zip_path = format!("{formatted_resource_folder}/{blueprint_file_name}");

                let inner_scale_size = if is_fallback {
                    (canvas_size as f32 * 0.67) as u32
                } else {
                    canvas_size
                };

                let properly_scaled_image =
                    decoded_source_image.resize_exact(inner_scale_size, inner_scale_size, image::imageops::FilterType::Lanczos3);

                let final_image = if is_fallback {
                    let mut canvas = image::RgbaImage::new(canvas_size, canvas_size);
                    let offset = ((canvas_size.saturating_sub(inner_scale_size)) / 2) as i64;
                    image::imageops::overlay(&mut canvas, &properly_scaled_image.to_rgba8(), offset, offset);
                    image::DynamicImage::ImageRgba8(canvas)
                } else {
                    properly_scaled_image
                };

                let mut memory_cursor = Cursor::new(Vec::new());
                if final_image
                    .write_to(&mut memory_cursor, image::ImageFormat::Png)
                    .is_err()
                {
                    continue;
                }

                let injection_options =
                    zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
                if zip_writer.start_file(&final_zip_path, injection_options).is_err() {
                    continue;
                };

                let _write_result = zip_writer.write_all(&memory_cursor.into_inner());
                successfully_injected_count += 1;
                trace!(file = %final_zip_path, "Injected scaled icon asset");
            }
        }
    }

    info!("Successfully built APK. Total injected files: {}", successfully_injected_count);
    zip_writer.finish().map_err(|error| {
        error!("Failed to finalize APK ZipWriter: {}", error);
        error
    })?;
    Ok(successfully_injected_count)
}

#[instrument(skip_all)]
pub fn normalize_apk(input_apk_path: &Path, output_apk_path: &Path, original_reference_apk: &Path) -> Result<()> {
    info!("Normalizing APK binaries for signature verification...");
    let mut stored_files_ledger = HashSet::new();

    let reference_file = fs::File::open(original_reference_apk).context("Failed to open original APK")?;
    let mut reference_zip_archive = ZipArchive::new(reference_file).context("Failed to read original APK")?;

    for archive_index in 0..reference_zip_archive.len() {
        let archive_file = reference_zip_archive.by_index(archive_index)?;
        if archive_file.compression() == zip::CompressionMethod::Stored {
            let archive_file_name = archive_file.name().to_string();
            stored_files_ledger.insert(archive_file_name);
        }
    }
    debug!("Identified {} stored files from original APK.", stored_files_ledger.len());

    let current_source_file = fs::File::open(input_apk_path).context("Failed to open APK")?;
    let mut current_zip_archive = ZipArchive::new(current_source_file).context("Failed to read APK archive")?;

    let output_destination_file = fs::File::create(output_apk_path).context("Failed to create normalized APK")?;
    let mut final_zip_writer = ZipWriter::new(output_destination_file);

    let uncompressed_extension_overrides = ["dex", "arsc", "so", "pack", "list", "ogg"];

    for archive_index in 0..current_zip_archive.len() {
        let mut inner_archive_file = current_zip_archive.by_index(archive_index)?;

        let internal_file_name = inner_archive_file.name().to_string();
        let internal_file_extension = Path::new(&internal_file_name)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("");

        let requires_forced_store = uncompressed_extension_overrides.contains(&internal_file_extension);
        let was_historically_stored = stored_files_ledger.contains(&internal_file_name);

        if !requires_forced_store && !was_historically_stored {
            final_zip_writer.raw_copy_file(inner_archive_file)?;
            continue;
        }

        let mut extracted_file_data = Vec::new();
        inner_archive_file
            .read_to_end(&mut extracted_file_data)
            .context(format!("Failed reading {internal_file_name}"))?;

        let required_byte_alignment = if internal_file_extension == "so" { 4096 } else { 4 };

        let normalized_write_options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .with_alignment(required_byte_alignment);

        final_zip_writer.start_file(&internal_file_name, normalized_write_options)?;
        final_zip_writer.write_all(&extracted_file_data)?;
        trace!(file = %internal_file_name, alignment = required_byte_alignment, "Re-aligned structural storage data block");
    }

    final_zip_writer.finish()?;
    info!("APK normalization complete.");
    Ok(())
}