//! The import system and the standard library: native modules written in Rust
//! and pure-Python modules embedded as source.
pub mod collections;
pub mod host;
pub mod json;
pub mod math;
pub mod random;
pub mod re;
pub mod structmod;
pub mod sys;
pub mod thread;
pub mod zlib;

use crate::builtins::native_fn;
use crate::value::*;
use crate::vm::*;
use std::rc::Rc;

/// Pure-Python standard library modules, compiled on first import.
pub const PY_MODULES: &[(&str, &str)] = &[
    ("abc", include_str!("../../lib/abc.py")),
    ("argparse", include_str!("../../lib/argparse.py")),
    ("asyncio", include_str!("../../lib/asyncio.py")),
    ("base64", include_str!("../../lib/base64.py")),
    ("bisect", include_str!("../../lib/bisect.py")),
    ("calendar", include_str!("../../lib/calendar.py")),
    ("collections", include_str!("../../lib/collections.py")),
    (
        "collections.abc",
        include_str!("../../lib/collections_abc.py"),
    ),
    ("concurrent", include_str!("../../lib/concurrent_init.py")),
    (
        "concurrent.futures",
        include_str!("../../lib/concurrent_futures.py"),
    ),
    ("contextlib", include_str!("../../lib/contextlib.py")),
    ("copy", include_str!("../../lib/copy.py")),
    ("csv", include_str!("../../lib/csv.py")),
    ("dataclasses", include_str!("../../lib/dataclasses.py")),
    ("datetime", include_str!("../../lib/datetime.py")),
    ("enum", include_str!("../../lib/enum.py")),
    ("fractions", include_str!("../../lib/fractions.py")),
    ("functools", include_str!("../../lib/functools.py")),
    ("glob", include_str!("../../lib/glob.py")),
    ("gzip", include_str!("../../lib/gzip.py")),
    ("heapq", include_str!("../../lib/heapq.py")),
    ("http", include_str!("../../lib/http_init.py")),
    ("http.client", include_str!("../../lib/http_client.py")),
    ("io", include_str!("../../lib/io.py")),
    ("itertools", include_str!("../../lib/itertools.py")),
    ("keyword", include_str!("../../lib/keyword.py")),
    ("logging", include_str!("../../lib/logging.py")),
    ("numbers", include_str!("../../lib/numbers.py")),
    ("operator", include_str!("../../lib/operator.py")),
    ("os", include_str!("../../lib/os.py")),
    ("posixpath", include_str!("../../lib/posixpath.py")),
    ("pathlib", include_str!("../../lib/pathlib.py")),
    ("pprint", include_str!("../../lib/pprint.py")),
    ("queue", include_str!("../../lib/queue.py")),
    ("random", include_str!("../../lib/random.py")),
    ("shlex", include_str!("../../lib/shlex.py")),
    ("shutil", include_str!("../../lib/shutil.py")),
    ("socket", include_str!("../../lib/socket.py")),
    ("ssl", include_str!("../../lib/ssl.py")),
    ("statistics", include_str!("../../lib/statistics.py")),
    ("string", include_str!("../../lib/string.py")),
    ("struct", include_str!("../../lib/struct.py")),
    ("subprocess", include_str!("../../lib/subprocess.py")),
    ("textwrap", include_str!("../../lib/textwrap.py")),
    ("threading", include_str!("../../lib/threading.py")),
    ("traceback", include_str!("../../lib/traceback.py")),
    ("types", include_str!("../../lib/types.py")),
    ("typing", include_str!("../../lib/typing.py")),
    ("unittest", include_str!("../../lib/unittest.py")),
    ("urllib", include_str!("../../lib/urllib_init.py")),
    ("urllib.error", include_str!("../../lib/urllib_error.py")),
    ("urllib.parse", include_str!("../../lib/urllib_parse.py")),
    (
        "urllib.request",
        include_str!("../../lib/urllib_request.py"),
    ),
    (
        "urllib.response",
        include_str!("../../lib/urllib_response.py"),
    ),
    ("warnings", include_str!("../../lib/warnings.py")),
    ("weakref", include_str!("../../lib/weakref.py")),
    ("zlib", include_str!("../../lib/zlib.py")),
];

