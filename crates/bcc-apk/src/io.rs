use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::debug;

pub fn get_local_dir() -> PathBuf {
    let current_executable = match std::env::current_exe() {
        Ok(executable_path) => executable_path,
        Err(_) => return PathBuf::from("."),
    };

    match current_executable.parent() {
        Some(parent_directory) => parent_directory.to_path_buf(),
        None => PathBuf::from("."),
    }
}

pub fn save_local<TypeToSerialize: Serialize>(filename: &str, data_payload: &TypeToSerialize) {
    let mut destination_path = get_local_dir();
    destination_path.push(filename);

    let serialized_json = match serde_json::to_string_pretty(data_payload) {
        Ok(json_string) => json_string,
        Err(_) => return,
    };

    let temporary_path = destination_path.with_extension("tmp");
    if fs::write(&temporary_path, serialized_json).is_err() {
        return;
    }

    if fs::rename(&temporary_path, &destination_path).is_ok() {
        debug!("Successfully saved local file: {}", filename);
    }
}

pub fn load_local<TypeToDeserialize: DeserializeOwned>(filename: &str) -> Option<TypeToDeserialize> {
    let mut target_path = get_local_dir();
    target_path.push(filename);

    if !target_path.exists() {
        return None;
    }

    let file_contents = fs::read_to_string(&target_path).ok()?;
    let parsed = serde_json::from_str::<TypeToDeserialize>(&file_contents).ok();

    if parsed.is_some() {
        debug!("Successfully loaded local file: {}", filename);
    }

    parsed
}