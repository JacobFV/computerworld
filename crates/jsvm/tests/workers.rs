//! `worker_threads` in the simulated interpreter: how the contexts take turns,
//! what they share, and what a program sees when they cannot go on.
//!
//! The Node-for-Node output of ordinary worker programs is checked by the
//! conformance corpus (`tests/programs/workers.js`, `atomics.js`); these tests
//! are about the properties the simulation itself has to keep.
use cw_script_host::{memory::MemoryHost, Invocation, Outcome, ScriptHost};

fn script_on(host: &mut MemoryHost, src: &str) -> Outcome {
    host.write_file("/home/user/main.js", src.as_bytes(), false)
        .unwrap();
    cw_jsvm::run(
        host,
        &Invocation {
            args: vec!["main.js".into()],
            ..Default::default()
        },
    )
}

fn script(src: &str) -> Outcome {
    script_on(&mut MemoryHost::default(), src)
}

#[test]
fn threads_interleave_the_same_way_in_every_run() {
    let src = r#"
      const { Worker } = require('worker_threads');
      const sab = new SharedArrayBuffer(8);
      const cell = new Int32Array(sab);
      const body = `const { workerData, parentPort, threadId } = require('worker_threads');
        const cell = new Int32Array(workerData);
        for (let i = 0; i < 50; i++) Atomics.add(cell, 0, threadId);
        parentPort.postMessage(Atomics.load(cell, 0));`;
      const seen = [];
      for (let i = 0; i < 4; i++) {
        const w = new Worker(body, { eval: true, workerData: sab });
        w.on('message', (m) => seen.push(m));
        w.on('exit', () => {
          if (seen.length === 4) console.log(seen.join(','), Atomics.load(cell, 0));
        });
      }
    "#;
    let first = script(src);
    assert_eq!(first.exit_code, 0, "{}", first.stderr);
    for _ in 0..3 {
        let again = script(src);
        assert_eq!(again.stdout, first.stdout);
        assert_eq!(again.elapsed_micros, first.elapsed_micros);
    }
    // 1+2+3+4 threads, fifty turns each, on one shared counter.
    assert!(
        first.stdout.trim().ends_with("500"),
        "counter ended at {}",
        first.stdout
    );
}

#[test]
fn a_worker_writes_to_the_terminal_through_its_parent() {
    let out = script(
        r#"
          const { Worker } = require('worker_threads');
          console.log('main first');
          const w = new Worker('console.log("from the worker"); console.error("and an error");', {
            eval: true,
          });
          w.on('exit', () => console.log('main last'));
        "#,
    );
    assert_eq!(out.stdout, "main first\nfrom the worker\nmain last\n");
    assert_eq!(out.stderr, "and an error\n");
}

#[test]
fn a_worker_exit_does_not_end_the_program() {
    let out = script(
        r#"
          const { Worker } = require('worker_threads');
          const w = new Worker('process.exit(9)', { eval: true });
          w.on('exit', (code) => console.log('worker left with', code));
        "#,
    );
    assert_eq!(out.stdout, "worker left with 9\n");
    assert_eq!(out.exit_code, 0);
}

#[test]
fn a_shared_buffer_is_shared_and_a_message_is_copied() {
    let out = script(
        r#"
          const { Worker } = require('worker_threads');
          const shared = new SharedArrayBuffer(4);
          const copied = { n: 1 };
          const w = new Worker(
            `const { workerData, parentPort } = require('worker_threads');
             new Uint8Array(workerData.shared)[0] = 42;
             workerData.copied.n = 99;
             parentPort.postMessage(workerData.copied.n);`,
            { eval: true, workerData: { shared, copied } }
          );
          w.on('message', (m) => {
            console.log('worker saw', m, 'here', copied.n, new Uint8Array(shared)[0]);
          });
        "#,
    );
    assert_eq!(out.stdout, "worker saw 99 here 1 42\n");
}

#[test]
fn a_worker_left_waiting_lets_the_program_finish() {
    // Node would keep the process alive for ever here; the simulation ends the
    // worker once nothing anywhere can move, so a world never wedges.
    let out = script(
        r#"
          const { Worker } = require('worker_threads');
          const w = new Worker(
            `require('worker_threads').parentPort.on('message', () => {});`,
            { eval: true }
          );
          w.on('exit', (code) => console.log('ended with', code));
        "#,
    );
    assert_eq!(out.stdout, "ended with 0\n");
    assert_eq!(out.exit_code, 0);
}

#[test]
fn a_wait_that_nothing_can_end_is_reported() {
    let out = script(
        r#"
          const cell = new Int32Array(new SharedArrayBuffer(4));
          console.log('waiting');
          Atomics.wait(cell, 0, 0);
          console.log('never');
        "#,
    );
    assert_eq!(out.stdout, "waiting\n");
    // Node would hang here for ever; the simulation stops the program the way
    // it stops one that runs out of steps.
    assert_eq!(out.exit_code, cw_jsvm::TIMEOUT_EXIT);
    assert!(
        out.stderr.contains("every thread is waiting"),
        "unexpected report: {}",
        out.stderr
    );
}

#[test]
fn a_worker_can_start_a_worker() {
    let out = script(
        r#"
          const { Worker } = require('worker_threads');
          const inner = `const { parentPort } = require('worker_threads');
            parentPort.postMessage('from the inner worker');`;
          const outer = `const { Worker, parentPort } = require('worker_threads');
            const w = new Worker(${JSON.stringify(inner)}, { eval: true });
            w.on('message', (m) => parentPort.postMessage(m + ', through the outer one'));`;
          const w = new Worker(outer, { eval: true });
          w.on('message', (m) => console.log(m));
        "#,
    );
    assert_eq!(out.stdout, "from the inner worker, through the outer one\n");
}

#[test]
fn starting_a_worker_takes_simulated_time() {
    let quick = script("console.log('nothing')");
    let with_worker = script(
        r#"
          const { Worker } = require('worker_threads');
          const w = new Worker('0', { eval: true });
          w.on('exit', () => console.log('nothing'));
        "#,
    );
    assert!(
        with_worker.elapsed_micros >= quick.elapsed_micros + 10_000,
        "a worker start should cost time: {} vs {}",
        with_worker.elapsed_micros,
        quick.elapsed_micros
    );
}
