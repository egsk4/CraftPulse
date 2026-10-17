//! An OS-held lock is released automatically on exit, including crashes.
use std::fs::{File, OpenOptions};
pub struct Instance(File);
impl Drop for Instance {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
pub fn acquire() -> Result<Option<Instance>, String> {
    let path = crate::config::Config::path().with_file_name("instance.lock");
    std::fs::create_dir_all(path.parent().ok_or("Missing config directory")?)
        .map_err(|e| e.to_string())?;
    acquire_at(&path)
}
fn acquire_at(path: &std::path::Path) -> Result<Option<Instance>, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    match file.try_lock() {
        Ok(()) => Ok(Some(Instance(file))),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn rejects_second_instance_and_releases_on_close() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("instance.lock");
        let first = super::acquire_at(&path).unwrap().unwrap();
        assert!(super::acquire_at(&path).unwrap().is_none());
        drop(first);
        assert!(super::acquire_at(&path).unwrap().is_some());
    }
}
