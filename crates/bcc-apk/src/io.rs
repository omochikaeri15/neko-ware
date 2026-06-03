use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::PathBuf;

pub fn get_local_dir() -> PathBuf {
    let current_executable = match std::env::current_exe() {
        Ok(executable_path) => executable_path,
        Err(_executable_error) => return PathBuf::from("."),
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
        Err(_serialization_error) => return,
    };

    let temporary_path = destination_path.with_extension("tmp");

    if fs::write(&temporary_path, serialized_json).is_err() {
        return;
    }

    let _rename_result = fs::rename(&temporary_path, &destination_path);
}

pub fn load_local<TypeToDeserialize: DeserializeOwned>(filename: &str) -> Option<TypeToDeserialize> {
    let mut target_path = get_local_dir();
    target_path.push(filename);

    if !target_path.exists() {
        return None;
    }

    let file_contents = fs::read_to_string(&target_path).ok()?;
    serde_json::from_str::<TypeToDeserialize>(&file_contents).ok()
}