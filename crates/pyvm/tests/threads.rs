//! The deterministic thread scheduler: preemption at a seed-derived quantum,
//! reproducible races, thread failure reporting and deadlock detection.
use cw_script_host::{memory::MemoryHost, Invocation, Outcome, ScriptHost};

fn run_seeded(src: &str, seed: u64) -> Outcome {
    let mut host = MemoryHost {
        rng: seed,
        ..MemoryHost::default()
    };
    host.write_file("/home/user/main.py", src.as_bytes(), false)
        .unwrap();
    cw_pyvm::run(
        &mut host,
        &Invocation {
            args: vec!["main.py".into()],
            ..Default::default()
        },
    )
}

fn script(src: &str) -> Outcome {
    run_seeded(src, MemoryHost::default().rng)
}

/// A race with no locks at all: threads take turns only where the quantum ends.
const RACE: &str = r#"
import threading
log = []


def spin(name):
    total = 0
    for i in range(2000):
        total += i
        if i % 50 == 0:
            log.append((name, i))
    log.append((name, total))


ts = [threading.Thread(target=spin, args=(n,)) for n in 'abc']
for t in ts:
    t.start()
for t in ts:
    t.join()
print(log)
"#;

#[test]
fn races_replay_identically_under_one_seed() {
    let a = script(RACE);
    let b = script(RACE);
    assert_eq!(a.exit_code, 0, "{}", a.stderr);
    assert_eq!(
        a.stdout, b.stdout,
        "the same seed must replay the same race"
    );
    // The threads really do interleave rather than run to completion in turn.
    let out = &a.stdout;
    let first = out.find("'b'").expect("b runs");
    let last_a = out.rfind("'a'").expect("a runs");
    assert!(first < last_a, "threads interleave: {out}");
}

#[test]
fn a_different_world_seed_interleaves_differently() {
    let a = script(RACE);
    let mut differs = false;
    for seed in [1u64, 2, 3, 12345, 0xdead_beef] {
        let b = run_seeded(RACE, seed);
        assert_eq!(b.exit_code, 0, "{}", b.stderr);
        differs |= b.stdout != a.stdout;
    }
    assert!(differs, "the quantum derives from the world seed");
}

#[test]
fn switch_interval_changes_the_quantum() {
    let src = format!("import sys\nsys.setswitchinterval(0.0005)\n{RACE}");
    let a = script(RACE);
    let b = script(&src);
    assert_eq!(b.exit_code, 0, "{}", b.stderr);
    assert_ne!(a.stdout, b.stdout, "a shorter quantum switches more often");
}

#[test]
fn unsynchronised_counters_lose_updates_reproducibly() {
    let src = r#"
import threading
counter = 0


def bump():
    global counter
    for _ in range(2000):
        value = counter
        counter = value + 1


ts = [threading.Thread(target=bump) for _ in range(4)]
for t in ts:
    t.start()
for t in ts:
    t.join()
print(counter <= 8000, counter == 8000)
"#;
    let a = script(src);
    assert_eq!(a.exit_code, 0, "{}", a.stderr);
    assert_eq!(a.stdout, "True False\n", "a lost-update race is visible");
    assert_eq!(script(src).stdout, a.stdout);
}

#[test]
fn a_failing_thread_is_reported_and_the_program_goes_on() {
    let out = script(
        r#"
import threading


def boom():
    raise ValueError('nope')


t = threading.Thread(target=boom, name='worker')
t.start()
t.join()
print('alive', t.is_alive())
"#,
    );
    assert_eq!(out.exit_code, 0);
    assert_eq!(out.stdout, "alive False\n");
    assert!(
        out.stderr.starts_with("Exception in thread worker:\n"),
        "{}",
        out.stderr
    );
    assert!(out.stderr.ends_with("ValueError: nope\n"), "{}", out.stderr);
    assert!(out.stderr.contains("in boom"), "{}", out.stderr);
}

#[test]
fn a_wait_nothing_can_satisfy_is_a_deadlock() {
    // Nothing else runs, so the wait can never end: the simulated interpreter
    // says so instead of hanging the world.
    let out = script(
        r#"
import threading

print('waiting')
threading.Event().wait()
print('unreachable')
"#,
    );
    assert_eq!(out.exit_code, 1, "{}{}", out.stdout, out.stderr);
    assert_eq!(out.stdout, "waiting\n");
    assert!(out.stderr.contains("deadlock"), "{}", out.stderr);
}

#[test]
fn a_lock_cycle_is_reported_and_does_not_hang() {
    let out = script(
        r#"
import threading
import time

a = threading.Lock()
b = threading.Lock()


def one():
    with a:
        time.sleep(0.01)
        with b:
            print('one')


def two():
    with b:
        time.sleep(0.01)
        with a:
            print('two')


t1 = threading.Thread(target=one, name='one')
t2 = threading.Thread(target=two, name='two')
t1.start()
t2.start()
t1.join()
t2.join()
print('finished')
"#,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    assert!(out.stdout.ends_with("finished\n"), "{}", out.stdout);
    assert!(
        out.stderr.contains("deadlock") && out.stderr.contains("Exception in thread"),
        "{}",
        out.stderr
    );
    // The lock the dead thread held is released as its frames unwind, so the
    // other thread finishes.
    assert!(out.stdout.contains("one") || out.stdout.contains("two"));
}

#[test]
fn threads_sleep_on_the_simulated_clock() {
    // Three threads sleeping 50ms each finish in 50ms of simulated time, not
    // 150ms, and no wall clock is involved.
    let out = script(
        r#"
import threading
import time

start = time.monotonic()
ts = [threading.Thread(target=time.sleep, args=(0.05,)) for _ in range(3)]
for t in ts:
    t.start()
for t in ts:
    t.join()
print(round(time.monotonic() - start, 3))
"#,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "0.05\n");
}

#[test]
fn the_interpreter_waits_for_non_daemon_threads() {
    let out = script(
        r#"
import threading
import time


def late():
    time.sleep(0.02)
    print('late')


threading.Thread(target=late).start()
threading.Thread(target=lambda: time.sleep(100), daemon=True).start()
print('main done')
"#,
    );
    assert_eq!(out.exit_code, 0, "{}", out.stderr);
    assert_eq!(out.stdout, "main done\nlate\n");
}
