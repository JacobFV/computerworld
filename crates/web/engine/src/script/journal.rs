//! Record and replay of the answers the browser host gave a realm, so a realm can be
//! restored into a fresh VM by rerunning the same inputs: the VM heap is not
//! serialisable (see `cw_script_host::journal`), but the realm is deterministic, so
//! the same document, scripts, events and host answers rebuild the same heap.

use serde::{Deserialize, Serialize};

use super::{FetchResponse, StorageArea};

/// One answered host call, in call order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JournalEntry {
    Fetch(Result<FetchResponse, String>),
    Now(i64),
    Random(u64),
    Viewport(u32, u32, u8, u16),
    StorageGet(Option<String>),
    StorageKeys(Vec<String>),
    Cookie(String),
    /// A write the host performed (recorded so replay skips it): `navigate`,
    /// `storage_set`, `storage_remove`, `storage_clear`, `cookie_set`.
    Write,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    pub entries: Vec<JournalEntry>,
    /// How many entries a restore still has to replay before calls reach the host.
    #[serde(skip)]
    pub replay_pos: usize,
    #[serde(skip)]
    pub replaying: bool,
}

impl Journal {
    pub fn recording() -> Journal {
        Journal::default()
    }
    pub fn replay(entries: Vec<JournalEntry>) -> Journal {
        Journal {
            entries,
            replay_pos: 0,
            replaying: true,
        }
    }
    /// True while a restore is still answering calls from the record.
    pub fn in_replay(&self) -> bool {
        self.replaying && self.replay_pos < self.entries.len()
    }
    /// The next recorded entry during replay, or `None` when live.
    pub fn next_replayed(&mut self) -> Option<&JournalEntry> {
        if self.in_replay() {
            let e = &self.entries[self.replay_pos];
            self.replay_pos += 1;
            Some(e)
        } else {
            self.replaying = false;
            None
        }
    }
    pub fn record(&mut self, e: JournalEntry) {
        self.entries.push(e);
    }
}

/// The key of a storage entry, for the journal's own bookkeeping.
pub fn area_name(area: StorageArea) -> &'static str {
    match area {
        StorageArea::Local => "local",
        StorageArea::Session => "session",
    }
}
