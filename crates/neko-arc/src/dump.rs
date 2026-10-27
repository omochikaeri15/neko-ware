mod keys;
mod nca;
mod nsp;
mod patch;
mod romfs;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

use colored::Colorize;
use rayon::prelude::*;
use tracing::{debug, error, info, warn};

use crate::io::{beside_binary, get_local_dir, has_extension, write_all_at};

const KEYS_NAME: &str = "prod.keys";
const CHUNK_SIZE: u64 = 0x10_0000;

trait Storage: Sync {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String>;
}

enum Source<'a> {
    Base(nca::SectionReader<'a>),
    Patched(patch::PatchedStorage<'a>),
}

struct Package {
    name: String,
    file: File,
}

struct Content {
    package: usize,
    section: nca::RomSection,
}

struct Game {
    program_id: u64,
    base: Option<usize>,
    update: Option<usize>,
}

struct Output {
    offset: u64,
    size: u64,
    path: PathBuf,
}

pub(crate) fn execute(targets: &[PathBuf], keys_path: Option<&str>, output_dir: Option<&str>, show_ui: bool) {
    let paths: Vec<&PathBuf> = targets.iter().filter(|path| path.is_file()).collect();

    if paths.is_empty() {
        if show_ui {
            println!("\n  {} No NSP files found at the given paths\n", "✗".red());
        }
        error!("No NSP files found at the given paths");
        std::process::exit(1);
    }

    let Some(keys_path) = keys_path.map(PathBuf::from).or_else(|| beside_binary(KEYS_NAME)) else {
        if show_ui {
            println!(
                "\n  {} Could not locate the neko-arc binary, pass --keys instead\n",
                "✗".red()
            );
        }
        error!("Could not locate the neko-arc binary to find prod.keys beside it");
        std::process::exit(1);
    };

    let keys = keys::KeySet::load(&keys_path).unwrap_or_else(|err| {
        if show_ui {
            println!("\n  {} {}\n", "✗".red(), err);
        }
        error!(path = %keys_path.display(), error = %err, "Failed to load keys");
        std::process::exit(1);
    });
    debug!(path = %keys_path.display(), "Loaded console keys");
    debug!(packages = paths.len(), "Starting NSP dump");

    if show_ui {
        println!();
    }

    let mut packages = Vec::new();
    let mut contents = Vec::new();
    let mut failed_count = 0;

    for path in paths {
        let name = path
            .file_stem()
            .map_or_else(|| String::from("package"), |stem| stem.to_string_lossy().into_owned());

        match scan_package(path, &keys) {
            Ok((file, sections)) => {
                contents.extend(sections.into_iter().map(|section| Content {
                    package: packages.len(),
                    section,
                }));
                packages.push(Package { name, file });
            }
            Err(err) => {
                failed_count += 1;
                if show_ui {
                    println!("  {} Failed to read {}: {}", "✗".red(), name.cyan(), err);
                }
                error!(package = %name, error = %err, "Failed to read package");
            }
        }
    }

    let games = pair_games(&packages, &contents, show_ui);
    let nested = games.len() > 1;
    let mut total_dumped = 0;

    for game in &games {
        match (game.base, game.update) {
            (Some(base), update) => {
                let package_name = &packages[contents[base].package].name;
                let output_base = match output_dir {
                    Some(dir) if nested => Path::new(dir).join(package_name),
                    Some(dir) => PathBuf::from(dir),
                    None => get_local_dir().join(package_name),
                };

                match dump_game(&packages, &contents, base, update, &output_base, show_ui) {
                    Ok(dumped) => total_dumped += dumped,
                    Err(err) => {
                        failed_count += 1;
                        if show_ui {
                            println!("  {} Failed to dump {}: {}", "✗".red(), package_name.cyan(), err);
                        }
                        error!(package = %package_name, error = %err, "Failed to dump game");
                    }
                }
            }
            (None, Some(update)) => {
                failed_count += 1;
                let package_name = &packages[contents[update].package].name;
                if show_ui {
                    println!(
                        "  {} Skipped {}: the update needs its base game, name the base NSP alongside it",
                        "✗".red(),
                        package_name.cyan()
                    );
                }
                error!(package = %package_name, program = %format!("{:016x}", game.program_id), "Update has no base game");
            }
            (None, None) => {}
        }
    }

    if total_dumped == 0 {
        if show_ui {
            println!("\nFAILURE: Dumped no files!\n");
        }
        error!(failed = failed_count, "Dumped no files");
        std::process::exit(1);
    }

    if show_ui {
        println!("\nSUCCESS: Dumped {} files!\n", total_dumped.to_string().cyan());
    }
    info!(dumped = total_dumped, failed = failed_count, "NSP dump complete");
}

fn scan_package(path: &Path, keys: &keys::KeySet) -> Result<(File, Vec<nca::RomSection>), String> {
    let file = File::open(path).map_err(|err| err.to_string())?;
    let members = nsp::read_members(&file)?;
    let title_keys = nsp::read_title_keys(&file, &members)?;

    let mut sections = Vec::new();
    let mut last_error = None;

    for member in members
        .iter()
        .filter(|member| has_extension(Path::new(&member.name), "nca"))
    {
        match nca::read_program_romfs(&file, member, keys, &title_keys) {
            Ok(Some(section)) => sections.push(section),
            Ok(None) => {}
            Err(err) => {
                debug!(content = %member.name, error = %err, "Skipped unreadable content");
                last_error = Some(err);
            }
        }
    }

    if sections.is_empty() {
        return Err(last_error.unwrap_or_else(|| String::from("No game content found in the NSP")));
    }

    Ok((file, sections))
}

fn pair_games(packages: &[Package], contents: &[Content], show_ui: bool) -> Vec<Game> {
    let mut games: Vec<Game> = Vec::new();

    for (index, content) in contents.iter().enumerate() {
        let program_id = content.section.program_id;
        let position = games
            .iter()
            .position(|game| game.program_id == program_id)
            .unwrap_or_else(|| {
                games.push(Game {
                    program_id,
                    base: None,
                    update: None,
                });
                games.len() - 1
            });

        let game = &mut games[position];
        let (slot, kind) = if content.section.patch.is_some() {
            (&mut game.update, "update")
        } else {
            (&mut game.base, "base game")
        };

        if slot.is_some() {
            let package_name = &packages[content.package].name;
            if show_ui {
                println!(
                    "  {} Ignoring a second {} for {:016x} in {}",
                    "!".yellow(),
                    kind,
                    program_id,
                    package_name.cyan()
                );
            }
            warn!(package = %package_name, kind, program = %format!("{program_id:016x}"), "Ignoring duplicate content");
            continue;
        }

        *slot = Some(index);
    }

    games
}

fn dump_game(
    packages: &[Package],
    contents: &[Content],
    base: usize,
    update: Option<usize>,
    output_base: &Path,
    show_ui: bool,
) -> Result<usize, String> {
    let base = &contents[base];
    let package_name = &packages[base.package].name;
    let base_reader = nca::SectionReader::open(&packages[base.package].file, &base.section);
    let update = update.map(|index| &contents[index]);

    let (source, data_offset) = match update {
        Some(update) => (
            Source::Patched(patch::PatchedStorage::open(
                base_reader,
                nca::SectionReader::open(&packages[update.package].file, &update.section),
            )?),
            update.section.data_offset,
        ),
        None => (Source::Base(base_reader), base.section.data_offset),
    };

    let applied = match update.map(|update| update.package) {
        Some(package) if package == base.package => String::from(" with its update applied"),
        Some(package) => format!(" with {} applied", packages[package].name.cyan()),
        None => String::new(),
    };

    let files = romfs::list_files(&source, data_offset)?;
    let archive_count = files
        .iter()
        .filter(|entry| has_extension(Path::new(&entry.path), "arc"))
        .count();

    if show_ui {
        println!(
            "  {} Dumping {} files, {} of them archives, from {}{}",
            "!".yellow(),
            files.len().to_string().cyan(),
            archive_count.to_string().cyan(),
            package_name.cyan(),
            applied
        );
    }
    debug!(
        files = files.len(),
        archives = archive_count,
        patched = update.is_some(),
        "Listed RomFS files"
    );

    let outputs: Vec<Output> = files
        .into_iter()
        .map(|entry| Output {
            offset: entry.offset,
            size: entry.size,
            path: output_base.join(&entry.path),
        })
        .collect();

    if outputs.is_empty() {
        return Err(String::from("No files found in the NSP RomFS"));
    }

    let mut directories: Vec<&Path> = outputs.iter().filter_map(|output| output.path.parent()).collect();
    directories.sort_unstable();
    directories.dedup();

    for directory in directories {
        fs::create_dir_all(directory).map_err(|err| format!("Could not create {}: {err}", directory.display()))?;
    }

    outputs.par_iter().try_for_each(|output| {
        File::create(&output.path)
            .map(drop)
            .map_err(|err| format!("Could not create {}: {err}", output.path.display()))
    })?;

    let chunks: Vec<(usize, u64)> = outputs
        .iter()
        .enumerate()
        .flat_map(|(index, output)| {
            (0..output.size)
                .step_by(CHUNK_SIZE as usize)
                .map(move |start| (index, start))
        })
        .collect();

    let failures: Vec<(usize, String)> = chunks
        .par_iter()
        .map_init(Vec::new, |buffer, &(index, start)| {
            let output = &outputs[index];
            let length = CHUNK_SIZE.min(output.size - start) as usize;
            buffer.resize(length, 0);

            let outcome = source
                .read_at(output.offset + start, buffer)
                .and_then(|()| {
                    OpenOptions::new()
                        .write(true)
                        .open(&output.path)
                        .and_then(|file| write_all_at(&file, buffer, start))
                        .map_err(|err| err.to_string())
                });

            outcome.err().map(|err| {
                warn!(file = %output.path.display(), offset = start, error = %err, "Failed to dump file chunk");
                (index, err)
            })
        })
        .flatten()
        .collect();

    let mut failed: Vec<usize> = failures.iter().map(|(index, _)| *index).collect();
    failed.sort_unstable();
    failed.dedup();

    for &index in &failed {
        let output = &outputs[index];
        let _ = fs::remove_file(&output.path);
        warn!(file = %output.path.display(), "Removed partially dumped file");
    }

    if let Some((_, err)) = failures.first() {
        if show_ui {
            println!(
                "  {} Could not dump {} files from {}: {}",
                "✗".red(),
                failed.len().to_string().cyan(),
                package_name.cyan(),
                err
            );
        }
        warn!(package = %package_name, failed = failed.len(), error = %err, "Could not dump files");
    }

    let dumped = outputs.len() - failed.len();
    if dumped == 0 {
        return Err(String::from("Every file failed to dump"));
    }

    if show_ui {
        println!(
            "  {} Dumped {} files to {}/",
            "✓".green(),
            dumped.to_string().cyan(),
            output_base.display().to_string().cyan()
        );
    }
    info!(package = %package_name, dumped, dest = %output_base.display(), "Game dumped");

    Ok(dumped)
}

impl Storage for Source<'_> {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        match self {
            Self::Base(reader) => reader.read_at(offset, buf),
            Self::Patched(storage) => storage.read_at(offset, buf),
        }
    }
}
