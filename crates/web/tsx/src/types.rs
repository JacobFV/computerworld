//! Operations on static types (`cw_ui::ir::Ty`) for the checker in `lower`.
//!
//! The checker is not tsc: it does not reject programs tsc would reject (the author's
//! own toolchain does that). It computes the type of every expression well enough to
//! pick the native operation (an array's `includes` or a string's), to type the
//! parameters of callbacks from their context, and to refuse what the runtime cannot
//! represent (`any`, values of unknown shape).

use cw_ui::ir::Ty;

/// `a | b`, flattened, without duplicates.
pub fn union(a: Ty, b: Ty) -> Ty {
    if a == b {
        return a;
    }
    if matches!(a, Ty::Unknown) {
        return b;
    }
    if matches!(b, Ty::Unknown) {
        return a;
    }
    // Two object shapes merge into one (fields missing on one side are optional),
    // which is how an array of records written as literals reads.
    if let (Ty::Object(fa), Ty::Object(fb)) = (&a, &b) {
        let mut out: Vec<(String, Ty, bool)> = Vec::new();
        for (n, t, o) in fa {
            match fb.iter().find(|(m, _, _)| m == n) {
                Some((_, u, p)) => out.push((n.clone(), union(t.clone(), u.clone()), *o || *p)),
                None => out.push((n.clone(), t.clone(), true)),
            }
        }
        for (n, t, _) in fb {
            if !fa.iter().any(|(m, _, _)| m == n) {
                out.push((n.clone(), t.clone(), true));
            }
        }
        return Ty::Object(out);
    }
    if let (Ty::Array(x), Ty::Array(y)) = (&a, &b) {
        return Ty::Array(Box::new(union((**x).clone(), (**y).clone())));
    }
    if let (Ty::Tuple(x), Ty::Tuple(y)) = (&a, &b) {
        if x.len() == y.len() {
            return Ty::Tuple(
                x.iter()
                    .zip(y)
                    .map(|(p, q)| union(p.clone(), q.clone()))
                    .collect(),
            );
        }
    }
    let mut items = Vec::new();
    for t in [a, b] {
        match t {
            Ty::Union(ts) => {
                for t in ts {
                    push_unique(&mut items, t);
                }
            }
            t => push_unique(&mut items, t),
        }
    }
    if items.len() == 1 {
        items.pop().unwrap()
    } else {
        Ty::Union(items)
    }
}

fn push_unique(items: &mut Vec<Ty>, t: Ty) {
    if !items.contains(&t) {
        items.push(t);
    }
}

pub fn union_all(ts: impl IntoIterator<Item = Ty>) -> Ty {
    ts.into_iter().fold(Ty::Unknown, union)
}

/// The type without `null` and `undefined` (what `x!`, `x?.` and `x ?? y` see).
pub fn non_null(t: &Ty) -> Ty {
    match t {
        Ty::Union(ts) => union_all(
            ts.iter()
                .filter(|t| !matches!(t, Ty::Null | Ty::Undefined | Ty::Void))
                .cloned(),
        ),
        t => t.clone(),
    }
}

/// Literal types widened to their primitive (what a `let` or a state cell holds).
pub fn widen(t: &Ty) -> Ty {
    match t {
        Ty::Lit(_) => Ty::String,
        Ty::NumLit(_) => Ty::Number,
        Ty::Union(ts) if ts.iter().all(|t| matches!(t, Ty::Lit(_) | Ty::String)) => Ty::String,
        Ty::Union(ts) if ts.iter().all(|t| matches!(t, Ty::NumLit(_) | Ty::Number)) => Ty::Number,
        t => t.clone(),
    }
}

pub fn is_stringy(t: &Ty) -> bool {
    match t {
        Ty::String | Ty::Lit(_) => true,
        Ty::Union(ts) => ts.iter().all(is_stringy),
        _ => false,
    }
}

pub fn is_numeric(t: &Ty) -> bool {
    match t {
        Ty::Number | Ty::NumLit(_) => true,
        Ty::Union(ts) => ts.iter().all(is_numeric),
        _ => false,
    }
}

pub fn is_array(t: &Ty) -> bool {
    match non_null(t) {
        Ty::Array(_) | Ty::Tuple(_) => true,
        Ty::Union(ts) => !ts.is_empty() && ts.iter().all(is_array),
        _ => false,
    }
}

