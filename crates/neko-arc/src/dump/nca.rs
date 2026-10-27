use std::collections::HashMap;
use std::fs::File;

use aes::Aes128;
use aes::cipher::{Array, BlockCipherDecrypt, KeyInit, KeyIvInit, StreamCipher, StreamCipherSeek, consts::U16};
use tracing::trace;
use xts_mode::Xts128;

use crate::io::{read_exact_at, read_u32, read_u64};

use super::Storage;
use super::keys::{Key, KeySet};
use super::nsp::Member;

type Aes128Ctr = ctr::Ctr128BE<Aes128>;

const HEADER_LENGTH: usize = 0xC00;
const SECTOR_SIZE: usize = 0x200;
const MEDIA_UNIT: u64 = 0x200;
const SECTION_COUNT: usize = 4;
const CONTENT_MAGIC: &[u8] = b"NCA3";
const INTEGRITY_MAGIC: &[u8] = b"IVFC";
const TABLE_MAGIC: &[u8] = b"BKTR";
const PROGRAM_CONTENT: u8 = 0;
const ROMFS_SECTION: u8 = 0;
const INTEGRITY_HASH: u8 = 3;
const MAX_INTEGRITY_LEVELS: u32 = 7;

enum Crypto {
    Plain,
    Counter,
}

pub(super) struct Table {
    pub(super) offset: u64,
    pub(super) size: u64,
    pub(super) count: u32,
}

pub(super) struct PatchTables {
    pub(super) indirect: Table,
    pub(super) counter: Table,
}

pub(super) struct RomSection {
    position: u64,
    nca_offset: u64,
    size: u64,
    crypto: Crypto,
    key: Key,
    upper_iv: [u8; 8],
    pub(super) program_id: u64,
    pub(super) data_offset: u64,
    pub(super) patch: Option<PatchTables>,
}

#[derive(Clone, Copy)]
pub(super) struct SectionReader<'a> {
    file: &'a File,
    section: &'a RomSection,
}

pub(super) fn read_program_romfs(
    file: &File,
    member: &Member,
    keys: &KeySet,
    title_keys: &HashMap<[u8; 16], Key>,
) -> Result<Option<RomSection>, String> {
    if member.size < HEADER_LENGTH as u64 {
        return Err(String::from("Content is smaller than its own header"));
    }

    let mut header = vec![0u8; HEADER_LENGTH];
    read_exact_at(file, &mut header, member.offset).map_err(|_| String::from("Content header is truncated"))?;

    let (first_key, second_key) = keys.header.split_at(16);
    let xts = Xts128::<Aes128>::new(cipher_from(first_key)?, cipher_from(second_key)?);
    xts.decrypt_area(&mut header, SECTOR_SIZE, 0, sector_tweak);

    if &header[0x200..0x204] != CONTENT_MAGIC {
        return Err(String::from("Header did not decrypt to NCA3, check header_key"));
    }

    let content_type = header[0x205];
    trace!(content = %member.name, kind = content_type, "Decrypted content header");

    if content_type != PROGRAM_CONTENT {
        return Ok(None);
    }

    let Some(index) = (0..SECTION_COUNT).find(|&i| {
        let fs_header = &header[0x400 + i * 0x200..0x600 + i * 0x200];
        read_u32(&header[0x244 + i * 0x10..]) != 0 && fs_header[2] == ROMFS_SECTION && fs_header[3] == INTEGRITY_HASH
    }) else {
        return Ok(None);
    };

    let key = content_key(&header, keys, title_keys)?;
    let fs_header = &header[0x400 + index * 0x200..0x600 + index * 0x200];
    let start = u64::from(read_u32(&header[0x240 + index * 0x10..])) * MEDIA_UNIT;
    let end = u64::from(read_u32(&header[0x244 + index * 0x10..])) * MEDIA_UNIT;

    let (crypto, patch) = match fs_header[4] {
        1 => (Crypto::Plain, None),
        3 => (Crypto::Counter, None),
        4 => (Crypto::Counter, Some(read_patch_tables(fs_header)?)),
        other => return Err(format!("RomFS uses unsupported encryption type {other}")),
    };

    let mut upper_iv = [0u8; 8];
    upper_iv.copy_from_slice(&fs_header[0x140..0x148]);
    upper_iv.reverse();

    Ok(Some(RomSection {
        position: member.offset + start,
        nca_offset: start,
        size: end.saturating_sub(start),
        crypto,
        key,
        upper_iv,
        program_id: read_u64(&header[0x210..]),
        data_offset: integrity_data_offset(fs_header)?,
        patch,
    }))
}

