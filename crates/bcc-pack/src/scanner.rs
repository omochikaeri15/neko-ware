use colored::Colorize;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use tracing::{debug, error, trace};
use zip::ZipArchive;

use crate::io::get_local_dir;

#[derive(Debug)]
pub struct PackPair {
    pub name: String,
    pub pack_path: PathBuf,
    pub list_path: PathBuf,
}

pub fn scan_and_resolve(input_path: &Path, show_ui: bool) -> Result<Vec<PackPair>, String> {
    if show_ui {
        println!();
    }
    debug!("Commencing path scan at: {}", input_path.display());

    let mut raw_packs = Vec::new();
    let mut raw_lists = Vec::new();

    if input_path.is_file() {
        let ext = input_path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        match ext.as_str() {
            "apk" | "xapk" | "apkm" | "apks" | "zip" => {
                debug!("Detected archive input type. Attempting extraction...");
                let archive_name = input_path.file_stem().unwrap_or_default().to_string_lossy();
                let mut target_dir = get_local_dir();
                target_dir.push("apk");
                target_dir.push(archive_name.as_ref());
                extract_archive(input_path, &target_dir, &mut raw_packs, &mut raw_lists);
            }
            "pack" => {
                raw_packs.push(input_path.to_path_buf());
                let sibling_list = input_path.with_extension("list");
                if sibling_list.exists() {
                    raw_lists.push(sibling_list);
                }
            }
            "list" => {
                raw_lists.push(input_path.to_path_buf());
                let sibling_pack = input_path.with_extension("pack");
                if sibling_pack.exists() {
                    raw_packs.push(sibling_pack);
                }
            }
            _ => return Err("Unsupported file type provided as input.".to_string()),
        }
    } else if input_path.is_dir() {
        debug!("Detected directory input type. Walking hierarchy...");
        traverse_directory(input_path, &mut raw_packs, &mut raw_lists);
    } else {
        return Err("Input path does not exist.".to_string());
    }

    let mut grouped_files: HashMap<String, (Vec<PathBuf>, Vec<PathBuf>)> = HashMap::new();

    for pack in raw_packs {
        let stem = pack.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        grouped_files.entry(stem).or_default().0.push(pack);
    }
    for list in raw_lists {
        let stem = list.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        grouped_files.entry(stem).or_default().1.push(list);
    }

    let mut valid_pairs = Vec::new();
    let mut has_skipped_items = false;

    let mut missing_pack_stems = Vec::new();
    let mut missing_list_stems = Vec::new();

    for (stem, (packs, lists)) in grouped_files {
        if packs.len() > 1 || lists.len() > 1 {
            has_skipped_items = true;
            if show_ui {
                println!("  {} ERROR: Conflict for {} found:", "!".yellow(), stem.cyan());
                for pack_path in &packs {
                    println!("    - {}", pack_path.display());
                }
                for list_path in &lists {
                    println!("    - {}", list_path.display());
                }
                println!();
            }
            error!(pack = %stem, "Conflict found during pair resolution");
            continue;
        }

        if packs.is_empty() {
            has_skipped_items = true;
            missing_pack_stems.push(stem);
            continue;
        }

        if lists.is_empty() {
            has_skipped_items = true;
            missing_list_stems.push(stem);
            continue;
        }

        let pack_path = packs[0].clone();
        let list_path = lists[0].clone();

        if pack_path.parent() != list_path.parent() {
            has_skipped_items = true;
            if show_ui {
                println!(
                    "  {} ERROR: {} pack and list are in different directories:",
                    "!".yellow(),
                    stem.cyan()
                );
                println!("    - Pack: {}", pack_path.display());
                println!("    - List: {}", list_path.display());
                println!();
            }
            error!(pack = %stem, "Pack and list mismatch detected");
            continue;
        }

        debug!("Resolved valid pair for {}", stem);
        valid_pairs.push(PackPair {
            name: stem,
            pack_path,
            list_path,
        });
    }

    if !missing_pack_stems.is_empty() || !missing_list_stems.is_empty() {
        if show_ui {
            println!(
                "  {} ERROR: The following .list files have no matching .pack file:",
                "!".yellow()
            );
            for stem in &missing_pack_stems {
                println!("    - {}.list", stem.cyan());
            }
            println!();

            println!(
                "  {} ERROR: The following .pack files have no matching .list file:",
                "!".yellow()
            );
            for stem in &missing_list_stems {
                println!("    - {}.pack", stem.cyan());
            }
            println!();
        }
        error!("Orphaned lists or packs detected during scan");
    }

    if has_skipped_items {
        if show_ui {
            println!("  {} Skipping conflicting and fragmented packs\n", "✗".red());
        }
        error!("Skipping fragmented packs");
    }

    if valid_pairs.is_empty() {
        return Err("No valid, non-conflicting pack/list pairs found to decrypt.".to_string());
    }

    Ok(valid_pairs)
}

fn traverse_directory(dir: &Path, packs: &mut Vec<PathBuf>, lists: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            traverse_directory(&path, packs, lists);
            continue;
        }

        let ext = path.extension().unwrap_or_default().to_string_lossy().to_lowercase();

        match ext.as_str() {
            "apk" | "xapk" | "apkm" | "apks" | "zip" => {
                let archive_name = path.file_stem().unwrap_or_default().to_string_lossy();
                let mut target_dir = get_local_dir();
                target_dir.push("apk");
                target_dir.push(archive_name.as_ref());
                extract_archive(&path, &target_dir, packs, lists);
            }
            "pack" => packs.push(path),
            "list" => lists.push(path),
            _ => {}
        }
    }
}

fn extract_archive(archive_path: &Path, target_dir: &Path, packs: &mut Vec<PathBuf>, lists: &mut Vec<PathBuf>) {
    let _ = fs::create_dir_all(target_dir);

    let Ok(file) = fs::File::open(archive_path) else {
        return;
    };
    let Ok(mut archive) = ZipArchive::new(file) else {
        return;
    };

    let mut nested_archives = Vec::new();

    for index in 0..archive.len() {
        let Ok(mut zip_file) = archive.by_index(index) else {
            continue;
        };
        if zip_file.is_dir() {
            continue;
        }

        let file_name = zip_file.name().to_string();
        let lower_name = file_name.to_lowercase();

        let is_pack = lower_name.ends_with(".pack");
        let is_list = lower_name.ends_with(".list");
        let is_nested_apk = lower_name.ends_with(".apk");

        if !is_pack && !is_list && !is_nested_apk {
            continue;
        }

        if let Some(safe_name) = Path::new(&file_name).file_name() {
            let dest_path = target_dir.join(safe_name);

            if let Ok(mut out_file) = fs::File::create(&dest_path) {
                let _ = std::io::copy(&mut zip_file, &mut out_file);

                if is_pack {
                    trace!(file = %dest_path.display(), "Extracted .pack from archive");
                    packs.push(dest_path);
                } else if is_list {
                    trace!(file = %dest_path.display(), "Extracted .list from archive");
                    lists.push(dest_path);
                } else if is_nested_apk {
                    trace!(file = %dest_path.display(), "Found nested archive");
                    nested_archives.push(dest_path);
                }
            }
        }
    }

    for nested_apk in nested_archives {
        extract_archive(&nested_apk, target_dir, packs, lists);
    }
}
