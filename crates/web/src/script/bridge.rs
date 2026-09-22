//! The `cw_script_host::ScriptHost` the VM sees: module sources as a virtual file
//! system (URLs mapped to paths under `/__modules/`), the world clock and entropy
//! through the realm's journaled host, and no files, processes or sockets.

use std::cell::RefCell;
use std::rc::Rc;

use cw_script_host::{FileStat, FsError, FsErrorKind, ScriptHost};

use super::inner::Inner;
use super::FetchRequest;

pub struct Bridge {
    pub inner: Rc<RefCell<Inner>>,
}

/// The virtual path of a module URL: `https://h/a/b.js` is `/__modules/h/a/b.js`.
pub fn module_path(url: &str) -> String {
    let no_frag = url.replace('#', "__");
    let rest = no_frag
        .split_once("://")
        .map(|(_, r)| r)
        .unwrap_or(no_frag.as_str());
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let host = if host.is_empty() { "local" } else { host };
    format!("/__modules/{host}{path}")
}

/// The URL of a module path made by `module_path`.
pub fn module_url(path: &str, scheme: &str) -> String {
    let rest = path.strip_prefix("/__modules/").unwrap_or(path);
    let (host, p) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if host == "local" {
        p.to_owned()
    } else {
        format!("{scheme}://{host}{p}")
    }
}

impl Bridge {
    fn module_source(&self, path: &str) -> Option<String> {
        let mut inner = self.inner.borrow_mut();
        if let Some(s) = inner.module_sources.get(path) {
            return Some(s.clone());
        }
        if !path.starts_with("/__modules/") {
            return None;
        }
        let scheme = inner
            .url
            .split_once("://")
            .map(|(s, _)| s.to_owned())
            .unwrap_or_else(|| "https".into());
        let url = module_url(path, &scheme);
        let url = inner.resolve_url(&url);
        let r = inner.host_fetch(&FetchRequest {
            url: url.clone(),
            method: "GET".into(),
            headers: vec![],
            body: None,
        });
        match r {
            Ok(resp) if resp.status < 400 => {
                let src = String::from_utf8_lossy(&resp.body).into_owned();
                inner.module_sources.insert(path.to_owned(), src.clone());
                Some(src)
            }
            _ => None,
        }
    }
}

impl ScriptHost for Bridge {
    fn read_file(&mut self, path: &str) -> Result<Vec<u8>, FsError> {
        match self.module_source(path) {
            Some(s) => Ok(s.into_bytes()),
            None => Err(FsError::new(FsErrorKind::NotFound)),
        }
    }
    fn write_file(&mut self, _path: &str, _data: &[u8], _append: bool) -> Result<(), FsError> {
        Err(FsError::new(FsErrorKind::PermissionDenied))
    }
    fn stat(&mut self, path: &str) -> Result<FileStat, FsError> {
        match self.module_source(path) {
            Some(s) => Ok(FileStat {
                is_dir: false,
                size: s.len() as u64,
                ..Default::default()
            }),
            None => Err(FsError::new(FsErrorKind::NotFound)),
        }
    }
    fn list_dir(&mut self, _path: &str) -> Result<Vec<String>, FsError> {
        Err(FsError::new(FsErrorKind::NotFound))
    }
    fn mkdir(&mut self, _path: &str, _parents: bool) -> Result<(), FsError> {
        Err(FsError::new(FsErrorKind::PermissionDenied))
    }
    fn remove(&mut self, _path: &str, _recursive: bool) -> Result<(), FsError> {
        Err(FsError::new(FsErrorKind::PermissionDenied))
    }
    fn rename(&mut self, _from: &str, _to: &str) -> Result<(), FsError> {
        Err(FsError::new(FsErrorKind::PermissionDenied))
    }
    fn cwd(&self) -> String {
        "/".into()
    }
    fn chdir(&mut self, _path: &str) -> Result<(), FsError> {
        Ok(())
    }
    fn resolve(&self, path: &str) -> String {
        if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        }
    }
    fn now_micros(&self) -> i64 {
        self.inner.borrow_mut().host_now_micros()
    }
    fn random_u64(&mut self) -> u64 {
        self.inner.borrow_mut().host_random_u64()
    }
    fn user(&self) -> String {
        "user".into()
    }
    fn hostname(&self) -> String {
        "browser".into()
    }
    fn pid(&self) -> u64 {
        1
    }
}