/// The element type of an array or tuple.
pub fn element(t: &Ty) -> Option<Ty> {
    match non_null(t) {
        Ty::Array(e) => Some(*e),
        Ty::Set(e) => Some(*e),
        Ty::Map(k, v) => Some(Ty::Tuple(vec![*k, *v])),
        Ty::Tuple(ts) => Some(union_all(ts)),
        Ty::Union(ts) => {
            let mut out = Ty::Unknown;
            for t in &ts {
                out = union(out, element(t)?);
            }
            Some(out)
        }
        _ => None,
    }
}

/// The type of `t.name`, or `None` when `t` has no such property.
pub fn property(t: &Ty, name: &str) -> Option<Ty> {
    match t {
        Ty::Object(fields) => fields.iter().find(|(n, _, _)| n == name).map(|(_, t, o)| {
            if *o {
                union(t.clone(), Ty::Undefined)
            } else {
                t.clone()
            }
        }),
        Ty::Dict(v) => Some(union((**v).clone(), Ty::Undefined)),
        Ty::Array(_) | Ty::Tuple(_) if name == "length" => Some(Ty::Number),
        Ty::String | Ty::Lit(_) if name == "length" => Some(Ty::Number),
        Ty::Ref(inner) if name == "current" => Some((**inner).clone()),
        Ty::Event => match name {
            "target" | "currentTarget" => Some(Ty::DomNode),
            "key" | "code" | "type" => Some(Ty::String),
            "shiftKey" | "ctrlKey" | "altKey" | "metaKey" | "defaultPrevented" | "repeat" => {
                Some(Ty::Boolean)
            }
            "clientX" | "clientY" | "button" | "detail" | "deltaX" | "deltaY" => Some(Ty::Number),
            _ => None,
        },
        Ty::DomNode => match name {
            "value" | "id" | "tagName" | "name" | "type" | "textContent" | "className" => {
                Some(Ty::String)
            }
            "checked" | "disabled" => Some(Ty::Boolean),
            "offsetWidth" | "offsetHeight" | "scrollTop" | "scrollLeft" | "selectionStart"
            | "selectionEnd" | "valueAsNumber" | "scrollHeight" | "scrollWidth"
            | "clientHeight" | "clientWidth" => Some(Ty::Number),
            _ => None,
        },
        Ty::Set(_) | Ty::Map(..) if name == "size" => Some(Ty::Number),
        Ty::Error => match name {
            "message" | "name" | "stack" => Some(Ty::String),
            _ => None,
        },
        Ty::Regex => match name {
            "source" | "flags" => Some(Ty::String),
            "global" => Some(Ty::Boolean),
            "lastIndex" => Some(Ty::Number),
            _ => None,
        },
        Ty::Response | Ty::CwResponse => match name {
            "ok" => Some(Ty::Boolean),
            "status" => Some(Ty::Number),
            "statusText" | "url" => Some(Ty::String),
            "headers" => Some(Ty::Headers),
            // A fetch's body is a stream; a `cw.fetch` reply's is its text.
            "body" if matches!(t, Ty::CwResponse) => Some(Ty::String),
            _ => None,
        },
        Ty::Union(ts) => {
            let mut out: Option<Ty> = None;
            for t in ts {
                if matches!(t, Ty::Null | Ty::Undefined | Ty::Void) {
                    continue;
                }
                let p = property(t, name)?;
                out = Some(match out {
                    Some(o) => union(o, p),
                    None => p,
                });
            }
            out
        }
        _ => None,
    }
}

/// The type of `t[k]` for a key of type `k`.
pub fn index(t: &Ty, k: &Ty) -> Option<Ty> {
    match non_null(t) {
        Ty::Union(ts) => {
            let mut out = Ty::Unknown;
            for t in &ts {
                out = union(out, index(t, k)?);
            }
            Some(out)
        }
        Ty::Array(e) => Some(union(*e, Ty::Undefined)),
        Ty::Tuple(ts) => match k {
            Ty::NumLit(n) if *n >= 0.0 && (*n as usize) < ts.len() => Some(ts[*n as usize].clone()),
            _ => Some(union_all(ts)),
        },
        Ty::Dict(v) => Some(*v),
        o @ Ty::Object(_) => match k {
            Ty::Lit(name) => property(&o, name),
            Ty::Union(ks) => {
                let mut out = Ty::Unknown;
                for k in ks {
                    out = union(out, index(&o, k)?);
                }
                Some(out)
            }
            _ => match &o {
                Ty::Object(fields) => Some(union_all(fields.iter().map(|(_, t, _)| t.clone()))),
                _ => None,
            },
        },
        Ty::String | Ty::Lit(_) => Some(Ty::String),
        _ => None,
    }
}

