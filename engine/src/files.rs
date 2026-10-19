//! Where the engine keeps its private files, and how it writes them.

use anyhow::{Result, bail};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

fn uid() -> u32 {
    fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(u32::MAX)
}

/// The engine's runtime folder: its socket, session, and the AI's answer.
/// `$XDG_RUNTIME_DIR/omadora` in a desktop session. Without one it falls
/// back to `/tmp/omadora-UID`, but only as a folder this user owns that no
/// one else can enter; anything else there is refused, not reused.
pub fn runtime_dir() -> Result<PathBuf> {
    let dir = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(base) if !base.is_empty() => PathBuf::from(base).join("omadora"),
        _ => PathBuf::from(format!("/tmp/omadora-{}", uid())),
    };
    if fs::symlink_metadata(&dir).is_err() {
        let _ = fs::DirBuilder::new().mode(0o700).create(&dir);
    }
    let meta = fs::symlink_metadata(&dir)?;
    if !meta.is_dir() || meta.uid() != uid() {
        bail!("{} isn't a folder of this user's; not using it", dir.display());
    }
    if meta.mode() & 0o077 != 0 {
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}

/// Write a file only this user can read: always a new file (mode 0600, not
/// following a link someone planted), then moved into place.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let _ = fs::remove_file(&tmp);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc_o_nofollow())
        .open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&tmp, path)
}

/// O_NOFOLLOW, without a libc dependency.
fn libc_o_nofollow() -> i32 {
    if cfg!(any(target_arch = "aarch64", target_arch = "arm")) { 0o100000 } else { 0o400000 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_files_are_owner_only_and_replace_what_was_there() {
        let dir = std::env::temp_dir().join(format!("omadora-files-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("secret.json");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, b"new").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        fs::remove_dir_all(&dir).unwrap();
    }
}
