//! CSSOM: computed style declarations, the inline `style` declaration, stylesheets
//! and their rule lists, `CSS.supports`/`CSS.escape` and media query matching.

use cw_jsvm::value::{Args, HostHooks, JsResult, Key, Obj, Value};
use cw_jsvm::vm::Vm;

use super::dom::set_attribute_value;
use super::{
    arg_node, arg_num, arg_str, dom_exception, handle_obj, inner, node_array, opt_node, str_array,
    string_val,
};
use crate::css::parser::{component_values_to_string, parse_declaration_block, parse_stylesheet};
use crate::css::{Declaration, KeyframeSelector, Origin, Rule, SupportsCondition};
use crate::dom::NodeId;
use crate::script::inner::{Inner, SheetOwner};
use crate::style::properties::LonghandId;
use crate::Strictness;

// ---------------------------------------------------------------- property names

/// `backgroundColor` to `background-color`; `cssFloat` to `float`; kebab passes.
pub fn css_name(prop: &str) -> String {
    if prop == "cssFloat" {
        return "float".into();
    }
    if prop.starts_with("--") || prop.contains('-') {
        return prop.to_owned();
    }
    let mut out = String::with_capacity(prop.len() + 4);
    let mut chars = prop.chars().peekable();
    // A leading upper-case letter is a vendor prefix (`WebkitTransform`).
    if let Some(c) = chars.peek().copied() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
            chars.next();
        }
    }
    for c in chars {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn is_member(name: &str) -> bool {
    matches!(
        name,
        "length"
            | "cssText"
            | "parentRule"
            | "getPropertyValue"
            | "setProperty"
            | "removeProperty"
            | "getPropertyPriority"
            | "item"
            | "constructor"
            | "then"
            | "toString"
            | "valueOf"
            | "toJSON"
    ) || name.starts_with("__")
        || name.starts_with('%')
}

// ---------------------------------------------------------------- computed style

fn computed_get(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<Value>> {
    let (Key::Str(s), Some(id)) = (k, o.host_id()) else {
        return Ok(None);
    };
    if is_member(s) {
        return Ok(None);
    }
    if let Some(idx) = k.array_index() {
        return Ok(LonghandId::all()
            .nth(idx as usize)
            .map(|l| Value::str(l.name())));
    }
    let pseudo = match o.host_slot(1) {
        Some(Value::Str(p)) => p.to_string(),
        _ => String::new(),
    };
    let name = css_name(s);
    Ok(Some(string_val(computed_value(
        vm,
        NodeId(id),
        &pseudo,
        &name,
    ))))
}
fn computed_set(_vm: &mut Vm, _o: &Obj, k: &Key, _v: &Value) -> JsResult<Option<bool>> {
    Ok(if k.as_str().map(is_member).unwrap_or(true) {
        None
    } else {
        Some(false)
    })
}
fn computed_delete(_vm: &mut Vm, _o: &Obj, _k: &Key) -> JsResult<Option<bool>> {
    Ok(None)
}
fn computed_keys(_vm: &mut Vm, _o: &Obj) -> JsResult<Vec<Key>> {
    Ok((0..LonghandId::all().count())
        .map(|i| Key::str(&i.to_string()))
        .collect())
}
pub static COMPUTED_HOOKS: HostHooks = HostHooks {
    class: "CSSStyleDeclaration",
    get: computed_get,
    set: computed_set,
    delete: computed_delete,
    keys: computed_keys,
};

/// The computed value string of a property, flushing pending style work.
pub fn computed_value(vm: &mut Vm, n: NodeId, pseudo: &str, name: &str) -> String {
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    i.ensure_styles();
    let style = match pseudo {
        "::before" | ":before" => i.styles.before(n),
        "::after" | ":after" => i.styles.after(n),
        "::marker" | ":marker" => i.styles.marker(n),
        _ => i.styles.get(n),
    };
    let Some(style) = style else {
        return String::new();
    };
    if let Some(v) = style.serialize(name) {
        return v;
    }
    // Shorthands and unknown names: rebuild the common shorthands from longhands.
    shorthand_value(style, name).unwrap_or_default()
}

fn shorthand_value(s: &crate::style::ComputedStyle, name: &str) -> Option<String> {
    let four = |a: &str, b: &str, c: &str, d: &str| -> Option<String> {
        let v: Vec<String> = [a, b, c, d]
            .iter()
            .map(|p| s.serialize(p).unwrap_or_default())
            .collect();
        if v.iter().any(|x| x.is_empty()) {
            return None;
        }
        Some(if v[0] == v[1] && v[1] == v[2] && v[2] == v[3] {
            v[0].clone()
        } else if v[0] == v[2] && v[1] == v[3] {
            format!("{} {}", v[0], v[1])
        } else if v[1] == v[3] {
            format!("{} {} {}", v[0], v[1], v[2])
        } else {
            v.join(" ")
        })
    };
    match name {
        "margin" => four("margin-top", "margin-right", "margin-bottom", "margin-left"),
        "padding" => four(
            "padding-top",
            "padding-right",
            "padding-bottom",
            "padding-left",
        ),
        "border-width" => four(
            "border-top-width",
            "border-right-width",
            "border-bottom-width",
            "border-left-width",
        ),
        "border-color" => four(
            "border-top-color",
            "border-right-color",
            "border-bottom-color",
            "border-left-color",
        ),
        "border-style" => four(
            "border-top-style",
            "border-right-style",
            "border-bottom-style",
            "border-left-style",
        ),
        "border-radius" => four(
            "border-top-left-radius",
            "border-top-right-radius",
            "border-bottom-right-radius",
            "border-bottom-left-radius",
        ),
        "inset" => four("top", "right", "bottom", "left"),
        "border" | "border-top" | "border-right" | "border-bottom" | "border-left" => {
            let side = if name == "border" { "top" } else { &name[7..] };
            Some(format!(
                "{} {} {}",
                s.serialize(&format!("border-{side}-width"))?,
                s.serialize(&format!("border-{side}-style"))?,
                s.serialize(&format!("border-{side}-color"))?
            ))
        }
        "font" => Some(format!(
            "{} {}px {}",
            s.serialize("font-weight")?,
            s.font.size.to_f64_px(),
            s.serialize("font-family")?
        )),
        "background" => s.serialize("background-color"),
        "overflow" => {
            let x = s.serialize("overflow-x")?;
            let y = s.serialize("overflow-y")?;
            Some(if x == y { x } else { format!("{x} {y}") })
        }
        "gap" => {
            let r = s.serialize("row-gap")?;
            let c = s.serialize("column-gap")?;
            Some(if r == c { r } else { format!("{r} {c}") })
        }
        "flex" => Some(format!(
            "{} {} {}",
            s.serialize("flex-grow")?,
            s.serialize("flex-shrink")?,
            s.serialize("flex-basis")?
        )),
        "flex-flow" => Some(format!(
            "{} {}",
            s.serialize("flex-direction")?,
            s.serialize("flex-wrap")?
        )),
        "text-decoration" => s.serialize("text-decoration-line"),
        "list-style" => Some(format!(
            "{} {}",
            s.serialize("list-style-type")?,
            s.serialize("list-style-position")?
        )),
        "transition" | "animation" | "grid-template" | "grid" | "place-items" | "place-content"
        | "columns" | "outline" => Some(String::new()),
        _ => None,
    }
}

fn computed_style(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let pseudo = arg_str(vm, a, 1)?;
    let proto = inner(vm)
        .borrow()
        .protos
        .get("CSSStyleDeclaration")
        .cloned();
    Ok(Value::Obj(vm.host_obj(
        proto,
        &COMPUTED_HOOKS,
        vec![Value::Num(n.0 as f64), string_val(pseudo)],
    )))
}

fn computed_value_native(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let pseudo = arg_str(vm, a, 1)?;
    let name = css_name(&arg_str(vm, a, 2)?);
    Ok(string_val(computed_value(vm, n, &pseudo, &name)))
}

// ---------------------------------------------------------------- declaration blocks

fn serialize_declarations(decls: &[Declaration]) -> String {
    let mut out = String::new();
    for d in decls {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&d.name);
        out.push_str(": ");
        out.push_str(&component_values_to_string(&d.value));
        if d.important {
            out.push_str(" !important");
        }
        out.push(';');
    }
    out
}

/// Properties a `CSSStyleDeclaration` accepts: the engine's, custom properties,
/// vendor-prefixed names and the SVG presentation attributes (stored as written).
fn settable_property(name: &str) -> bool {
    name.starts_with("--")
        || name.starts_with("-webkit-")
        || name.starts_with("-moz-")
        || name.starts_with("-ms-")
        || crate::style::cascade::is_known_property(name)
        || matches!(
            name,
            "fill"
                | "stroke"
                | "stroke-width"
                | "fill-opacity"
                | "stroke-opacity"
                | "stroke-dasharray"
                | "stroke-dashoffset"
                | "stroke-linecap"
                | "stroke-linejoin"
                | "fill-rule"
                | "clip-rule"
                | "clip-path"
                | "mask"
                | "filter"
                | "backdrop-filter"
                | "will-change"
                | "touch-action"
                | "scroll-behavior"
                | "overscroll-behavior"
                | "text-rendering"
                | "shape-rendering"
                | "vector-effect"
                | "paint-order"
                | "stop-color"
                | "stop-opacity"
                | "marker"
                | "marker-start"
                | "marker-mid"
                | "marker-end"
                | "color-interpolation"
                | "dominant-baseline"
                | "text-anchor"
                | "alignment-baseline"
                | "transform-box"
                | "transform-origin"
                | "perspective"
                | "backface-visibility"
                | "mix-blend-mode"
                | "isolation"
                | "contain"
                | "content-visibility"
                | "aspect-ratio"
                | "appearance"
                | "resize"
                | "caret-color"
                | "accent-color"
                | "scrollbar-width"
                | "scrollbar-color"
                | "text-size-adjust"
                | "font-display"
                | "font-feature-settings"
                | "font-variant-numeric"
                | "font-variation-settings"
                | "text-underline-offset"
                | "text-decoration-thickness"
                | "text-decoration-skip-ink"
                | "hyphens"
                | "tab-size"
                | "column-count"
                | "column-gap"
                | "column-width"
                | "columns"
                | "break-inside"
                | "page-break-inside"
                | "page-break-after"
                | "page-break-before"
                | "user-select"
                | "pointer-events"
                | "image-rendering"
                | "object-position"
                | "translate"
                | "rotate"
                | "scale"
                | "offset"
                | "inset-inline"
                | "inset-block"
                | "margin-inline"
                | "margin-block"
                | "padding-inline"
                | "padding-block"
                | "border-inline"
                | "border-block"
                | "inline-size"
                | "block-size"
                | "min-inline-size"
                | "min-block-size"
                | "max-inline-size"
                | "max-block-size"
                | "place-self"
                | "justify-self"
                | "justify-items"
                | "grid-area"
                | "grid-row"
                | "grid-column"
                | "grid-template-areas"
                | "grid-auto-rows"
                | "grid-auto-columns"
                | "grid-auto-flow"
                | "box-decoration-break"
                | "text-wrap"
                | "white-space-collapse"
                | "text-spacing-trim"
                | "field-sizing"
                | "anchor-name"
                | "view-transition-name"
                | "container"
                | "container-type"
                | "container-name"
                | "zoom"
                | "outline"
                | "outline-color"
                | "outline-style"
                | "outline-width"
                | "outline-offset"
                | "quotes"
                | "counter-reset"
                | "counter-increment"
                | "unicode-bidi"
                | "writing-mode"
                | "text-orientation"
                | "ruby-position"
                | "speak"
                | "widows"
                | "orphans"
                | "all"
        )
}

fn set_declaration(decls: &mut Vec<Declaration>, name: &str, value: &str, important: bool) -> bool {
    let value = value.trim();
    if value.is_empty() {
        let before = decls.len();
        decls.retain(|d| d.name != name);
        return decls.len() != before;
    }
    let parsed = parse_declaration_block(&format!(
        "{name}: {value}{}",
        if important { " !important" } else { "" }
    ));
    let Some(mut d) = parsed.into_iter().next() else {
        return false;
    };
    if !settable_property(&d.name) {
        return false;
    }
    // Values the engine understands are validated; the rest are kept as written.
    if crate::style::cascade::is_known_property(&d.name)
        && !d.name.starts_with("--")
        && !crate::style::cascade::is_supported_declaration(&d.name, &d.value)
    {
        let text = component_values_to_string(&d.value).to_ascii_lowercase();
        if !matches!(text.as_str(), "inherit" | "initial" | "unset" | "revert")
            && !text.contains("var(")
            && !text.starts_with("calc(")
            && !text.starts_with("min(")
            && !text.starts_with("max(")
            && !text.starts_with("clamp(")
        {
            return false;
        }
    }
    d.important = important;
    match decls.iter_mut().find(|x| x.name == d.name) {
        Some(x) => {
            if x.value == d.value && x.important == d.important {
                return false;
            }
            *x = d;
        }
        None => decls.push(d),
    }
    true
}

fn inline_decls(i: &mut Inner, n: NodeId) -> Vec<Declaration> {
    let text = i.doc.attr(n, "style").unwrap_or("").to_owned();
    if let Some((t, d)) = i.inline_cache.get(&n) {
        if *t == text {
            return d.clone();
        }
    }
    let d = parse_declaration_block(&text);
    i.inline_cache.insert(n, (text, d.clone()));
    d
}

fn inline_get(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<Value>> {
    let (Key::Str(s), Some(id)) = (k, o.host_id()) else {
        return Ok(None);
    };
    if is_member(s) {
        return Ok(None);
    }
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let decls = inline_decls(&mut i, NodeId(id));
    if let Some(idx) = k.array_index() {
        return Ok(Some(
            decls
                .get(idx as usize)
                .map(|d| Value::str(&d.name))
                .unwrap_or(Value::Undefined),
        ));
    }
    let name = css_name(s);
    if !settable_property(&name) {
        return Ok(None);
    }
    Ok(Some(match decls.iter().find(|d| d.name == name) {
        Some(d) => string_val(component_values_to_string(&d.value)),
        None => Value::str(""),
    }))
}
fn inline_set(vm: &mut Vm, o: &Obj, k: &Key, v: &Value) -> JsResult<Option<bool>> {
    let (Key::Str(s), Some(id)) = (k, o.host_id()) else {
        return Ok(None);
    };
    if is_member(s) || k.array_index().is_some() {
        return Ok(None);
    }
    let name = css_name(s);
    if !settable_property(&name) {
        return Ok(None);
    }
    let value = if v.is_nullish() {
        String::new()
    } else {
        vm.to_string(v)?.to_string()
    };
    set_inline_property(vm, NodeId(id), &name, &value, false)?;
    Ok(Some(true))
}
fn inline_delete(_vm: &mut Vm, _o: &Obj, _k: &Key) -> JsResult<Option<bool>> {
    Ok(None)
}
fn inline_keys(vm: &mut Vm, o: &Obj) -> JsResult<Vec<Key>> {
    let Some(id) = o.host_id() else {
        return Ok(Vec::new());
    };
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let n = inline_decls(&mut i, NodeId(id)).len();
    Ok((0..n).map(|x| Key::str(&x.to_string())).collect())
}
pub static INLINE_HOOKS: HostHooks = HostHooks {
    class: "CSSStyleDeclaration",
    get: inline_get,
    set: inline_set,
    delete: inline_delete,
    keys: inline_keys,
};

pub fn set_inline_property(
    vm: &mut Vm,
    n: NodeId,
    name: &str,
    value: &str,
    important: bool,
) -> JsResult<()> {
    let text = {
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        let mut decls = inline_decls(&mut i, n);
        if !set_declaration(&mut decls, name, value, important) {
            return Ok(());
        }
        serialize_declarations(&decls)
    };
    set_attribute_value(vm, n, "style", Some(&text))
}

fn inline_style(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let proto = inner(vm)
        .borrow()
        .protos
        .get("CSSStyleDeclaration")
        .cloned();
    Ok(Value::Obj(vm.host_obj(
        proto,
        &INLINE_HOOKS,
        vec![Value::Num(n.0 as f64)],
    )))
}

/// `W.declOp(decl, op, name, value, priority)`: the methods of a
/// `CSSStyleDeclaration` (inline, computed or rule-backed).
fn decl_op(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let Value::Obj(o) = a.arg(0) else {
        return Ok(Value::Undefined);
    };
    let op = arg_str(vm, a, 1)?;
    let name = css_name(&arg_str(vm, a, 2)?);
    let hooks = o.host_hooks();
    let is_inline = hooks
        .map(|h| std::ptr::eq(h, &INLINE_HOOKS))
        .unwrap_or(false);
    let is_computed = hooks
        .map(|h| std::ptr::eq(h, &COMPUTED_HOOKS))
        .unwrap_or(false);
    let is_rule = hooks
        .map(|h| std::ptr::eq(h, &RULE_STYLE_HOOKS))
        .unwrap_or(false);
    if is_computed {
        let n = NodeId(o.host_id().unwrap_or(0));
        let pseudo = match o.host_slot(1) {
            Some(Value::Str(p)) => p.to_string(),
            _ => String::new(),
        };
        return Ok(match op.as_str() {
            "get" => string_val(computed_value(vm, n, &pseudo, &name)),
            "priority" => Value::str(""),
            "length" => Value::Num(LonghandId::all().count() as f64),
            "cssText" => Value::str(""),
            "set" | "remove" | "setCssText" => {
                return Err(dom_exception(
                    vm,
                    "NoModificationAllowedError",
                    "These styles are computed, and therefore read-only.",
                ))
            }
            _ => Value::Undefined,
        });
    }
    // The declaration list to operate on.
    let (mut decls, rule_ref) = if is_inline {
        let n = NodeId(o.host_id().unwrap_or(0));
        let rc = inner(vm);
        let mut i = rc.borrow_mut();
        (inline_decls(&mut i, n), None)
    } else if is_rule {
        let (sheet, path) = rule_ref_of(&o);
        let rc = inner(vm);
        let i = rc.borrow();
        let decls = rule_at(&i, sheet, &path).and_then(|r| match r {
            Rule::Style { declarations, .. }
            | Rule::Page { declarations, .. }
            | Rule::FontFace(declarations) => Some(declarations.clone()),
            _ => None,
        });
        (decls.unwrap_or_default(), Some((sheet, path)))
    } else {
        return Ok(Value::Undefined);
    };
    match op.as_str() {
        "get" => Ok(match decls.iter().find(|d| d.name == name) {
            Some(d) => string_val(component_values_to_string(&d.value)),
            None => Value::str(""),
        }),
        "priority" => Ok(Value::str(
            if decls.iter().any(|d| d.name == name && d.important) {
                "important"
            } else {
                ""
            },
        )),
        "length" => Ok(Value::Num(decls.len() as f64)),
        "item" => {
            let idx = arg_num(vm, a, 2)? as usize;
            Ok(Value::str(
                decls.get(idx).map(|d| d.name.as_str()).unwrap_or(""),
            ))
        }
        "cssText" => Ok(string_val(serialize_declarations(&decls))),
        "set" | "remove" | "setCssText" => {
            let changed = if op == "setCssText" {
                let text = arg_str(vm, a, 2)?;
                decls = parse_declaration_block(&text);
                true
            } else if op == "remove" {
                let before = decls.len();
                decls.retain(|d| d.name != name);
                decls.len() != before
            } else {
                let value = arg_str(vm, a, 3)?;
                let important = arg_str(vm, a, 4)?.eq_ignore_ascii_case("important");
                set_declaration(&mut decls, &name, &value, important)
            };
            if !changed {
                return Ok(Value::Undefined);
            }
            if is_inline {
                let n = NodeId(o.host_id().unwrap_or(0));
                let text = serialize_declarations(&decls);
                set_attribute_value(vm, n, "style", Some(&text))?;
            } else if let Some((sheet, path)) = rule_ref {
                let rc = inner(vm);
                let mut i = rc.borrow_mut();
                if let Some(
                    Rule::Style { declarations, .. }
                    | Rule::Page { declarations, .. }
                    | Rule::FontFace(declarations),
                ) = rule_at_mut(&mut i, sheet, &path)
                {
                    *declarations = decls;
                }
                i.sheet_changed();
            }
            Ok(Value::Undefined)
        }
        _ => Ok(Value::Undefined),
    }
}

// ---------------------------------------------------------------- sheets and rules

fn rule_ref_of(o: &Obj) -> (u32, Vec<usize>) {
    let sheet = o.host_id().unwrap_or(0);
    let path = match o.host_slot(1) {
        Some(Value::Str(p)) => p
            .split('/')
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect(),
        _ => Vec::new(),
    };
    (sheet, path)
}

fn rules_at<'a>(rules: &'a [Rule], path: &[usize]) -> Option<&'a [Rule]> {
    let mut cur = rules;
    for &p in path {
        cur = match cur.get(p)? {
            Rule::Media { rules, .. }
            | Rule::Supports { rules, .. }
            | Rule::Layer { rules, .. } => rules,
            _ => return None,
        };
    }
    Some(cur)
}

