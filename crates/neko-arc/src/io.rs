use std::fs::{self, File};
use std::path::{Path, PathBuf};

pub const DEFAULT_LEVEL: usize = 10;

pub fn get_local_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn walk_files(root: &Path, level: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    collect_files(root, 1, level.max(1), &mut found);
    found.sort();
    found
}

fn collect_files(dir: &Path, depth: usize, level: usize, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if path.is_dir() {
            if depth < level {
                collect_files(&path, depth + 1, level, found);
            }
            continue;
        }

        found.push(path);
    }
}

pub fn beside_binary(name: &str) -> Option<PathBuf> {
    let binary = std::env::current_exe().ok()?;
    let resolved = fs::canonicalize(&binary).unwrap_or(binary);
    Some(resolved.parent()?.join(name))
}

pub fn file_name_of(path: &Path) -> Option<&str> {
    path.file_name().and_then(|name| name.to_str())
}

pub fn has_extension(path: &Path, wanted: &str) -> bool {
    path.extension()
        .is_some_and(|ext| ext.to_str().is_some_and(|ext| ext.eq_ignore_ascii_case(wanted)))
}

pub fn read_u32(bytes: &[u8]) -> u32 {
    let mut buffer = [0u8; 4];
    buffer.copy_from_slice(&bytes[..4]);
    u32::from_le_bytes(buffer)
}

pub fn read_u64(bytes: &[u8]) -> u64 {
    let mut buffer = [0u8; 8];
    buffer.copy_from_slice(&bytes[..8]);
    u64::from_le_bytes(buffer)
}

#[cfg(unix)]
pub fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
}

#[cfg(windows)]
pub fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        let read = std::os::windows::fs::FileExt::seek_read(file, buf, offset)?;
        if read == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf = &mut buf[read..];
        offset += read as u64;
    }
    Ok(())
}

#[cfg(unix)]
pub fn write_all_at(file: &File, buf: &[u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::write_all_at(file, buf, offset)
}

#[cfg(windows)]
pub fn write_all_at(file: &File, mut buf: &[u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        let written = std::os::windows::fs::FileExt::seek_write(file, buf, offset)?;
        if written == 0 {
            return Err(std::io::ErrorKind::WriteZero.into());
        }
        buf = &buf[written..];
        offset += written as u64;
    }
    Ok(())
}