/// Native modules and their constructors.
fn native_module(vm: &mut Vm, name: &str) -> Option<Value> {
    Some(match name {
        "sys" => sys::make_sys(vm),
        "_os" => sys::make_os(vm),
        "time" => sys::make_time(vm),
        "math" => math::make(vm),
        "_random" => random::make(vm),
        "json" => json::make(vm),
        "re" => re::make(vm),
        "_collections" => collections::make(vm),
        "_cw" => host::make(vm),
        "_zlib" => zlib::make(vm),
        "_struct" => structmod::make(vm),
        "_thread" => thread::make(vm),
        "gc" => {
            let m = new_module("gc");
            set_fn(&m, "collect", |_, _| Ok(Value::Int(0)));
            set_fn(&m, "enable", |_, _| Ok(Value::None));
            set_fn(&m, "disable", |_, _| Ok(Value::None));
            Value::Module(m)
        }
        "hashlib" => sys::make_hashlib(vm),
        "platform" => sys::make_platform(vm),
        "secrets" => sys::make_secrets(vm),
        _ => return None,
    })
}

pub fn new_module(name: &str) -> Rc<Module> {
    let d = new_ref(Dict::new());
    d.borrow_mut().set_str("__name__", Value::str(name));
    d.borrow_mut().set_str("__doc__", Value::None);
    Rc::new(Module {
        name: name.into(),
        dict: d,
    })
}
pub fn set_fn(m: &Rc<Module>, name: &str, f: NativeFn) {
    let q = format!("{}.{}", m.name, name);
    let _ = q;
    m.dict.borrow_mut().set_str(name, native_fn(name, f));
}
pub fn set_val(m: &Rc<Module>, name: &str, v: Value) {
    m.dict.borrow_mut().set_str(name, v);
}

impl<'h> Vm<'h> {
    fn resolve_name(&mut self, name: &str, level: usize, globals: &Ref<Dict>) -> PyResult<String> {
        if level == 0 {
            return Ok(name.to_string());
        }
        let g = globals.borrow();
        let package = match g.get_str("__package__") {
            Some(Value::Str(p)) if !p.s.is_empty() => p.s.clone(),
            _ => {
                let modname = match g.get_str("__name__") {
                    Some(Value::Str(n)) => n.s.clone(),
                    _ => String::new(),
                };
                if g.contains_str("__path__") {
                    modname
                } else {
                    modname
                        .rsplit_once('.')
                        .map(|(p, _)| p.to_string())
                        .unwrap_or_default()
                }
            }
        };
        drop(g);
        if package.is_empty() {
            return Err(err(
                "ImportError",
                "attempted relative import with no known parent package",
            ));
        }
        let mut parts: Vec<&str> = package.split('.').collect();
        for _ in 1..level {
            if parts.pop().is_none() {
                return Err(err(
                    "ImportError",
                    "attempted relative import beyond top-level package",
                ));
            }
        }
        let base = parts.join(".");
        Ok(if name.is_empty() {
            base
        } else {
            format!("{base}.{name}")
        })
    }

    pub fn import(
        &mut self,
        name: &str,
        fromlist: &Value,
        level: usize,
        globals: &Ref<Dict>,
    ) -> PyResult<Value> {
        let full = self.resolve_name(name, level, globals)?;
        if full.is_empty() {
            return Err(value_err("Empty module name"));
        }
        let parts: Vec<&str> = full.split('.').collect();
        let mut top = None;
        let mut last = Value::None;
        for i in 0..parts.len() {
            let modname = parts[..=i].join(".");
            let m = self.load_module(&modname, if i > 0 { Some(&last) } else { None })?;
            if i == 0 {
                top = Some(m.clone());
            }
            last = m;
        }
        let has_fromlist = match fromlist {
            Value::Tuple(t) => !t.is_empty(),
            Value::List(l) => !l.borrow().is_empty(),
            _ => false,
        };
        if has_fromlist {
            // `from pkg import submodule` loads the submodule if needed.
            let names = self.iterate(fromlist)?;
            if let Value::Module(m) = &last {
                let is_pkg = m.dict.borrow().contains_str("__path__");
                if is_pkg {
                    for n in names {
                        if let Value::Str(s) = n {
                            if s.s != "*" && !m.dict.borrow().contains_str(&s.s) {
                                let sub = format!("{full}.{}", s.s);
                                let _ = self.load_module(&sub, Some(&last));
                            }
                        }
                    }
                }
            }
            return Ok(last);
        }
        if level > 0 && name.is_empty() {
            return Ok(last);
        }
        Ok(top.unwrap_or(last))
    }