fn rules_at_mut<'a>(rules: &'a mut Vec<Rule>, path: &[usize]) -> Option<&'a mut Vec<Rule>> {
    let mut cur = rules;
    for &p in path {
        cur = match cur.get_mut(p)? {
            Rule::Media { rules, .. }
            | Rule::Supports { rules, .. }
            | Rule::Layer { rules, .. } => rules,
            _ => return None,
        };
    }
    Some(cur)
}

fn rule_at<'a>(i: &'a Inner, sheet: u32, path: &[usize]) -> Option<&'a Rule> {
    let (last, parent) = path.split_last()?;
    rules_at(&i.sheet(sheet)?.sheet.rules, parent)?.get(*last)
}

fn rule_at_mut<'a>(i: &'a mut Inner, sheet: u32, path: &[usize]) -> Option<&'a mut Rule> {
    let (last, parent) = path.split_last()?;
    rules_at_mut(&mut i.sheet_mut(sheet)?.sheet.rules, parent)?.get_mut(*last)
}

/// Renumbers the style rules' source order after an edit.
fn renumber(rules: &mut [Rule], next: &mut u32) {
    for r in rules {
        match r {
            Rule::Style { source_order, .. } => {
                *source_order = *next;
                *next += 1;
            }
            Rule::Media { rules, .. }
            | Rule::Supports { rules, .. }
            | Rule::Layer { rules, .. } => renumber(rules, next),
            _ => {}
        }
    }
}

