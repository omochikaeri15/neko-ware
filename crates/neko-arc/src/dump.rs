mod keys;
mod nca;
mod nsp;
mod patch;
mod romfs;

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use colored::Colorize;
use rayon::prelude::*;
use tracing::{debug, error, info, warn};

use crate::io::{beside_binary, get_local_dir, has_extension, write_all_at};

pub(crate) use keys::KeySet;
pub(crate) use romfs::RomFile;

const KEYS_NAME: &str = "prod.keys";
const CHUNK_SIZE: u64 = 0x10_0000;

pub(crate) trait Storage: Sync {
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
    handle: Mutex<Option<Arc<File>>>,
    pending: AtomicUsize,
}

pub(crate) struct Rom<'a> {
    source: Source<'a>,
    pub(crate) package: &'a str,
    pub(crate) applied: String,
    pub(crate) patched: bool,
    pub(crate) files: Vec<RomFile>,
    pub(crate) output_base: PathBuf,
}

struct Copied {
    written: usize,
    failed: usize,
    error: Option<String>,
}

pub(crate) fn execute(
    targets: &[PathBuf],
    keys_path: Option<&str>,
    output_dir: Option<&str>,
    extra: bool,
    show_ui: bool,
) {
    let paths: Vec<&PathBuf> = targets.iter().filter(|path| path.is_file()).collect();

    if paths.is_empty() {
        if show_ui {
            println!("\n  {} No NSP files found at the given paths\n", "✗".red());
        }
        error!("No NSP files found at the given paths");
        std::process::exit(1);
    }

    let keys = load_keys(keys_path, show_ui);
    debug!(packages = paths.len(), extra, "Starting NSP dump");

    let (total_dumped, failed_count) = for_each_rom(&paths, &keys, output_dir, "dump", show_ui, |rom| {
        dump_rom(rom, extra, show_ui)
    });

    let noun = if extra { "files" } else { "archives" };

    if total_dumped == 0 {
        if show_ui {
            println!("\nFAILURE: Dumped no {noun}!\n");
        }
        error!(failed = failed_count, "Dumped no files");
        std::process::exit(1);
    }

    if show_ui {
        println!("\nSUCCESS: Dumped {} {noun}!\n", total_dumped.to_string().cyan());
    }
    info!(dumped = total_dumped, failed = failed_count, "NSP dump complete");
}

pub(crate) fn load_keys(keys_path: Option<&str>, show_ui: bool) -> KeySet {
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

    let keys = KeySet::load(&keys_path).unwrap_or_else(|err| {
        if show_ui {
            println!("\n  {} {}\n", "✗".red(), err);
        }
        error!(path = %keys_path.display(), error = %err, "Failed to load keys");
        std::process::exit(1);
    });
    debug!(path = %keys_path.display(), "Loaded console keys");

    keys
}

