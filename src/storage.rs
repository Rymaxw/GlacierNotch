use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Serialize, de::DeserializeOwned};

const MAX_BYTES: u64 = 4 * 1024 * 1024;
static WRITE_ID: AtomicU64 = AtomicU64::new(0);

pub fn data_path(name: &str) -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("glacier").join(name)
}

pub fn config_path(name: &str) -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("glacier").join(name)
}

fn read_bytes_at(path: &Path) -> io::Result<Option<Vec<u8>>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let mut raw = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut raw)?;
    if raw.len() as u64 > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "store exceeds size limit",
        ));
    }
    Ok(Some(raw))
}

fn write_bytes_at(path: &Path, raw: &[u8]) -> io::Result<()> {
    if raw.len() as u64 > MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "store exceeds size limit",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("store needs a parent directory"))?;
    fs::create_dir_all(parent)?;
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
    let serial = WRITE_ID.fetch_add(1, Ordering::Relaxed);
    let temp = parent.join(format!(".glacier-{}-{serial}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temp)?;
    let result = (|| {
        file.write_all(raw)?;
        file.sync_all()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

pub fn read_at<T: DeserializeOwned>(path: &Path) -> io::Result<Vec<T>> {
    let Some(raw) = read_bytes_at(path)? else {
        return Ok(Vec::new());
    };
    serde_json::from_slice(&raw).map_err(io::Error::other)
}

pub fn write_at<T: Serialize>(path: &Path, values: &[T]) -> io::Result<()> {
    let raw = serde_json::to_vec(values).map_err(io::Error::other)?;
    write_bytes_at(path, &raw)
}

pub fn read_object_at<T: DeserializeOwned>(path: &Path) -> io::Result<Option<T>> {
    let Some(raw) = read_bytes_at(path)? else {
        return Ok(None);
    };
    serde_json::from_slice(&raw)
        .map(Some)
        .map_err(io::Error::other)
}

pub fn write_object_at<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let raw = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    write_bytes_at(path, &raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_temp_collision_does_not_delete_someone_elses_file() {
        let root =
            std::env::temp_dir().join(format!("glacier-storage-collision-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let serial = WRITE_ID.load(Ordering::Relaxed);
        let temp = root.join(format!(".glacier-{}-{serial}.tmp", std::process::id()));
        fs::write(&temp, b"pre-existing writer data").unwrap();
        let result = write_at(&root.join("notes.json"), &["new note"]);
        let preserved = fs::read(&temp).ok();
        fs::remove_dir_all(root).unwrap();
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            preserved.as_deref(),
            Some(b"pre-existing writer data".as_slice())
        );
    }
}