fn rule_type(r: &Rule) -> u32 {
    match r {
        Rule::Style { .. } => 1,
        Rule::Import { .. } => 3,
        Rule::Media { .. } => 4,
        Rule::FontFace(_) => 5,
        Rule::Page { .. } => 6,
        Rule::Keyframes { .. } => 7,
        Rule::Namespace { .. } => 10,
        Rule::Supports { .. } => 12,
        Rule::Layer { .. } => 0,
        Rule::Unknown { .. } => 0,
    }
}

fn supports_text(c: &SupportsCondition) -> String {
    match c {
        SupportsCondition::Not(x) => format!("not {}", supports_text(x)),
        SupportsCondition::And(v) => v
            .iter()
            .map(supports_text)
            .collect::<Vec<_>>()
            .join(" and "),
        SupportsCondition::Or(v) => v.iter().map(supports_text).collect::<Vec<_>>().join(" or "),
        SupportsCondition::Declaration { property, value } => {
            format!("({property}: {})", component_values_to_string(value))
        }
        SupportsCondition::Selector { text, .. } => format!("selector({text})"),
        SupportsCondition::Unknown(s) => s.clone(),
    }
}

fn keyframe_selector(k: &KeyframeSelector) -> String {
    match k {
        KeyframeSelector::From => "from".into(),
        KeyframeSelector::To => "to".into(),
        KeyframeSelector::Percentage(n) => format!("{}%", number_text(*n)),
    }
}

