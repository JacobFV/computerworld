//! The `re` module over the `cw-regex` backtracking engine.
use super::{new_module, set_fn, set_val};
use crate::builtins::*;
use crate::value::*;
use crate::vm::*;
use cw_regex::{Flags, Flavor, Regex};
use std::cell::RefCell;
use std::rc::Rc;

pub struct PatternObj {
    pub re: Regex,
    pub pattern: Rc<PyStr>,
    pub flags: i64,
    pub class_match: Rc<Class>,
}

pub struct MatchObj {
    pub pattern: Value,
    pub pat: Rc<PatternObj>,
    pub text: Rc<PyStr>,
    pub chars: Rc<Vec<char>>,
    pub slots: Vec<Option<(usize, usize)>>,
    pub pos: usize,
    pub endpos: usize,
}

const I: i64 = 2;
const L: i64 = 4;
const M: i64 = 8;
const S: i64 = 16;
const U: i64 = 32;
const X: i64 = 64;
const A: i64 = 256;

fn flag_repr(flags: i64) -> String {
    let names = [
        (I, "re.IGNORECASE"),
        (L, "re.LOCALE"),
        (M, "re.MULTILINE"),
        (S, "re.DOTALL"),
        (X, "re.VERBOSE"),
        (A, "re.ASCII"),
    ];
    let parts: Vec<&str> = names
        .iter()
        .filter(|(b, _)| flags & b != 0)
        .map(|(_, n)| *n)
        .collect();
    parts.join("|")
}

fn pattern_of(v: &Value) -> Option<Rc<PatternObj>> {
    if let Value::Native(n) = v {
        if let NativeKind::Pattern(p) = &*n.data.borrow() {
            return Some(p.clone());
        }
    }
    None
}
fn match_of(v: &Value) -> Option<Rc<MatchObj>> {
    if let Value::Native(n) = v {
        if let NativeKind::Match(m) = &*n.data.borrow() {
            return Some(m.clone());
        }
    }
    None
}

struct ReClasses {
    pattern: Rc<Class>,
    matchc: Rc<Class>,
}

fn classes(vm: &mut Vm) -> PyResult<ReClasses> {
    let m = vm.modules.borrow().get_str("re");
    let Some(Value::Module(m)) = m else {
        return Err(err("SystemError", "re module missing"));
    };
    let get = |n: &str| match m.dict.borrow().get_str(n) {
        Some(Value::Class(c)) => Ok(c),
        _ => Err(err("SystemError", "re classes missing")),
    };
    Ok(ReClasses {
        pattern: get("Pattern")?,
        matchc: get("Match")?,
    })
}

fn re_error(msg: &str, pattern: &Rc<PyStr>, pos: usize) -> Box<PyErr> {
    err_args(
        "re.error",
        vec![
            Value::str(msg),
            Value::Str(pattern.clone()),
            Value::Int(pos as i64),
        ],
    )
}

pub fn compile(vm: &mut Vm, pat: &Value, flags: i64) -> PyResult<Value> {
    if pattern_of(pat).is_some() {
        if flags != 0 {
            return Err(value_err(
                "cannot process flags argument with a compiled pattern",
            ));
        }
        return Ok(pat.clone());
    }
    let p = match vm.base_value(pat) {
        Value::Str(s) => s,
        Value::Bytes(_) => {
            return Err(type_err(
                "bytes patterns are not supported by this interpreter's re module",
            ))
        }
        other => {
            return Err(type_err(format!(
                "first argument must be string or compiled pattern, not {}",
                vm.type_name(&other)
            )))
        }
    };
    // Cache compiled patterns per (pattern, flags).
    let key = Value::tuple(vec![Value::Str(p.clone()), Value::Int(flags)]);
    let cache = cache_dict(vm);
    if let Some(c) = &cache {
        if let Some(v) = vm.dict_get(c, &key)? {
            return Ok(v);
        }
    }
    let re = Regex::new(
        &p.s,
        Flavor::Python,
        Flags {
            ignore_case: flags & I != 0,
            multiline: flags & M != 0,
            dot_all: flags & S != 0,
            verbose: flags & X != 0,
            ascii: flags & A != 0,
            unicode: false,
        },
    )
    .map_err(|e| re_error(&e.message, &p, e.position))?;
    // Inline flags become part of the pattern's flags.
    let f = re.flags();
    let mut all = flags | U;
    if f.ignore_case {
        all |= I;
    }
    if f.multiline {
        all |= M;
    }
    if f.dot_all {
        all |= S;
    }
    if f.verbose {
        all |= X;
    }
    if f.ascii {
        all = (all | A) & !U;
    }
    let cls = classes(vm)?;
    let obj = Rc::new(PatternObj {
        re,
        pattern: p,
        flags: all,
        class_match: cls.matchc,
    });
    let v = Value::Native(Rc::new(Native {
        class: cls.pattern,
        data: RefCell::new(NativeKind::Pattern(obj)),
    }));
    if let Some(c) = cache {
        if c.borrow().len() < 512 {
            vm.dict_set(&c, key, v.clone())?;
        }
    }
    Ok(v)
}

