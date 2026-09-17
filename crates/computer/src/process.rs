use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProcessState {
    Running,
    Sleeping { until: u64 },
    Stopped,
    Zombie { code: i32 },
    Exited { code: i32 },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FileDescriptor {
    Stdin,
    Stdout,
    Stderr,
    File {
        path: String,
        offset: u64,
        writable: bool,
    },
    Pipe {
        pipe: u64,
        write: bool,
    },
    Socket {
        listener: String,
    },
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum SignalDisposition {
    #[default]
    Default,
    Ignore,
    Notify,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Process {
    pub pid: u64,
    pub parent: u64,
    pub group: u64,
    pub owner: String,
    pub command: String,
    pub state: ProcessState,
    pub started: u64,
    pub ended: Option<u64>,
    pub fds: BTreeMap<u32, FileDescriptor>,
    pub listeners: BTreeSet<String>,
    #[serde(default)]
    pub signal_dispositions: BTreeMap<String, SignalDisposition>,
    #[serde(default)]
    pub pending_signals: Vec<String>,
    #[serde(default)]
    pub wake_exit: Option<i32>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProcessTable {
    next_pid: u64,
    processes: Arc<BTreeMap<u64, Process>>,
}
impl Default for ProcessTable {
    fn default() -> Self {
        Self::new()
    }
}
impl ProcessTable {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.processes.get(&1),Some(p) if p.parent==0&&p.state==ProcessState::Running)
        {
            return Err("invalid init process".into());
        }
        for (id, p) in self.processes.iter() {
            if *id != p.pid
                || *id >= self.next_pid
                || (*id != 1 && !self.processes.contains_key(&p.parent))
            {
                return Err("invalid process identity/parent".into());
            }
        }
        Ok(())
    }
    pub fn new() -> Self {
        let init = Process {
            pid: 1,
            parent: 0,
            group: 1,
            owner: "root".into(),
            command: "init".into(),
            state: ProcessState::Running,
            started: 0,
            ended: None,
            fds: BTreeMap::new(),
            listeners: BTreeSet::new(),
            signal_dispositions: BTreeMap::new(),
            pending_signals: Vec::new(),
            wake_exit: None,
        };
        Self {
            next_pid: 2,
            processes: Arc::new(BTreeMap::from([(1, init)])),
        }
    }
    pub fn spawn(&mut self, parent: u64, owner: &str, command: &str, tick: u64) -> u64 {
        self.reap_exited();
        let pid = self.next_pid;
        self.next_pid += 1;
        let parent = if self.processes.contains_key(&parent) {
            parent
        } else {
            1
        };
        let fds = self.processes[&parent].fds.clone();
        let fds = if fds.is_empty() {
            BTreeMap::from([
                (0, FileDescriptor::Stdin),
                (1, FileDescriptor::Stdout),
                (2, FileDescriptor::Stderr),
            ])
        } else {
            fds
        };
        Arc::make_mut(&mut self.processes).insert(
            pid,
            Process {
                pid,
                parent,
                group: pid,
                owner: owner.into(),
                command: command.into(),
                state: ProcessState::Running,
                started: tick,
                ended: None,
                fds,
                listeners: BTreeSet::new(),
                signal_dispositions: BTreeMap::new(),
                pending_signals: Vec::new(),
                wake_exit: None,
            },
        );
        pid
    }
    /// Discard reaped process records; zombies remain until waited or adopted by init.
    pub fn reap_exited(&mut self) -> usize {
        let ids: Vec<_> = self
            .processes
            .values()
            .filter(|p| matches!(p.state, ProcessState::Exited { .. }))
            .map(|p| p.pid)
            .collect();
        let count = ids.len();
        if count > 0 {
            let nodes = Arc::make_mut(&mut self.processes);
            for id in ids {
                nodes.remove(&id);
            }
        }
        count
    }
    pub fn get(&self, pid: u64) -> Option<&Process> {
        self.processes.get(&pid)
    }
    pub fn list(&self) -> Vec<Process> {
        self.processes
            .values()
            .filter(|p| !matches!(p.state, ProcessState::Exited { .. }))
            .cloned()
            .collect()
    }
    pub fn exit(&mut self, pid: u64, code: i32, tick: u64) -> Result<Vec<String>, String> {
        if pid == 1 {
            return Err("cannot terminate init".into());
        }
        let nodes = Arc::make_mut(&mut self.processes);
        let p = nodes.get_mut(&pid).ok_or("no such process")?;
        if matches!(
            p.state,
            ProcessState::Exited { .. } | ProcessState::Zombie { .. }
        ) {
            return Ok(Vec::new());
        }
        p.state = if p.parent == 1 {
            ProcessState::Exited { code }
        } else {
            ProcessState::Zombie { code }
        };
        p.ended = Some(tick);
        p.fds.clear();
        let listeners = std::mem::take(&mut p.listeners).into_iter().collect();
        for child in nodes.values_mut().filter(|c| c.parent == pid) {
            child.parent = 1;
            if let ProcessState::Zombie { code } = child.state {
                child.state = ProcessState::Exited { code };
            }
        }
        Ok(listeners)
    }
    pub fn wait(&mut self, parent: u64, pid: u64) -> Result<Option<i32>, String> {
        let p = Arc::make_mut(&mut self.processes)
            .get_mut(&pid)
            .ok_or("no such process")?;
        if p.parent != parent {
            return Err("not a child".into());
        }
        match p.state {
            ProcessState::Zombie { code } | ProcessState::Exited { code } => {
                p.state = ProcessState::Exited { code };
                Ok(Some(code))
            }
            _ => Ok(None),
        }
    }
    pub fn sleep(&mut self, pid: u64, until: u64) -> Result<(), String> {
        let p = Arc::make_mut(&mut self.processes)
            .get_mut(&pid)
            .ok_or("no such process")?;
        if !matches!(p.state, ProcessState::Running) {
            return Err("process not running".into());
        }
        p.state = ProcessState::Sleeping { until };
        Ok(())
    }
    pub fn schedule_exit(&mut self, pid: u64, until: u64, code: i32) -> Result<(), String> {
        self.sleep(pid, until)?;
        Arc::make_mut(&mut self.processes)
            .get_mut(&pid)
            .unwrap()
            .wake_exit = Some(code);
        Ok(())
    }
    /// Wake scheduled processes in PID order. Execution adapters decide what runs next.
    pub fn next_deadline(&self) -> Option<u64> {
        self.processes
            .values()
            .filter_map(|p| match p.state {
                ProcessState::Sleeping { until } => Some(until),
                _ => None,
            })
            .min()
    }
    pub fn advance(&mut self, tick: u64) -> Vec<u64> {
        let ready: Vec<_> = self
            .processes
            .values()
            .filter(|p| matches!(p.state,ProcessState::Sleeping{until} if until<=tick))
            .map(|p| p.pid)
            .collect();
        for id in &ready {
            let code = self.processes[id].wake_exit;
            if let Some(code) = code {
                let until = match self.processes[id].state {
                    ProcessState::Sleeping { until } => until,
                    _ => tick,
                };
                let _ = self.exit(*id, code, until);
            } else {
                Arc::make_mut(&mut self.processes)
                    .get_mut(id)
                    .unwrap()
                    .state = ProcessState::Running;
            }
        }
        ready
    }
    pub fn signal(
        &mut self,
        pid: u64,
        signal: &str,
        actor: &str,
        tick: u64,
    ) -> Result<Vec<String>, String> {
        let p = self.processes.get(&pid).ok_or("no such process")?;
        if actor != "root" && actor != p.owner {
            return Err("permission denied".into());
        }
        let normalized = match signal.trim_start_matches("SIG") {
            "15" => "TERM",
            "9" => "KILL",
            "2" => "INT",
            "19" => "STOP",
            "18" => "CONT",
            s => s,
        }
        .to_string();
        if !matches!(
            normalized.as_str(),
            "KILL" | "9" | "STOP" | "19" | "CONT" | "18" | "0"
        ) {
            match p
                .signal_dispositions
                .get(&normalized)
                .cloned()
                .unwrap_or_default()
            {
                SignalDisposition::Ignore => return Ok(Vec::new()),
                SignalDisposition::Notify => {
                    Arc::make_mut(&mut self.processes)
                        .get_mut(&pid)
                        .unwrap()
                        .pending_signals
                        .push(normalized);
                    return Ok(Vec::new());
                }
                SignalDisposition::Default => {}
            }
        }
        match normalized.as_str() {
            "TERM" | "KILL" | "INT" | "15" | "9" | "2" => self.exit(
                pid,
                match signal.trim_start_matches("SIG") {
                    "KILL" | "9" => 137,
                    "INT" | "2" => 130,
                    _ => 143,
                },
                tick,
            ),
            "STOP" | "TSTP" | "19" => {
                if pid == 1 {
                    return Err("cannot stop init".into());
                }
                Arc::make_mut(&mut self.processes)
                    .get_mut(&pid)
                    .unwrap()
                    .state = ProcessState::Stopped;
                Ok(Vec::new())
            }
            "CONT" | "18" => {
                let p = Arc::make_mut(&mut self.processes).get_mut(&pid).unwrap();
                if p.state == ProcessState::Stopped {
                    p.state = ProcessState::Running;
                }
                Ok(Vec::new())
            }
            "0" => Ok(Vec::new()),
            _ => Err("unsupported signal".into()),
        }
    }
    pub fn set_signal_disposition(
        &mut self,
        pid: u64,
        signal: &str,
        disposition: SignalDisposition,
    ) -> Result<(), String> {
        let signal = signal.trim_start_matches("SIG");
        if matches!(signal, "KILL" | "STOP" | "9" | "19") {
            return Err("signal disposition cannot be changed".into());
        }
        let p = Arc::make_mut(&mut self.processes)
            .get_mut(&pid)
            .ok_or("no such process")?;
        p.signal_dispositions.insert(signal.into(), disposition);
        Ok(())
    }
    pub fn take_signals(&mut self, pid: u64) -> Result<Vec<String>, String> {
        let p = Arc::make_mut(&mut self.processes)
            .get_mut(&pid)
            .ok_or("no such process")?;
        Ok(std::mem::take(&mut p.pending_signals))
    }
    pub fn seek_fd(&mut self, pid: u64, fd: u32, offset: u64) -> Result<(), String> {
        let p = Arc::make_mut(&mut self.processes)
            .get_mut(&pid)
            .ok_or("no such process")?;
        match p.fds.get_mut(&fd) {
            Some(FileDescriptor::File {
                offset: position, ..
            }) => {
                *position = offset;
                Ok(())
            }
            _ => Err("descriptor is not seekable".into()),
        }
    }
    pub fn read_fd(
        &mut self,
        pid: u64,
        fd: u32,
        vfs: &crate::Vfs,
        len: usize,
    ) -> Result<Vec<u8>, String> {
        let p = Arc::make_mut(&mut self.processes)
            .get_mut(&pid)
            .ok_or("no such process")?;
        match p.fds.get_mut(&fd) {
            Some(FileDescriptor::File { path, offset, .. }) => {
                let bytes = vfs.read_as(path, &p.owner).map_err(|e| e.to_string())?;
                let start = (*offset as usize).min(bytes.len());
                let end = start.saturating_add(len).min(bytes.len());
                *offset = end as u64;
                Ok(bytes[start..end].to_vec())
            }
            _ => Err("descriptor is not a file".into()),
        }
    }
    pub fn write_fd(
        &mut self,
        pid: u64,
        fd: u32,
        vfs: &mut crate::Vfs,
        bytes: &[u8],
        tick: u64,
    ) -> Result<usize, String> {
        let p = Arc::make_mut(&mut self.processes)
            .get_mut(&pid)
            .ok_or("no such process")?;
        match p.fds.get_mut(&fd) {
            Some(FileDescriptor::File {
                path,
                offset,
                writable: true,
            }) => {
                let mut content = vfs.read_as(path, &p.owner).map_err(|e| e.to_string())?;
                let start = usize::try_from(*offset).map_err(|_| "offset too large")?;
                let end = start.checked_add(bytes.len()).ok_or("offset too large")?;
                if end > 64 * 1024 * 1024 {
                    return Err("virtual file descriptor write exceeds 64 MiB limit".into());
                }
                content.resize(content.len().max(end), 0);
                content[start..end].copy_from_slice(bytes);
                vfs.write_as(path, &content, &p.owner, tick)
                    .map_err(|e| e.to_string())?;
                *offset = end as u64;
                Ok(bytes.len())
            }
            _ => Err("descriptor is not writable".into()),
        }
    }
    pub fn set_group(&mut self, pid: u64, group: u64) -> Result<(), String> {
        let p = Arc::make_mut(&mut self.processes)
            .get_mut(&pid)
            .ok_or("no such process")?;
        p.group = group;
        Ok(())
    }
    pub fn signal_group(
        &mut self,
        group: u64,
        signal: &str,
        actor: &str,
        tick: u64,
    ) -> Result<Vec<String>, String> {
        let ids: Vec<_> = self
            .processes
            .values()
            .filter(|p| p.group == group)
            .map(|p| p.pid)
            .collect();
        let mut closed = Vec::new();
        for id in ids {
            closed.extend(self.signal(id, signal, actor, tick)?)
        }
        Ok(closed)
    }
    pub fn open_fd(&mut self, pid: u64, fd: FileDescriptor) -> Result<u32, String> {
        let p = Arc::make_mut(&mut self.processes)
            .get_mut(&pid)
            .ok_or("no such process")?;
        if matches!(
            p.state,
            ProcessState::Exited { .. } | ProcessState::Zombie { .. }
        ) {
            return Err("process exited".into());
        }
        let id = (0..u32::MAX)
            .find(|id| !p.fds.contains_key(id))
            .ok_or("descriptor exhaustion")?;
        p.fds.insert(id, fd);
        Ok(id)
    }
    pub fn close_fd(&mut self, pid: u64, fd: u32) -> Result<(), String> {
        let p = Arc::make_mut(&mut self.processes)
            .get_mut(&pid)
            .ok_or("no such process")?;
        p.fds.remove(&fd).ok_or("bad descriptor")?;
        Ok(())
    }
    pub fn own_listener(&mut self, pid: u64, listener: String) -> Result<(), String> {
        let p = Arc::make_mut(&mut self.processes)
            .get_mut(&pid)
            .ok_or("no such process")?;
        if matches!(
            p.state,
            ProcessState::Exited { .. } | ProcessState::Zombie { .. }
        ) {
            return Err("process exited".into());
        }
        p.listeners.insert(listener);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_ownership_and_resume() {
        let mut p = ProcessTable::new();
        let parent = p.spawn(1, "u", "sh", 0);
        let child = p.spawn(parent, "u", "daemon", 1);
        p.own_listener(child, "http".into()).unwrap();
        p.sleep(child, 10).unwrap();
        let encoded = serde_json::to_string(&p).unwrap();
        let mut restored: ProcessTable = serde_json::from_str(&encoded).unwrap();
        assert!(restored.advance(9).is_empty());
        assert_eq!(restored.advance(10), [child]);
        assert!(restored.signal(child, "TERM", "other", 10).is_err());
        assert_eq!(restored.exit(child, 7, 11).unwrap(), ["http"]);
        assert_eq!(restored.wait(parent, child).unwrap(), Some(7));
        assert!(restored.get(child).unwrap().fds.is_empty());
    }
    #[test]
    fn orphan_reparent_and_snapshot() {
        let mut p = ProcessTable::new();
        let parent = p.spawn(1, "u", "sh", 0);
        let child = p.spawn(parent, "u", "sleep", 0);
        let snap = p.clone();
        p.exit(parent, 0, 1).unwrap();
        assert_eq!(p.get(child).unwrap().parent, 1);
        assert_eq!(snap.get(child).unwrap().parent, parent);
        assert!(p.exit(1, 0, 1).is_err());
    }
}
#[cfg(test)]
mod descriptor_tests {
    use super::*;
    #[test]
    fn file_offsets_survive_snapshot() {
        let mut fs = crate::Vfs::new(true);
        fs.write("/f", b"abcd", "u", 0).unwrap();
        let mut table = ProcessTable::new();
        let pid = table.spawn(1, "u", "editor", 0);
        let fd = table
            .open_fd(
                pid,
                FileDescriptor::File {
                    path: "/f".into(),
                    offset: 1,
                    writable: true,
                },
            )
            .unwrap();
        assert_eq!(fd, 3);
        assert_eq!(table.read_fd(pid, fd, &fs, 2).unwrap(), b"bc");
        let mut restored: ProcessTable =
            serde_json::from_str(&serde_json::to_string(&table).unwrap()).unwrap();
        restored.write_fd(pid, fd, &mut fs, b"XYZ", 1).unwrap();
        assert_eq!(fs.read("/f").unwrap(), b"abcXYZ");
        restored.seek_fd(pid, fd, 0).unwrap();
        assert_eq!(restored.read_fd(pid, fd, &fs, 2).unwrap(), b"ab");
    }
    #[test]
    fn serializable_signal_dispositions() {
        let mut table = ProcessTable::new();
        let pid = table.spawn(1, "u", "daemon", 0);
        table
            .set_signal_disposition(pid, "TERM", SignalDisposition::Notify)
            .unwrap();
        table.signal(pid, "TERM", "u", 1).unwrap();
        assert_eq!(table.take_signals(pid).unwrap(), ["TERM"]);
        assert_eq!(table.get(pid).unwrap().state, ProcessState::Running);
        assert!(table
            .set_signal_disposition(pid, "KILL", SignalDisposition::Ignore)
            .is_err());
        table.signal(pid, "KILL", "u", 2).unwrap();
        assert!(matches!(
            table.get(pid).unwrap().state,
            ProcessState::Exited { code: 137 }
        ));
    }
    #[test]
    fn repeated_short_commands_do_not_accumulate_processes() {
        let mut table = ProcessTable::new();
        for t in 0..1000 {
            let p = table.spawn(1, "u", "echo", t);
            table.exit(p, 0, t).unwrap();
        }
        assert!(table.processes.len() <= 2);
        assert!(table.next_pid > 1000);
    }
}
#[cfg(test)]
mod scheduler_tests {
    use super::*;
    #[test]
    fn deadline_state_is_independent_of_clock_partition() {
        let mut a = ProcessTable::new();
        let p = a.spawn(1, "u", "sleep 1", 0);
        a.schedule_exit(p, 25, 0).unwrap();
        let mut b = a.clone();
        assert_eq!(a.next_deadline(), Some(25));
        a.advance(100);
        b.advance(50);
        b.advance(100);
        assert_eq!(a, b);
        assert_eq!(a.get(p).unwrap().ended, Some(25));
        assert_eq!(a.next_deadline(), None);
    }
    #[test]
    fn rejects_invalid_imported_process_identity() {
        let table = ProcessTable::new();
        let mut value = serde_json::to_value(table).unwrap();
        value["processes"]["1"]["pid"] = serde_json::json!(42);
        let invalid: ProcessTable = serde_json::from_value(value).unwrap();
        assert!(invalid.validate().is_err());
    }
}