    fn load_module(&mut self, fullname: &str, parent: Option<&Value>) -> PyResult<Value> {
        if let Some(m) = self.modules.borrow().get_str(fullname) {
            return Ok(m);
        }
        if let Some(m) = native_module(self, fullname) {
            self.modules.borrow_mut().set_str(fullname, m.clone());
            self.bind_submodule(parent, fullname, &m);
            return Ok(m);
        }
        // User modules on sys.path shadow the pure-Python stdlib, as in CPython
        // (except for modules CPython itself treats as builtin).
        if let Some(m) = self.find_user_module(fullname, parent)? {
            self.bind_submodule(parent, fullname, &m);
            return Ok(m);
        }
        if let Some((_, src)) = PY_MODULES.iter().find(|(n, _)| *n == fullname) {
            let m =
                self.exec_module_source(fullname, src, &format!("<frozen {fullname}>"), None)?;
            self.bind_submodule(parent, fullname, &m);
            return Ok(m);
        }
        if let Some(Value::Module(p)) = parent {
            if !p.dict.borrow().contains_str("__path__") {
                return Err(err_args(
                    "ModuleNotFoundError",
                    vec![Value::string(format!(
                        "No module named '{fullname}'; '{}' is not a package",
                        p.name
                    ))],
                ));
            }
        }
        Err(err_args(
            "ModuleNotFoundError",
            vec![Value::string(format!("No module named '{fullname}'"))],
        ))
    }

    fn bind_submodule(&mut self, parent: Option<&Value>, fullname: &str, m: &Value) {
        if let Some(Value::Module(p)) = parent {
            let short = fullname.rsplit('.').next().unwrap_or(fullname);
            p.dict.borrow_mut().set_str(short, m.clone());
        }
    }

    fn search_path(&mut self) -> Vec<String> {
        let mut out = vec![];
        if let Some(Value::Module(sys)) = self.modules.borrow().get_str("sys") {
            if let Some(Value::List(l)) = sys.dict.borrow().get_str("path") {
                for p in l.borrow().iter() {
                    if let Value::Str(s) = p {
                        out.push(s.s.clone());
                    }
                }
            }
        }
        if out.is_empty() {
            out.push(self.script_dir.clone());
        }
        out
    }

    fn find_user_module(
        &mut self,
        fullname: &str,
        parent: Option<&Value>,
    ) -> PyResult<Option<Value>> {
        let short = fullname.rsplit('.').next().unwrap_or(fullname);
        let dirs: Vec<String> = match parent {
            Some(Value::Module(p)) => match p.dict.borrow().get_str("__path__") {
                Some(Value::List(l)) => l
                    .borrow()
                    .iter()
                    .filter_map(|v| v.as_pystr().map(|s| s.s.clone()))
                    .collect(),
                _ => return Ok(None),
            },
            _ => self.search_path(),
        };
        for dir in dirs {
            let dir = if dir.is_empty() { self.host.cwd() } else { dir };
            let file = format!("{}/{short}.py", dir.trim_end_matches('/'));
            let pkg = format!("{}/{short}", dir.trim_end_matches('/'));
            let init = format!("{pkg}/__init__.py");
            if let Ok(st) = self.host.stat(&pkg) {
                if st.is_dir {
                    if let Ok(src) = self.host.read_file(&init) {
                        let src = String::from_utf8_lossy(&src).into_owned();
                        let path = self.host.resolve(&init);
                        let pkgdir = self.host.resolve(&pkg);
                        return self
                            .exec_module_source(fullname, &src, &path, Some(pkgdir))
                            .map(Some);
                    }
                    // Namespace package.
                    let m = new_module(fullname);
                    let pkgdir = self.host.resolve(&pkg);
                    set_val(&m, "__path__", Value::list(vec![Value::string(pkgdir)]));
                    let v = Value::Module(m);
                    self.modules.borrow_mut().set_str(fullname, v.clone());
                    return Ok(Some(v));
                }
            }
            if let Ok(src) = self.host.read_file(&file) {
                let src = String::from_utf8_lossy(&src).into_owned();
                let path = self.host.resolve(&file);
                return self
                    .exec_module_source(fullname, &src, &path, None)
                    .map(Some);
            }
        }
        Ok(None)
    }

