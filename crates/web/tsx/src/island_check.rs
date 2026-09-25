//! What the island can run. A module there (a package's, or one of the app's
//! outside the compiled subset) runs on the jsvm VM behind cw-ui's React shim,
//! which is not a browser: it has the language's globals, jsvm's Intl, URL and
//! text codecs, and the shim's React, timers, console and a small `document`.
//! A module that needs more (the page's `fetch`, `localStorage`, `history`,
//! `matchMedia`, `requestAnimationFrame`, portals, class components, raw HTML)
//! would build and then behave differently from the page's React, so it is
//! refused here, and the app takes its React fallback whole.

use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Class, Expression, IdentifierReference, StaticMemberExpression, UnaryExpression, UnaryOperator,
};
use oxc_ast_visit::{walk, Visit};
use oxc_parser::Parser;
use oxc_semantic::{Scoping, SemanticBuilder};

use crate::{Diagnostic, Source};

/// Globals the island has with a browser's meaning: the language's, jsvm's
/// Web-compatible objects that need no page, and the shim's.
const GLOBALS: &[&str] = &[
    // ECMAScript.
    "AggregateError",
    "Array",
    "ArrayBuffer",
    "Atomics",
    "BigInt",
    "BigInt64Array",
    "BigUint64Array",
    "Boolean",
    "DataView",
    "Date",
    "Error",
    "EvalError",
    "FinalizationRegistry",
    "Float32Array",
    "Float64Array",
    "Function",
    "Infinity",
    "Int16Array",
    "Int32Array",
    "Int8Array",
    "Intl",
    "Iterator",
    "JSON",
    "Map",
    "Math",
    "NaN",
    "Number",
    "Object",
    "Promise",
    "Proxy",
    "RangeError",
    "ReferenceError",
    "Reflect",
    "RegExp",
    "Set",
    "SharedArrayBuffer",
    "String",
    "Symbol",
    "SyntaxError",
    "TypeError",
    "URIError",
    "Uint16Array",
    "Uint32Array",
    "Uint8Array",
    "Uint8ClampedArray",
    "WeakMap",
    "WeakRef",
    "WeakSet",
    "decodeURI",
    "decodeURIComponent",
    "encodeURI",
    "encodeURIComponent",
    "escape",
    "unescape",
    "eval",
    "isFinite",
    "isNaN",
    "parseFloat",
    "parseInt",
    "arguments",
    "undefined",
    "globalThis",
    // jsvm's, the same objects the page's Realm has.
    "AbortController",
    "AbortSignal",
    "Blob",
    "DOMException",
    "Event",
    "EventTarget",
    "File",
    "Headers",
    "Request",
    "Response",
    "TextDecoder",
    "TextEncoder",
    "URL",
    "URLSearchParams",
    "atob",
    "btoa",
    "structuredClone",
    "crypto",
    "performance",
    "queueMicrotask",
    // The shim's.
    "React",
    "ReactDOM",
    "window",
    "self",
    "document",
    "console",
    "setTimeout",
    "setInterval",
    "clearTimeout",
    "clearInterval",
    "addEventListener",
    "removeEventListener",
    // The page's, through the same builtins compiled code uses.
    "localStorage",
    "sessionStorage",
    "location",
    "innerWidth",
    "innerHeight",
    "scrollX",
    "scrollY",
    "pageXOffset",
    "pageYOffset",
    "scrollTo",
    "scroll",
    "scrollBy",
    "alert",
    "confirm",
    "prompt",
    "fetch",
    "history",
    "requestAnimationFrame",
    "cancelAnimationFrame",
    "matchMedia",
];

/// Node's globals, which a browser lacks too: `typeof process` is `"undefined"`
/// on the island as on the page, so only such a test may name them.
const NODE_ONLY: &[&str] = &[
    "process",
    "Buffer",
    "global",
    "require",
    "module",
    "exports",
    "setImmediate",
    "clearImmediate",
    "__dirname",
    "__filename",
];

/// The shim's `document`.
const DOCUMENT: &[&str] = &[
    "getElementById",
    "querySelector",
    "querySelectorAll",
    "body",
    "documentElement",
    "activeElement",
    "title",
    "addEventListener",
    "removeEventListener",
    "defaultView",
    "location",
    "head",
];

/// What the shim's `location` reads (it navigates nowhere).
const LOCATION: &[&str] = &[
    "href", "origin", "protocol", "host", "hostname", "port", "pathname", "search", "hash",
    "toString", "assign", "replace", "reload",
];

/// Why each module the island would run cannot run there (empty: it can).
pub fn check(src: &Source) -> Vec<Diagnostic> {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, &src.text, crate::source_type(&src.file)).parse();
    if !ret.diagnostics.is_empty() {
        // The bundle reports these.
        return Vec::new();
    }
    let semantic = SemanticBuilder::new().build(&ret.program).semantic;
    let scoping = semantic.scoping();
    let mut v = Check {
        src,
        scoping,
        out: Vec::new(),
        allowed: Vec::new(),
    };
    v.visit_program(&ret.program);
    v.out.sort_by_key(|d| (d.line, d.col));
    v.out.dedup_by(|a, b| a.message == b.message);
    v.out
}

struct Check<'s> {
    src: &'s Source,
    scoping: &'s Scoping,
    out: Vec<Diagnostic>,
    /// Spans of references already judged by their context (`typeof process`,
    /// `process.env.NODE_ENV`, `window.x`).
    allowed: Vec<oxc_span::Span>,
}

