//! The cw-jsvm side of the framework micro-benchmark that `bench/driver.mjs`
//! runs on Node (`node --jitless` for V8's interpreter alone):
//!
//!     cargo run --release -p cw-jsvm --example dom_bench -- [react|vue] [runs]
//!
//! A fresh VM per run evaluates `bench/dom-shim.js` (a DOM written in plain JS,
//! so both engines pay for it as script) and `bench/phases.js`, then times four
//! phases: load (evaluate the framework's production bundles from
//! `crates/web/engine/tests/vendor`), mount (evaluate the fixture app and run
//! the event loop until idle), click (a re-rendering click on `#check-2`) and
//! key (one keystroke into the controlled `#new-task` input). Prints the median
//! of each in milliseconds and checks the resulting DOM against the same
//! summary the Node driver checks.
//!
//! Each run starts with the compile cache (`cw_jsvm::codecache`) empty, as a
//! fresh Node process compiles everything anew; `DOM_BENCH_CACHE=warm` keeps it
//! across runs instead, which is what a second realm loading the same bundles
//! sees. `DOM_BENCH_PROFILE=<phase>` (load, mount, click or key) prints the
//! built-in profiler's report for that phase of the first run.
use cw_jsvm::value::{Ctl, Value};
use cw_jsvm::vm::Vm;
use cw_script_host::memory::MemoryHost;
use std::path::PathBuf;
use std::time::Instant;

const EXPECTED: &str = "task|task done|task done|task|task|task done / x";

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn check(vm: &mut Vm, r: Result<Value, Ctl>, what: &str) -> Value {
    match r {
        Ok(v) => v,
        Err(Ctl::Throw(v)) | Err(Ctl::Fatal(v)) => {
            let s = vm
                .get_str(&v, "stack")
                .ok()
                .and_then(|s| match s {
                    Value::Str(s) => Some(s.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| "exception".into());
            panic!("{what}: {s}");
        }
        Err(Ctl::Exit(c)) => panic!("{what}: exit {c}"),
    }
}

struct Sources {
    shim: String,
    phases: String,
    bundles: Vec<(String, String)>,
    app: String,
}

/// Starts the profiler if `DOM_BENCH_PROFILE` names `phase` (first run only).
fn prof_start(vm: &mut Vm, phase: &str, first: bool) {
    if first && std::env::var("DOM_BENCH_PROFILE").as_deref() == Ok(phase) {
        vm.profile_start(cw_jsvm::profile::ProfileOptions {
            op_time: false,
            prop_names: true,
        });
    }
}

fn prof_stop(vm: &mut Vm, phase: &str) {
    if let Some(p) = vm.profile_stop() {
        eprintln!("==== {phase} profile ====\n{}", p.to_text(30));
    }
}

fn once(which: &str, src: &Sources, first: bool) -> [f64; 4] {
    if std::env::var("DOM_BENCH_CACHE").as_deref() != Ok("warm") {
        cw_jsvm::codecache::clear();
    }
    let mut host = MemoryHost::default();
    let mut vm = Vm::new(&mut host, vec!["/usr/bin/node".into()], vec![], None);
    let r = vm.eval_source_with(&src.shim, "dom-shim.js", false, true);
    check(&mut vm, r, "dom-shim.js");
    let r = vm.eval_source_with(&src.phases, "phases.js", false, true);
    check(&mut vm, r, "phases.js");
    let idle = |vm: &mut Vm, what: &str| {
        let r = vm.run_microtasks().and_then(|_| vm.event_loop());
        check(vm, r.map(|_| Value::Undefined), what);
    };
    prof_start(&mut vm, "load", first);
    let t = Instant::now();
    for (name, code) in &src.bundles {
        let r = vm.eval_source_with(code, name, false, true);
        check(&mut vm, r, name);
    }
    let load = t.elapsed().as_secs_f64() * 1000.0;
    prof_stop(&mut vm, "load");
    prof_start(&mut vm, "mount", first);
    let t = Instant::now();
    let r = vm.eval_source_with(&src.app, &format!("{which}-app.js"), false, true);
    check(&mut vm, r, "app");
    idle(&mut vm, "mount");
    let mount = t.elapsed().as_secs_f64() * 1000.0;
    prof_stop(&mut vm, "mount");
    prof_start(&mut vm, "click", first);
    let t = Instant::now();
    let r = vm.eval_source_with("benchClick('check-2')", "click", false, true);
    check(&mut vm, r, "click");
    idle(&mut vm, "click");
    let click = t.elapsed().as_secs_f64() * 1000.0;
    prof_stop(&mut vm, "click");
    prof_start(&mut vm, "key", first);
    let t = Instant::now();
    let r = vm.eval_source_with("benchKey()", "key", false, true);
    check(&mut vm, r, "key");
    idle(&mut vm, "key");
    let key = t.elapsed().as_secs_f64() * 1000.0;
    prof_stop(&mut vm, "key");
    let r = vm.eval_source_with("benchSummary()", "summary", false, true);
    let summary = match check(&mut vm, r, "summary") {
        Value::Str(s) => s.to_string(),
        _ => String::new(),
    };
    assert_eq!(
        summary, EXPECTED,
        "{which}: the DOM after the phases is not what Node produces"
    );
    [load, mount, click, key]
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let which = args
        .first()
        .map(String::as_str)
        .unwrap_or("react")
        .to_string();
    let runs: usize = args.get(1).and_then(|r| r.parse().ok()).unwrap_or(15);
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let vendor = here.join("../../web/engine/tests/vendor");
    let bundles: &[&str] = match which.as_str() {
        "react" => &[
            "react-18.3.1.production.min.js",
            "react-dom-18.3.1.production.min.js",
        ],
        "vue" => &["vue-3.4.38.global.prod.js"],
        other => panic!("unknown framework {other} (react or vue)"),
    };
    let src = Sources {
        shim: read(here.join("bench/dom-shim.js")),
        phases: read(here.join("bench/phases.js")),
        bundles: bundles
            .iter()
            .map(|b| (b.to_string(), read(vendor.join(b))))
            .collect(),
        app: read(here.join(format!("bench/{which}-app.js"))),
    };
    let mut all: Vec<[f64; 4]> = (0..runs).map(|i| once(&which, &src, i == 0)).collect();
    let mut med = |i: usize| {
        all.sort_by(|a, b| a[i].partial_cmp(&b[i]).unwrap());
        all[all.len() / 2][i]
    };
    let (load, mount, click, key) = (med(0), med(1), med(2), med(3));
    let cache = if std::env::var("DOM_BENCH_CACHE").as_deref() == Ok("warm") {
        "warm"
    } else {
        "cold"
    };
    println!(
        "{which} load {load:.2} mount {mount:.2} click {click:.2} key {key:.2} (ms, median of {runs}, cw-jsvm, {cache} compile cache)"
    );
}
