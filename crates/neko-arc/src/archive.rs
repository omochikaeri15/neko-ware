use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use colored::Colorize;
use flate2::read::ZlibDecoder;
use rayon::prelude::*;
use tracing::{debug, error, info, trace, warn};

use crate::io::{get_local_dir, read_exact_at, read_u32, read_u64};

const ARCHIVE_MAGIC: &[u8] = b"FCRA";
const ZLIB_MAGIC: [u8; 2] = [0x78, 0xDA];
const HEADER_SIZE: u64 = 16;
const ENTRY_SIZE: u64 = 24;
const MAX_CHUNKS: usize = 0x10000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Extraction {
    Extracted,
    Corrupted,
    Unwritable,
}

pub(crate) struct ArchiveEntry {
    pub(crate) hash: u64,
    pub(crate) decompressed_size: u32,
    pub(crate) data_position: u64,
    pub(crate) span: u64,
}

pub fn execute(targets: &[PathBuf], output_dir: Option<&str>, show_ui: bool) {
    let archives: Vec<&PathBuf> = targets.iter().filter(|path| path.is_file()).collect();

    if archives.is_empty() {
        if show_ui {
            println!("\n  {} No archives found at the given paths\n", "✗".red());
        }
        error!("No archives found at the given paths");
        std::process::exit(1);
    }

    let nested = archives.len() > 1;
    debug!(archives = archives.len(), "Starting archive extraction");

    if show_ui {
        println!();
    }

    let mut total_extracted = 0;
    let mut failed_count = 0;

    for archive in archives {
        let archive_name = archive
            .file_stem()
            .map_or_else(|| String::from("archive"), |stem| stem.to_string_lossy().into_owned());

        let output_base = match output_dir {
            Some(dir) if nested => Path::new(dir).join(&archive_name),
            Some(dir) => PathBuf::from(dir),
            None => get_local_dir().join(&archive_name),
        };

        match extract_archive(archive, &archive_name, &output_base, show_ui) {
            Ok(extracted) => total_extracted += extracted,
            Err(err) => {
                failed_count += 1;
                if show_ui {
                    println!("  {} Failed to extract {}: {}", "✗".red(), archive_name.cyan(), err);
                }
                error!(archive = %archive_name, error = %err, "Failed to extract archive");
            }
        }
    }

    if total_extracted == 0 {
        if show_ui {
            println!("\nFAILURE: Extracted no files!\n");
        }
        error!(failed = failed_count, "Extracted no files");
        std::process::exit(1);
    }

    if show_ui {
        println!("\nSUCCESS: Decrypted {} files!\n", total_extracted.to_string().cyan());
    }
    info!(
        extracted = total_extracted,
        failed = failed_count,
        "Archive extraction complete"
    );
}

fn extract_archive(archive: &Path, archive_name: &str, output_base: &Path, show_ui: bool) -> Result<usize, String> {
    let archive_size = fs::metadata(archive).map_err(|err| err.to_string())?.len();
    let file = File::open(archive).map_err(|err| err.to_string())?;
    let entries = read_entries(archive_size, |offset, buf| {
        read_exact_at(&file, buf, offset).map_err(|err| err.to_string())
    })?;

    if entries.is_empty() {
        return Err(String::from("No files listed in archive"));
    }

    fs::create_dir_all(output_base).map_err(|err| err.to_string())?;

    if show_ui {
        println!(
            "  {} Extracting {} files from {}",
            "!".yellow(),
            entries.len().to_string().cyan(),
            archive_name.cyan()
        );
    }

    let outcomes: Vec<Extraction> = entries
        .par_iter()
        .with_max_len(1)
        .map_init(Vec::new, |span, entry| extract_member(&file, entry, output_base, span))
        .collect();

    let count = |wanted: Extraction| outcomes.iter().filter(|&&outcome| outcome == wanted).count();
    let extracted_count = count(Extraction::Extracted);
    let corrupted_count = count(Extraction::Corrupted);
    let unwritable_count = count(Extraction::Unwritable);

    if corrupted_count > 0 {
        if show_ui {
            println!(
                "  {} Skipped {} unreadable files in {}",
                "✗".red(),
                corrupted_count.to_string().cyan(),
                archive_name.cyan()
            );
        }
        warn!(archive = %archive_name, corrupted = corrupted_count, "Skipped unreadable archive members");
    }

    if unwritable_count > 0 {
        if show_ui {
            println!(
                "  {} Could not write {} files from {}, check free space and permissions",
                "✗".red(),
                unwritable_count.to_string().cyan(),
                archive_name.cyan()
            );
        }
        warn!(archive = %archive_name, unwritable = unwritable_count, "Could not write extracted members");
    }

    if extracted_count == 0 {
        return Err(if unwritable_count > 0 {
            String::from("Could not write any member to disk")
        } else {
            String::from("Every member failed to inflate")
        });
    }

    if show_ui {
        println!(
            "  {} Extracted {} files to {}",
            "✓".green(),
            extracted_count.to_string().cyan(),
            format!("{}/", output_base.display()).cyan()
        );
    }
    info!(archive = %archive_name, extracted = extracted_count, dest = %output_base.display(), "Archive extracted");

    Ok(extracted_count)
}

