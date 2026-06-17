use std::fs::File;
use std::io::Read;
use std::path::Path;
use byteorder::{ByteOrder, LittleEndian};

pub fn extract_payload_from_binary(
    path_str: &str,
    region: &str,
) -> Result<Vec<i32>, Box<dyn std::error::Error>> {
    let mut binary_bytes = Vec::new();
    let path = Path::new(path_str);

    if !path.exists() {
        return Err(format!("ERROR: File not found at '{}'", path_str).into());
    }

    if path.extension().map_or(false, |ext| ext == "apk") {
        let file = File::open(path)?;
        let mut archive = zip::ZipArchive::new(file)?;
        let mut internal_so = archive.by_name("lib/x86_64/libnative-lib.so")?;
        internal_so.read_to_end(&mut binary_bytes)?;
    } else {
        let mut file = File::open(path)?;
        file.read_to_end(&mut binary_bytes)?;
    }

    let magic_header = match region.to_uppercase().as_str() {
        "JP" | "JA" => vec![5, 5, 5, 7000000],
        "EN" => vec![3, 2, 2, 6100000],
        "KR" | "KO" => vec![3, 2, 1, 6100000],
        "TW" => vec![2, 3, 1, 6100000],
        _ => return Err(format!("ERROR: Unsupported region '{}' for binary scanning.", region).into()),
    };

    let mut search_pattern = vec![0u8; 16];
    for (i, &val) in magic_header.iter().enumerate() {
        LittleEndian::write_i32(&mut search_pattern[i * 4..(i * 4) + 4], val);
    }

    let start_offset = binary_bytes
        .windows(search_pattern.len())
        .position(|window| window == search_pattern)
        .ok_or(format!("ERROR: Could not find the {} magic version array header signature!", region.to_uppercase()))?;

    let mut versions = Vec::new();
    let mut current_offset = start_offset;

    while current_offset + 4 <= binary_bytes.len() {
        let val = LittleEndian::read_i32(&binary_bytes[current_offset..current_offset + 4]);
        if val == 0 || val == -1 {
            break;
        }
        versions.push(val);
        current_offset += 4;
    }

    Ok(versions)
}