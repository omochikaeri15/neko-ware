use std::collections::HashMap;
use std::fs::File;

use crate::io::{read_exact_at, read_u32, read_u64};

use super::keys::Key;

const PARTITION_MAGIC: &[u8] = b"PFS0";
const HEADER_SIZE: u64 = 16;
const ENTRY_SIZE: u64 = 24;
const MAX_ENTRIES: u64 = 0x1000;
const COMMON_TICKET: u8 = 0;
const MAX_TICKET_SIZE: u64 = 0x1000;

pub(super) struct Member {
    pub(super) name: String,
    pub(super) offset: u64,
    pub(super) size: u64,
}

pub(super) fn read_members(file: &File) -> Result<Vec<Member>, String> {
    let mut header = [0u8; HEADER_SIZE as usize];
    read_exact_at(file, &mut header, 0).map_err(|_| String::from("File is too small to be an NSP"))?;

    if &header[..4] != PARTITION_MAGIC {
        return Err(String::from("Missing PFS0 signature"));
    }

    let count = u64::from(read_u32(&header[4..8]));
    let table_size = u64::from(read_u32(&header[8..12]));

    if count > MAX_ENTRIES {
        return Err(String::from("Partition table exceeded sane limits"));
    }

    let mut entries = vec![0u8; (count * ENTRY_SIZE) as usize];
    read_exact_at(file, &mut entries, HEADER_SIZE).map_err(|_| String::from("Partition table is truncated"))?;

    let mut names = vec![0u8; table_size as usize];
    let names_start = HEADER_SIZE + count * ENTRY_SIZE;
    read_exact_at(file, &mut names, names_start).map_err(|_| String::from("Partition name table is truncated"))?;

    let data_start = names_start + table_size;

    entries
        .chunks_exact(ENTRY_SIZE as usize)
        .map(|entry| {
            let name_start = read_u32(&entry[16..20]) as usize;
            let name = names
                .get(name_start..)
                .and_then(|tail| tail.split(|&byte| byte == 0).next())
                .ok_or_else(|| String::from("Partition name points outside the name table"))?;

            Ok(Member {
                name: String::from_utf8_lossy(name).into_owned(),
                offset: data_start + read_u64(&entry[0..8]),
                size: read_u64(&entry[8..16]),
            })
        })
        .collect()
}

pub(super) fn read_title_keys(file: &File, members: &[Member]) -> Result<HashMap<[u8; 16], Key>, String> {
    let mut title_keys = HashMap::new();

    for member in members.iter().filter(|member| member.name.ends_with(".tik")) {
        if member.size > MAX_TICKET_SIZE {
            return Err(format!("Ticket {} exceeded sane limits", member.name));
        }

        let mut ticket = vec![0u8; member.size as usize];
        read_exact_at(file, &mut ticket, member.offset).map_err(|_| format!("Ticket {} is truncated", member.name))?;

        let body =
            signature_length(&ticket).ok_or_else(|| format!("Ticket {} has an unknown signature", member.name))?;
        let field = |start: usize, length: usize| {
            ticket
                .get(body + start..body + start + length)
                .ok_or_else(|| format!("Ticket {} is truncated", member.name))
        };

        if field(0x141, 1)?[0] != COMMON_TICKET {
            return Err(format!(
                "Ticket {} is personalized, only common tickets can be read",
                member.name
            ));
        }

        let mut rights_id = [0u8; 16];
        rights_id.copy_from_slice(field(0x160, 16)?);
        let mut title_key = Key::default();
        title_key.copy_from_slice(field(0x40, 16)?);

        title_keys.insert(rights_id, title_key);
    }

    Ok(title_keys)
}

fn signature_length(ticket: &[u8]) -> Option<usize> {
    let kind = read_u32(ticket.get(..4)?);
    let length = match kind {
        0x10000 | 0x10003 => 0x200 + 0x3C,
        0x10001 | 0x10004 => 0x100 + 0x3C,
        0x10002 | 0x10005 => 0x3C + 0x40,
        _ => return None,
    };
    Some(4 + length)
}
