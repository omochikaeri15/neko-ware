use crate::io::{read_u32, read_u64};

use super::Storage;

const HEADER_SIZE: usize = 0x50;
const DIRECTORY_ENTRY: usize = 0x18;
const FILE_ENTRY: usize = 0x20;
const EMPTY: u32 = u32::MAX;
const MAX_TABLE_SIZE: u64 = 0x1000_0000;

pub(super) struct RomFile {
    pub(super) path: String,
    pub(super) offset: u64,
    pub(super) size: u64,
}

struct Tables {
    directories: Vec<u8>,
    files: Vec<u8>,
    data_offset: u64,
}

pub(super) fn list_files(storage: &impl Storage, base: u64) -> Result<Vec<RomFile>, String> {
    let mut header = [0u8; HEADER_SIZE];
    storage.read_at(base, &mut header)?;

    if read_u64(&header[0..8]) != HEADER_SIZE as u64 {
        return Err(String::from(
            "RomFS header is malformed, the content key is likely wrong",
        ));
    }

    let read_table = |offset: u64, size: u64| -> Result<Vec<u8>, String> {
        if size > MAX_TABLE_SIZE {
            return Err(String::from("RomFS table exceeded sane limits"));
        }
        let mut table = vec![0u8; size as usize];
        storage.read_at(base + offset, &mut table)?;
        Ok(table)
    };

    let tables = Tables {
        directories: read_table(read_u64(&header[0x18..]), read_u64(&header[0x20..]))?,
        files: read_table(read_u64(&header[0x38..]), read_u64(&header[0x40..]))?,
        data_offset: base + read_u64(&header[0x48..]),
    };

    let mut found = Vec::new();
    let mut pending = vec![(0u32, String::new())];
    let limit = tables.directories.len() / DIRECTORY_ENTRY + tables.files.len() / FILE_ENTRY + 1;
    let mut visited = 0;

    while let Some((directory, prefix)) = pending.pop() {
        let entry = tables.directory(directory)?;
        let mut file = read_u32(&entry[0xC..]);
        let mut child = read_u32(&entry[0x8..]);

        while file != EMPTY {
            visited += 1;
            if visited > limit {
                return Err(String::from("RomFS file table loops back on itself"));
            }

            let entry = tables.file(file)?;
            found.push(RomFile {
                path: format!("{prefix}{}", entry_name(&tables.files, file, FILE_ENTRY)?),
                offset: tables.data_offset + read_u64(&entry[0x8..]),
                size: read_u64(&entry[0x10..]),
            });
            file = read_u32(&entry[0x4..]);
        }

        while child != EMPTY {
            visited += 1;
            if visited > limit {
                return Err(String::from("RomFS directory table loops back on itself"));
            }

            let entry = tables.directory(child)?;
            pending.push((
                child,
                format!("{prefix}{}/", entry_name(&tables.directories, child, DIRECTORY_ENTRY)?),
            ));
            child = read_u32(&entry[0x4..]);
        }
    }

    found.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(found)
}

impl Tables {
    fn directory(&self, offset: u32) -> Result<&[u8], String> {
        entry_at(&self.directories, offset, DIRECTORY_ENTRY)
    }

    fn file(&self, offset: u32) -> Result<&[u8], String> {
        entry_at(&self.files, offset, FILE_ENTRY)
    }
}

fn entry_name(table: &[u8], offset: u32, entry_size: usize) -> Result<String, String> {
    let length = read_u32(&entry_at(table, offset, entry_size)?[entry_size - 4..]) as usize;
    let start = offset as usize + entry_size;

    table
        .get(start..start + length)
        .map(|name| String::from_utf8_lossy(name).into_owned())
        .ok_or_else(|| String::from("RomFS entry name is truncated"))
}

fn entry_at(table: &[u8], offset: u32, entry_size: usize) -> Result<&[u8], String> {
    table
        .get(offset as usize..offset as usize + entry_size)
        .ok_or_else(|| String::from("RomFS entry points outside its table"))
}
