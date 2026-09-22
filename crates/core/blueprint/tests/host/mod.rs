//! A filesystem-backed [`Files`] provider for the integration tests.
//!
//! The library itself never touches the host — `cw-world` supplies the bytes, and
//! so does this. Integration tests are outside the pure-crate boundary check, so
//! this is where `std::fs` is allowed to appear.
use cw_blueprint::{Entry, Files};
use std::path::{Path, PathBuf};

pub struct Host {
    pub root: PathBuf,
}

impl Host {
    pub fn at(relative: &str) -> Self {
        Self {
            root: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../..")
                .join(relative),
        }
    }
    fn locate(&self, path: &str) -> PathBuf {
        self.root.join(path)
    }
}

impl Files for Host {
    fn read(&self, path: &str) -> Result<Vec<u8>, String> {
        std::fs::read(self.locate(path)).map_err(|e| e.to_string())
    }
    fn list_dir(&self, path: &str) -> Result<Vec<Entry>, String> {
        let dir = if path.is_empty() {
            self.root.clone()
        } else {
            self.locate(path)
        };
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let kind = entry.file_type().map_err(|e| e.to_string())?;
            entries.push(Entry {
                name: entry.file_name().to_string_lossy().into_owned(),
                directory: kind.is_dir(),
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }
    fn exists(&self, path: &str) -> bool {
        self.locate(path).exists()
    }
    fn is_dir(&self, path: &str) -> bool {
        self.locate(path).is_dir()
    }
}
