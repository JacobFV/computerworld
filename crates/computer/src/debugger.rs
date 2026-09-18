//! Debugging a program on this machine: the runtime's DAP-shaped debugger
//! ([`cw_script_host::debug`]) driven by a session ([`cw_script_host::dap`]).
//!
//! A debug session outlives the actions of the world, so it cannot keep an
//! interpreter alive; it replays the program instead, answering the host calls
//! the earlier run made from a journal. [`MachineDebugger`] is what the session
//! reruns the program with.
use crate::runtimes::{MachineHost, Runtime};
use crate::{Computer, ShellHost};
use cw_script_host::dap::DebugRunner;
use cw_script_host::debug::{DebugRunInfo, Debugger};
use cw_script_host::journal::{Journal, JournalHost};
use cw_script_host::{Invocation, Outcome};

/// Runs one program on a machine as often as a debug session needs it.
pub struct MachineDebugger<'a> {
    pub computer: &'a mut Computer,
    pub shell: &'a mut dyn ShellHost,
    pub tick: u64,
    pub runtime: Runtime,
    /// The interpreter's arguments, `["main.py"]` and the program's own.
    pub args: Vec<String>,
    pub stdin: String,
}

impl<'a> MachineDebugger<'a> {
    /// A debugger for `command` (`python3 main.py`, `node app.js …`), or `None`
    /// when the command does not name a runtime this machine debugs.
    pub fn for_command(
        computer: &'a mut Computer,
        shell: &'a mut dyn ShellHost,
        tick: u64,
        command: &[String],
    ) -> Option<Self> {
        let runtime = crate::runtimes::runtime_for(command.first()?)?;
        Some(Self {
            computer,
            shell,
            tick,
            runtime,
            args: command[1..].to_vec(),
            stdin: String::new(),
        })
    }
}

impl DebugRunner for MachineDebugger<'_> {
    fn run(
        &mut self,
        debugger: &mut dyn Debugger,
        journal: Journal,
    ) -> (Outcome, DebugRunInfo, Journal) {
        let env: Vec<(String, String)> = self
            .computer
            .env
            .iter()
            .filter(|(k, _)| {
                k.chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let invocation = Invocation {
            args: self.args.clone(),
            env,
            stdin: self.stdin.clone(),
            ..Invocation::default()
        };
        let mut machine = MachineHost::new(self.computer, self.shell, self.tick);
        let mut host = JournalHost::new(&mut machine, journal);
        let (out, info) = match self.runtime {
            Runtime::Python => cw_pyvm::run_debug(&mut host, &invocation, debugger),
            Runtime::Node => cw_jsvm::run_debug(&mut host, &invocation, debugger),
        };
        let journal = host.into_journal();
        self.computer.runtime_elapsed_micros = self
            .computer
            .runtime_elapsed_micros
            .saturating_add(out.elapsed_micros);
        (out, info, journal)
    }
}