fn number_text(n: crate::css::token::Number) -> String {
    let neg = n.micro < 0;
    let m = n.micro.abs();
    let int = m / 1_000_000;
    let frac = m % 1_000_000;
    let mut s = format!("{}{}", if neg { "-" } else { "" }, int);
    if frac != 0 {
        let mut f = format!("{frac:06}");
        while f.ends_with('0') {
            f.pop();
        }
        s.push('.');
        s.push_str(&f);
    }
    s
}

pub fn rule_css_text(r: &Rule) -> String {
    match r {
        Rule::Style {
            selectors,
            declarations,
            ..
        } => format!(
            "{selectors} {{ {}{}}}",
            serialize_declarations(declarations),
            if declarations.is_empty() { "" } else { " " }
        ),
        Rule::Media { query, rules } => format!(
            "@media {query} {{\n{}}}",
            rules
                .iter()
                .map(|r| format!("  {}\n", rule_css_text(r)))
                .collect::<String>()
        ),
        Rule::Import { url, media } => {
            let m = media.to_string();
            format!(
                "@import url(\"{url}\"){}{};",
                if m.is_empty() { "" } else { " " },
                m
            )
        }
        Rule::FontFace(d) => format!("@font-face {{ {} }}", serialize_declarations(d)),
        Rule::Keyframes { name, frames } => format!(
            "@keyframes {name} {{ {}}}",
            frames
                .iter()
                .map(|(sel, d)| format!(
                    "{} {{ {} }} ",
                    sel.iter()
                        .map(keyframe_selector)
                        .collect::<Vec<_>>()
                        .join(", "),
                    serialize_declarations(d)
                ))
                .collect::<String>()
        ),
        Rule::Supports { condition, rules } => format!(
            "@supports {} {{\n{}}}",
            supports_text(condition),
            rules
                .iter()
                .map(|r| format!("  {}\n", rule_css_text(r)))
                .collect::<String>()
        ),
        Rule::Layer { names, rules } => {
            if rules.is_empty() {
                format!("@layer {};", names.join(", "))
            } else {
                format!(
                    "@layer {} {{\n{}}}",
                    names.join(", "),
                    rules
                        .iter()
                        .map(|r| format!("  {}\n", rule_css_text(r)))
                        .collect::<String>()
                )
            }
        }
        Rule::Page {
            selectors,
            declarations,
        } => format!(
            "@page {}{{ {} }}",
            if selectors.is_empty() {
                String::new()
            } else {
                format!("{} ", selectors.join(", "))
            },
            serialize_declarations(declarations)
        ),
        Rule::Namespace { prefix, url } => format!(
            "@namespace {}url(\"{url}\");",
            prefix.as_ref().map(|p| format!("{p} ")).unwrap_or_default()
        ),
        Rule::Unknown {
            name,
            prelude,
            block,
        } => format!(
            "@{name} {}{}",
            component_values_to_string(prelude).trim(),
            match block {
                Some(b) => format!(" {{ {} }}", component_values_to_string(b).trim()),
                None => ";".into(),
            }
        ),
    }
}