impl Check<'_> {
    fn refuse(&mut self, at: u32, what: String) {
        let mut d = Diagnostic::at(&self.src.text, at, what);
        d.file = self.src.file.clone();
        self.out.push(d);
    }

    fn is_free(&self, id: &IdentifierReference) -> bool {
        match id.reference_id.get() {
            Some(r) => {
                let r = self.scoping.get_reference(r);
                r.symbol_id().is_none() && r.flags().is_value()
            }
            None => false,
        }
    }

    fn free_ident<'e, 'x>(&self, e: &'e Expression<'x>) -> Option<&'e IdentifierReference<'x>> {
        match e.without_parentheses() {
            Expression::Identifier(id) if self.is_free(id) => Some(id),
            _ => None,
        }
    }
}

impl Check<'_> {
    /// Whether `e` is the page's `location` (`location`, `window.location`).
    fn is_location(&self, e: &Expression<'_>) -> bool {
        match e.without_parentheses() {
            Expression::Identifier(id) => id.name == "location" && self.is_free(id),
            Expression::StaticMemberExpression(m) => {
                m.property.name == "location"
                    && self.free_ident(&m.object).is_some_and(|w| {
                        matches!(
                            w.name.as_str(),
                            "window" | "self" | "globalThis" | "document"
                        )
                    })
            }
            _ => false,
        }
    }
}

impl<'a> Visit<'a> for Check<'_> {
    fn visit_identifier_reference(&mut self, id: &IdentifierReference<'a>) {
        if self.allowed.contains(&id.span) || !self.is_free(id) {
            return;
        }
        let name = id.name.as_str();
        let cjs = self.src.commonjs && matches!(name, "module" | "exports" | "require");
        if !GLOBALS.contains(&name) && !cjs {
            self.refuse(
                id.span.start,
                format!("`{name}` is not on the island (its React shim is no browser page)"),
            );
        }
    }

    fn visit_unary_expression(&mut self, u: &UnaryExpression<'a>) {
        if u.operator == UnaryOperator::Typeof {
            if let Some(id) = self.free_ident(&u.argument) {
                if NODE_ONLY.contains(&id.name.as_str()) {
                    self.allowed.push(id.span);
                }
            }
        }
        walk::walk_unary_expression(self, u);
    }

    fn visit_static_member_expression(&mut self, m: &StaticMemberExpression<'a>) {
        let prop = m.property.name.as_str();
        // `process.env.NODE_ENV`: "production", as a bundler defines it.
        if prop == "NODE_ENV" {
            if let Expression::StaticMemberExpression(inner) = m.object.without_parentheses() {
                if inner.property.name == "env" {
                    if let Some(id) = self.free_ident(&inner.object) {
                        if id.name == "process" {
                            self.allowed.push(id.span);
                            return;
                        }
                    }
                }
            }
        }
        if prop == "lazy" {
            if let Expression::Identifier(id) = m.object.without_parentheses() {
                if id.name == "React" {
                    self.refuse(m.span.start, "`React.lazy` on the island".into());
                }
            }
        }
        if self.is_location(&m.object) && !LOCATION.contains(&prop) {
            self.refuse(
                m.span.start,
                format!("`location.{prop}` is not on the island (its location only reads)"),
            );
        }
        if let Some(id) = self.free_ident(&m.object) {
            let ok = match id.name.as_str() {
                "window" | "self" | "globalThis" => GLOBALS.contains(&prop),
                "document" => DOCUMENT.contains(&prop),
                _ => true,
            };
            if !ok {
                self.refuse(
                    m.span.start,
                    format!(
                        "`{}.{prop}` is not on the island (its React shim is no browser page)",
                        id.name
                    ),
                );
            }
        }
        walk::walk_static_member_expression(self, m);
    }

    fn visit_import_declaration(&mut self, d: &oxc_ast::ast::ImportDeclaration<'a>) {
        if d.source.value == "react" {
            for s in d.specifiers.iter().flatten() {
                if let oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(s) = s {
                    if s.imported.name() == "lazy" {
                        self.refuse(s.span.start, "`React.lazy` on the island".into());
                    }
                }
            }
        }
    }

    fn visit_class(&mut self, c: &Class<'a>) {
        // A class component runs over hooks (the shim's classHost): the phases
        // hooks do not have are refused, an error boundary's among them.
        let component = c.heritage.as_ref().is_some_and(|h| {
            let name = match h.expression.without_parentheses() {
                Expression::Identifier(id) => Some(id.name.as_str()),
                Expression::StaticMemberExpression(m) => Some(m.property.name.as_str()),
                _ => None,
            };
            matches!(name, Some("Component" | "PureComponent"))
        });
        if component {
            for e in &c.body.body {
                if let oxc_ast::ast::ClassElement::MethodDefinition(m) = e {
                    let Some(name) = m.key.static_name() else {
                        continue;
                    };
                    if matches!(
                        &*name,
                        "getDerivedStateFromError"
                            | "componentDidCatch"
                            | "getSnapshotBeforeUpdate"
                            | "componentWillMount"
                            | "componentWillReceiveProps"
                            | "componentWillUpdate"
                    ) || name.starts_with("UNSAFE_")
                    {
                        self.refuse(
                            m.span.start,
                            format!("a class component's `{name}` on the island"),
                        );
                    }
                }
            }
        }
        walk::walk_class(self, c);
    }
}