/// Whether a value of type `t` can be rendered by React as a child.
pub fn is_renderable(t: &Ty) -> bool {
    match t {
        Ty::Number
        | Ty::String
        | Ty::Boolean
        | Ty::Null
        | Ty::Undefined
        | Ty::Void
        | Ty::Node
        | Ty::Lit(_)
        | Ty::NumLit(_) => true,
        Ty::Array(e) => is_renderable(e),
        Ty::Tuple(ts) | Ty::Union(ts) => ts.iter().all(is_renderable),
        _ => false,
    }
}

/// Human-readable type, for diagnostics.
pub fn show(t: &Ty) -> String {
    match t {
        Ty::Number => "number".into(),
        Ty::String => "string".into(),
        Ty::Boolean => "boolean".into(),
        Ty::Null => "null".into(),
        Ty::Undefined => "undefined".into(),
        Ty::Void => "void".into(),
        Ty::Array(e) => format!("{}[]", show(e)),
        Ty::Tuple(ts) => format!("[{}]", ts.iter().map(show).collect::<Vec<_>>().join(", ")),
        Ty::Object(fs) => format!(
            "{{ {} }}",
            fs.iter()
                .map(|(n, t, o)| format!("{n}{}: {}", if *o { "?" } else { "" }, show(t)))
                .collect::<Vec<_>>()
                .join("; ")
        ),
        Ty::Dict(v) => format!("Record<string, {}>", show(v)),
        Ty::Union(ts) => ts.iter().map(show).collect::<Vec<_>>().join(" | "),
        Ty::Function(ps, r) => format!(
            "({}) => {}",
            ps.iter().map(show).collect::<Vec<_>>().join(", "),
            show(r)
        ),
        Ty::Node => "ReactNode".into(),
        Ty::Ref(t) => format!("RefObject<{}>", show(t)),
        Ty::Setter(t) => format!("Dispatch<SetStateAction<{}>>", show(t)),
        Ty::Dispatch(t) => format!("Dispatch<{}>", show(t)),
        Ty::Context(t) => format!("Context<{}>", show(t)),
        Ty::Event => "Event".into(),
        Ty::DomNode => "HTMLElement".into(),
        Ty::Promise(t) => format!("Promise<{}>", show(t)),
        Ty::Response => "Response".into(),
        Ty::CwResponse => "CwResponse".into(),
        Ty::Headers => "Headers".into(),
        Ty::Regex => "RegExp".into(),
        Ty::Error => "Error".into(),
        Ty::Set(t) => format!("Set<{}>", show(t)),
        Ty::Map(k, v) => format!("Map<{}, {}>", show(k), show(v)),
        Ty::Lit(s) => format!("{s:?}"),
        Ty::NumLit(n) => format!("{n}"),
        Ty::Unknown => "unknown".into(),
        Ty::Param(n) => n.clone(),
    }
}

/// `keyof t`: the names of an object type's members.
pub fn keys_of(t: &Ty) -> Ty {
    match non_null(t) {
        Ty::Object(fs) => union_all(fs.into_iter().map(|(n, _, _)| Ty::Lit(n))),
        Ty::Dict(_) => Ty::String,
        Ty::Array(_) | Ty::Tuple(_) => Ty::Number,
        _ => Ty::Unknown,
    }
}

/// `a & b`: the members of both.
pub fn intersect(a: Ty, b: Ty) -> Ty {
    match (a, b) {
        (Ty::Object(mut fa), Ty::Object(fb)) => {
            for (n, t, o) in fb {
                match fa.iter_mut().find(|(m, _, _)| *m == n) {
                    Some(f) => f.2 = f.2 && o,
                    None => fa.push((n, t, o)),
                }
            }
            Ty::Object(fa)
        }
        (Ty::Unknown, b) => b,
        (a, _) => a,
    }
}

/// The names a type of literal keys lists (`'a' | 'b'`).
pub fn literal_names(t: &Ty) -> Vec<String> {
    match t {
        Ty::Lit(s) => vec![s.clone()],
        Ty::Union(ts) => ts.iter().flat_map(literal_names).collect(),
        _ => Vec::new(),
    }
}