fn rule_object(vm: &mut Vm, sheet: u32, path: &[usize], proto: &str) -> Value {
    let p: Vec<String> = path.iter().map(|x| x.to_string()).collect();
    handle_obj(vm, proto, sheet, vec![string_val(p.join("/"))])
}

fn rule_style_get(vm: &mut Vm, o: &Obj, k: &Key) -> JsResult<Option<Value>> {
    let Key::Str(s) = k else { return Ok(None) };
    if is_member(s) {
        return Ok(None);
    }
    let (sheet, path) = rule_ref_of(o);
    let rc = inner(vm);
    let i = rc.borrow();
    let decls = match rule_at(&i, sheet, &path) {
        Some(Rule::Style { declarations, .. })
        | Some(Rule::Page { declarations, .. })
        | Some(Rule::FontFace(declarations)) => declarations,
        _ => return Ok(None),
    };
    if let Some(idx) = k.array_index() {
        return Ok(Some(
            decls
                .get(idx as usize)
                .map(|d| Value::str(&d.name))
                .unwrap_or(Value::Undefined),
        ));
    }
    let name = css_name(s);
    Ok(Some(match decls.iter().find(|d| d.name == name) {
        Some(d) => string_val(component_values_to_string(&d.value)),
        None => Value::str(""),
    }))
}
fn rule_style_set(vm: &mut Vm, o: &Obj, k: &Key, v: &Value) -> JsResult<Option<bool>> {
    let Key::Str(s) = k else { return Ok(None) };
    if is_member(s) || k.array_index().is_some() {
        return Ok(None);
    }
    let name = css_name(s);
    let value = if v.is_nullish() {
        String::new()
    } else {
        vm.to_string(v)?.to_string()
    };
    let (sheet, path) = rule_ref_of(o);
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    let changed = match rule_at_mut(&mut i, sheet, &path) {
        Some(Rule::Style { declarations, .. })
        | Some(Rule::Page { declarations, .. })
        | Some(Rule::FontFace(declarations)) => set_declaration(declarations, &name, &value, false),
        _ => false,
    };
    if changed {
        i.sheet_changed();
    }
    Ok(Some(true))
}
fn rule_style_keys(vm: &mut Vm, o: &Obj) -> JsResult<Vec<Key>> {
    let (sheet, path) = rule_ref_of(o);
    let rc = inner(vm);
    let i = rc.borrow();
    let n = match rule_at(&i, sheet, &path) {
        Some(Rule::Style { declarations, .. }) => declarations.len(),
        _ => 0,
    };
    Ok((0..n).map(|x| Key::str(&x.to_string())).collect())
}
pub static RULE_STYLE_HOOKS: HostHooks = HostHooks {
    class: "CSSStyleDeclaration",
    get: rule_style_get,
    set: rule_style_set,
    delete: inline_delete,
    keys: rule_style_keys,
};

