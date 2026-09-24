//! The built-in profiler observes without changing anything: the same program
//! runs the same instructions and produces the same values with it on or off,
//! and what it reports adds up.
use cw_jsvm::profile::ProfileOptions;
use cw_jsvm::value::Value;
use cw_jsvm::vm::Vm;
use cw_script_host::memory::MemoryHost;

const PROGRAM: &str = r#"
function fib(n) { return n < 2 ? n : fib(n - 1) + fib(n - 2); }
function Point(x, y) { this.x = x; this.y = y; }
Point.prototype.len = function () { return Math.sqrt(this.x * this.x + this.y * this.y); };
let total = 0;
for (let i = 0; i < 50; i++) total += new Point(i, 1).len();
const words = ['a', 'bb', 'ccc'].map(w => w.toUpperCase()).join('-');
String(fib(15)) + ' ' + total.toFixed(3) + ' ' + words + ' ' + Date.now();
"#;

fn run(profile: bool) -> (String, u64, Option<cw_jsvm::profile::ProfileReport>) {
    let mut host = MemoryHost::default();
    let mut vm = Vm::new(&mut host, vec!["/usr/bin/node".into()], vec![], None);
    if profile {
        vm.profile_start(ProfileOptions {
            op_time: true,
            prop_names: true,
        });
    }
    let v = vm.eval_source(PROGRAM, "prog.js", false).ok().unwrap();
    let text = match v {
        Value::Str(s) => s.to_string(),
        _ => panic!("not a string"),
    };
    let report = vm.profile_stop();
    (text, vm.steps, report)
}

#[test]
fn profiling_changes_nothing_the_program_sees() {
    let (plain, plain_steps, none) = run(false);
    let (profiled, profiled_steps, report) = run(true);
    assert!(none.is_none());
    assert_eq!(plain, profiled);
    assert_eq!(plain_steps, profiled_steps);
    let report = report.expect("a report");
    assert!(report.steps > 0 && report.steps <= profiled_steps);
}

#[test]
fn the_report_counts_calls_and_instructions() {
    let (_, _, report) = run(true);
    let report = report.unwrap();
    let steps = report.steps;
    let fib = report
        .functions
        .iter()
        .find(|f| f.name == "fib")
        .expect("fib profiled");
    // fib(15) makes 1973 calls.
    assert_eq!(fib.calls, 1973);
    assert!(fib.self_steps > 0 && fib.total_steps >= fib.self_steps);
    let native = report
        .functions
        .iter()
        .find(|f| f.name == "toUpperCase")
        .expect("natives profiled");
    assert!(native.native);
    assert_eq!(native.calls, 3);
    // Exclusive instruction counts partition the run.
    let sum: u64 = report.functions.iter().map(|f| f.self_steps).sum();
    assert_eq!(sum, steps);
    let ops: u64 = report.ops.iter().map(|o| o.count).sum();
    assert_eq!(ops, steps);
    assert!(report.counter("property reads") > 0);
    assert!(report.props.iter().any(|(k, _, _)| k == "len"));
    assert!(report.sources.iter().any(|s| s.file == "prog.js"));
    assert!(report.to_text(10).contains("fib"));
}
