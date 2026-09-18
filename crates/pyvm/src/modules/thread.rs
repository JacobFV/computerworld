//! `_thread`: threads, locks and the waiting primitive `threading`, `queue`,
//! `socket` and `concurrent.futures` are built on. The scheduler lives in
//! [`crate::sched`].
use super::{new_module, set_fn, set_val};
use crate::builtins::*;
use crate::sched::{lock_of, lock_value, with_lock, Status};
use crate::value::*;
use crate::vm::*;

const TIMEOUT_MAX: f64 = 9223372036.0;

fn timeout_deadline(vm: &mut Vm, blocking: bool, timeout: Option<f64>) -> PyResult<Option<i64>> {
    if !blocking {
        return Ok(Some(vm.now_micros()));
    }
    match timeout {
        None => Ok(None),
        Some(t) if t < 0.0 => Ok(None),
        Some(t) => Ok(Some(vm.now_micros() + (t * 1e6).ceil() as i64)),
    }
}

fn float_arg(vm: &mut Vm, v: Option<&Value>) -> PyResult<Option<f64>> {
    match v {
        None | Some(Value::None) => Ok(None),
        Some(v) => Ok(Some(crate::bfuncs::float_from(vm, v)?)),
    }
}

/// `lock.acquire(blocking=True, timeout=-1)`.
fn acquire(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let lock = lock_of(&a.args[0]).ok_or_else(|| type_err("not a lock"))?;
    let blocking = match a.kw("blocking").or_else(|| a.args.get(1).cloned()) {
        Some(v) => vm.truthy(&v)?,
        None => true,
    };
    let timeout = match a.kw("timeout").or_else(|| a.args.get(2).cloned()) {
        Some(v) => Some(crate::bfuncs::float_from(vm, &v)?),
        None => None,
    };
    if let Some(t) = timeout {
        if t > TIMEOUT_MAX {
            return Err(value_err("timeout value is too large"));
        }
        if !blocking && t >= 0.0 {
            return Err(value_err("can't specify a timeout for a non-blocking call"));
        }
    }
    let me = vm.sched.as_ref().map(|s| s.current_id()).unwrap_or(1);
    // A reentrant lock the running thread already holds is taken again.
    let taken = with_lock(&lock, |d| {
        if d.reentrant && d.locked && d.owner == me {
            d.count += 1;
            return true;
        }
        if !d.locked {
            d.locked = true;
            d.owner = me;
            d.count = 1;
            return true;
        }
        false
    });
    if taken {
        return Ok(Value::Bool(true));
    }
    if !blocking {
        return Ok(Value::Bool(false));
    }
    let deadline = timeout_deadline(vm, blocking, timeout)?;
    let l = lock.clone();
    let taken = lock.clone();
    // Parking gives the lock to the waiting thread when it becomes free; the
    // nested fallback has to take it here.
    let parked = vm.can_park();
    let r = vm.wait_for(crate::sched::Wait::Lock(l), deadline, move |_vm| {
        Ok(with_lock(&taken, |d| !d.locked))
    })?;
    if !parked && matches!(r, Value::Bool(true)) {
        with_lock(&lock, |d| {
            d.locked = true;
            d.owner = me;
            d.count = 1;
        });
    }
    Ok(r)
}

fn release(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let lock = lock_of(&a.args[0]).ok_or_else(|| type_err("not a lock"))?;
    let me = vm.sched.as_ref().map(|s| s.current_id()).unwrap_or(1);
    let r = with_lock(&lock, |d| {
        if !d.locked {
            return Err("release unlocked lock");
        }
        if d.reentrant && d.owner != me {
            return Err("cannot release un-acquired lock");
        }
        d.count -= 1;
        if d.count == 0 {
            d.locked = false;
            d.owner = 0;
        }
        Ok(())
    });
    match r {
        Ok(()) => Ok(Value::None),
        Err(m) => Err(err("RuntimeError", m)),
    }
}