/// `W.sheetOp(sheetId, op, ...)`.
fn sheet_op(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let id = arg_num(vm, a, 0)? as u32;
    let op = arg_str(vm, a, 1)?;
    match op.as_str() {
        "new" => {
            let text = arg_str(vm, a, 2)?;
            let id = inner(vm).borrow_mut().new_constructed_sheet(&text);
            Ok(Value::Num(id as f64))
        }
        "replace" => {
            let text = arg_str(vm, a, 2)?;
            let rc = inner(vm);
            let mut i = rc.borrow_mut();
            if let Some(e) = i.sheet_mut(id) {
                e.sheet = parse_stylesheet(&text, Origin::Author, Strictness::Lenient)
                    .unwrap_or_default();
                e.source = text;
            }
            i.sheet_changed();
            Ok(Value::Undefined)
        }
        "info" => {
            let rc = inner(vm);
            let (href, disabled, media, owner, ctor) = {
                let i = rc.borrow();
                match i.sheet(id) {
                    Some(e) => (
                        e.href.clone(),
                        e.disabled,
                        e.media.clone(),
                        match e.owner {
                            SheetOwner::Element(n) => Some(n),
                            _ => None,
                        },
                        matches!(e.owner, SheetOwner::Constructed),
                    ),
                    None => return Ok(Value::Null),
                }
            };
            let owner = opt_node(vm, owner);
            let o = cw_jsvm::builtins::new_obj_from(
                vm,
                vec![
                    ("href", href.map(string_val).unwrap_or(Value::Null)),
                    ("disabled", Value::Bool(disabled)),
                    ("media", string_val(media)),
                    ("ownerNode", owner),
                    ("constructed", Value::Bool(ctor)),
                ],
            );
            Ok(Value::Obj(o))
        }
        "setDisabled" => {
            let on = a.arg(2).truthy();
            let rc = inner(vm);
            let mut i = rc.borrow_mut();
            if let Some(e) = i.sheet_mut(id) {
                e.disabled = on;
            }
            i.sheet_changed();
            Ok(Value::Undefined)
        }
        "count" => {
            let path = path_arg(vm, a, 2)?;
            let rc = inner(vm);
            let i = rc.borrow();
            let n = i
                .sheet(id)
                .and_then(|e| rules_at(&e.sheet.rules, &path))
                .map(|r| r.len())
                .unwrap_or(0);
            Ok(Value::Num(n as f64))
        }
        "rule" => {
            let mut path = path_arg(vm, a, 2)?;
            let idx = arg_num(vm, a, 3)? as usize;
            path.push(idx);
            let info = {
                let rc = inner(vm);
                let i = rc.borrow();
                match rule_at(&i, id, &path) {
                    Some(r) => {
                        let (sel, cond, name) = match r {
                            Rule::Style { selectors, .. } => {
                                (selectors.to_string(), String::new(), String::new())
                            }
                            Rule::Media { query, .. } => {
                                (String::new(), query.to_string(), String::new())
                            }
                            Rule::Supports { condition, .. } => {
                                (String::new(), supports_text(condition), String::new())
                            }
                            Rule::Keyframes { name, .. } => {
                                (String::new(), String::new(), name.clone())
                            }
                            Rule::Layer { names, .. } => {
                                (String::new(), String::new(), names.join(", "))
                            }
                            Rule::Import { url, media } => {
                                (String::new(), media.to_string(), url.clone())
                            }
                            _ => (String::new(), String::new(), String::new()),
                        };
                        Some((rule_type(r), rule_css_text(r), sel, cond, name))
                    }
                    None => None,
                }
            };
            let Some((ty, text, sel, cond, name)) = info else {
                return Ok(Value::Null);
            };
            let p: Vec<String> = path.iter().map(|x| x.to_string()).collect();
            let o = cw_jsvm::builtins::new_obj_from(
                vm,
                vec![
                    ("type", Value::Num(ty as f64)),
                    ("cssText", string_val(text)),
                    ("selectorText", string_val(sel)),
                    ("conditionText", string_val(cond)),
                    ("name", string_val(name)),
                    ("path", string_val(p.join("/"))),
                ],
            );
            Ok(Value::Obj(o))
        }
        "ruleStyle" => {
            let path = path_arg(vm, a, 2)?;
            let proto = inner(vm)
                .borrow()
                .protos
                .get("CSSStyleDeclaration")
                .cloned();
            let p: Vec<String> = path.iter().map(|x| x.to_string()).collect();
            Ok(Value::Obj(vm.host_obj(
                proto,
                &RULE_STYLE_HOOKS,
                vec![Value::Num(id as f64), string_val(p.join("/"))],
            )))
        }
        "insert" => {
            let path = path_arg(vm, a, 2)?;
            let text = arg_str(vm, a, 3)?;
            let idx = arg_num(vm, a, 4)? as usize;
            let parsed =
                parse_stylesheet(&text, Origin::Author, Strictness::Lenient).unwrap_or_default();
            let mut rules = parsed.rules;
            if rules.len() != 1 {
                return Err(dom_exception(
                    vm,
                    "SyntaxError",
                    &format!("Failed to parse the rule '{text}'."),
                ));
            }
            let rule = rules.remove(0);
            let rc = inner(vm);
            let mut i = rc.borrow_mut();
            let ok = {
                let Some(e) = i.sheet_mut(id) else {
                    return Ok(Value::Undefined);
                };
                match rules_at_mut(&mut e.sheet.rules, &path) {
                    Some(list) if idx <= list.len() => {
                        list.insert(idx, rule);
                        let mut n = 0;
                        renumber(&mut e.sheet.rules, &mut n);
                        true
                    }
                    _ => false,
                }
            };
            if !ok {
                drop(i);
                return Err(dom_exception(
                    vm,
                    "IndexSizeError",
                    "The index provided is larger than the maximum index.",
                ));
            }
            i.sheet_changed();
            Ok(Value::Num(idx as f64))
        }
        "delete" => {
            let path = path_arg(vm, a, 2)?;
            let idx = arg_num(vm, a, 3)? as usize;
            let rc = inner(vm);
            let mut i = rc.borrow_mut();
            let ok = {
                let Some(e) = i.sheet_mut(id) else {
                    return Ok(Value::Undefined);
                };
                match rules_at_mut(&mut e.sheet.rules, &path) {
                    Some(list) if idx < list.len() => {
                        list.remove(idx);
                        let mut n = 0;
                        renumber(&mut e.sheet.rules, &mut n);
                        true
                    }
                    _ => false,
                }
            };
            if !ok {
                drop(i);
                return Err(dom_exception(
                    vm,
                    "IndexSizeError",
                    "The index provided is larger than the maximum index.",
                ));
            }
            i.sheet_changed();
            Ok(Value::Undefined)
        }
        "setSelector" => {
            let path = path_arg(vm, a, 2)?;
            let text = arg_str(vm, a, 3)?;
            if let Ok(list) = crate::css::parse_selector_list(&text) {
                let rc = inner(vm);
                let mut i = rc.borrow_mut();
                if let Some(Rule::Style { selectors, .. }) = rule_at_mut(&mut i, id, &path) {
                    *selectors = list;
                }
                i.sheet_changed();
            }
            Ok(Value::Undefined)
        }
        "keyframes" => {
            let path = path_arg(vm, a, 2)?;
            let rc = inner(vm);
            let i = rc.borrow();
            let frames: Vec<String> = match rule_at(&i, id, &path) {
                Some(Rule::Keyframes { frames, .. }) => frames
                    .iter()
                    .map(|(sel, d)| {
                        format!(
                            "{} {{ {} }}",
                            sel.iter()
                                .map(keyframe_selector)
                                .collect::<Vec<_>>()
                                .join(", "),
                            serialize_declarations(d)
                        )
                    })
                    .collect(),
                _ => Vec::new(),
            };
            drop(i);
            Ok(str_array(vm, &frames))
        }
        _ => Ok(Value::Undefined),
    }
}