fn cache_dict(vm: &mut Vm) -> Option<Ref<Dict>> {
    let m = vm.modules.borrow().get_str("re");
    if let Some(Value::Module(m)) = m {
        if let Some(Value::Dict(d)) = m.dict.borrow().get_str("_cache") {
            return Some(d);
        }
    }
    None
}

fn text_arg(vm: &Vm, v: &Value) -> PyResult<Rc<PyStr>> {
    match vm.base_value(v) {
        Value::Str(s) => Ok(s),
        other => Err(type_err(format!(
            "expected string or bytes-like object, got '{}'",
            vm.type_name(&other)
        ))),
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Search,
    Match,
    Full,
}

#[allow(clippy::too_many_arguments)]
fn exec_at(
    vm: &mut Vm,
    pv: &Value,
    p: &Rc<PatternObj>,
    text: &Rc<PyStr>,
    chars: &Rc<Vec<char>>,
    pos: usize,
    endpos: usize,
    mode: Mode,
    forbid: Option<usize>,
) -> PyResult<Option<Value>> {
    let end = endpos.min(chars.len());
    let view = &chars[..end];
    if pos > end {
        return Ok(None);
    }
    let r = p
        .re
        .exec_opts(
            view,
            pos,
            !matches!(mode, Mode::Search),
            matches!(mode, Mode::Full),
            forbid,
        )
        .map_err(|_| {
            err(
                "RecursionError",
                "regular expression matching exceeded the simulated step budget (catastrophic backtracking)",
            )
        })?;
    Ok(r.map(|slots| {
        let m = MatchObj {
            pattern: pv.clone(),
            pat: p.clone(),
            text: text.clone(),
            chars: chars.clone(),
            slots,
            pos,
            endpos: end,
        };
        let _ = vm;
        Value::Native(Rc::new(Native {
            class: p.class_match.clone(),
            data: RefCell::new(NativeKind::Match(Rc::new(m))),
        }))
    }))
}

fn norm_pos(v: Option<&Value>, len: usize, vm: &mut Vm, default: usize) -> PyResult<usize> {
    match v {
        None | Some(Value::None) => Ok(default),
        Some(x) => {
            let i = to_int_arg(vm, x)?;
            Ok(if i < 0 { 0 } else { (i as usize).min(len) })
        }
    }
}

/// All successive matches, with CPython's empty-match stepping.
fn all_matches(
    vm: &mut Vm,
    pv: &Value,
    p: &Rc<PatternObj>,
    text: &Rc<PyStr>,
    pos: usize,
    endpos: usize,
    limit: usize,
) -> PyResult<Vec<Value>> {
    let chars: Rc<Vec<char>> = Rc::new(text.s.chars().collect());
    let mut out = vec![];
    let mut at = pos;
    let mut forbid = None;
    while limit == 0 || out.len() < limit {
        let Some(m) = exec_at(vm, pv, p, text, &chars, at, endpos, Mode::Search, forbid)? else {
            break;
        };
        let (s, e) = match_of(&m).unwrap().slots[0].unwrap();
        out.push(m);
        if s == e {
            forbid = Some(e);
        } else {
            forbid = None;
        }
        at = e;
        if at > chars.len().min(endpos) {
            break;
        }
    }
    Ok(out)
}

fn slot_str(m: &MatchObj, g: usize) -> Option<String> {
    m.slots
        .get(g)
        .copied()
        .flatten()
        .map(|(a, b)| m.chars[a..b].iter().collect())
}
fn slot_val(m: &MatchObj, g: usize, default: &Value) -> Value {
    match slot_str(m, g) {
        Some(s) => Value::string(s),
        None => default.clone(),
    }
}

fn findall_items(vm: &mut Vm, matches: &[Value]) -> Vec<Value> {
    let _ = vm;
    matches
        .iter()
        .map(|m| {
            let m = match_of(m).unwrap();
            let n = m.slots.len() - 1;
            match n {
                0 => slot_val(&m, 0, &Value::str("")),
                1 => slot_val(&m, 1, &Value::str("")),
                _ => Value::tuple((1..=n).map(|g| slot_val(&m, g, &Value::str(""))).collect()),
            }
        })
        .collect()
}

fn do_sub(
    vm: &mut Vm,
    pv: &Value,
    p: &Rc<PatternObj>,
    repl: &Value,
    text: &Rc<PyStr>,
    count: usize,
) -> PyResult<(String, usize)> {
    let matches = all_matches(vm, pv, p, text, 0, usize::MAX, count)?;
    let chars: Vec<char> = text.s.chars().collect();
    let mut out = String::new();
    let mut last = 0;
    let template = match vm.base_value(repl) {
        Value::Str(s) => Some(s),
        _ => None,
    };
    if template.is_none()
        && !matches!(
            repl,
            Value::Func(_)
                | Value::Builtin(_)
                | Value::Method(_)
                | Value::Class(_)
                | Value::Instance(_)
        )
    {
        return Err(type_err(format!(
            "expected str or callable, got '{}'",
            vm.type_name(repl)
        )));
    }
    let n = matches.len();
    for m in &matches {
        let mo = match_of(m).unwrap();
        let (s, e) = mo.slots[0].unwrap();
        out.extend(&chars[last..s]);
        match &template {
            Some(t) => {
                let groups: Vec<Option<String>> =
                    (0..mo.slots.len()).map(|g| slot_str(&mo, g)).collect();
                let r = cw_regex::expand_python_template(&t.s, &groups, p.re.group_names())
                    .map_err(|e| re_error(&e.message, t, e.position))?;
                out.push_str(&r);
            }
            None => {
                let r = vm.call(repl, vec![m.clone()])?;
                match vm.base_value(&r) {
                    Value::Str(s) => out.push_str(&s.s),
                    other => {
                        return Err(type_err(format!(
                            "expected str instance, {} found",
                            vm.type_name(&other)
                        )))
                    }
                }
            }
        }
        last = e;
    }
    out.extend(&chars[last..]);
    Ok((out, n))
}

fn do_split(
    vm: &mut Vm,
    pv: &Value,
    p: &Rc<PatternObj>,
    text: &Rc<PyStr>,
    maxsplit: usize,
) -> PyResult<Value> {
    let matches = all_matches(vm, pv, p, text, 0, usize::MAX, maxsplit)?;
    let chars: Vec<char> = text.s.chars().collect();
    let mut out = vec![];
    let mut last = 0;
    for m in &matches {
        let mo = match_of(m).unwrap();
        let (s, e) = mo.slots[0].unwrap();
        out.push(Value::string(chars[last..s].iter().collect()));
        for g in 1..mo.slots.len() {
            out.push(slot_val(&mo, g, &Value::None));
        }
        last = e;
    }
    out.push(Value::string(chars[last..].iter().collect()));
    Ok(Value::list(out))
}

// ---------- module-level functions ----------

fn flags_arg(vm: &mut Vm, v: Option<&Value>) -> PyResult<i64> {
    match v {
        None => Ok(0),
        Some(x) => to_int_arg(vm, x),
    }
}

fn m_compile(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let p = take_params(&mut a, "compile", &["pattern", "flags"], 1)?;
    let f = flags_arg(vm, p[1].as_ref())?;
    compile(vm, p[0].as_ref().unwrap(), f)
}
fn search_like(vm: &mut Vm, mut a: Args, name: &str, mode: Mode) -> PyResult<Value> {
    let p = take_params(&mut a, name, &["pattern", "string", "flags"], 2)?;
    let f = flags_arg(vm, p[2].as_ref())?;
    let pv = compile(vm, p[0].as_ref().unwrap(), f)?;
    let pat = pattern_of(&pv).unwrap();
    let text = text_arg(vm, p[1].as_ref().unwrap())?;
    let chars = Rc::new(text.s.chars().collect::<Vec<_>>());
    Ok(exec_at(vm, &pv, &pat, &text, &chars, 0, usize::MAX, mode, None)?.unwrap_or(Value::None))
}
fn m_search(vm: &mut Vm, a: Args) -> PyResult<Value> {
    search_like(vm, a, "search", Mode::Search)
}
fn m_match(vm: &mut Vm, a: Args) -> PyResult<Value> {
    search_like(vm, a, "match", Mode::Match)
}
fn m_fullmatch(vm: &mut Vm, a: Args) -> PyResult<Value> {
    search_like(vm, a, "fullmatch", Mode::Full)
}
fn m_findall(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let p = take_params(&mut a, "findall", &["pattern", "string", "flags"], 2)?;
    let f = flags_arg(vm, p[2].as_ref())?;
    let pv = compile(vm, p[0].as_ref().unwrap(), f)?;
    let pat = pattern_of(&pv).unwrap();
    let text = text_arg(vm, p[1].as_ref().unwrap())?;
    let ms = all_matches(vm, &pv, &pat, &text, 0, usize::MAX, 0)?;
    Ok(Value::list(findall_items(vm, &ms)))
}
fn m_finditer(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let p = take_params(&mut a, "finditer", &["pattern", "string", "flags"], 2)?;
    let f = flags_arg(vm, p[2].as_ref())?;
    let pv = compile(vm, p[0].as_ref().unwrap(), f)?;
    let pat = pattern_of(&pv).unwrap();
    let text = text_arg(vm, p[1].as_ref().unwrap())?;
    let ms = all_matches(vm, &pv, &pat, &text, 0, usize::MAX, 0)?;
    Ok(Value::Iter(new_ref(IterObj::List { items: ms, idx: 0 })))
}
fn m_sub_impl(vm: &mut Vm, mut a: Args, name: &str, with_count: bool) -> PyResult<Value> {
    let p = take_params(
        &mut a,
        name,
        &["pattern", "repl", "string", "count", "flags"],
        3,
    )?;
    let count = match &p[3] {
        Some(v) => to_int_arg(vm, v)?.max(0) as usize,
        None => 0,
    };
    let f = flags_arg(vm, p[4].as_ref())?;
    let pv = compile(vm, p[0].as_ref().unwrap(), f)?;
    let pat = pattern_of(&pv).unwrap();
    let text = text_arg(vm, p[2].as_ref().unwrap())?;
    let (s, n) = do_sub(vm, &pv, &pat, p[1].as_ref().unwrap(), &text, count)?;
    Ok(if with_count {
        Value::tuple(vec![Value::string(s), Value::Int(n as i64)])
    } else {
        Value::string(s)
    })
}
fn m_sub(vm: &mut Vm, a: Args) -> PyResult<Value> {
    m_sub_impl(vm, a, "sub", false)
}
fn m_subn(vm: &mut Vm, a: Args) -> PyResult<Value> {
    m_sub_impl(vm, a, "subn", true)
}
fn m_split(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let p = take_params(
        &mut a,
        "split",
        &["pattern", "string", "maxsplit", "flags"],
        2,
    )?;
    let maxsplit = match &p[2] {
        Some(v) => to_int_arg(vm, v)?.max(0) as usize,
        None => 0,
    };
    let f = flags_arg(vm, p[3].as_ref())?;
    let pv = compile(vm, p[0].as_ref().unwrap(), f)?;
    let pat = pattern_of(&pv).unwrap();
    let text = text_arg(vm, p[1].as_ref().unwrap())?;
    do_split(vm, &pv, &pat, &text, maxsplit)
}
fn m_escape(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let s = text_arg(vm, a.args.first().unwrap_or(&Value::None))?;
    Ok(Value::string(cw_regex::escape_python(&s.s)))
}
fn m_purge(vm: &mut Vm, _a: Args) -> PyResult<Value> {
    if let Some(c) = cache_dict(vm) {
        c.borrow_mut().clear();
    }
    Ok(Value::None)
}

// ---------- Pattern methods ----------

fn pat_self(vm: &Vm, a: &Args) -> PyResult<(Value, Rc<PatternObj>)> {
    let v = vm.base_value(&a.args[0]);
    let p = pattern_of(&v).ok_or_else(|| type_err("descriptor requires a 're.Pattern' object"))?;
    Ok((v, p))
}
fn pat_search_like(vm: &mut Vm, mut a: Args, name: &str, mode: Mode) -> PyResult<Value> {
    let (pv, pat) = pat_self(vm, &a)?;
    a.args.remove(0);
    let p = take_params(&mut a, name, &["string", "pos", "endpos"], 1)?;
    let text = text_arg(vm, p[0].as_ref().unwrap())?;
    let chars = Rc::new(text.s.chars().collect::<Vec<_>>());
    let pos = norm_pos(p[1].as_ref(), chars.len(), vm, 0)?;
    let endpos = norm_pos(p[2].as_ref(), chars.len(), vm, chars.len())?;
    Ok(exec_at(vm, &pv, &pat, &text, &chars, pos, endpos, mode, None)?.unwrap_or(Value::None))
}
fn p_search(vm: &mut Vm, a: Args) -> PyResult<Value> {
    pat_search_like(vm, a, "search", Mode::Search)
}
fn p_match(vm: &mut Vm, a: Args) -> PyResult<Value> {
    pat_search_like(vm, a, "match", Mode::Match)
}
fn p_fullmatch(vm: &mut Vm, a: Args) -> PyResult<Value> {
    pat_search_like(vm, a, "fullmatch", Mode::Full)
}
fn p_findall(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let (pv, pat) = pat_self(vm, &a)?;
    a.args.remove(0);
    let p = take_params(&mut a, "findall", &["string", "pos", "endpos"], 1)?;
    let text = text_arg(vm, p[0].as_ref().unwrap())?;
    let n = text.nchars;
    let pos = norm_pos(p[1].as_ref(), n, vm, 0)?;
    let endpos = norm_pos(p[2].as_ref(), n, vm, n)?;
    let ms = all_matches(vm, &pv, &pat, &text, pos, endpos, 0)?;
    Ok(Value::list(findall_items(vm, &ms)))
}
fn p_finditer(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let (pv, pat) = pat_self(vm, &a)?;
    a.args.remove(0);
    let p = take_params(&mut a, "finditer", &["string", "pos", "endpos"], 1)?;
    let text = text_arg(vm, p[0].as_ref().unwrap())?;
    let n = text.nchars;
    let pos = norm_pos(p[1].as_ref(), n, vm, 0)?;
    let endpos = norm_pos(p[2].as_ref(), n, vm, n)?;
    let ms = all_matches(vm, &pv, &pat, &text, pos, endpos, 0)?;
    Ok(Value::Iter(new_ref(IterObj::List { items: ms, idx: 0 })))
}
fn p_sub_impl(vm: &mut Vm, mut a: Args, name: &str, with_count: bool) -> PyResult<Value> {
    let (pv, pat) = pat_self(vm, &a)?;
    a.args.remove(0);
    let p = take_params(&mut a, name, &["repl", "string", "count"], 2)?;
    let count = match &p[2] {
        Some(v) => to_int_arg(vm, v)?.max(0) as usize,
        None => 0,
    };
    let text = text_arg(vm, p[1].as_ref().unwrap())?;
    let (s, n) = do_sub(vm, &pv, &pat, p[0].as_ref().unwrap(), &text, count)?;
    Ok(if with_count {
        Value::tuple(vec![Value::string(s), Value::Int(n as i64)])
    } else {
        Value::string(s)
    })
}
fn p_sub(vm: &mut Vm, a: Args) -> PyResult<Value> {
    p_sub_impl(vm, a, "sub", false)
}
fn p_subn(vm: &mut Vm, a: Args) -> PyResult<Value> {
    p_sub_impl(vm, a, "subn", true)
}
fn p_split(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let (pv, pat) = pat_self(vm, &a)?;
    a.args.remove(0);
    let p = take_params(&mut a, "split", &["string", "maxsplit"], 1)?;
    let maxsplit = match &p[1] {
        Some(v) => to_int_arg(vm, v)?.max(0) as usize,
        None => 0,
    };
    let text = text_arg(vm, p[0].as_ref().unwrap())?;
    do_split(vm, &pv, &pat, &text, maxsplit)
}

// ---------- Match methods ----------

fn m_self(vm: &Vm, a: &Args) -> PyResult<Rc<MatchObj>> {
    match_of(&vm.base_value(&a.args[0]))
        .ok_or_else(|| type_err("descriptor requires a 're.Match' object"))
}
fn group_index(vm: &mut Vm, m: &MatchObj, g: &Value) -> PyResult<usize> {
    let idx = match g {
        Value::Int(i) => {
            if *i < 0 || *i as usize >= m.slots.len() {
                return Err(err("IndexError", "no such group"));
            }
            *i as usize
        }
        Value::Str(s) => m
            .pat
            .re
            .group_names()
            .iter()
            .find(|(n, _)| *n == s.s)
            .map(|x| x.1)
            .ok_or_else(|| err("IndexError", "no such group"))?,
        Value::Bool(b) => *b as usize,
        other => {
            let i = vm
                .index_of(other)
                .map_err(|_| err("IndexError", "no such group"))?;
            if i < 0 || i as usize >= m.slots.len() {
                return Err(err("IndexError", "no such group"));
            }
            i as usize
        }
    };
    Ok(idx)
}
fn mt_group(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let m = m_self(vm, &a)?;
    if a.args.len() <= 1 {
        return Ok(slot_val(&m, 0, &Value::None));
    }
    if a.args.len() == 2 {
        let g = group_index(vm, &m, &a.args[1])?;
        return Ok(slot_val(&m, g, &Value::None));
    }
    let mut out = vec![];
    for g in &a.args[1..] {
        let g = group_index(vm, &m, g)?;
        out.push(slot_val(&m, g, &Value::None));
    }
    Ok(Value::tuple(out))
}
fn mt_getitem(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let m = m_self(vm, &a)?;
    let g = group_index(vm, &m, &a.args[1])?;
    Ok(slot_val(&m, g, &Value::None))
}
fn mt_groups(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let m = m_self(vm, &a)?;
    let default = a
        .kw("default")
        .or_else(|| a.args.get(1).cloned())
        .unwrap_or(Value::None);
    Ok(Value::tuple(
        (1..m.slots.len())
            .map(|g| slot_val(&m, g, &default))
            .collect(),
    ))
}
fn mt_groupdict(vm: &mut Vm, mut a: Args) -> PyResult<Value> {
    let m = m_self(vm, &a)?;
    let default = a
        .kw("default")
        .or_else(|| a.args.get(1).cloned())
        .unwrap_or(Value::None);
    let mut d = Dict::new();
    for (name, idx) in m.pat.re.group_names() {
        d.set_str(name, slot_val(&m, *idx, &default));
    }
    Ok(Value::dict(d))
}
fn span_of(vm: &mut Vm, a: &Args) -> PyResult<(i64, i64)> {
    let m = m_self(vm, a)?;
    let g = match a.args.get(1) {
        Some(g) => group_index(vm, &m, g)?,
        None => 0,
    };
    Ok(match m.slots[g] {
        Some((s, e)) => (s as i64, e as i64),
        None => (-1, -1),
    })
}
fn mt_start(vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(Value::Int(span_of(vm, &a)?.0))
}
fn mt_end(vm: &mut Vm, a: Args) -> PyResult<Value> {
    Ok(Value::Int(span_of(vm, &a)?.1))
}
fn mt_span(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let (s, e) = span_of(vm, &a)?;
    Ok(Value::tuple(vec![Value::Int(s), Value::Int(e)]))
}
fn mt_expand(vm: &mut Vm, a: Args) -> PyResult<Value> {
    let m = m_self(vm, &a)?;
    let t = text_arg(vm, &a.args[1])?;
    let groups: Vec<Option<String>> = (0..m.slots.len()).map(|g| slot_str(&m, g)).collect();
    let r = cw_regex::expand_python_template(&t.s, &groups, m.pat.re.group_names())
        .map_err(|e| re_error(&e.message, &t, e.position))?;
    Ok(Value::string(r))
}

pub fn native_attr(vm: &mut Vm, obj: &Value, name: &str) -> PyResult<Option<Value>> {
    let _ = vm;
    if let Some(p) = pattern_of(obj) {
        return Ok(match name {
            "pattern" => Some(Value::Str(p.pattern.clone())),
            "flags" => Some(Value::Int(p.flags)),
            "groups" => Some(Value::Int(p.re.group_count() as i64)),
            "groupindex" => {
                let mut d = Dict::new();
                for (n, i) in p.re.group_names() {
                    d.set_str(n, Value::Int(*i as i64));
                }
                Some(Value::dict(d))
            }
            _ => None,
        });
    }
    if let Some(m) = match_of(obj) {
        return Ok(match name {
            "string" => Some(Value::Str(m.text.clone())),
            "re" => Some(m.pattern.clone()),
            "pos" => Some(Value::Int(m.pos as i64)),
            "endpos" => Some(Value::Int(m.endpos as i64)),
            "lastindex" => {
                // The last group to close: the participating group with the greatest end.
                let mut best: Option<(usize, usize)> = None;
                for g in 1..m.slots.len() {
                    if let Some((_, e)) = m.slots[g] {
                        if best.is_none_or(|(_, be)| e >= be) {
                            best = Some((g, e));
                        }
                    }
                }
                Some(
                    best.map(|(g, _)| Value::Int(g as i64))
                        .unwrap_or(Value::None),
                )
            }
            "lastgroup" => {
                let mut best: Option<(usize, usize)> = None;
                for g in 1..m.slots.len() {
                    if let Some((_, e)) = m.slots[g] {
                        if best.is_none_or(|(_, be)| e >= be) {
                            best = Some((g, e));
                        }
                    }
                }
                Some(
                    best.and_then(|(g, _)| {
                        m.pat
                            .re
                            .group_names()
                            .iter()
                            .find(|(_, i)| *i == g)
                            .map(|(n, _)| Value::str(n))
                    })
                    .unwrap_or(Value::None),
                )
            }
            "regs" => Some(Value::tuple(
                m.slots
                    .iter()
                    .map(|s| match s {
                        Some((a, b)) => {
                            Value::tuple(vec![Value::Int(*a as i64), Value::Int(*b as i64)])
                        }
                        None => Value::tuple(vec![Value::Int(-1), Value::Int(-1)]),
                    })
                    .collect(),
            )),
            _ => None,
        });
    }
    Ok(None)
}

pub fn native_repr(vm: &mut Vm, v: &Value) -> PyResult<String> {
    if let Some(p) = pattern_of(v) {
        let pr = crate::format::str_repr(&p.pattern.s);
        let shown = p.flags & !U;
        return Ok(if shown != 0 {
            format!("re.compile({pr}, {})", flag_repr(shown))
        } else {
            format!("re.compile({pr})")
        });
    }
    if let Some(m) = match_of(v) {
        let (s, e) = m.slots[0].unwrap_or((0, 0));
        let text: String = m.chars[s..e].iter().collect();
        return Ok(format!(
            "<re.Match object; span=({s}, {e}), match={}>",
            crate::format::str_repr(&text)
        ));
    }
    Ok(vm.default_repr(v))
}

pub fn native_getitem(vm: &mut Vm, obj: &Value, idx: &Value) -> PyResult<Value> {
    if let Some(m) = match_of(obj) {
        let g = group_index(vm, &m, idx)?;
        return Ok(slot_val(&m, g, &Value::None));
    }
    Err(type_err(format!(
        "'{}' object is not subscriptable",
        vm.type_name(obj)
    )))
}

pub fn make(vm: &mut Vm) -> Value {
    let m = new_module("re");
    let pattern = new_class("Pattern", vec![vm.t.object.clone()], Kind::Other, true);
    pattern
        .dict
        .borrow_mut()
        .set_str("__module__", Value::str("re"));
    let matchc = new_class("Match", vec![vm.t.object.clone()], Kind::Other, true);
    matchc
        .dict
        .borrow_mut()
        .set_str("__module__", Value::str("re"));
    for (n, f) in [
        ("search", p_search as NativeFn),
        ("match", p_match),
        ("fullmatch", p_fullmatch),
        ("findall", p_findall),
        ("finditer", p_finditer),
        ("sub", p_sub),
        ("subn", p_subn),
        ("split", p_split),
    ] {
        add_fn(&pattern, n, f);
    }
    for (n, f) in [
        ("group", mt_group as NativeFn),
        ("__getitem__", mt_getitem),
        ("groups", mt_groups),
        ("groupdict", mt_groupdict),
        ("start", mt_start),
        ("end", mt_end),
        ("span", mt_span),
        ("expand", mt_expand),
    ] {
        add_fn(&matchc, n, f);
    }
    set_val(&m, "Pattern", Value::Class(pattern));
    set_val(&m, "Match", Value::Class(matchc));
    for (n, f) in [
        ("compile", m_compile as NativeFn),
        ("search", m_search),
        ("match", m_match),
        ("fullmatch", m_fullmatch),
        ("findall", m_findall),
        ("finditer", m_finditer),
        ("sub", m_sub),
        ("subn", m_subn),
        ("split", m_split),
        ("escape", m_escape),
        ("purge", m_purge),
    ] {
        set_fn(&m, n, f);
    }
    for (n, v) in [
        ("I", I),
        ("IGNORECASE", I),
        ("L", L),
        ("LOCALE", L),
        ("M", M),
        ("MULTILINE", M),
        ("S", S),
        ("DOTALL", S),
        ("U", U),
        ("UNICODE", U),
        ("X", X),
        ("VERBOSE", X),
        ("A", A),
        ("ASCII", A),
        ("NOFLAG", 0),
    ] {
        set_val(&m, n, Value::Int(v));
    }
    set_val(&m, "_cache", Value::dict(Dict::new()));
    let error = new_class("error", vec![vm.t.exc("Exception")], Kind::Exception, false);
    error
        .dict
        .borrow_mut()
        .set_str("__module__", Value::str("re"));
    vm.t.exceptions.insert("re.error", error.clone());
    set_val(&m, "error", Value::Class(error.clone()));
    set_val(&m, "PatternError", Value::Class(error));
    super::sys::exec_snippet(
        vm,
        &m,
        r#"
def _error_init(self, msg, pattern=None, pos=None):
    Exception.__init__(self, msg)
    self.msg = msg.rsplit(' at position ', 1)[0] if pos is not None else msg
    self.pattern = pattern
    self.pos = pos
    if pattern is not None and pos is not None:
        self.lineno = pattern.count('\n', 0, pos) + 1
        self.colno = pos - pattern.rfind('\n', 0, pos)
    else:
        self.lineno = self.colno = None
error.__init__ = _error_init
del _error_init
"#,
    );
    Value::Module(m)
}
