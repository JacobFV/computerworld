//! Serving HTTP from a Node program, for an embedder that is itself a server:
//! a world service that runs an app's own Node backend (`cw-service-node-app`).
//!
//! [`serve`] boots the program on a fresh VM over the embedder's host (files,
//! clock and entropy all come from it), waits for it to listen on `port`, hands
//! it one request exactly as a loopback client's request reaches it (the
//! program sees an ordinary `http.IncomingMessage` and `ServerResponse`), runs
//! the event loop until the response has ended and the work due at that moment
//! is done, and returns the response with whatever the program printed. Timers
//! still pending further off are dropped with the VM.
//!
//! One VM per request is what makes it deterministic and snapshot-safe for the
//! embedder: everything the app keeps between requests has to go through the
//! host's files, which the embedder owns and serialises, so a restored or forked
//! world answers the next request exactly as the original would have.

use crate::value::*;
use crate::vm::{Tail, Vm};
use cw_script_host::ScriptHost;

/// One request from outside the program.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServeRequest {
    pub method: String,
    /// The path and query (`/api/articles?limit=10`).
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// What the program answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServeResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// The outcome of one [`serve`].
#[derive(Clone, Debug, Default)]
pub struct Served {
    /// `None` when the program failed to boot, never listened, or never ended
    /// the response; `error` says which.
    pub response: Option<ServeResponse>,
    pub error: Option<String>,
    pub stdout: String,
    pub stderr: String,
    /// VM instructions executed, boot included.
    pub steps: u64,
    /// Virtual milliseconds the program's clock advanced.
    pub elapsed_ms: f64,
}

/// How long (virtual milliseconds) a program may take to start listening, and
/// then to answer, before the request fails as a gateway timeout would.
pub const BOOT_DEADLINE_MS: f64 = 30_000.0;
pub const ANSWER_DEADLINE_MS: f64 = 30_000.0;

/// Boots `main` (an absolute path on the host) with `args` after it and `env`,
/// and answers `request` with the server listening on `port`. `budget` caps the
/// instructions the whole exchange may execute.
pub fn serve(
    host: &mut dyn ScriptHost,
    main: &str,
    args: Vec<String>,
    env: Vec<(String, String)>,
    port: u16,
    request: &ServeRequest,
    budget: u64,
) -> Served {
    let mut argv = vec!["/usr/bin/node".to_string(), main.to_string()];
    argv.extend(args);
    let mut vm = Vm::new(host, argv, env, Some(String::new()));
    vm.budget = budget;
    let result = exchange(&mut vm, main, port, request);
    let mut out = Served {
        stdout: std::mem::take(&mut vm.stdout),
        stderr: std::mem::take(&mut vm.stderr),
        steps: vm.steps,
        elapsed_ms: vm.clock(),
        ..Served::default()
    };
    match result {
        Ok(response) => out.response = Some(response),
        Err(Failure::Message(m)) => out.error = Some(m),
        Err(Failure::Js(ctl)) => {
            let message = match ctl {
                Ctl::Throw(v) | Ctl::Fatal(v) => {
                    vm.report_uncaught(&v);
                    let text = std::mem::take(&mut vm.stderr);
                    out.stderr.push_str(&text);
                    // The `Name: message` line, not the source line quoted above it.
                    text.lines()
                        .find(|l| {
                            l.split_once(':').is_some_and(|(n, _)| {
                                n.ends_with("Error") && !n.contains(char::is_whitespace)
                            })
                        })
                        .unwrap_or("uncaught exception")
                        .trim()
                        .to_string()
                }
                Ctl::Exit(code) => format!("the program exited with status {code}"),
            };
            out.error = Some(message);
        }
    }
    out
}

enum Failure {
    Message(String),
    Js(Ctl),
}

impl From<Ctl> for Failure {
    fn from(c: Ctl) -> Self {
        Failure::Js(c)
    }
}

fn exchange(
    vm: &mut Vm,
    main: &str,
    port: u16,
    request: &ServeRequest,
) -> Result<ServeResponse, Failure> {
    let Some(file) = vm.resolve_main(main) else {
        return Err(Failure::Message(format!("Cannot find module '{main}'")));
    };
    vm.main_file = file.clone();
    vm.tail = Tail::Main;
    vm.load_file_module(&file)?;
    vm.tail = Tail::Microtask(None, false);
    let serve = vm.require("internal/serve", "/node_internal", "")?;
    let listening = vm.get_str(&serve, "listening")?;
    let port_v = Value::Num(port as f64);
    // Boot: until the server listens (an app may connect to its store first).
    let boot_deadline = vm.clock() + BOOT_DEADLINE_MS;
    let mut probe_err = None;
    let up = vm.event_loop_until(
        &mut |vm| match vm.call(&listening, Value::Undefined, vec![port_v.clone()]) {
            Ok(v) => v.truthy(),
            Err(e) => {
                probe_err = Some(e);
                true
            }
        },
        boot_deadline,
    )?;
    if let Some(e) = probe_err {
        return Err(e.into());
    }
    if !up {
        return Err(Failure::Message(format!(
            "the program did not listen on port {port}"
        )));
    }
    let flat: Vec<Value> = request
        .headers
        .iter()
        .flat_map(|(k, v)| [Value::string(k.clone()), Value::string(v.clone())])
        .collect();
    let raw = vm.arr(flat);
    let body = vm.make_buffer(request.body.clone());
    let dispatch = vm.get_str(&serve, "dispatch")?;
    let ex = vm.call(
        &dispatch,
        Value::Undefined,
        vec![
            port_v,
            Value::string(request.method.clone()),
            Value::string(request.path.clone()),
            raw,
            body,
        ],
    )?;
    let deadline = vm.clock() + ANSWER_DEADLINE_MS;
    let mut probe_err = None;
    let done = vm.event_loop_until(
        &mut |vm| match vm.get_str(&ex, "done") {
            Ok(v) => v.truthy(),
            Err(e) => {
                probe_err = Some(e);
                true
            }
        },
        deadline,
    )?;
    if let Some(e) = probe_err {
        return Err(e.into());
    }
    if !done {
        return Err(Failure::Message(
            "the program did not answer the request".into(),
        ));
    }
    let error = vm.get_str(&ex, "error")?;
    if !matches!(error, Value::Null | Value::Undefined) {
        return Err(Failure::Message(vm.to_str(&error)?));
    }
    let status = vm.get_str(&ex, "status")?;
    let status = vm.to_number(&status)? as u16;
    let headers = vm.get_str(&ex, "headers")?;
    let headers = crate::hostio::pairs(vm, &headers)?;
    let body = vm.get_str(&ex, "body")?;
    let body = crate::hostio::bytes_arg(vm, &body)?;
    Ok(ServeResponse {
        status,
        headers,
        body,
    })
}