fn path_arg(vm: &mut Vm, a: &Args, i: usize) -> JsResult<Vec<usize>> {
    let s = arg_str(vm, a, i)?;
    Ok(s.split('/')
        .filter(|x| !x.is_empty())
        .filter_map(|x| x.parse().ok())
        .collect())
}

fn doc_sheets(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let ids = inner(vm).borrow_mut().document_sheet_ids();
    let v: Vec<Value> = ids.iter().map(|i| Value::Num(*i as f64)).collect();
    Ok(vm.arr(v))
}

fn sheet_of(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let n = arg_node(vm, a, 0)?;
    let id = inner(vm).borrow_mut().sheet_of_element(n);
    Ok(id.map(|i| Value::Num(i as f64)).unwrap_or(Value::Null))
}

fn set_adopted(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let items = super::array_values(vm, &a.arg(0))?;
    let ids: Vec<u32> = items
        .iter()
        .filter_map(|v| match v {
            Value::Num(n) => Some(*n as u32),
            _ => None,
        })
        .collect();
    let rc = inner(vm);
    let mut i = rc.borrow_mut();
    i.adopted = ids;
    i.sheet_changed();
    Ok(Value::Undefined)
}

// ---------------------------------------------------------------- CSS namespace, media

fn css_supports(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let first = arg_str(vm, a, 0)?;
    if a.len() >= 2 {
        let value = arg_str(vm, a, 1)?;
        let values = crate::css::parser::parse_component_value_list(&value);
        return Ok(Value::Bool(
            crate::style::cascade::is_supported_declaration(&first, &values),
        ));
    }
    let values = crate::css::parser::parse_component_value_list(&first);
    let ok = match crate::css::media::parse_supports_condition(&values) {
        Some(c) => c.evaluate(&crate::style::cascade::is_supported_declaration),
        None => {
            // A bare `prop: value` is accepted as a condition too.
            let inner_vals = match values.first() {
                Some(crate::css::ComponentValue::Block { contents, .. }) if values.len() == 1 => {
                    contents.clone()
                }
                _ => values.clone(),
            };
            let text = component_values_to_string(&inner_vals);
            match text.split_once(':') {
                Some((p, v)) => crate::style::cascade::is_supported_declaration(
                    p.trim(),
                    &crate::css::parser::parse_component_value_list(v.trim()),
                ),
                None => false,
            }
        }
    };
    Ok(Value::Bool(ok))
}