/// `t` with each type parameter replaced by its binding (`unknown` when unbound).
pub fn subst(t: &Ty, b: &std::collections::BTreeMap<String, Ty>) -> Ty {
    let s = |t: &Ty| subst(t, b);
    let bx = |t: &Ty| Box::new(subst(t, b));
    match t {
        Ty::Param(n) => b.get(n).cloned().unwrap_or(Ty::Unknown),
        Ty::Array(e) => Ty::Array(bx(e)),
        Ty::Tuple(ts) => Ty::Tuple(ts.iter().map(s).collect()),
        Ty::Object(fs) => Ty::Object(fs.iter().map(|(n, t, o)| (n.clone(), s(t), *o)).collect()),
        Ty::Dict(v) => Ty::Dict(bx(v)),
        Ty::Union(ts) => union_all(ts.iter().map(s)),
        Ty::Function(ps, r) => Ty::Function(ps.iter().map(s).collect(), bx(r)),
        Ty::Ref(t) => Ty::Ref(bx(t)),
        Ty::Setter(t) => Ty::Setter(bx(t)),
        Ty::Dispatch(t) => Ty::Dispatch(bx(t)),
        Ty::Context(t) => Ty::Context(bx(t)),
        Ty::Promise(t) => Ty::Promise(bx(t)),
        Ty::Set(t) => Ty::Set(bx(t)),
        Ty::Map(k, v) => Ty::Map(bx(k), bx(v)),
        t => t.clone(),
    }
}

/// Binds the type parameters in `p` (a parameter's declared type) from `a` (the
/// argument's type), structurally; a parameter keeps its first binding.
pub fn unify(p: &Ty, a: &Ty, b: &mut std::collections::BTreeMap<String, Ty>) {
    match (p, a) {
        (Ty::Param(n), a) => {
            if !matches!(a, Ty::Unknown) && !b.contains_key(n) {
                b.insert(n.clone(), a.clone());
            }
        }
        (Ty::Union(ps), a) => {
            // `T | null` against `X | null`: T is what is left of the argument.
            let params: Vec<&Ty> = ps.iter().filter(|t| mentions_param(t)).collect();
            if params.len() == 1 {
                let rest = match a {
                    Ty::Union(xs) => union_all(xs.iter().filter(|x| !ps.contains(x)).cloned()),
                    x if ps.contains(x) => return,
                    x => x.clone(),
                };
                unify(params[0], &rest, b);
            }
        }
        (Ty::Array(p), Ty::Array(a))
        | (Ty::Dict(p), Ty::Dict(a))
        | (Ty::Ref(p), Ty::Ref(a))
        | (Ty::Setter(p), Ty::Setter(a))
        | (Ty::Dispatch(p), Ty::Dispatch(a))
        | (Ty::Context(p), Ty::Context(a))
        | (Ty::Promise(p), Ty::Promise(a))
        | (Ty::Set(p), Ty::Set(a)) => unify(p, a, b),
        (Ty::Array(p), Ty::Tuple(xs)) => unify(p, &union_all(xs.iter().cloned()), b),
        (Ty::Map(pk, pv), Ty::Map(ak, av)) => {
            unify(pk, ak, b);
            unify(pv, av, b);
        }
        (Ty::Tuple(ps), Ty::Tuple(xs)) => {
            for (p, x) in ps.iter().zip(xs) {
                unify(p, x, b);
            }
        }
        (Ty::Object(ps), Ty::Object(xs)) => {
            for (n, p, _) in ps {
                if let Some((_, x, _)) = xs.iter().find(|(m, _, _)| m == n) {
                    unify(p, x, b);
                }
            }
        }
        (Ty::Function(ps, pr), Ty::Function(xs, xr)) => {
            for (p, x) in ps.iter().zip(xs) {
                unify(p, x, b);
            }
            unify(pr, xr, b);
        }
        _ => {}
    }
}

/// Whether `t` mentions a type parameter.
pub fn mentions_param(t: &Ty) -> bool {
    match t {
        Ty::Param(_) => true,
        Ty::Array(e)
        | Ty::Dict(e)
        | Ty::Ref(e)
        | Ty::Setter(e)
        | Ty::Dispatch(e)
        | Ty::Context(e)
        | Ty::Promise(e)
        | Ty::Set(e) => mentions_param(e),
        Ty::Map(k, v) => mentions_param(k) || mentions_param(v),
        Ty::Tuple(ts) | Ty::Union(ts) => ts.iter().any(mentions_param),
        Ty::Object(fs) => fs.iter().any(|(_, t, _)| mentions_param(t)),
        Ty::Function(ps, r) => ps.iter().any(mentions_param) || mentions_param(r),
        _ => false,
    }
}
