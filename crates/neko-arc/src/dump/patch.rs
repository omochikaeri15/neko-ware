use crate::io::{read_u32, read_u64};

use super::Storage;
use super::nca::{SectionReader, Table};

const NODE_SIZE: usize = 0x4000;
const NODE_HEADER: usize = 0x10;
const INDIRECT_ENTRY: usize = 0x14;
const COUNTER_ENTRY: usize = 0x10;
const BASE_STORAGE: u32 = 0;
const PATCH_STORAGE: u32 = 1;
const UNENCRYPTED: u8 = 1;

struct Relocation {
    virtual_offset: u64,
    physical_offset: u64,
    storage: u32,
}

struct Subsection {
    offset: u64,
    encrypted: bool,
    generation: u32,
}

pub(super) struct PatchedStorage<'a> {
    base: SectionReader<'a>,
    patch: SectionReader<'a>,
    relocations: Vec<Relocation>,
    virtual_size: u64,
    subsections: Vec<Subsection>,
    physical_size: u64,
}

impl<'a> PatchedStorage<'a> {
    pub(super) fn open(base: SectionReader<'a>, patch: SectionReader<'a>) -> Result<Self, String> {
        let tables = patch
            .section()
            .patch
            .as_ref()
            .ok_or_else(|| String::from("Update RomFS carries no patch tables"))?;
        let (indirect, counter) = (
            read_table(&patch, &tables.indirect)?,
            read_table(&patch, &tables.counter)?,
        );
        let (indirect_count, counter_count) = (tables.indirect.count, tables.counter.count);

        let (relocations, virtual_size) = parse_entries(&indirect, indirect_count, INDIRECT_ENTRY, |raw| Relocation {
            virtual_offset: read_u64(&raw[0..8]),
            physical_offset: read_u64(&raw[8..16]),
            storage: read_u32(&raw[16..20]),
        })?;

        let (subsections, physical_size) = parse_entries(&counter, counter_count, COUNTER_ENTRY, |raw| Subsection {
            offset: read_u64(&raw[0..8]),
            encrypted: raw[8] != UNENCRYPTED,
            generation: read_u32(&raw[12..16]),
        })?;

        if relocations.is_empty() || subsections.is_empty() {
            return Err(String::from("Update patch tables are empty"));
        }

        Ok(Self {
            base,
            patch,
            relocations,
            virtual_size,
            subsections,
            physical_size,
        })
    }

    fn read_patch(&self, mut offset: u64, buf: &mut [u8]) -> Result<(), String> {
        let mut index = self
            .subsections
            .partition_point(|entry| entry.offset <= offset)
            .saturating_sub(1);
        let mut filled = 0;

        while filled < buf.len() {
            let entry = self
                .subsections
                .get(index)
                .ok_or_else(|| String::from("Read runs past the update's subsection table"))?;
            let next = self
                .subsections
                .get(index + 1)
                .map_or(self.physical_size, |next| next.offset);
            let length = ((next.saturating_sub(offset)) as usize).min(buf.len() - filled);

            if length == 0 {
                return Err(String::from("Update subsection table is out of order"));
            }

            let chunk = &mut buf[filled..filled + length];

            if entry.encrypted {
                self.patch.read_generation(offset, chunk, Some(entry.generation))?;
            } else {
                self.patch.read_raw(offset, chunk)?;
            }

            filled += length;
            offset += length as u64;
            index += 1;
        }

        Ok(())
    }
}

impl Storage for PatchedStorage<'_> {
    fn read_at(&self, mut offset: u64, buf: &mut [u8]) -> Result<(), String> {
        if offset + buf.len() as u64 > self.virtual_size {
            return Err(String::from("Read runs past the end of the patched RomFS"));
        }

        let mut index = self
            .relocations
            .partition_point(|entry| entry.virtual_offset <= offset)
            .saturating_sub(1);
        let mut filled = 0;

        while filled < buf.len() {
            let entry = self
                .relocations
                .get(index)
                .ok_or_else(|| String::from("Read runs past the update's relocation table"))?;
            let next = self
                .relocations
                .get(index + 1)
                .map_or(self.virtual_size, |next| next.virtual_offset);
            let length = ((next.saturating_sub(offset)) as usize).min(buf.len() - filled);

            if length == 0 {
                return Err(String::from("Update relocation table is out of order"));
            }

            let physical = entry.physical_offset + (offset - entry.virtual_offset);
            let storage = entry.storage;
            let chunk = &mut buf[filled..filled + length];

            match storage {
                BASE_STORAGE => self.base.read_at(physical, chunk)?,
                PATCH_STORAGE => self.read_patch(physical, chunk)?,
                other => return Err(format!("Update relocates into unknown storage {other}")),
            }

            filled += length;
            offset += length as u64;
            index += 1;
        }

        Ok(())
    }
}

fn read_table(patch: &SectionReader, table: &Table) -> Result<Vec<u8>, String> {
    let mut raw = vec![0u8; table.size as usize];
    patch.read_at(table.offset, &mut raw)?;
    Ok(raw)
}

fn parse_entries<T>(
    raw: &[u8],
    expected: u32,
    entry_size: usize,
    parse: impl Fn(&[u8]) -> T,
) -> Result<(Vec<T>, u64), String> {
    let header = raw
        .get(..NODE_HEADER)
        .ok_or_else(|| String::from("Patch table is truncated"))?;
    let buckets = read_u32(&header[4..8]) as usize;
    let end_offset = read_u64(&header[8..16]);

    let mut entries = Vec::with_capacity(expected as usize);

    for bucket in 0..buckets {
        let node = raw
            .get(NODE_SIZE * (bucket + 1)..NODE_SIZE * (bucket + 2))
            .ok_or_else(|| String::from("Patch table bucket is truncated"))?;
        let count = read_u32(&node[4..8]) as usize;

        let body = node
            .get(NODE_HEADER..NODE_HEADER + count * entry_size)
            .ok_or_else(|| String::from("Patch table bucket overflows its node"))?;
        entries.extend(body.chunks_exact(entry_size).map(&parse));
    }

    if entries.len() != expected as usize {
        return Err(format!(
            "Patch table lists {expected} entries but holds {}",
            entries.len()
        ));
    }

    Ok((entries, end_offset))
}
