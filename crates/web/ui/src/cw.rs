//! The `cw` global of computerworld's desktop web applications, run natively.
//!
//! It speaks the protocol `crates/applications/src/web_app/bridge.js` speaks for an
//! application on the JS VM, over the same transport: the host's synchronous call
//! (`ScriptHostDocument::host_call`, what the bridge reaches as `__cw_host(name,
//! payload)`). `boot` answers the boot facts (`{kind, argument, state, env}`), `now`
//! the world clock in microseconds, and `out` takes each message (`{op: "request" |
//! "state" | "refuse" | "chrome", …}`), so a host that runs the bridge on a Realm
//! hosts a compiled application unchanged, interpreted or generated. What the bridge receives through `__cw_deliver(replies)`
//! and `__cw_env(env)` arrives here through [`crate::UiApp::cw_deliver`] and
//! [`crate::UiApp::cw_env`], with the same JSON. The types an application sees are
//! `crates/applications/web/types/cw.d.ts`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use cw_web::script::FetchResponse;

use crate::interp::{new_promise, obj_get};
use crate::ir::Builtin;
use crate::runtime::*;
use crate::value::*;

/// The bridge's state: the boot facts, the environment, the declared state, the
/// `onEnv` listeners and the requests awaiting a reply.
#[derive(Default)]
pub(crate) struct CwBridge {
    pub loaded: bool,
    pub kind: String,
    pub argument: String,
    pub env: Value,
    pub state: Value,
    pub listeners: Vec<(u32, Value)>,
    pub next_listener: u32,
    pub next_request: u64,
    /// Whether the app declared its state (`cw.state.set`).
    pub declared: bool,
    /// Request id to its promise, and whether its reply is an HTTP response.
    pub pending: BTreeMap<u64, (Rc<RefCell<Promise>>, bool)>,
}

fn str_arg(args: &[Value], i: usize) -> Value {
    Value::str(
        &args
            .get(i)
            .cloned()
            .unwrap_or(Value::Undefined)
            .to_js_string(),
    )
}

impl Runtime {
    /// Reads the boot facts on first use.
    fn cw(&mut self) -> &mut CwBridge {
        if !self.cw.loaded {
            self.cw.loaded = true;
            let boot = self
                .inner
                .host
                .host_call("boot", "")
                .ok()
                .and_then(|b| crate::json::parse(&b).ok())
                .unwrap_or(Value::Null);
            let field = |k: &str| match &boot {
                Value::Object(o) => obj_get(&o.borrow(), k).unwrap_or(Value::Undefined),
                _ => Value::Undefined,
            };
            let text = |v: Value| match v {
                Value::Undefined | Value::Null => String::new(),
                v => v.to_js_string(),
            };
            self.cw.kind = text(field("kind"));
            self.cw.argument = text(field("argument"));
            self.cw.env = field("env");
            self.cw.state = match field("state") {
                Value::Undefined => Value::Null,
                v => v,
            };
        }
        &mut self.cw
    }

    fn cw_send(&mut self, message: Value) {
        if let Some(json) = crate::json::stringify(&message, &Value::Undefined) {
            // A host that does not take messages has nowhere to put them.
            let _ = self.inner.host.host_call("out", &json);
        }
    }

    fn cw_request(&mut self, kind: &str, fields: Vec<(&str, Value)>, http: bool) -> Value {
        self.cw();
        self.cw.next_request += 1;
        let id = self.cw.next_request;
        let p = new_promise();
        self.cw.pending.insert(id, (p.clone(), http));
        let mut message: Vec<(Str, Value)> = vec![
            (Rc::from("op"), Value::str("request")),
            (Rc::from("id"), Value::Num(id as f64)),
            (Rc::from("kind"), Value::str(kind)),
        ];
        message.extend(fields.into_iter().map(|(k, v)| (Rc::from(k), v)));
        self.cw_send(Value::object(message));
        Value::Promise(p)
    }

