//! Same-directory replacement: a failed write must not truncate the last saved configuration.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

static SERIAL: AtomicU64 = AtomicU64::new(0);

pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing parent directory"))?;
    fs::create_dir_all(parent)?;
    let filename = path
        .file_name()
        .ok_or_else(|| io::Error::other("Missing filename"))?;
    let mut temporary;
    let mut file = loop {
        temporary = parent.join(format!(
            ".{}.{}-{}.tmp",
            filename.to_string_lossy(),
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => break file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    let result = (|| {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_existing_config_and_cleans_up_failed_replacements() {
        let dir = std::env::temp_dir().join(format!(
            "usoai-atomic-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let path = dir.join("config.json");
        write(&path, b"old").unwrap();
        write(&path, b"new").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        let blocked = dir.join("directory.json");
        fs::create_dir(&blocked).unwrap();
        assert!(write(&blocked, b"cannot replace directory").is_err());
        assert!(blocked.is_dir());
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        fs::remove_file(path).unwrap();
        fs::remove_dir(blocked).unwrap();
        fs::remove_dir(dir).unwrap();
    }
}