pub(crate) fn for_each_rom(
    paths: &[&PathBuf],
    keys: &KeySet,
    output_dir: Option<&str>,
    action: &str,
    show_ui: bool,
    mut handle: impl FnMut(&Rom) -> Result<usize, String>,
) -> (usize, usize) {
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

        match scan_package(path, keys) {
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
    let mut total_written = 0;

    for game in &games {
        match (game.base, game.update) {
            (Some(base), update) => {
                let package_name = &packages[contents[base].package].name;
                let output_base = match output_dir {
                    Some(dir) if nested => Path::new(dir).join(package_name),
                    Some(dir) => PathBuf::from(dir),
                    None => get_local_dir().join(package_name),
                };

                let outcome = open_rom(&packages, &contents, base, update, output_base).and_then(|rom| handle(&rom));

                match outcome {
                    Ok(written) => total_written += written,
                    Err(err) => {
                        failed_count += 1;
                        if show_ui {
                            println!("  {} Failed to {} {}: {}", "✗".red(), action, package_name.cyan(), err);
                        }
                        error!(package = %package_name, action, error = %err, "Failed to process game");
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

    (total_written, failed_count)
}

fn scan_package(path: &Path, keys: &KeySet) -> Result<(File, Vec<nca::RomSection>), String> {
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

fn open_rom<'a>(
    packages: &'a [Package],
    contents: &'a [Content],
    base: usize,
    update: Option<usize>,
    output_base: PathBuf,
) -> Result<Rom<'a>, String> {
    let base = &contents[base];
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

    if files.is_empty() {
        return Err(String::from("No files found in the NSP RomFS"));
    }

    Ok(Rom {
        source,
        package: &packages[base.package].name,
        applied,
        patched: update.is_some(),
        files,
        output_base,
    })
}

fn dump_rom(rom: &Rom, extra: bool, show_ui: bool) -> Result<usize, String> {
    let noun = if extra { "files" } else { "archives" };
    let entries: Vec<&RomFile> = rom
        .files
        .iter()
        .filter(|entry| has_extension(Path::new(&entry.path), "arc") != extra)
        .collect();

    if entries.is_empty() {
        return Err(if extra {
            String::from("No files besides archives found in the NSP")
        } else {
            String::from("No .arc files found in the NSP")
        });
    }

    if show_ui {
        println!(
            "  {} Dumping {} {} from {}{}",
            "!".yellow(),
            entries.len().to_string().cyan(),
            noun,
            rom.package.cyan(),
            rom.applied
        );
    }
    debug!(
        files = entries.len(),
        extra,
        patched = rom.patched,
        "Listed RomFS files"
    );

    let copied = copy_files(rom, &entries)?;

    if let Some(err) = &copied.error {
        if show_ui {
            println!(
                "  {} Could not dump {} {} from {}: {}",
                "✗".red(),
                copied.failed.to_string().cyan(),
                noun,
                rom.package.cyan(),
                err
            );
        }
        warn!(package = %rom.package, failed = copied.failed, error = %err, "Could not dump files");
    }

    if copied.written == 0 {
        return Err(if extra {
            String::from("Every file failed to dump")
        } else {
            String::from("Every archive failed to dump")
        });
    }

    if show_ui {
        println!(
            "  {} Dumped {} {} to {}",
            "✓".green(),
            copied.written.to_string().cyan(),
            noun,
            format!("{}/", rom.output_base.display()).cyan()
        );
    }
    info!(package = %rom.package, dumped = copied.written, dest = %rom.output_base.display(), "Game dumped");

    Ok(copied.written)
}

fn copy_files(rom: &Rom, entries: &[&RomFile]) -> Result<Copied, String> {
    let outputs: Vec<Output> = entries
        .iter()
        .map(|entry| Output {
            offset: entry.offset,
            size: entry.size,
            path: rom.output_base.join(&entry.path),
            handle: Mutex::new(None),
            pending: AtomicUsize::new(entry.size.div_ceil(CHUNK_SIZE) as usize),
        })
        .collect();

    let mut directories: Vec<&Path> = outputs.iter().filter_map(|output| output.path.parent()).collect();
    directories.sort_unstable();
    directories.dedup();

    for directory in directories {
        fs::create_dir_all(directory).map_err(|err| format!("Could not create {}: {err}", directory.display()))?;
    }

    outputs
        .par_iter()
        .filter(|output| output.size == 0)
        .try_for_each(|output| {
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

            let outcome = rom.read_at(output.offset + start, buffer).and_then(|()| {
                let file = output.open()?;
                write_all_at(&file, buffer, start).map_err(|err| err.to_string())
            });
            output.release();

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

    Ok(Copied {
        written: outputs.len() - failed.len(),
        failed: failed.len(),
        error: failures.into_iter().next().map(|(_, err)| err),
    })
}

impl Output {
    fn open(&self) -> Result<Arc<File>, String> {
        let mut slot = self
            .handle
            .lock()
            .map_err(|_| String::from("Output handle lock was poisoned"))?;

        if let Some(file) = slot.as_ref() {
            return Ok(Arc::clone(file));
        }

        let file = File::create(&self.path).map_err(|err| format!("Could not create {}: {err}", self.path.display()))?;
        let file = Arc::new(file);
        *slot = Some(Arc::clone(&file));
        Ok(file)
    }

    fn release(&self) {
        if self.pending.fetch_sub(1, Ordering::AcqRel) == 1
            && let Ok(mut slot) = self.handle.lock()
        {
            slot.take();
        }
    }
}

impl Storage for Source<'_> {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        match self {
            Self::Base(reader) => reader.read_at(offset, buf),
            Self::Patched(storage) => storage.read_at(offset, buf),
        }
    }
}

impl Storage for Rom<'_> {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        self.source.read_at(offset, buf)
    }
}
