//! A test host for apps that use the `cw` global: it answers the bridge's host
//! calls (`boot`, `now`, `out`) as the desktop's web-app host does
//! (`crates/applications/src/web_app/runtime.rs`), and keeps the last message the
//! app sent where a test can read it: `storage_get(_, LAST_OUT)`.

use cw_web::script::{
    FetchRequest, FetchResponse, LogLevel, MemoryHost, ScriptHostDocument, StorageArea,
};
use cw_web::Viewport;

/// The key under which the test host shows the last `out` message.
pub const LAST_OUT: &str = "test:last-out";

pub struct CwHost {
    pub mem: MemoryHost,
    /// The boot facts `boot` answers (JSON).
    pub boot: String,
    pub out: String,
}

impl CwHost {
    pub fn new(boot: &str) -> CwHost {
        CwHost {
            mem: MemoryHost::new(),
            boot: boot.to_owned(),
            out: String::new(),
        }
    }
}

impl ScriptHostDocument for CwHost {
    fn fetch(&mut self, request: &FetchRequest) -> Result<FetchResponse, String> {
        self.mem.fetch(request)
    }
    fn navigate(&mut self, url: &str) {
        self.mem.navigate(url)
    }
    fn now_micros(&self) -> i64 {
        self.mem.now_micros()
    }
    fn random_u64(&mut self) -> u64 {
        self.mem.random_u64()
    }
    fn viewport(&self) -> Viewport {
        self.mem.viewport()
    }
    fn request_relayout(&mut self) {
        self.mem.request_relayout()
    }
    fn storage_get(&self, area: StorageArea, key: &str) -> Option<String> {
        if key == LAST_OUT {
            return Some(self.out.clone());
        }
        self.mem.storage_get(area, key)
    }
    fn storage_set(&mut self, area: StorageArea, key: &str, value: &str) {
        self.mem.storage_set(area, key, value)
    }
    fn storage_remove(&mut self, area: StorageArea, key: &str) {
        self.mem.storage_remove(area, key)
    }
    fn storage_keys(&self, area: StorageArea) -> Vec<String> {
        self.mem.storage_keys(area)
    }
    fn log(&mut self, level: LogLevel, text: &str) {
        self.mem.log(level, text)
    }
    fn host_call(&mut self, name: &str, payload: &str) -> Result<String, String> {
        match name {
            "boot" => Ok(self.boot.clone()),
            "now" => Ok(self.mem.now_micros().to_string()),
            "out" => {
                self.out = payload.to_owned();
                Ok(String::new())
            }
            other => Err(format!("the test host does not answer `{other}`")),
        }
    }
}
