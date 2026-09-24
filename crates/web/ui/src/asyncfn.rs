//! Async functions.
//!
//! Calling an async function runs its body synchronously up to its first `await`,
//! as JavaScript does, and returns a promise for its result. An `await` stops the
//! walk: the awaited value's promise gets reactions that resume the task (with the
//! value, or throwing the rejection) from the microtask queue. The walk is a stack of
//! continuations over the function's statements (sequences, loops, `switch`es,
//! `try`s), so an `await` may sit inside any of them; statements that contain no
//! `await` run on the ordinary interpreter. The compiler places `await` only where
//! the walk can stop: a whole `let` initialiser, expression statement, assignment
//! right side or `return` value.
//!
//! A task in flight is not part of a snapshot: a restored app carries on without the
//! continuations of async functions that were waiting when it was saved.

use std::cell::RefCell;
use std::rc::Rc;

use crate::interp::{Flow, Frame};
use crate::ir::{Expr, LValue, Module, Pattern, Stmt};
use crate::runtime::*;
use crate::value::*;

/// Extends a borrow of the module to `'static`.
///
/// SAFETY: every `Task` holds an `Rc<Module>` for as long as it holds references
/// into it, and a `Module` is never mutated after the runtime is built, so the
/// references stay valid for the task's whole life and never outlive it.
fn extend<T: ?Sized>(r: &T) -> &'static T {
    unsafe { &*(r as *const T) }
}

