use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use colored::Colorize;
use rayon::prelude::*;
use tracing::{debug, error, info, trace, warn};

use crate::archive::{ArchiveEntry, detect_extension, inflate, read_entries};
use crate::convert::{write_table, write_textures};
use crate::decode::load_dictionary;
use crate::dictionary::DEFAULT_FILE_NAME;
use crate::dump::{self, Rom, RomFile, Storage};
use crate::io::{beside_binary, has_extension};

struct Archive {
    offset: u64,
    destination: PathBuf,
}

struct Done {
    written: usize,
    renamed: bool,
    converted: bool,
}

#[derive(Default)]
struct Tally {
    members: usize,
    written: usize,
    renamed: usize,
    converted: usize,
    failed: usize,
    error: Option<String>,
}

pub(crate) fn execute(targets: &[PathBuf], keys_path: Option<&str>, dictionary_path: Option<&str>, show_ui: bool) {
    let keys = dump::load_keys(keys_path, show_ui);
    let names = load_names(dictionary_path, show_ui);
    let paths: Vec<&PathBuf> = targets.iter().collect();
    debug!(packages = paths.len(), entries = names.len(), "Starting rip");

    let (total_written, failed_count) = dump::for_each_rom(&paths, &keys, None, "process", show_ui, |rom| {
        process_rom(rom, &names, show_ui)
    });

    if total_written == 0 {
        if show_ui {
            println!("\nFAILURE: Wrote no files!\n");
        }
        error!(failed = failed_count, "Rip wrote no files");
        std::process::exit(1);
    }

    if show_ui {
        println!("\nSUCCESS: Wrote {} files!\n", total_written.to_string().cyan());
    }
    info!(written = total_written, failed = failed_count, "Rip complete");
}

fn load_names(dictionary_path: Option<&str>, show_ui: bool) -> HashMap<u64, String> {
    let Some(path) = dictionary_path.map(PathBuf::from).or_else(|| beside_binary(DEFAULT_FILE_NAME)) else {
        abort("Could not locate the neko-arc binary, pass --dictionary instead", show_ui);
    };

    let pairs = load_dictionary(&path).unwrap_or_else(|err| {
        abort(&format!("Could not read dictionary {}: {err}", path.display()), show_ui);
    });
    debug!(path = %path.display(), entries = pairs.len(), "Loaded dictionary");

    let mut names = HashMap::with_capacity(pairs.len());

    for (digest, name) in pairs {
        let Ok(hash) = u64::from_str_radix(&digest, 16) else {
            trace!(hash = %digest, "Ignoring dictionary entry with a malformed hash");
            continue;
        };
        names.entry(hash).or_insert(name);
    }

    names
}

fn process_rom(rom: &Rom, names: &HashMap<u64, String>, show_ui: bool) -> Result<usize, String> {
    let archive_files: Vec<&RomFile> = rom
        .files
        .iter()
        .filter(|entry| has_extension(Path::new(&entry.path), "arc"))
        .collect();

    if archive_files.is_empty() {
        return Err(String::from("No .arc files found in the NSP"));
    }

    if show_ui {
        println!(
            "  {} Processing {} archives from {}{}",
            "!".yellow(),
            archive_files.len().to_string().cyan(),
            rom.package.cyan(),
            rom.applied
        );
    }
    debug!(archives = archive_files.len(), patched = rom.patched, "Listed RomFS archives");

    let mut archives = Vec::with_capacity(archive_files.len());
    let mut members = Vec::new();

    for file in archive_files {
        let name = Path::new(&file.path)
            .file_stem()
            .map_or_else(|| String::from("archive"), |stem| stem.to_string_lossy().into_owned());

        let entries = match read_entries(file.size, |offset, buf| rom.read_at(file.offset + offset, buf)) {
            Ok(entries) => entries,
            Err(err) => {
                if show_ui {
                    println!("  {} Failed to read {}: {}", "✗".red(), name.cyan(), err);
                }
                warn!(archive = %name, error = %err, "Failed to read archive table");
                continue;
            }
        };
        debug!(archive = %name, members = entries.len(), "Read archive table");

        let destination = rom.output_base.join(&name);
        fs::create_dir_all(&destination).map_err(|err| format!("Could not create {}: {err}", destination.display()))?;

        let index = archives.len();
        archives.push(Archive {
            offset: file.offset,
            destination,
        });
        members.extend(entries.into_iter().map(|entry| (index, entry)));
    }

    members.sort_unstable_by_key(|(_, entry)| std::cmp::Reverse(entry.decompressed_size));

    let results: Vec<Result<Done, String>> = members
        .par_iter()
        .with_max_len(1)
        .map_init(Vec::new, |buffer, (archive, entry)| {
            let outcome = process_member(rom, &archives[*archive], entry, names, buffer);
            if let Err(err) = &outcome {
                warn!(hash = %format!("{:x}", entry.hash), error = %err, "Failed to process archive member");
            }
            outcome
        })
        .collect();

    let mut tally = Tally::default();

    for outcome in results {
        match outcome {
            Ok(done) => {
                tally.members += 1;
                tally.written += done.written;
                tally.renamed += usize::from(done.renamed);
                tally.converted += usize::from(done.converted);
            }
            Err(err) => {
                tally.failed += 1;
                tally.error.get_or_insert(err);
            }
        }
    }

    if let Some(err) = &tally.error {
        if show_ui {
            println!(
                "  {} Could not process {} files from {}: {}",
                "✗".red(),
                tally.failed.to_string().cyan(),
                rom.package.cyan(),
                err
            );
        }
        warn!(package = %rom.package, failed = tally.failed, error = %err, "Could not process archive members");
    }

    if tally.written == 0 {
        return Err(String::from("Every archive member failed to process"));
    }

    if show_ui {
        println!(
            "  {} Unpacked {} files from {} archives, {} renamed and {} converted",
            "✓".green(),
            tally.members.to_string().cyan(),
            archives.len().to_string().cyan(),
            tally.renamed.to_string().cyan(),
            tally.converted.to_string().cyan()
        );
        println!(
            "  {} Wrote {} files to {}",
            "✓".green(),
            tally.written.to_string().cyan(),
            format!("{}/", rom.output_base.display()).cyan()
        );
    }
    info!(package = %rom.package, written = tally.written, dest = %rom.output_base.display(), "Game processed");

    Ok(tally.written)
}

fn process_member(
    rom: &Rom,
    archive: &Archive,
    entry: &ArchiveEntry,
    names: &HashMap<u64, String>,
    buffer: &mut Vec<u8>,
) -> Result<Done, String> {
    buffer.resize(entry.span as usize, 0);
    rom.read_at(archive.offset + entry.data_position, buffer)
        .map_err(|_| String::from("Member data is truncated"))?;

    let payload = inflate(buffer, entry.decompressed_size)?;
    let resolved = names.get(&entry.hash);
    let fallback = format!("{:x}", entry.hash);
    let name = resolved.map_or(fallback.as_str(), String::as_str);

    let (written, converted) = match detect_extension(&payload) {
        ".bntx" => (write_textures(&payload, name, &archive.destination, false)?, true),
        ".csb" => (write_table(&payload, name, &archive.destination)?, true),
        _ => {
            fs::write(archive.destination.join(name), &payload).map_err(|err| err.to_string())?;
            (1, false)
        }
    };

    Ok(Done {
        written,
        renamed: resolved.is_some(),
        converted,
    })
}

fn abort(message: &str, show_ui: bool) -> ! {
    if show_ui {
        println!("\n  {} {}\n", "✗".red(), message);
    }
    error!("{}", message);
    std::process::exit(1);
}
