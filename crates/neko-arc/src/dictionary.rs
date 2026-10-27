use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use colored::Colorize;
use tracing::{debug, error, info, trace};

use crate::hash::hash_hex;
use crate::io::{file_name_of, get_local_dir, walk_files};

pub(crate) const DEFAULT_FILE_NAME: &str = "dictionary.csv";

pub fn execute(input_target: &str, level: usize, output_target: Option<&str>, show_ui: bool) {
    let source_dir = Path::new(input_target);

    if !source_dir.is_dir() {
        if show_ui {
            println!("\n  {} Directory not found: {}\n", "✗".red(), source_dir.display());
        }
        error!(path = %source_dir.display(), "Directory not found");
        std::process::exit(1);
    }

    let destination = resolve_destination(output_target);
    debug!(source = %source_dir.display(), level, dest = %destination.display(), "Building dictionary");

    let mut seen = HashSet::new();
    let mut lines = String::new();
    let mut entry_count = 0;

    for path in walk_files(source_dir, level) {
        let Some(name) = file_name_of(&path) else {
            continue;
        };

        let digest = hash_hex(name);

        if !seen.insert(digest.clone()) {
            trace!(hash = %digest, name = %name, "Ignoring duplicate hash");
            continue;
        }

        lines.push_str(&digest);
        lines.push(',');
        lines.push_str(name);
        lines.push('\n');
        entry_count += 1;
    }

    if entry_count == 0 {
        if show_ui {
            println!(
                "\n  {} No files found to hash in {}\n",
                "!".yellow(),
                source_dir.display()
            );
        }
        error!(path = %source_dir.display(), "No files found to hash");
        std::process::exit(1);
    }

    if let Some(parent) = destination.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Err(err) = fs::write(&destination, lines) {
        if show_ui {
            println!("\n  {} Could not write dictionary: {}\n", "✗".red(), err);
        }
        error!(dest = %destination.display(), error = %err, "Could not write dictionary");
        std::process::exit(1);
    }

    if show_ui {
        println!(
            "\n  {} Wrote {} entries to {}",
            "✓".green(),
            entry_count.to_string().cyan(),
            destination.display().to_string().cyan()
        );
        println!("\nSUCCESS: Mapped {} names!\n", entry_count.to_string().cyan());
    }
    info!(entries = entry_count, dest = %destination.display(), "Dictionary written");
}

fn resolve_destination(output_target: Option<&str>) -> PathBuf {
    let Some(target) = output_target else {
        return get_local_dir().join(DEFAULT_FILE_NAME);
    };

    let path = PathBuf::from(target);

    if path.is_dir() || path.extension().is_none() {
        return path.join(DEFAULT_FILE_NAME);
    }

    path
}