#[derive(Debug)]
enum Bind {
    Let(&'static Pattern),
    Assign(&'static LValue),
    Discard,
    Return,
}

#[derive(Debug)]
enum Completion {
    Normal,
    Return(Value),
    Throw(Value),
    Break,
    Continue,
}

#[derive(Debug)]
enum TryPhase {
    Block,
    Handler,
    Finally(Completion),
}

#[derive(Debug)]
enum Cont {
    Seq {
        stmts: &'static [Stmt],
        next: usize,
    },
    ForOf {
        pat: &'static Pattern,
        items: Vec<Value>,
        next: usize,
        body: &'static [Stmt],
    },
    For {
        test: Option<&'static Expr>,
        update: Option<&'static Expr>,
        body: &'static [Stmt],
    },
    Switch {
        bodies: Vec<&'static [Stmt]>,
        next: usize,
    },
    Try {
        param: Option<&'static Pattern>,
        handler: Option<&'static [Stmt]>,
        finalizer: Option<&'static [Stmt]>,
        phase: TryPhase,
    },
}

/// A suspended (or running) async function call.
#[derive(Debug)]
pub struct Task {
    _module: Rc<Module>,
    frame: Frame,
    stack: Vec<Cont>,
    result: Rc<RefCell<Promise>>,
    bind: Option<Bind>,
}

enum Input {
    Start,
    Value(Value),
    Throw(Value),
}

/// Whether a statement has an `await` anywhere the walk must stop at.
fn has_await(s: &Stmt) -> bool {
    let top = |e: &Expr| match e {
        Expr::Await(_) => true,
        Expr::Assign(_, None, v) => matches!(**v, Expr::Await(_)),
        _ => false,
    };
    match s {
        Stmt::Let(_, Some(e)) | Stmt::Expr(e) | Stmt::Return(Some(e)) => top(e),
        Stmt::If(_, a, b) => a.iter().any(has_await) || b.iter().any(has_await),
        Stmt::Block(b) | Stmt::ForOf(_, _, b) => b.iter().any(has_await),
        Stmt::For { body, .. } => body.iter().any(has_await),
        Stmt::Switch(_, cases) => cases.iter().any(|(_, b)| b.iter().any(has_await)),
        Stmt::Try {
            block,
            handler,
            finalizer,
            ..
        } => {
            block.iter().any(has_await)
                || handler.iter().flatten().any(has_await)
                || finalizer.iter().flatten().any(has_await)
        }
        _ => false,
    }
}

/// Calls async function `func` with its frame already bound: runs to the first
/// `await` and returns the promise of its result.
pub(crate) fn start(rt: &mut Runtime, module: Rc<Module>, func: u32, frame: Frame) -> Value {
    let body = extend(module.functions[func as usize].body.as_slice());
    let result = crate::interp::new_promise();
    let task = Task {
        _module: module,
        frame,
        stack: vec![Cont::Seq {
            stmts: body,
            next: 0,
        }],
        result: result.clone(),
        bind: None,
    };
    let cell = Rc::new(RefCell::new(Some(task)));
    run(rt, &cell, Input::Start);
    Value::Promise(result)
}

/// Resumes a task after its `await` settled.
pub(crate) fn resume(rt: &mut Runtime, cell: &Rc<RefCell<Option<Task>>>, v: Value, throw: bool) {
    run(
        rt,
        cell,
        if throw {
            Input::Throw(v)
        } else {
            Input::Value(v)
        },
    );
}

fn completion(r: R<Flow>) -> Completion {
    match r {
        Ok(Flow::Normal) | Err(Throw::Short) => Completion::Normal,
        Ok(Flow::Return(v)) => Completion::Return(v),
        Ok(Flow::Break) => Completion::Break,
        Ok(Flow::Continue) => Completion::Continue,
        Err(Throw::Value(v)) => Completion::Throw(v),
    }
}

fn run(rt: &mut Runtime, cell: &Rc<RefCell<Option<Task>>>, input: Input) {
    let Some(mut task) = cell.borrow_mut().take() else {
        return;
    };
    let mut pending: Option<Completion> = None;
    match input {
        Input::Start => {}
        Input::Value(v) => match task.bind.take() {
            Some(Bind::Let(p)) => {
                if let Err(Throw::Value(e)) = rt.bind(&mut task.frame, p, v) {
                    pending = Some(Completion::Throw(e));
                }
            }
            Some(Bind::Assign(lv)) => {
                if let Err(Throw::Value(e)) = rt.write_lvalue(&mut task.frame, lv, v) {
                    pending = Some(Completion::Throw(e));
                }
            }
            Some(Bind::Return) => pending = Some(Completion::Return(v)),
            Some(Bind::Discard) | None => {}
        },
        Input::Throw(v) => {
            task.bind = None;
            pending = Some(Completion::Throw(v));
        }
    }
    loop {
        if let Some(c) = pending.take() {
            match unwind(rt, &mut task, c) {
                Some(done) => return finish(rt, task, done),
                None => continue,
            }
        }
        let Some(top) = task.stack.last_mut() else {
            return finish(rt, task, Completion::Normal);
        };
        let stmt = match top {
            Cont::Seq { stmts, next } => {
                if *next < stmts.len() {
                    let s = &stmts[*next];
                    *next += 1;
                    s
                } else {
                    task.stack.pop();
                    pending = child_done(rt, &mut task);
                    continue;
                }
            }
            // Loops, switches and trys only ever see their children finish.
            _ => {
                pending = child_done(rt, &mut task);
                continue;
            }
        };
        if !has_await(stmt) {
            match completion(rt.exec_stmt(&mut task.frame, stmt)) {
                Completion::Normal => {}
                c => pending = Some(c),
            }
            continue;
        }
        let awaited = match stmt {
            Stmt::Let(p, Some(Expr::Await(e))) => Some((e, Bind::Let(extend(p)))),
            Stmt::Expr(Expr::Await(e)) => Some((e, Bind::Discard)),
            Stmt::Expr(Expr::Assign(lv, None, v)) => match &**v {
                Expr::Await(e) => Some((e, Bind::Assign(extend(&**lv)))),
                _ => None,
            },
            Stmt::Return(Some(Expr::Await(e))) => Some((e, Bind::Return)),
            _ => None,
        };
        if let Some((e, bind)) = awaited {
            match rt.eval(&mut task.frame, e) {
                Ok(v) => {
                    let p = match v {
                        Value::Promise(p) => p,
                        other => {
                            let p = crate::interp::new_promise();
                            rt.resolve_promise(&p, other);
                            p
                        }
                    };
                    task.bind = Some(bind);
                    *cell.borrow_mut() = Some(task);
                    let ok = Value::Native(Rc::new(NativeFn::Resume {
                        task: cell.clone(),
                        throw: false,
                    }));
                    let bad = Value::Native(Rc::new(NativeFn::Resume {
                        task: cell.clone(),
                        throw: true,
                    }));
                    rt.promise_then(&p, ReactionKind::Then, ok, bad);
                    return;
                }
                Err(Throw::Value(v)) => pending = Some(Completion::Throw(v)),
                Err(Throw::Short) => {}
            }
            continue;
        }
        pending = enter(rt, &mut task, stmt);
    }
}

/// Starts a compound statement that contains an `await`.
fn enter(rt: &mut Runtime, task: &mut Task, stmt: &Stmt) -> Option<Completion> {
    let throw = |t: Throw| match t {
        Throw::Value(v) => Some(Completion::Throw(v)),
        Throw::Short => None,
    };
    match stmt {
        Stmt::If(c, a, b) => match rt.eval(&mut task.frame, c) {
            Ok(v) => {
                let branch = if v.truthy() { a } else { b };
                task.stack.push(Cont::Seq {
                    stmts: extend(branch.as_slice()),
                    next: 0,
                });
                None
            }
            Err(t) => throw(t),
        },
        Stmt::Block(b) => {
            task.stack.push(Cont::Seq {
                stmts: extend(b.as_slice()),
                next: 0,
            });
            None
        }
        Stmt::ForOf(p, it, body) => {
            let items = match rt.eval(&mut task.frame, it).and_then(|v| rt.iterate(&v)) {
                Ok(i) => i,
                Err(t) => return throw(t),
            };
            task.stack.push(Cont::ForOf {
                pat: extend(p),
                items,
                next: 0,
                body: extend(body.as_slice()),
            });
            next_iteration(rt, task)
        }
        Stmt::For {
            init,
            test,
            update,
            body,
        } => {
            for s in init {
                match completion(rt.exec_stmt(&mut task.frame, s)) {
                    Completion::Normal => {}
                    c => return Some(c),
                }
            }
            task.stack.push(Cont::For {
                test: test.as_ref().map(extend),
                update: update.as_ref().map(extend),
                body: extend(body.as_slice()),
            });
            loop_test(rt, task, false)
        }
        Stmt::Switch(d, cases) => {
            let v = match rt.eval(&mut task.frame, d) {
                Ok(v) => v,
                Err(t) => return throw(t),
            };
            let mut start = None;
            for (i, (t, _)) in cases.iter().enumerate() {
                if let Some(t) = t {
                    match rt.eval(&mut task.frame, t) {
                        Ok(x) if strict_equals(&v, &x) => {
                            start = Some(i);
                            break;
                        }
                        Ok(_) => {}
                        Err(t) => return throw(t),
                    }
                }
            }
            let start = start.or_else(|| cases.iter().position(|(t, _)| t.is_none()));
            let start = start?;
            let bodies: Vec<&'static [Stmt]> = cases[start..]
                .iter()
                .map(|(_, b)| extend(b.as_slice()))
                .collect();
            task.stack.push(Cont::Switch { bodies, next: 0 });
            None
        }
        Stmt::Try {
            block,
            param,
            handler,
            finalizer,
        } => {
            task.stack.push(Cont::Try {
                param: param.as_ref().map(extend),
                handler: handler.as_ref().map(|h| extend(h.as_slice())),
                finalizer: finalizer.as_ref().map(|f| extend(f.as_slice())),
                phase: TryPhase::Block,
            });
            task.stack.push(Cont::Seq {
                stmts: extend(block.as_slice()),
                next: 0,
            });
            None
        }
        other => match completion(rt.exec_stmt(&mut task.frame, other)) {
            Completion::Normal => None,
            c => Some(c),
        },
    }
}

/// A `for...of` on top of the stack: binds the next item and pushes the body, or
/// finishes.
fn next_iteration(rt: &mut Runtime, task: &mut Task) -> Option<Completion> {
    let Some(Cont::ForOf {
        pat,
        items,
        next,
        body,
    }) = task.stack.last_mut()
    else {
        return None;
    };
    if *next >= items.len() {
        task.stack.pop();
        return None;
    }
    let v = items[*next].clone();
    *next += 1;
    let (pat, body) = (*pat, *body);
    if let Err(Throw::Value(e)) = rt.bind(&mut task.frame, pat, v) {
        return Some(Completion::Throw(e));
    }
    task.stack.push(Cont::Seq {
        stmts: body,
        next: 0,
    });
    None
}

/// A `for`/`while` on top of the stack: runs the update (after an iteration), then
/// the test, and pushes the body or finishes.
fn loop_test(rt: &mut Runtime, task: &mut Task, after_body: bool) -> Option<Completion> {
    let Some(Cont::For { test, update, body }) = task.stack.last() else {
        return None;
    };
    let (test, update, body) = (*test, *update, *body);
    if after_body {
        if let Some(u) = update {
            if let Err(Throw::Value(e)) = rt.eval(&mut task.frame, u) {
                return Some(Completion::Throw(e));
            }
        }
    }
    let go = match test {
        Some(t) => match rt.eval(&mut task.frame, t) {
            Ok(v) => v.truthy(),
            Err(Throw::Value(e)) => return Some(Completion::Throw(e)),
            Err(Throw::Short) => false,
        },
        None => true,
    };
    if go {
        task.stack.push(Cont::Seq {
            stmts: body,
            next: 0,
        });
    } else {
        task.stack.pop();
    }
    None
}

/// The top of the stack saw its child sequence finish normally.
fn child_done(rt: &mut Runtime, task: &mut Task) -> Option<Completion> {
    match task.stack.last_mut()? {
        Cont::Seq { .. } => None,
        Cont::ForOf { .. } => next_iteration(rt, task),
        Cont::For { .. } => loop_test(rt, task, true),
        Cont::Switch { bodies, next } => {
            if *next < bodies.len() {
                let b = bodies[*next];
                *next += 1;
                task.stack.push(Cont::Seq { stmts: b, next: 0 });
            } else {
                task.stack.pop();
            }
            None
        }
        Cont::Try {
            finalizer, phase, ..
        } => match phase {
            TryPhase::Block | TryPhase::Handler => {
                match finalizer {
                    Some(f) => {
                        let f = *f;
                        *phase = TryPhase::Finally(Completion::Normal);
                        task.stack.push(Cont::Seq { stmts: f, next: 0 });
                    }
                    None => {
                        task.stack.pop();
                    }
                }
                None
            }
            TryPhase::Finally(_) => {
                let Some(Cont::Try {
                    phase: TryPhase::Finally(then),
                    ..
                }) = task.stack.pop()
                else {
                    unreachable!()
                };
                match then {
                    Completion::Normal => None,
                    c => Some(c),
                }
            }
        },
    }
}

/// Propagates an abrupt completion up the stack. `Some` when it leaves the
/// function; `None` when a loop, switch or `try` took it over.
fn unwind(rt: &mut Runtime, task: &mut Task, c: Completion) -> Option<Completion> {
    let mut c = c;
    loop {
        let Some(top) = task.stack.last_mut() else {
            return Some(c);
        };
        match (top, &c) {
            (Cont::Seq { .. }, _) => {
                task.stack.pop();
            }
            (Cont::ForOf { .. } | Cont::For { .. }, Completion::Break) => {
                task.stack.pop();
                return None;
            }
            (Cont::ForOf { .. }, Completion::Continue) => {
                return next_iteration(rt, task).and_then(|c| unwind(rt, task, c));
            }
            (Cont::For { .. }, Completion::Continue) => {
                return loop_test(rt, task, true).and_then(|c| unwind(rt, task, c));
            }
            (Cont::Switch { .. }, Completion::Break) => {
                task.stack.pop();
                return None;
            }
            (
                Cont::Try {
                    phase: TryPhase::Block,
                    handler: Some(h),
                    param,
                    ..
                },
                Completion::Throw(v),
            ) => {
                let (h, param, v) = (*h, *param, v.clone());
                if let Some(Cont::Try { phase, .. }) = task.stack.last_mut() {
                    *phase = TryPhase::Handler;
                }
                if let Some(p) = param {
                    if let Err(Throw::Value(e)) = rt.bind(&mut task.frame, p, v) {
                        c = Completion::Throw(e);
                        continue;
                    }
                }
                task.stack.push(Cont::Seq { stmts: h, next: 0 });
                return None;
            }
            (
                Cont::Try {
                    phase: TryPhase::Block | TryPhase::Handler,
                    finalizer: Some(f),
                    ..
                },
                _,
            ) => {
                let f = *f;
                if let Some(Cont::Try { phase, .. }) = task.stack.last_mut() {
                    *phase = TryPhase::Finally(std::mem::replace(&mut c, Completion::Normal));
                }
                task.stack.push(Cont::Seq { stmts: f, next: 0 });
                return None;
            }
            // The finally block itself ended abruptly: that completion wins.
            _ => {
                task.stack.pop();
            }
        }
    }
}

fn finish(rt: &mut Runtime, task: Task, c: Completion) {
    match c {
        Completion::Return(v) => rt.resolve_promise(&task.result, v),
        Completion::Throw(v) => rt.reject_promise(&task.result, v),
        _ => rt.resolve_promise(&task.result, Value::Undefined),
    }
}
