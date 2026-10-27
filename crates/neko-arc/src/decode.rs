use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use colored::Colorize;
use tracing::{debug, error, info, trace, warn};

use crate::convert::{TAB_SEPARATOR, name_separator};
use crate::hash::hash_hex;
use crate::io::{file_name_of, walk_files};

const CONVERTED_TABLES: [&str; 2] = ["csv", "tsv"];

struct Restore {
    name: String,
    contents: Option<String>,
}

pub struct DecodeRequest<'a> {
    pub names: &'a [String],
    pub directories: &'a [(PathBuf, usize)],
    pub dictionary: Option<&'a Path>,
}

struct Mapping {
    order: Vec<String>,
    names: HashMap<String, String>,
}

impl Mapping {
    fn new() -> Self {
        Self {
            order: Vec::new(),
            names: HashMap::new(),
        }
    }

    fn insert(&mut self, digest: String, name: String) {
        if self.names.contains_key(&digest) {
            trace!(hash = %digest, name = %name, "Ignoring duplicate hash");
            return;
        }

        self.order.push(digest.clone());
        self.names.insert(digest, name);
    }

    fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

pub fn execute(request: &DecodeRequest, show_ui: bool) {
    let mut mapping = Mapping::new();

    for name in request.names {
        mapping.insert(hash_hex(name), name.to_string());
    }

    for (directory, level) in request.directories {
        if !directory.is_dir() {
            if show_ui {
                println!("\n  {} Directory not found: {}\n", "✗".red(), directory.display());
            }
            error!(path = %directory.display(), "Directory not found");
            std::process::exit(1);
        }

        debug!(path = %directory.display(), level, "Walking directory for asset names");

        for path in walk_files(directory, *level) {
            if let Some(name) = file_name_of(&path) {
                mapping.insert(hash_hex(name), name.to_string());
            }
        }
    }

    if let Some(dictionary_path) = request.dictionary {
        insert_dictionary(&mut mapping, dictionary_path, show_ui);
    }

    if mapping.is_empty() {
        if show_ui {
            println!("\n  {} No names resolved from the given input\n", "!".yellow());
        }
        warn!("No names resolved from the given input");
        std::process::exit(1);
    }

    print_mapping(&mapping, show_ui);
}

pub fn rename(targets: &[PathBuf], dictionary_path: &Path, print: bool, show_ui: bool) {
    let mut mapping = Mapping::new();
    insert_dictionary(&mut mapping, dictionary_path, show_ui);
    debug!(
        files = targets.len(),
        entries = mapping.order.len(),
        "Renaming hashed files"
    );
    rename_matches(&mapping, targets, print, show_ui);
}

fn insert_dictionary(mapping: &mut Mapping, dictionary_path: &Path, show_ui: bool) {
    match load_dictionary(dictionary_path) {
        Ok(pairs) => {
            for (digest, name) in pairs {
                mapping.insert(digest, name);
            }
        }
        Err(err) => {
            if show_ui {
                println!("\n  {} Could not read dictionary: {}\n", "✗".red(), err);
            }
            error!(path = %dictionary_path.display(), error = %err, "Could not read dictionary");
            std::process::exit(1);
        }
    }
}

fn print_mapping(mapping: &Mapping, show_ui: bool) {
    if show_ui {
        println!();
    }

    for digest in &mapping.order {
        let Some(name) = mapping.names.get(digest) else {
            continue;
        };

        if show_ui {
            println!("  {}  {}", digest.cyan(), name);
        }
        info!(hash = %digest, name = %name, "Resolved asset hash");
    }

    if show_ui {
        println!("\nSUCCESS: Hashed {} names!\n", mapping.order.len().to_string().cyan());
    }
}

fn rename_matches(mapping: &Mapping, targets: &[PathBuf], print: bool, show_ui: bool) {
    let mut renamed_count = 0;
    let mut skipped_count = 0;

    if show_ui && print {
        println!();
    }

    for path in targets {
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };

        let Some(proper_name) = mapping.names.get(&stem.to_ascii_lowercase()) else {
            continue;
        };

        let Some(parent) = path.parent() else {
            continue;
        };

        let restore = plan_restore(proper_name, path);
        let target = parent.join(&restore.name);

        if target == *path {
            continue;
        }

        if target.exists() {
            if show_ui {
                println!(
                    "  {} Skipped {} ({} already exists)",
                    "!".yellow(),
                    stem.cyan(),
                    proper_name.cyan()
                );
            }
            warn!(hash = %stem, name = %proper_name, "Rename target already exists");
            skipped_count += 1;
            continue;
        }

        let outcome = match restore.contents {
            Some(contents) => fs::write(&target, contents).and_then(|()| fs::remove_file(path)),
            None => fs::rename(path, &target),
        };

        if let Err(err) = outcome {
            if show_ui {
                println!("  {} Failed to rename {}: {}", "✗".red(), stem.cyan(), err);
            }
            error!(hash = %stem, error = %err, "Failed to rename file");
            skipped_count += 1;
            continue;
        }

        renamed_count += 1;
        if show_ui && print {
            println!("  {}  {}", stem.cyan(), restore.name);
        }
        trace!(from = %path.display(), to = %target.display(), "Renamed hashed file");
    }