fn css_escape(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let s = arg_str(vm, a, 0)?;
    let mut out = String::new();
    for (idx, c) in s.chars().enumerate() {
        let code = c as u32;
        if code == 0 {
            out.push('\u{FFFD}');
        } else if (1..=0x1F).contains(&code)
            || code == 0x7F
            || (idx == 0 && c.is_ascii_digit())
            || (idx == 1 && c.is_ascii_digit() && s.starts_with('-'))
        {
            out.push_str(&format!("\\{code:x} "));
        } else if idx == 0 && c == '-' && s.len() == 1 {
            out.push_str("\\-");
        } else if code >= 0x80 || c == '-' || c == '_' || c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            out.push('\\');
            out.push(c);
        }
    }
    Ok(string_val(out))
}

fn media_matches(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let q = arg_str(vm, a, 0)?;
    let rc = inner(vm);
    let i = rc.borrow();
    let list = crate::css::MediaQueryList::parse(&q);
    let m = i.media();
    Ok(vm.arr(vec![
        Value::Bool(list.evaluate(&m)),
        string_val(list.to_string()),
    ]))
}

fn style_elements(vm: &mut Vm, _a: &mut Args) -> JsResult<Value> {
    let nodes: Vec<NodeId> = {
        let rc = inner(vm);
        let i = rc.borrow();
        i.doc
            .descendants(crate::dom::Document::ROOT)
            .filter(|n| i.doc.is(*n, "style") || i.doc.is(*n, "link"))
            .collect()
    };
    Ok(node_array(vm, &nodes))
}

fn rule_object_native(vm: &mut Vm, a: &mut Args) -> JsResult<Value> {
    let id = arg_num(vm, a, 0)? as u32;
    let path = path_arg(vm, a, 1)?;
    let proto = arg_str(vm, a, 2)?;
    Ok(rule_object(vm, id, &path, &proto))
}

pub fn install(vm: &mut Vm, w: &Obj) {
    vm.method(w, "computedStyle", 2, computed_style);
    vm.method(w, "computedValue", 3, computed_value_native);
    vm.method(w, "inlineStyle", 1, inline_style);
    vm.method(w, "declOp", 5, decl_op);
    vm.method(w, "sheetOp", 5, sheet_op);
    vm.method(w, "docSheets", 0, doc_sheets);
    vm.method(w, "sheetOf", 1, sheet_of);
    vm.method(w, "setAdopted", 1, set_adopted);
    vm.method(w, "cssSupports", 2, css_supports);
    vm.method(w, "cssEscape", 1, css_escape);
    vm.method(w, "mediaMatches", 1, media_matches);
    vm.method(w, "styleElements", 0, style_elements);
    vm.method(w, "ruleObject", 3, rule_object_native);
}