pub fn make(vm: &mut Vm) -> Value {
    let m = new_module("_thread");
    set_val(&m, "TIMEOUT_MAX", Value::Float(TIMEOUT_MAX));
    let exc = new_class("error", vec![vm.t.exc("RuntimeError")], Kind::Object, false);
    exc.dict
        .borrow_mut()
        .set_str("__module__", Value::str("_thread"));
    set_val(&m, "error", Value::Class(exc));
    set_fn(&m, "allocate_lock", |vm, _| Ok(lock_value(vm, false)));
    set_fn(&m, "RLock", |vm, _| Ok(lock_value(vm, true)));
    set_fn(&m, "acquire", acquire);
    set_fn(&m, "release", release);
    set_fn(&m, "locked", |_, a| {
        let lock = lock_of(&a.args[0]).ok_or_else(|| type_err("not a lock"))?;
        Ok(Value::Bool(with_lock(&lock, |d| d.locked)))
    });
    set_fn(&m, "owner", |_, a| {
        let lock = lock_of(&a.args[0]).ok_or_else(|| type_err("not a lock"))?;
        Ok(with_lock(&lock, |d| {
            if d.locked {
                Value::Int(d.owner as i64)
            } else {
                Value::None
            }
        }))
    });
    set_fn(&m, "get_ident", |vm, _| {
        Ok(Value::Int(
            vm.sched.as_ref().map(|s| s.current_id()).unwrap_or(1) as i64,
        ))
    });
    set_fn(&m, "get_native_id", |vm, _| {
        Ok(Value::Int(
            vm.host.pid() as i64 * 1000
                + vm.sched.as_ref().map(|s| s.current_id()).unwrap_or(1) as i64,
        ))
    });
    set_fn(&m, "stack_size", |_, _| Ok(Value::Int(0)));
    set_fn(&m, "interrupt_main", |_, _| Ok(Value::None));
    set_fn(&m, "_count", |vm, _| {
        let n = vm
            .sched
            .as_ref()
            .map(|s| {
                s.threads
                    .iter()
                    .filter(|t| t.status != Status::Finished)
                    .count()
            })
            .unwrap_or(1);
        Ok(Value::Int(n as i64 - 1))
    });
    // start_new_thread(target, args, kwargs=None, name=None, daemon=False, obj=None)
    set_fn(&m, "start_new_thread", |vm, mut a| {
        let target = a.args[0].clone();
        let args = a.args.get(1).cloned().unwrap_or(Value::tuple(vec![]));
        let kwargs = match a.args.get(2) {
            Some(Value::Dict(d)) => Value::Dict(d.clone()),
            _ => Value::dict(Dict::new()),
        };
        let name = match a.kw("name").or_else(|| a.args.get(3).cloned()) {
            Some(Value::Str(s)) => Some(s.s.clone()),
            _ => None,
        };
        let daemon = match a.kw("daemon").or_else(|| a.args.get(4).cloned()) {
            Some(v) => vm.truthy(&v)?,
            None => false,
        };
        let obj = a
            .kw("obj")
            .or_else(|| a.args.get(5).cloned())
            .unwrap_or(Value::None);
        let id = vm.spawn_thread(target, args, kwargs, name, daemon, obj)?;
        Ok(Value::Int(id as i64))
    });
    set_fn(&m, "join", |vm, a| {
        let id = to_int_arg(vm, &a.args[0])? as u64;
        let timeout = float_arg(vm, a.args.get(1))?;
        let deadline = timeout_deadline(vm, true, timeout)?;
        vm.join_thread(id, deadline)
    });
    set_fn(&m, "is_alive", |vm, a| {
        let id = to_int_arg(vm, &a.args[0])? as u64;
        let alive = match vm.sched.as_ref() {
            Some(s) => s
                .index_of(id)
                .is_some_and(|i| s.threads[i].status != Status::Finished),
            // Before any thread starts, only the main thread exists.
            None => id == 1,
        };
        Ok(Value::Bool(alive))
    });
    set_fn(&m, "_failed", |vm, a| {
        let id = to_int_arg(vm, &a.args[0])? as u64;
        let failed = vm
            .sched
            .as_ref()
            .and_then(|s| s.index_of(id).map(|i| s.threads[i].failed))
            .unwrap_or(false);
        Ok(Value::Bool(failed))
    });
    set_fn(&m, "_enumerate", |vm, _| {
        let mut out = vec![];
        if let Some(s) = vm.sched.as_ref() {
            for t in &s.threads {
                if t.status != Status::Finished {
                    out.push(Value::tuple(vec![
                        Value::Int(t.id as i64),
                        Value::string(t.name.clone()),
                        Value::Bool(t.daemon),
                        t.obj.clone(),
                    ]));
                }
            }
        }
        Ok(Value::list(out))
    });
    set_fn(&m, "_set_name", |vm, a| {
        let id = to_int_arg(vm, &a.args[0])? as u64;
        let name = to_str_arg(vm, &a.args[1], "name")?.s.clone();
        if let Some(s) = vm.sched.as_mut() {
            if let Some(i) = s.index_of(id) {
                s.threads[i].name = name;
            }
        }
        Ok(Value::None)
    });
    set_fn(&m, "_set_daemon", |vm, a| {
        let id = to_int_arg(vm, &a.args[0])? as u64;
        let daemon = vm.truthy(&a.args[1])?;
        if let Some(s) = vm.sched.as_mut() {
            if let Some(i) = s.index_of(id) {
                s.threads[i].daemon = daemon;
            }
        }
        Ok(Value::None)
    });
    set_fn(&m, "_current", |vm, _| {
        let (id, name, daemon, obj) = match vm.sched.as_ref() {
            Some(s) => {
                let t = &s.threads[s.current];
                (t.id, t.name.clone(), t.daemon, t.obj.clone())
            }
            None => (1, "MainThread".to_string(), false, Value::None),
        };
        Ok(Value::tuple(vec![
            Value::Int(id as i64),
            Value::string(name),
            Value::Bool(daemon),
            obj,
        ]))
    });
    set_fn(&m, "_set_object", |vm, a| {
        let id = to_int_arg(vm, &a.args[0])? as u64;
        let obj = a.args[1].clone();
        if let Some(s) = vm.sched.as_mut() {
            if let Some(i) = s.index_of(id) {
                s.threads[i].obj = obj;
            }
        }
        Ok(Value::None)
    });
    // _wait_until(predicate, timeout=None): runs other threads until truthy.
    set_fn(&m, "_wait_until", |vm, a| {
        let pred = a.args[0].clone();
        let timeout = float_arg(vm, a.args.get(1))?;
        let deadline = timeout_deadline(vm, true, timeout)?;
        // An already-true condition does not give up the thread's turn.
        let v = vm.call(&pred, vec![])?;
        if vm.truthy(&v)? {
            return Ok(Value::Bool(true));
        }
        let check = pred.clone();
        vm.wait_for(crate::sched::Wait::Predicate(pred), deadline, move |vm| {
            let v = vm.call(&check, vec![])?;
            vm.truthy(&v)
        })
    });
    set_fn(&m, "_yield", |vm, _| {
        // Give the other threads a turn without waiting for anything.
        let done = std::cell::Cell::new(false);
        vm.block_until(
            move |_| {
                let first = !done.get();
                done.set(true);
                Ok(!first)
            },
            None,
        )?;
        Ok(Value::None)
    });
    set_fn(&m, "_sleep", |vm, a| {
        let s = crate::bfuncs::float_from(vm, &a.args[0])?;
        vm.thread_sleep(s.max(0.0))
    });
    set_fn(&m, "_setswitchinterval", |vm, a| {
        let s = crate::bfuncs::float_from(vm, &a.args[0])?;
        if s <= 0.0 {
            return Err(value_err("switch interval must be strictly positive"));
        }
        vm.start_scheduler();
        if let Some(sc) = vm.sched.as_mut() {
            sc.set_switch_interval(s);
        }
        Ok(Value::None)
    });
    set_fn(&m, "_getswitchinterval", |vm, _| {
        Ok(Value::Float(
            vm.sched
                .as_ref()
                .map(|s| s.switch_interval)
                .unwrap_or(crate::sched::DEFAULT_SWITCH_INTERVAL),
        ))
    });
    Value::Module(m)
}