    if renamed_count == 0 {
        if show_ui {
            println!("\nFAILURE: Renamed no files!\n");
        }
        error!(skipped = skipped_count, "Renamed no files");
        std::process::exit(1);
    }

    if show_ui {
        println!("\nSUCCESS: Renamed {} files!\n", renamed_count.to_string().cyan());
    }
    info!(renamed = renamed_count, skipped = skipped_count, "Rename pass complete");
}

fn plan_restore(proper_name: &str, hashed_path: &Path) -> Restore {
    let Some(extension) = hashed_path.extension().and_then(|ext| ext.to_str()) else {
        return Restore::renamed(proper_name.to_string());
    };

    let suffix = format!(".{extension}");
    if proper_name.to_ascii_lowercase().ends_with(&suffix.to_ascii_lowercase()) {
        return Restore::renamed(proper_name.to_string());
    }

    let converted = CONVERTED_TABLES
        .iter()
        .any(|table| extension.eq_ignore_ascii_case(table));
    if converted && Path::new(proper_name).extension().is_some() {
        let current = name_separator(&suffix);
        let wanted = name_separator(proper_name);

        if current == wanted {
            return Restore::renamed(proper_name.to_string());
        }

        match fs::read_to_string(hashed_path) {
            Ok(text) if wanted == TAB_SEPARATOR && text.contains(TAB_SEPARATOR) => {
                warn!(name = %proper_name, "Table fields hold both separators, keeping its guessed extension");
            }
            Ok(text) => {
                trace!(name = %proper_name, "Swapping the guessed separator for the one its name calls for");
                return Restore {
                    name: proper_name.to_string(),
                    contents: Some(text.replace(current, wanted)),
                };
            }
            Err(err) => {
                warn!(name = %proper_name, error = %err, "Could not read table to swap its separator");
            }
        }
    }

    Restore::renamed(format!("{proper_name}{suffix}"))
}

impl Restore {
    fn renamed(name: String) -> Self {
        Self { name, contents: None }
    }
}

fn load_dictionary(path: &Path) -> Result<Vec<(String, String)>, String> {
    let content = fs::read_to_string(path).map_err(|err| err.to_string())?;
    let mut pairs = Vec::new();

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let Some((digest, name)) = line.split_once(',') else {
            trace!(line = %line, "Ignoring malformed dictionary line");
            continue;
        };

        let digest = digest.trim().to_ascii_lowercase();
        let name = name.trim();

        if digest.is_empty() || name.is_empty() {
            continue;
        }

        pairs.push((digest, name.to_string()));
    }

    if pairs.is_empty() {
        return Err(String::from("Dictionary holds no usable entries"));
    }

    Ok(pairs)
}
