use std::fs::{self, File};
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use colored::Colorize;
use flate2::read::ZlibDecoder;
use tracing::{debug, error, info, trace, warn};

use crate::io::{get_local_dir, read_u32, read_u64};

const ARCHIVE_MAGIC: &[u8] = b"FCRA";
const ZLIB_MAGIC: [u8; 2] = [0x78, 0xDA];
const HEADER_SIZE: u64 = 16;
const ENTRY_SIZE: u64 = 24;
const MAX_CHUNKS: usize = 0x10000;

struct ArchiveEntry {
    hash: u64,
    decompressed_size: u32,
    data_position: u64,
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
    let entries = read_entry_table(archive)?;

    if entries.is_empty() {
        return Err(String::from("No files listed in archive"));
    }

    let file = File::open(archive).map_err(|err| err.to_string())?;
    fs::create_dir_all(output_base).map_err(|err| err.to_string())?;

    if show_ui {
        println!(
            "  {} Extracting {} files from {}",
            "!".yellow(),
            entries.len().to_string().cyan(),
            archive_name.cyan()
        );
    }

    let mut reader = BufReader::new(file);
    let mut extracted_count = 0;
    let mut corrupted_count = 0;
    let mut unwritable_count = 0;

    for entry in &entries {
        let hex_name = format!("{:x}", entry.hash);

        let payload = match read_payload(&mut reader, entry) {
            Ok(data) => data,
            Err(err) => {
                trace!(file = %hex_name, error = %err, "Failed to inflate archive member");
                corrupted_count += 1;
                continue;
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
                extracted_count += 1;
                trace!(file = %hex_name, size = payload.len(), dest = %final_path.display(), "Member extracted to disk");
            }
            Err(err) => {
                trace!(file = %hex_name, error = %err, "Failed to write extracted member to disk");
                unwritable_count += 1;
            }
        }
    }

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
            "  {} Extracted {} files to {}/",
            "✓".green(),
            extracted_count.to_string().cyan(),
            output_base.display().to_string().cyan()
        );
    }
    info!(archive = %archive_name, extracted = extracted_count, dest = %output_base.display(), "Archive extracted");

    Ok(extracted_count)
}

fn read_entry_table(path: &Path) -> Result<Vec<ArchiveEntry>, String> {
    let archive_size = fs::metadata(path).map_err(|err| err.to_string())?.len();
    let file = File::open(path).map_err(|err| err.to_string())?;
    let mut reader = BufReader::new(file);

    let mut header = [0u8; HEADER_SIZE as usize];
    reader
        .read_exact(&mut header)
        .map_err(|_| String::from("File is too small to be an FCRA archive"))?;

    if &header[..4] != ARCHIVE_MAGIC {
        return Err(String::from("Missing FCRA signature"));
    }

    let file_count = read_u64(&header[8..16]);
    if HEADER_SIZE + file_count.saturating_mul(ENTRY_SIZE) > archive_size {
        return Err(String::from("File table extends past the end of the archive"));
    }

    debug!(files = file_count, "Parsed FCRA header");

    let mut table = Vec::with_capacity(file_count as usize);
    let mut raw_entry = [0u8; ENTRY_SIZE as usize];

    for _ in 0..file_count {
        reader
            .read_exact(&mut raw_entry)
            .map_err(|_| String::from("File table is truncated"))?;

        table.push(ArchiveEntry {
            hash: read_u64(&raw_entry[0..8]),
            decompressed_size: read_u32(&raw_entry[12..16]),
            data_position: read_u64(&raw_entry[16..24]),
        });
    }

    Ok(table)
}

fn read_payload(reader: &mut BufReader<File>, entry: &ArchiveEntry) -> Result<Vec<u8>, String> {
    reader
        .seek(SeekFrom::Start(entry.data_position))
        .map_err(|err| err.to_string())?;

    let mut chunk_sizes = Vec::new();
    let mut probe = [0u8; 4];

    loop {
        reader
            .read_exact(&mut probe)
            .map_err(|_| String::from("Truncated chunk table"))?;

        if probe[..2] == ZLIB_MAGIC {
            reader.seek(SeekFrom::Current(-4)).map_err(|err| err.to_string())?;
            break;
        }

        if chunk_sizes.len() >= MAX_CHUNKS {
            return Err(String::from("Chunk table exceeded sane limits"));
        }

        chunk_sizes.push(read_u32(&probe) as usize);
    }

    if chunk_sizes.is_empty() {
        return Err(String::from("Member holds no compressed chunks"));
    }

    let mut payload = Vec::with_capacity(entry.decompressed_size as usize);
    let mut compressed = Vec::new();

    for size in chunk_sizes {
        compressed.clear();
        compressed.resize(size, 0);
        reader
            .read_exact(&mut compressed)
            .map_err(|_| String::from("Compressed chunk is truncated"))?;

        ZlibDecoder::new(compressed.as_slice())
            .read_to_end(&mut payload)
            .map_err(|err| err.to_string())?;
    }

    Ok(payload)
}

fn detect_extension(payload: &[u8]) -> &'static str {
    if payload.starts_with(b"CSB") {
        ".csb"
    } else if payload.starts_with(b"BNTX") {
        ".bntx"
    } else {
        ""
    }
}