fn extract_member(file: &File, entry: &ArchiveEntry, output_base: &Path, span: &mut Vec<u8>) -> Extraction {
    let hex_name = format!("{:x}", entry.hash);

    span.resize(entry.span as usize, 0);

    let payload = match read_exact_at(file, span, entry.data_position)
        .map_err(|_| String::from("Member data is truncated"))
        .and_then(|()| inflate(span, entry.decompressed_size))
    {
        Ok(data) => data,
        Err(err) => {
            trace!(file = %hex_name, error = %err, "Failed to inflate archive member");
            return Extraction::Corrupted;
        }
    };

    if payload.len() as u32 != entry.decompressed_size {
        trace!(
            file = %hex_name,
            expected = entry.decompressed_size,
            actual = payload.len(),
            "Inflated size mismatch"
        );
    }

    let final_path = output_base.join(format!("{hex_name}{}", detect_extension(&payload)));

    match fs::write(&final_path, &payload) {
        Ok(()) => {
            trace!(file = %hex_name, size = payload.len(), dest = %final_path.display(), "Member extracted to disk");
            Extraction::Extracted
        }
        Err(err) => {
            trace!(file = %hex_name, error = %err, "Failed to write extracted member to disk");
            Extraction::Unwritable
        }
    }
}

pub(crate) fn read_entries(
    archive_size: u64,
    read: impl Fn(u64, &mut [u8]) -> Result<(), String>,
) -> Result<Vec<ArchiveEntry>, String> {
    if archive_size < HEADER_SIZE {
        return Err(String::from("File is too small to be an FCRA archive"));
    }

    let mut header = [0u8; HEADER_SIZE as usize];
    read(0, &mut header)?;

    if &header[..4] != ARCHIVE_MAGIC {
        return Err(String::from("Missing FCRA signature"));
    }

    let file_count = read_u64(&header[8..16]);
    let table_size = file_count.saturating_mul(ENTRY_SIZE);
    if HEADER_SIZE.saturating_add(table_size) > archive_size {
        return Err(String::from("File table extends past the end of the archive"));
    }

    debug!(files = file_count, "Parsed FCRA header");

    let mut raw_table = vec![0u8; table_size as usize];
    read(HEADER_SIZE, &mut raw_table).map_err(|_| String::from("File table is truncated"))?;

    let mut table: Vec<ArchiveEntry> = raw_table
        .chunks_exact(ENTRY_SIZE as usize)
        .map(|raw_entry| ArchiveEntry {
            hash: read_u64(&raw_entry[0..8]),
            decompressed_size: read_u32(&raw_entry[12..16]),
            data_position: read_u64(&raw_entry[16..24]),
            span: 0,
        })
        .collect();

    let mut starts: Vec<u64> = table.iter().map(|entry| entry.data_position).collect();
    starts.sort_unstable();
    starts.dedup();

    for entry in &mut table {
        let next = starts
            .get(starts.partition_point(|&start| start <= entry.data_position))
            .copied()
            .unwrap_or(archive_size);
        entry.span = next.min(archive_size).saturating_sub(entry.data_position);
    }

    Ok(table)
}

pub(crate) fn inflate(span: &[u8], decompressed_size: u32) -> Result<Vec<u8>, String> {
    let mut chunk_sizes = Vec::new();
    let mut cursor = 0;

    loop {
        let probe = span
            .get(cursor..cursor + 4)
            .ok_or_else(|| String::from("Truncated chunk table"))?;

        if probe[..2] == ZLIB_MAGIC {
            break;
        }

        if chunk_sizes.len() >= MAX_CHUNKS {
            return Err(String::from("Chunk table exceeded sane limits"));
        }

        chunk_sizes.push(read_u32(probe) as usize);
        cursor += 4;
    }

    if chunk_sizes.is_empty() {
        return Err(String::from("Member holds no compressed chunks"));
    }

    let mut payload = Vec::with_capacity(decompressed_size as usize);

    for size in chunk_sizes {
        let compressed = span
            .get(cursor..cursor + size)
            .ok_or_else(|| String::from("Compressed chunk is truncated"))?;

        ZlibDecoder::new(compressed)
            .read_to_end(&mut payload)
            .map_err(|err| err.to_string())?;
        cursor += size;
    }

    Ok(payload)
}

pub(crate) fn detect_extension(payload: &[u8]) -> &'static str {
    if payload.starts_with(b"CSB") {
        ".csb"
    } else if payload.starts_with(b"BNTX") {
        ".bntx"
    } else {
        ""
    }
}
