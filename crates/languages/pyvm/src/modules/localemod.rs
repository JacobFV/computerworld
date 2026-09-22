//! `_locale` and `_zoneinfo`: the data the `locale` and `zoneinfo` modules of
//! the simulated interpreter stand on.
//!
//! The locale table is generated from the host's glibc (see
//! `tools/generate_locales.py`), the zones from the IANA database (`cw-tz`), so
//! a program sees what CPython on such a machine would see — without a
//! `/usr/share/locale` or `/usr/share/zoneinfo` to read.
use super::{new_module, set_fn, set_val};
use crate::builtins::*;
use crate::locale_data::{LocaleData, LOCALES};
use crate::value::*;
use crate::vm::*;

/// The recorded locale a name refers to. `C`/`POSIX` are the default, which the
/// interpreter answers for itself.
pub fn find(name: &str) -> Option<&'static LocaleData> {
    let n = name.trim();
    if n.is_empty() || n == "C" || n == "POSIX" || n == "C.UTF-8" {
        return None;
    }
    let base = n.split('.').next().unwrap_or(n);
    LOCALES
        .iter()
        .find(|l| l.name.eq_ignore_ascii_case(n))
        .or_else(|| {
            LOCALES
                .iter()
                .find(|l| l.name.split('.').next() == Some(base))
        })
        .or_else(|| {
            // `de` or `de-DE` name the same locale as `de_DE.UTF-8`.
            let tag = base.replace('_', "-");
            LOCALES
                .iter()
                .find(|l| l.tag.eq_ignore_ascii_case(&tag))
                .or_else(|| {
                    let lang = tag.split('-').next().unwrap_or(&tag).to_ascii_lowercase();
                    LOCALES
                        .iter()
                        .find(|l| l.tag.split('-').next() == Some(lang.as_str()))
                })
        })
}

fn strs(vm: &mut Vm, items: &[&'static str]) -> Value {
    let _ = vm;
    Value::list(items.iter().map(|s| Value::str(s)).collect())
}

fn locale_dict(vm: &mut Vm, l: &'static LocaleData) -> Value {
    let d = new_ref(Dict::new());
    {
        let mut m = d.borrow_mut();
        m.set_str("name", Value::str(l.name));
        m.set_str("tag", Value::str(l.tag));
        m.set_str("from_cldr", Value::Bool(l.from_cldr));
        m.set_str("d_t_fmt", Value::str(l.d_t_fmt));
        m.set_str("d_fmt", Value::str(l.d_fmt));
        m.set_str("t_fmt", Value::str(l.t_fmt));
        m.set_str("t_fmt_ampm", Value::str(l.t_fmt_ampm));
        m.set_str("codeset", Value::str(l.codeset));
    }
    let days = strs(vm, l.days);
    let abdays = strs(vm, l.abdays);
    let months = strs(vm, l.months);
    let abmonths = strs(vm, l.abmonths);
    let am_pm = strs(vm, l.am_pm);
    let conv = new_ref(Dict::new());
    {
        let mut c = conv.borrow_mut();
        for (k, v) in l.conv_text {
            c.set_str(k, Value::str(v));
        }
        for (k, v) in l.conv_num {
            c.set_str(k, Value::Int(*v));
        }
        c.set_str(
            "grouping",
            Value::list(l.grouping.iter().map(|g| Value::Int(*g)).collect()),
        );
        c.set_str(
            "mon_grouping",
            Value::list(l.mon_grouping.iter().map(|g| Value::Int(*g)).collect()),
        );
    }
    {
        let mut m = d.borrow_mut();
        m.set_str("days", days);
        m.set_str("abdays", abdays);
        m.set_str("months", months);
        m.set_str("abmonths", abmonths);
        m.set_str("am_pm", am_pm);
        m.set_str("conv", Value::Dict(conv));
    }
    Value::Dict(d)
}

pub fn make(_vm: &mut Vm) -> Value {
    let m = new_module("_locale");
    set_val(
        &m,
        "locales",
        Value::list(LOCALES.iter().map(|l| Value::str(l.name)).collect()),
    );
    // `_locale.data(name)`: the recorded locale, or None for `C`.
    set_fn(&m, "data", |vm, a| {
        let name = to_str_arg(vm, &a.args[0], "data() argument")?;
        match find(&name.s) {
            Some(l) => Ok(locale_dict(vm, l)),
            None => Ok(Value::None),
        }
    });
    // The locale the interpreter formats time in, which `setlocale` moves.
    set_fn(&m, "set_time_locale", |vm, a| {
        let name = to_str_arg(vm, &a.args[0], "set_time_locale() argument")?;
        vm.time_locale = find(&name.s).map(|l| l.name.to_string());
        Ok(Value::None)
    });
    set_fn(&m, "get_time_locale", |vm, _| {
        Ok(match &vm.time_locale {
            Some(n) => Value::string(n.clone()),
            None => Value::None,
        })
    });
    Value::Module(m)
}

/// The names `time.strftime` uses right now: the C locale's, or the recorded
/// ones of whatever `locale.setlocale(LC_TIME, ...)` chose.
pub fn current_time_locale(vm: &Vm) -> Option<&'static LocaleData> {
    vm.time_locale.as_deref().and_then(find)
}

// ------------------------------------------------------------------ zoneinfo

pub fn make_zoneinfo(vm: &mut Vm) -> Value {
    let _ = vm;
    let m = new_module("_zoneinfo");
    set_fn(&m, "available", |_vm, _a| {
        Ok(Value::list(cw_tz::zones().map(Value::str).collect()))
    });
    set_fn(&m, "exists", |vm, a| {
        let key = to_str_arg(vm, &a.args[0], "exists() argument")?;
        Ok(Value::Bool(cw_tz::exists(&key.s)))
    });
    // `offset(key, utc_seconds)` -> (seconds east of UTC, abbreviation, is dst)
    set_fn(&m, "offset", |vm, a| {
        let key = to_str_arg(vm, &a.args[0], "offset() argument 1")?;
        let at = to_int_arg(vm, &a.args[1])?;
        match cw_tz::offset_at(&key.s, at) {
            Some(o) => Ok(Value::tuple(vec![
                Value::Int(o.seconds as i64),
                Value::str(o.abbreviation),
                Value::Bool(o.is_dst),
            ])),
            None => Ok(Value::None),
        }
    });
    // `local(key, local_seconds, fold)`: the offset a wall clock reading has.
    set_fn(&m, "local", |vm, a| {
        let key = to_str_arg(vm, &a.args[0], "local() argument 1")?;
        let at = to_int_arg(vm, &a.args[1])?;
        let fold = match a.args.get(2) {
            Some(v) => vm.truthy(v)?,
            None => false,
        };
        match cw_tz::offset_for_local(&key.s, at, fold) {
            Some(o) => Ok(Value::tuple(vec![
                Value::Int(o.seconds as i64),
                Value::str(o.abbreviation),
                Value::Bool(o.is_dst),
            ])),
            None => Ok(Value::None),
        }
    });
    Value::Module(m)
}