fn content_key(header: &[u8], keys: &KeySet, title_keys: &HashMap<[u8; 16], Key>) -> Result<Key, String> {
    let revision = header[0x206].max(header[0x220]).saturating_sub(1);
    let mut rights_id = [0u8; 16];
    rights_id.copy_from_slice(&header[0x230..0x240]);

    if rights_id.iter().any(|&byte| byte != 0) {
        let title_key = title_keys
            .get(&rights_id)
            .ok_or_else(|| String::from("No ticket in the NSP matches this content"))?;
        return decrypt_key(&keys.titlekek(revision)?, title_key);
    }

    let mut wrapped = Key::default();
    wrapped.copy_from_slice(&header[0x320..0x330]);
    decrypt_key(&keys.key_area(header[0x207], revision)?, &wrapped)
}

fn decrypt_key(kek: &Key, wrapped: &Key) -> Result<Key, String> {
    let mut block = Array::from(*wrapped);
    cipher_from(kek)?.decrypt_block(&mut block);
    Ok(block.into())
}

fn cipher_from(key: &[u8]) -> Result<Aes128, String> {
    Aes128::new_from_slice(key).map_err(|_| String::from("Key is not 16 bytes"))
}

fn sector_tweak(sector: u128) -> Array<u8, U16> {
    Array::from(sector.to_be_bytes())
}

fn integrity_data_offset(fs_header: &[u8]) -> Result<u64, String> {
    if &fs_header[0x8..0xC] != INTEGRITY_MAGIC {
        return Err(String::from("RomFS is missing its IVFC header"));
    }

    let levels = read_u32(&fs_header[0x14..]);
    if !(2..=MAX_INTEGRITY_LEVELS).contains(&levels) {
        return Err(format!("RomFS lists {levels} integrity levels"));
    }

    let data_level = 0x18 + (levels as usize - 2) * 0x18;
    Ok(read_u64(&fs_header[data_level..]))
}

fn read_patch_tables(fs_header: &[u8]) -> Result<PatchTables, String> {
    let table = |start: usize| {
        if &fs_header[start + 0x10..start + 0x14] != TABLE_MAGIC {
            return Err(String::from("Update is missing its BKTR tables"));
        }
        Ok(Table {
            offset: read_u64(&fs_header[start..]),
            size: read_u64(&fs_header[start + 0x8..]),
            count: read_u32(&fs_header[start + 0x18..]),
        })
    };

    Ok(PatchTables {
        indirect: table(0x100)?,
        counter: table(0x120)?,
    })
}

impl<'a> SectionReader<'a> {
    pub(super) fn open(file: &'a File, section: &'a RomSection) -> Self {
        Self { file, section }
    }

    pub(super) fn section(&self) -> &'a RomSection {
        self.section
    }

    pub(super) fn read_raw(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        if offset + buf.len() as u64 > self.section.size {
            return Err(String::from("Read runs past the end of the RomFS section"));
        }

        read_exact_at(self.file, buf, self.section.position + offset)
            .map_err(|_| String::from("RomFS section is truncated"))
    }

    pub(super) fn read_generation(&self, offset: u64, buf: &mut [u8], generation: Option<u32>) -> Result<(), String> {
        self.read_raw(offset, buf)?;

        if let Crypto::Counter = self.section.crypto {
            let mut iv = [0u8; 16];
            iv[..8].copy_from_slice(&self.section.upper_iv);
            if let Some(generation) = generation {
                iv[4..8].copy_from_slice(&generation.to_be_bytes());
            }
            iv[8..].copy_from_slice(&(self.section.nca_offset >> 4).to_be_bytes());

            let mut cipher = Aes128Ctr::new(&self.section.key.into(), &iv.into());
            cipher.seek(offset);
            cipher.apply_keystream(buf);
        }

        Ok(())
    }
}

impl Storage for SectionReader<'_> {
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), String> {
        self.read_generation(offset, buf, None)
    }
}