    pub fn exec_module_source(
        &mut self,
        name: &str,
        src: &str,
        path: &str,
        pkgdir: Option<String>,
    ) -> PyResult<Value> {
        let m = new_module(name);
        if !path.starts_with("<frozen") {
            set_val(&m, "__file__", Value::str(path));
        }
        let package = match &pkgdir {
            Some(_) => name.to_string(),
            None => name
                .rsplit_once('.')
                .map(|(p, _)| p.to_string())
                .unwrap_or_default(),
        };
        set_val(&m, "__package__", Value::string(package));
        if let Some(d) = pkgdir {
            set_val(&m, "__path__", Value::list(vec![Value::string(d)]));
        }
        set_val(&m, "__builtins__", Value::Dict(self.builtins.clone()));
        let v = Value::Module(m.clone());
        self.modules.borrow_mut().set_str(name, v.clone());
        self.sources.insert(path.to_string(), src.into());
        let code = match crate::compile_source(self, src, path, "exec") {
            Ok(c) => c,
            Err(e) => {
                self.modules.borrow_mut().del_str(name);
                return Err(e);
            }
        };
        let frame = self.new_frame(code, m.dict.clone(), None);
        self.charge_depth()?;
        if let Err(e) = self.execute(frame, None) {
            self.modules.borrow_mut().del_str(name);
            return Err(e);
        }
        Ok(v)
    }

    pub fn import_from(&mut self, m: &Value, name: &Rc<PyStr>) -> PyResult<Value> {
        match self.getattr(m, name) {
            Ok(v) => Ok(v),
            Err(e) if self.err_matches(&e, "AttributeError") => {
                let (modname, file) = match m {
                    Value::Module(md) => {
                        (md.name.to_string(), md.dict.borrow().get_str("__file__"))
                    }
                    _ => ("?".into(), None),
                };
                // A submodule not yet imported.
                if let Value::Module(md) = m {
                    if md.dict.borrow().contains_str("__path__") {
                        let full = format!("{modname}.{}", name.s);
                        if let Ok(sub) = self.load_module(&full, Some(m)) {
                            return Ok(sub);
                        }
                    }
                }
                let loc = match file {
                    Some(Value::Str(f)) => f.s.clone(),
                    _ => "unknown location".into(),
                };
                Err(err(
                    "ImportError",
                    format!("cannot import name '{}' from '{modname}' ({loc})", name.s),
                ))
            }
            Err(e) => Err(e),
        }
    }

    pub fn import_star(&mut self, m: &Value, f: &mut crate::vm::Frame) -> PyResult<()> {
        let Value::Module(md) = m else {
            return Ok(());
        };
        let target = f.locals.clone().unwrap_or_else(|| f.globals.clone());
        let all = md.dict.borrow().get_str("__all__");
        let names: Vec<Value> = match all {
            Some(a) => self.iterate(&a)?,
            None => md
                .dict
                .borrow()
                .keys()
                .into_iter()
                .filter(|k| matches!(k, Value::Str(s) if !s.s.starts_with('_')))
                .collect(),
        };
        for n in names {
            if let Value::Str(s) = &n {
                let v = self.getattr(m, s)?;
                target.borrow_mut().set_pystr(s.clone(), v);
            }
        }
        Ok(())
    }
}

// Dispatch for Native values owned by modules.
pub fn native_attr(vm: &mut Vm, obj: &Value, name: &str) -> PyResult<Option<Value>> {
    re::native_attr(vm, obj, name)
}
pub fn native_repr(vm: &mut Vm, v: &Value) -> PyResult<String> {
    re::native_repr(vm, v)
}
pub fn native_getitem(vm: &mut Vm, obj: &Value, idx: &Value) -> PyResult<Value> {
    re::native_getitem(vm, obj, idx)
}
