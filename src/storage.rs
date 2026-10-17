//! Replace local state only after a complete, synced write in the same directory.
use std::{io::Write, path::Path};
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("Missing parent directory")?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    file.write_all(bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    #[test]
    fn replaces_complete_file_and_leaves_no_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        super::atomic_write(&path, b"old").unwrap();
        super::atomic_write(&path, b"new complete contents").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new complete contents");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
