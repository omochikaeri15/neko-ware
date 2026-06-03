use std::fs;
use tracing::debug;
use crate::io::get_local_dir;
use crate::keys::UserKeys;

pub fn init() -> std::io::Result<()> {
    debug!("Initializing default workspace configurations...");
    let keys = UserKeys::default();
    keys.save();

    let mut decrypted_dir = get_local_dir();
    decrypted_dir.push("decrypted");

    if decrypted_dir.exists() {
        debug!("Purging pre-existing decrypted directory.");
        fs::remove_dir_all(&decrypted_dir)?;
    }

    debug!("Creating fresh decrypted directory.");
    fs::create_dir_all(&decrypted_dir)?;

    Ok(())
}