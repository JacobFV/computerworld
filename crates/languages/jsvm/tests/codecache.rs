//! The compile cache is invisible: a program run with its sources already
//! compiled (by an earlier run on the thread, or earlier in the same run)
//! behaves exactly as when everything is compiled afresh — the same output,
//! stack traces, `Function.prototype.toString`, virtual time and instruction
//! count, distinct functions and template objects per compile, and the same
//! syntax errors.
use cw_jsvm::codecache;
use cw_script_host::{memory::MemoryHost, Invocation, ScriptHost};

const PROGRAM: &str = r#"
const t0 = Date.now();
globalThis.tag = function tag(s) { return s; };
const make = () => tag`a${1}b`;
const first = make();
// The same source compiled twice in one run: distinct functions and template
// objects, as two compiles give.
const src = 'return function inner(x) { return x * 2; /* body */ }';
const f1 = new Function(src)(), f2 = new Function(src)();
console.log(f1 === f2, f1.toString());
const e1 = eval('(function () { return tag`q`; })')();
const e2 = eval('(function () { return tag`q`; })')();
console.log(e1 === e2, first === make());
// Every first run of a body is charged to the virtual clock.
for (let i = 0; i < 3; i++) new Function('return ' + i)();
for (let i = 0; i < 3; i++) new Function('return 7')();
console.log(Date.now() - t0);
function thrower() { throw new Error('boom'); }
try { thrower(); } catch (e) { console.log(e.stack); }
try { new Function('let a; let a;'); } catch (e) { console.log(e.name, e.message); }
setTimeout(() => console.log('timer', Date.now() - t0), 5);
Promise.resolve().then(() => console.log('micro', Date.now() - t0));
"#;

fn run() -> (String, String, i32) {
    let mut host = MemoryHost::default();
    host.write_file("/home/user/main.js", PROGRAM.as_bytes(), false)
        .unwrap();
    let out = cw_jsvm::run(
        &mut host,
        &Invocation {
            args: vec!["main.js".into()],
            ..Default::default()
        },
    );
    (out.stdout, out.stderr, out.exit_code)
}

#[test]
fn cached_runs_match_fresh_compiles() {
    codecache::set_enabled(false);
    let fresh = run();
    codecache::set_enabled(true);
    let cold = run();
    let (_, _, hits_before, _) = codecache::stats();
    let warm = run();
    let (_, _, hits_after, _) = codecache::stats();
    assert!(hits_after > hits_before, "the second run used the cache");
    assert_eq!(fresh, cold);
    assert_eq!(fresh, warm);
    assert!(fresh.0.contains("false function inner(x)"), "{}", fresh.0);
    assert!(fresh.0.contains("false true"), "{:?}", fresh);
    assert!(
        fresh.0.contains("at thrower (/home/user/main.js:"),
        "{}",
        fresh.0
    );
    assert!(fresh.0.contains("SyntaxError"), "{}", fresh.0);
}

#[test]
fn cached_code_runs_the_same_instructions() {
    use cw_jsvm::vm::Vm;
    let steps = |cache: bool| {
        codecache::set_enabled(cache);
        let mut host = MemoryHost::default();
        let mut vm = Vm::new(&mut host, vec!["/usr/bin/node".into()], vec![], None);
        let src = "let n = 0; for (let i = 0; i < 100; i++) n += (() => i)(); n";
        vm.eval_source(src, "steps.js", false).ok().unwrap();
        let t = vm.clock();
        vm.eval_source(src, "steps.js", false).ok().unwrap();
        (vm.steps, t, vm.clock())
    };
    let fresh = steps(false);
    let cold = steps(true);
    let warm = steps(true);
    assert_eq!(fresh, cold);
    assert_eq!(fresh, warm);
}
