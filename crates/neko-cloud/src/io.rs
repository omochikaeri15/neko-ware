use serde::{de::DeserializeOwned, Serialize};
use std::fs;
use std::path::PathBuf;

pub fn get_local_dir() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

pub fn load_local<T: DeserializeOwned>(filename: &str) -> Option<T> {
    let mut file_path = get_local_dir();
    file_path.push(filename);
    let file_content = fs::read_to_string(file_path).ok()?;
    serde_json::from_str(&file_content).ok()
}

pub fn save_local<T: Serialize>(filename: &str, data_payload: &T) {
    let mut file_path = get_local_dir();
    file_path.push(filename);
    let Ok(serialized_json) = serde_json::to_string_pretty(data_payload) else {
        return;
    };
    let _ = fs::write(file_path, serialized_json);
}