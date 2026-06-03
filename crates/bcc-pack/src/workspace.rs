use std::fs;
use crate::io::get_local_dir;
use crate::keys::UserKeys;

pub fn init() -> std::io::Result<()> {
    let keys = UserKeys::default();
    keys.save();

    let mut decrypted_dir = get_local_dir();
    decrypted_dir.push("decrypted");

    if decrypted_dir.exists() {
        fs::remove_dir_all(&decrypted_dir)?;
    }

    fs::create_dir_all(&decrypted_dir)?;

    Ok(())
}