    /// Evaluates one of the `cw` builtins.
    pub(crate) fn cw_builtin(&mut self, b: Builtin, args: &[Value]) -> Value {
        use Builtin as B;
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Undefined);
        match b {
            B::CwKind => Value::str(&self.cw().kind.clone()),
            B::CwArgument => Value::str(&self.cw().argument.clone()),
            B::CwEnv => self.cw().env.clone(),
            B::CwOnEnv => {
                let listener = arg(0);
                let c = self.cw();
                c.next_listener += 1;
                let id = c.next_listener;
                c.listeners.push((id, listener));
                Value::Native(Rc::new(NativeFn::CwOffEnv(id)))
            }
            B::CwNow => self
                .inner
                .host
                .host_call("now", "")
                .map(|n| Value::Num(string_to_number(&n)))
                .unwrap_or(Value::Num(0.0)),
            B::CwStateGet => self.cw().state.clone(),
            B::CwStateSet => {
                let value = arg(0);
                self.cw().state = value.clone();
                self.cw.declared = true;
                self.cw_send(Value::object(vec![
                    (Rc::from("op"), Value::str("state")),
                    (Rc::from("value"), value),
                ]));
                Value::Undefined
            }
            B::CwReadFile => self.cw_request("read", vec![("path", str_arg(args, 0))], false),
            B::CwWriteFile => self.cw_request(
                "write",
                vec![("path", str_arg(args, 0)), ("content", str_arg(args, 1))],
                false,
            ),
            B::CwList => self.cw_request("list", vec![("path", str_arg(args, 0))], false),
            B::CwMkdir => self.cw_request("mkdir", vec![("path", str_arg(args, 0))], false),
            B::CwFetch => {
                let init = match arg(1) {
                    Value::Object(o) => Some(o),
                    _ => None,
                };
                let field = |k: &str| {
                    init.as_ref()
                        .and_then(|o| obj_get(&o.borrow(), k))
                        .filter(|v| !v.is_nullish())
                };
                let method = field("method")
                    .map(|m| m.to_js_string().to_uppercase())
                    .unwrap_or_else(|| "GET".into());
                let body = field("body").map(|b| b.to_js_string()).unwrap_or_default();
                self.cw_request(
                    "http",
                    vec![
                        ("url", str_arg(args, 0)),
                        ("method", Value::str(&method)),
                        ("body", Value::str(&body)),
                    ],
                    true,
                )
            }
            B::CwLaunch => {
                let argument = match arg(1) {
                    v if v.is_nullish() => Value::str(""),
                    v => Value::str(&v.to_js_string()),
                };
                self.cw_request(
                    "launch",
                    vec![("app", str_arg(args, 0)), ("argument", argument)],
                    false,
                )
            }
            B::CwEmit => {
                let data = match arg(1) {
                    Value::Undefined => Value::Null,
                    v => v,
                };
                self.cw_request(
                    "emit",
                    vec![("name", str_arg(args, 0)), ("data", data)],
                    false,
                )
            }
            B::CwRefuse => {
                self.cw_send(Value::object(vec![
                    (Rc::from("op"), Value::str("refuse")),
                    (Rc::from("message"), str_arg(args, 0)),
                ]));
                Value::Undefined
            }
            B::CwWindowSet => {
                self.cw_send(Value::object(vec![
                    (Rc::from("op"), Value::str("chrome")),
                    (Rc::from("chrome"), arg(0)),
                ]));
                Value::Undefined
            }
            _ => Value::Undefined,
        }
    }

    /// Removes an `onEnv` listener (the function `cw.onEnv` returned).
    pub(crate) fn cw_off_env(&mut self, id: u32) {
        self.cw.listeners.retain(|(i, _)| *i != id);
    }

    /// Settles the requests `replies` answer (`[{id, value} | {id, error}]`).
    pub(crate) fn cw_deliver(&mut self, replies: &str) -> Result<(), String> {
        let replies = crate::json::parse(replies)?;
        let Value::Array(items) = replies else {
            return Err("replies must be an array".into());
        };
        let items = items.borrow().clone();
        for reply in items {
            let Value::Object(o) = reply else { continue };
            let (id, value, error) = {
                let o = o.borrow();
                (
                    obj_get(&o, "id").map(|v| v.to_number()).unwrap_or(f64::NAN),
                    obj_get(&o, "value"),
                    obj_get(&o, "error"),
                )
            };
            if id.is_nan() || id < 0.0 {
                continue;
            }
            let Some((p, http)) = self.cw.pending.remove(&(id as u64)) else {
                continue;
            };
            match (error, value) {
                (Some(e), _) if !matches!(e, Value::Undefined) => {
                    let err = Value::error("Error", &e.to_js_string());
                    self.reject_promise(&p, err);
                }
                (_, value) => {
                    let value = value.unwrap_or(Value::Undefined);
                    let value = if http { http_response(&value) } else { value };
                    self.resolve_promise(&p, value);
                }
            }
        }
        self.settle();
        Ok(())
    }

    /// The environment changed: `cw.env` is `env` and every `onEnv` listener hears it.
    pub(crate) fn cw_env(&mut self, env: &str) -> Result<(), String> {
        let env = crate::json::parse(env)?;
        self.cw().env = env.clone();
        let listeners: Vec<Value> = self.cw.listeners.iter().map(|(_, l)| l.clone()).collect();
        for l in listeners {
            if let Err(e) = self.call_value(&l, vec![env.clone()]) {
                self.report(e);
            }
        }
        self.settle();
        Ok(())
    }
}

/// A `cw.fetch` reply (`{status, body}`) as a response.
fn http_response(v: &Value) -> Value {
    let (status, body) = match v {
        Value::Object(o) => {
            let o = o.borrow();
            (
                obj_get(&o, "status").map(|s| s.to_number()).unwrap_or(0.0),
                obj_get(&o, "body")
                    .map(|b| b.to_js_string())
                    .unwrap_or_default(),
            )
        }
        _ => (0.0, String::new()),
    };
    Value::Response(Rc::new(FetchResponse {
        status: status as u16,
        status_text: String::new(),
        headers: Vec::new(),
        body: body.into_bytes(),
        url: String::new(),
    }))
}
