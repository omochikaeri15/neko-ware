use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub(super) type Key = [u8; 16];

pub(super) struct KeySet {
    pub(super) header: [u8; 32],
    entries: HashMap<String, Vec<u8>>,
}

impl KeySet {
    pub(super) fn load(path: &Path) -> Result<Self, String> {
        let text = fs::read_to_string(path).map_err(|err| format!("Could not read {}: {err}", path.display()))?;

        let entries: HashMap<String, Vec<u8>> = text
            .lines()
            .filter_map(|line| line.split_once('='))
            .filter_map(|(name, value)| Some((name.trim().to_ascii_lowercase(), parse_hex(value.trim())?)))
            .collect();

        let header = entries
            .get("header_key")
            .and_then(|bytes| <[u8; 32]>::try_from(bytes.as_slice()).ok())
            .ok_or_else(|| format!("No valid header_key in {}", path.display()))?;

        Ok(Self { header, entries })
    }

    pub(super) fn titlekek(&self, revision: u8) -> Result<Key, String> {
        self.indexed("titlekek", revision)
    }

    pub(super) fn key_area(&self, index: u8, revision: u8) -> Result<Key, String> {
        let family = match index {
            0 => "key_area_key_application",
            1 => "key_area_key_ocean",
            2 => "key_area_key_system",
            _ => return Err(format!("Unknown key area index {index}")),
        };
        self.indexed(family, revision)
    }

    fn indexed(&self, family: &str, revision: u8) -> Result<Key, String> {
        let name = format!("{family}_{revision:02x}");
        self.entries
            .get(&name)
            .and_then(|bytes| Key::try_from(bytes.as_slice()).ok())
            .ok_or_else(|| format!("Missing {name} in prod.keys"))
    }
}

fn parse_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }

    (0..text.len())
        .step_by(2)
        .map(|i| text.get(i..i + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok()))
        .collect()
}
