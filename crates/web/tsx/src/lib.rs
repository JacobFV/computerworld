//! `cw-tsx`: compiles a React-syntax TypeScript module (`.tsx`) two ways from the same
//! source.
//!
//! * [`lower`] type-checks the module against the subset `cw-ui` runs natively and
//!   lowers it to the UI IR (`cw_ui::ir::Module`): static templates with holes,
//!   components with hooks, a typed expression language. Anything outside the subset
//!   is a [`Diagnostic`] with its line and column, and the module is marked for the
//!   fallback instead.
//! * [`emit_js`] strips the types and lowers JSX to `React.createElement`, giving a
//!   classic script React 18's UMD build runs, in Chrome and on the engine's JS
//!   `Realm`: the fallback, and what Chromium is compared against.
//!
//! * [`emit_rust`] translates the IR into a Rust module for `cw_ui::GenProgram`:
//!   ahead-of-time code for apps built into the binary (`cw-tsx build --emit rust`).
//!
//! The parser is oxc (`oxc_parser`, with its transformer and code generator for the
//! fallback). It parses all of TypeScript and JSX, so the fallback exists for any
//! valid module, not only for the subset; it is pure Rust and builds for wasm32. See
//! docs/tsx-apps.md.

pub mod corpus;
pub mod emit_js;
pub mod emit_rust;
pub mod island_check;
pub mod lower;
mod scan;
mod types;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Something the compiler could not accept, at a 1-based line and column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    /// The module it is in, relative to the entry's directory; empty for the entry
    /// of a single-file build.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub message: String,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if !self.file.is_empty() {
            write!(f, "{}: ", self.file)?;
        }
        write!(f, "line {}:{}: {}", self.line, self.col, self.message)
    }
}

impl Diagnostic {
    /// A diagnostic at byte offset `offset` of `source`.
    pub fn at(source: &str, offset: u32, message: String) -> Diagnostic {
        let (line, col) = line_col(source, offset);
        Diagnostic {
            file: String::new(),
            line,
            col,
            message,
        }
    }

    pub(crate) fn from_oxc(source: &str, d: &oxc_diagnostics::OxcDiagnostic) -> Diagnostic {
        let offset = d.labels.first().map(|l| l.offset()).unwrap_or(0);
        Diagnostic::at(source, offset, d.message.to_string())
    }
}

/// 1-based line and column (in characters) of a byte offset.
pub fn line_col(source: &str, offset: u32) -> (u32, u32) {
    let offset = (offset as usize).min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() as u32 + 1;
    let col = before
        .rsplit('\n')
        .next()
        .map(|l| l.chars().count())
        .unwrap_or(0) as u32
        + 1;
    (line, col)
}

/// One module of an app: its path (relative to the entry's directory), its text and
/// where its relative imports resolved (specifier to index in the module list).
#[derive(Clone, Debug, Default)]
pub struct Source {
    pub file: String,
    pub text: String,
    pub imports: BTreeMap<String, usize>,
    /// A module of an npm package (JavaScript, from `node_modules`): it runs on the
    /// app's island, never compiled.
    pub package: bool,
    /// Its import specifiers in source order (modules and stylesheets alike).
    pub order: Vec<String>,
    /// The build's `process.env` (`LoadOptions::env`).
    pub env: std::sync::Arc<BTreeMap<String, String>>,
    /// A CommonJS module (`require`, `module.exports`): a package's, run when first
    /// required or imported, as a bundler runs it.
    pub commonjs: bool,
    /// The stylesheets it imports, by specifier: the file and its text.
    pub stylesheets: BTreeMap<String, (String, String)>,
}

impl Source {
    /// A one-module app.
    pub fn single(text: &str, file: &str) -> Source {
        Source {
            file: file.to_owned(),
            text: text.to_owned(),
            imports: BTreeMap::new(),
            package: false,
            order: Vec::new(),
            env: Default::default(),
            commonjs: false,
            stylesheets: BTreeMap::new(),
        }
    }

    /// A declaration file (`.d.ts`): types and ambient declarations only, visible to
    /// every module of the app, and no code.
    pub fn is_ambient(&self) -> bool {
        self.file.ends_with(".d.ts")
    }

    /// The file name diagnostics carry: empty for a one-module app.
    pub(crate) fn display_file(&self, modules: usize) -> String {
        if modules > 1 || self.is_ambient() {
            self.file.clone()
        } else {
            String::new()
        }
    }
}

/// How many modules of an app hold code (not declaration files).
pub(crate) fn code_modules(sources: &[Source]) -> usize {
    sources.iter().filter(|s| !s.is_ambient()).count()
}

/// Whether an import specifier names a module of the app (not a package).
fn is_relative(spec: &str) -> bool {
    spec.starts_with("./") || spec.starts_with("../")
}

/// `dir/./a/../b` → `dir/b`.
fn normalize(path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|l| *l != "..") {
                    parts.pop();
                } else {
                    parts.push("..");
                }
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// The module specifiers a source imports or re-exports from.
fn specifiers(text: &str) -> Vec<(String, u32)> {
    specifiers_of(text, "x.tsx")
}

/// The source type a module's file name says it is (JavaScript files of packages
/// are parsed as JavaScript).
pub fn source_type(file: &str) -> oxc_span::SourceType {
    if file.ends_with(".mjs") || file.ends_with(".js") || file.ends_with(".cjs") {
        oxc_span::SourceType::mjs().with_jsx(true)
    } else {
        oxc_span::SourceType::tsx()
    }
}

fn specifiers_of(text: &str, file: &str) -> Vec<(String, u32)> {
    use oxc_ast::ast::Statement as S;
    let allocator = oxc_allocator::Allocator::default();
    let ret = oxc_parser::Parser::new(&allocator, text, source_type(file)).parse();
    let mut out = Vec::new();
    for stmt in &ret.program.body {
        match stmt {
            S::ImportDeclaration(i) => out.push((i.source.value.to_string(), i.span.start)),
            S::ExportFromDeclaration(e) => out.push((e.source.value.to_string(), e.span.start)),
            S::ExportAllDeclaration(e) => out.push((e.source.value.to_string(), e.span.start)),
            _ => {}
        }
    }
    if text.contains("import(") {
        // `import('./x')` with a literal specifier: loaded with the app, as a
        // bundler emits its chunk.
        use oxc_ast_visit::Visit;
        struct D(Vec<(String, u32)>);
        impl<'a> Visit<'a> for D {
            fn visit_import_expression(&mut self, e: &oxc_ast::ast::ImportExpression<'a>) {
                if let oxc_ast::ast::Expression::StringLiteral(s) = &e.source {
                    self.0.push((s.value.to_string(), e.span.start));
                }
                oxc_ast_visit::walk::walk_import_expression(self, e);
            }
        }
        let mut d = D(Vec::new());
        d.visit_program(&ret.program);
        out.extend(d.0);
    }
    out
}

/// Whether a package's JavaScript is CommonJS: a `.cjs` file, or one with no
/// `import`/`export` that uses `require` or `module.exports`/`exports`.
pub fn is_commonjs(text: &str, file: &str) -> bool {
    if file.ends_with(".cjs") {
        return true;
    }
    if !(file.ends_with(".js") || file.ends_with(".jsx")) {
        return false;
    }
    let allocator = oxc_allocator::Allocator::default();
    let ret = oxc_parser::Parser::new(&allocator, text, oxc_span::SourceType::cjs()).parse();
    if ret.program.body.iter().any(|s| s.is_module_declaration()) {
        return false;
    }
    text.contains("require(") || text.contains("exports")
}

/// The `require("…")` specifiers of a CommonJS module, in source order.
fn requires_of(text: &str) -> Vec<(String, u32)> {
    use oxc_ast_visit::Visit;
    struct R(Vec<(String, u32)>);
    impl<'a> Visit<'a> for R {
        fn visit_call_expression(&mut self, c: &oxc_ast::ast::CallExpression<'a>) {
            if let oxc_ast::ast::Expression::Identifier(id) = &c.callee {
                if id.name == "require" && c.arguments.len() == 1 {
                    if let Some(oxc_ast::ast::Expression::StringLiteral(s)) =
                        c.arguments[0].as_expression()
                    {
                        self.0.push((s.value.to_string(), c.span.start));
                    }
                }
            }
            oxc_ast_visit::walk::walk_call_expression(self, c);
        }
    }
    let allocator = oxc_allocator::Allocator::default();
    let ret = oxc_parser::Parser::new(&allocator, text, oxc_span::SourceType::cjs()).parse();
    let mut r = R(Vec::new());
    r.visit_program(&ret.program);
    r.0
}

/// The paths of a source's `/// <reference path="…" />` directives.
fn references(text: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let mut offset = 0u32;
    for line in text.split_inclusive('\n') {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("///") {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix("<reference") {
                if let Some(i) = rest.find("path=") {
                    let rest = &rest[i + 5..];
                    let q = rest.chars().next().unwrap_or('"');
                    if let Some(end) = rest[1..].find(q) {
                        out.push((rest[1..1 + end].to_owned(), offset));
                    }
                }
            }
        }
        offset += line.len() as u32;
    }
    out
}

/// Loads the app whose entry is `entry` (a path relative to the app's root, e.g.
/// `main.tsx`): the entry and every module it reaches through relative imports,
/// resolved as a bundler does (`./x` is `x`, `x.tsx`, `x.ts`, `x/index.tsx` or
/// `x/index.ts`), read with `read`. The list is in dependency order, the entry last.
pub fn load(
    entry: &str,
    read: &mut dyn FnMut(&str) -> Option<String>,
) -> Result<Vec<Source>, Vec<Diagnostic>> {
    let (sources, errors) = load_with(entry, read, &LoadOptions::default());
    if errors.is_empty() {
        Ok(sources)
    } else {
        Err(errors)
    }
}

/// How [`load_with`] resolves imports.
#[derive(Clone, Debug, Default)]
pub struct LoadOptions {
    /// Path aliases, as a `tsconfig.json`'s `paths` spells them for a bundler: an
    /// import starting with a key (`@/`) resolves as the path relative to the app's
    /// root that the value names (`src/`) followed by the rest of the specifier.
    pub aliases: Vec<(String, String)>,
    /// Where npm packages are (`node_modules`, relative to the app's root): a bare
    /// specifier resolves there as Node's ESM resolution does (`exports` with the
    /// `import`/`module`/`default` conditions, else `module`, else `main`). `None`
    /// leaves package imports unresolved.
    pub node_modules: Option<String>,
    /// What the build defines as `process.env` (a bundler's DefinePlugin, as Create
    /// React App defines `REACT_APP_*`): `process.env.NAME` is that string,
    /// `NODE_ENV` is `production` unless set, and any other name `undefined`.
    pub env: BTreeMap<String, String>,
}

/// What a relative import of a stylesheet or media file is, as Vite serves it: its
/// URL, the path from the app's root (`import logo from './logo.svg'`), for an
/// importer `file`. `None` for a module, a CSS module (`.module.css`, whose
/// default export is a class map) or an import with a query (`?raw`).
pub fn asset_url(file: &str, spec: &str) -> Option<String> {
    if !is_relative(spec) || spec.contains('?') || spec.contains(".module.") {
        return None;
    }
    let ext = spec.rsplit('.').next()?.to_ascii_lowercase();
    const ASSETS: &[&str] = &[
        "css", "scss", "sass", "less", "png", "jpg", "jpeg", "gif", "svg", "webp", "avif", "ico",
        "bmp", "woff", "woff2", "ttf", "otf", "mp3", "mp4", "webm", "wav", "ogg",
    ];
    if !ASSETS.contains(&ext.as_str()) {
        return None;
    }
    let dir = match file.rfind('/') {
        Some(i) => &file[..i],
        None => "",
    };
    Some(format!("/{}", normalize(&format!("{dir}/{spec}"))))
}

/// The app's stylesheet as its bundler would emit it: every stylesheet the
/// modules import, each once, in the order the modules run (depth first from the
/// entry, each module's imports in source order).
pub fn stylesheet(sources: &[Source]) -> String {
    fn visit(
        sources: &[Source],
        i: usize,
        seen: &mut Vec<bool>,
        done: &mut std::collections::BTreeSet<String>,
        out: &mut String,
    ) {
        if seen[i] {
            return;
        }
        seen[i] = true;
        let s = &sources[i];
        for spec in &s.order {
            if let Some((path, css)) = s.stylesheets.get(spec) {
                if done.insert(path.clone()) {
                    out.push_str(&format!("/* {path} */\n{css}\n"));
                }
            } else if let Some(&m) = s.imports.get(spec) {
                visit(sources, m, seen, done, out);
            }
        }
    }
    let mut out = String::new();
    if sources.is_empty() {
        return out;
    }
    let mut seen = vec![false; sources.len()];
    let mut done = std::collections::BTreeSet::new();
    visit(sources, sources.len() - 1, &mut seen, &mut done, &mut out);
    out
}

/// Whether a relative import names a stylesheet (imported for its effect).
pub fn is_stylesheet(spec: &str) -> bool {
    [".css", ".scss", ".sass", ".less"]
        .iter()
        .any(|e| spec.to_ascii_lowercase().ends_with(e))
}

/// Modules the island's React shim provides (never read from `node_modules`).
pub fn is_shim_module(spec: &str) -> bool {
    matches!(
        spec,
        "react" | "react-dom" | "react-dom/client" | "react/jsx-runtime" | "react/jsx-dev-runtime"
    )
}

/// `@scope/name/sub` → (`@scope/name`, `sub`); `name/sub` → (`name`, `sub`).
fn package_parts(spec: &str) -> (&str, &str) {
    let first = spec.find('/');
    let end = if spec.starts_with('@') {
        first.and_then(|i| spec[i + 1..].find('/').map(|j| i + 1 + j))
    } else {
        first
    };
    match end {
        Some(e) => (&spec[..e], &spec[e + 1..]),
        None => (spec, ""),
    }
}

/// A JSON value with its objects' keys in document order (what `exports`
/// conditions are matched in).
enum OrderedJson {
    Str(String),
    Obj(Vec<(String, OrderedJson)>),
    Arr(Vec<OrderedJson>),
    Other,
}

impl OrderedJson {
    /// `text`'s value at key `field` of its top-level object, read as a JavaScript
    /// object literal (which keeps key order).
    fn field(text: &str, field: &str) -> Option<OrderedJson> {
        let src = format!("({text})");
        let allocator = oxc_allocator::Allocator::default();
        let ret = oxc_parser::Parser::new(&allocator, &src, oxc_span::SourceType::mjs())
            .parse_expression();
        let e = ret.ok()?;
        let top = Self::of(&e);
        match top {
            OrderedJson::Obj(fields) => {
                fields.into_iter().find(|(k, _)| k == field).map(|(_, v)| v)
            }
            _ => None,
        }
    }

    fn of(e: &oxc_ast::ast::Expression<'_>) -> OrderedJson {
        use oxc_ast::ast::{ArrayExpressionElement, Expression as E, ObjectPropertyKind};
        match e.without_parentheses() {
            E::StringLiteral(s) => OrderedJson::Str(s.value.to_string()),
            E::ObjectExpression(o) => OrderedJson::Obj(
                o.properties
                    .iter()
                    .filter_map(|p| match p {
                        ObjectPropertyKind::ObjectProperty(p) => {
                            Some((p.key.static_name()?.to_string(), Self::of(&p.value)))
                        }
                        _ => None,
                    })
                    .collect(),
            ),
            E::ArrayExpression(a) => OrderedJson::Arr(
                a.elements
                    .iter()
                    .filter_map(|x| match x {
                        ArrayExpressionElement::SpreadElement(_)
                        | ArrayExpressionElement::Elision(_) => None,
                        x => x.as_expression().map(Self::of),
                    })
                    .collect(),
            ),
            _ => OrderedJson::Other,
        }
    }
}

/// The conditions a production browser bundle resolves `exports` with (webpack's
/// for `mode: production`, `target: web`, an ES import).
const CONDITIONS: &[&str] = &[
    "webpack",
    "production",
    "browser",
    "import",
    "module",
    "default",
];

/// The target of a package.json `exports` entry: the first key, in the entry's
/// own order, that is an active condition (Node's and the bundlers' rule); with
/// `require` too when nothing else matches.
fn export_target(v: &OrderedJson) -> Option<String> {
    fn go(v: &OrderedJson, require: bool) -> Option<String> {
        match v {
            OrderedJson::Str(s) => Some(s.clone()),
            OrderedJson::Obj(o) => o.iter().find_map(|(k, v)| {
                (CONDITIONS.contains(&k.as_str()) || (require && k == "require"))
                    .then(|| go(v, require))
                    .flatten()
            }),
            OrderedJson::Arr(a) => a.iter().find_map(|x| go(x, require)),
            OrderedJson::Other => None,
        }
    }
    go(v, false).or_else(|| go(v, true))
}

/// The file a bare specifier names in `node_modules` (relative to the app's root).
fn resolve_package(
    spec: &str,
    node_modules: &str,
    read: &mut dyn FnMut(&str) -> Option<String>,
) -> Option<String> {
    let (name, sub) = package_parts(spec);
    let dir = format!("{node_modules}/{name}");
    let pkg: serde_json::Value = read(&format!("{dir}/package.json"))
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or(serde_json::Value::Null);
    let key = if sub.is_empty() {
        ".".to_owned()
    } else {
        format!("./{sub}")
    };
    let exports =
        read(&format!("{dir}/package.json")).and_then(|t| OrderedJson::field(&t, "exports"));
    if let Some(exports) = &exports {
        let entry = match exports {
            OrderedJson::Obj(o) if o.iter().any(|(k, _)| k.starts_with('.')) => {
                match o.iter().find(|(k, _)| *k == key) {
                    Some((_, e)) => export_target(e),
                    // A subpath pattern (`"./*": "./esm/*.mjs"`).
                    None => o.iter().find_map(|(k, v)| {
                        let (pre, post) = k.split_once('*')?;
                        let mid = key.strip_prefix(pre)?.strip_suffix(post)?;
                        Some(export_target(v)?.replace('*', mid))
                    }),
                }
            }
            other if sub.is_empty() => export_target(other),
            _ => None,
        };
        if let Some(t) = entry {
            return Some(normalize(&format!("{dir}/{t}")));
        }
    }
    let base = if sub.is_empty() {
        let main = pkg
            .get("module")
            .or_else(|| pkg.get("main"))
            .and_then(|m| m.as_str())
            .unwrap_or("index.js");
        normalize(&format!("{dir}/{main}"))
    } else {
        normalize(&format!("{dir}/{sub}"))
    };
    [
        base.clone(),
        format!("{base}.mjs"),
        format!("{base}.js"),
        format!("{base}/index.mjs"),
        format!("{base}/index.js"),
    ]
    .into_iter()
    .find(|c| read(c).is_some())
}

/// [`load`] with options, lenient: an import that names no module of the app is
/// reported and left out of the source's `imports` (the lowerer then treats it as
/// the package import it cannot compile), and every module that could be read is
/// returned, so the parts of an app that do resolve can still be lowered.
pub fn load_with(
    entry: &str,
    read: &mut dyn FnMut(&str) -> Option<String>,
    options: &LoadOptions,
) -> (Vec<Source>, Vec<Diagnostic>) {
    struct Loader<'r, 'o> {
        read: &'r mut dyn FnMut(&str) -> Option<String>,
        options: &'o LoadOptions,
        out: Vec<Source>,
        index: BTreeMap<String, usize>,
        /// Files being loaded (an import of one of them closes a cycle).
        open: Vec<String>,
        /// Imports that close a cycle: the importer, its specifier, the file.
        back: Vec<(String, String, String)>,
        errors: Vec<Diagnostic>,
    }
    enum Found {
        Done(usize),
        Open(String),
    }
    impl Loader<'_, '_> {
        /// Loads `file` and what it imports; `None` when it cannot be read.
        fn visit(&mut self, file: &str) -> Option<Found> {
            if let Some(i) = self.index.get(file) {
                return Some(Found::Done(*i));
            }
            if self.open.iter().any(|f| f == file) {
                // A cycle, as ES modules allow: this import is patched in once
                // the file is loaded.
                return Some(Found::Open(file.to_owned()));
            }
            let text = (self.read)(file)?;
            // A JSON module is its value as a default export.
            let text = if file.ends_with(".json") {
                format!("export default {};\n", text.trim())
            } else {
                text
            };
            self.open.push(file.to_owned());
            let dir = match file.rfind('/') {
                Some(i) => &file[..i],
                None => "",
            };
            // Declaration files a module references come before it, like its imports.
            for (path, at) in references(&text) {
                let target = normalize(&format!("{dir}/{path}"));
                if self.visit(&target).is_none() {
                    let mut d = Diagnostic::at(&text, at, format!("cannot find `{path}`"));
                    d.file = file.to_owned();
                    self.errors.push(d);
                }
            }
            let mut imports = BTreeMap::new();
            let mut order = Vec::new();
            let mut stylesheets = BTreeMap::new();
            let in_package = file.starts_with(&format!(
                "{}/",
                self.options.node_modules.as_deref().unwrap_or("\u{0}")
            ));
            let commonjs = in_package && is_commonjs(&text, file);
            let specs = if commonjs {
                requires_of(&text)
            } else {
                specifiers_of(&text, file)
            };
            for (spec, at) in specs {
                order.push(spec.clone());
                if asset_url(file, &spec).is_some() {
                    // A stylesheet or media file, which a bundler serves, not a module;
                    // a stylesheet's text goes into the page's (see `stylesheet`).
                    if is_stylesheet(&spec) {
                        let path = normalize(&format!("{dir}/{spec}"));
                        if let Some(css) = (self.read)(&path) {
                            stylesheets.insert(spec.clone(), (path, css));
                        }
                    }
                    continue;
                }
                if !is_relative(&spec) && is_stylesheet(&spec) {
                    // A package's stylesheet (`import 'todomvc-app-css/index.css'`).
                    if let Some(nm) = self.options.node_modules.clone() {
                        if let Some(path) = resolve_package(&spec, &nm, self.read) {
                            if let Some(css) = (self.read)(&path) {
                                stylesheets.insert(spec.clone(), (path, css));
                                continue;
                            }
                        }
                    }
                    let mut d =
                        Diagnostic::at(&text, at, format!("cannot find stylesheet `{spec}`"));
                    d.file = file.to_owned();
                    self.errors.push(d);
                    continue;
                }
                let base = if is_relative(&spec) {
                    normalize(&format!("{dir}/{spec}"))
                } else if let Some((from, to)) = self
                    .options
                    .aliases
                    .iter()
                    .find(|(from, _)| spec.starts_with(from.as_str()))
                {
                    normalize(&format!("{to}{}", &spec[from.len()..]))
                } else if let (Some(nm), false) =
                    (self.options.node_modules.clone(), is_shim_module(&spec))
                {
                    match resolve_package(&spec, &nm, self.read) {
                        Some(f) => f,
                        None => {
                            let mut d =
                                Diagnostic::at(&text, at, format!("cannot find package `{spec}`"));
                            d.file = file.to_owned();
                            self.errors.push(d);
                            continue;
                        }
                    }
                } else {
                    continue;
                };
                // `./x.js` names `x.ts` in TypeScript's ES module resolution.
                let stem = [".js", ".jsx", ".mjs"]
                    .iter()
                    .find_map(|e| base.strip_suffix(e))
                    .map(str::to_owned);
                let mut candidates = vec![
                    base.clone(),
                    format!("{base}.tsx"),
                    format!("{base}.ts"),
                    format!("{base}/index.tsx"),
                    format!("{base}/index.ts"),
                    format!("{base}.mjs"),
                    format!("{base}.js"),
                    format!("{base}/index.mjs"),
                    format!("{base}/index.js"),
                    format!("{base}.jsx"),
                    format!("{base}/index.jsx"),
                    format!("{base}.d.ts"),
                    format!("{base}/index.d.ts"),
                ];
                if let Some(stem) = stem {
                    candidates.push(format!("{stem}.tsx"));
                    candidates.push(format!("{stem}.ts"));
                }
                let mut found = None;
                for c in &candidates {
                    if let Some(i) = self.index.get(c) {
                        found = Some(Found::Done(*i));
                        break;
                    }
                    let code = [".tsx", ".ts", ".mjs", ".js", ".cjs", ".jsx", ".json"]
                        .iter()
                        .any(|e| c.ends_with(e));
                    if !code {
                        continue;
                    }
                    if self.open.iter().any(|f| f == c) {
                        found = Some(Found::Open(c.clone()));
                        break;
                    }
                    if (self.read)(c).is_none() {
                        continue;
                    }
                    found = self.visit(c);
                    if found.is_some() {
                        break;
                    }
                }
                match found {
                    Some(Found::Done(i)) => {
                        imports.insert(spec, i);
                    }
                    Some(Found::Open(target)) => {
                        self.back.push((file.to_owned(), spec, target));
                    }
                    None => {
                        let mut d =
                            Diagnostic::at(&text, at, format!("cannot find module `{spec}`"));
                        d.file = file.to_owned();
                        self.errors.push(d);
                    }
                }
            }
            self.open.pop();
            let i = self.out.len();
            self.out.push(Source {
                package: file.starts_with(&format!(
                    "{}/",
                    self.options.node_modules.as_deref().unwrap_or("\u{0}")
                )),
                file: file.to_owned(),
                text,
                imports,
                order,
                env: std::sync::Arc::new(self.options.env.clone()),
                commonjs,
                stylesheets,
            });
            self.index.insert(file.to_owned(), i);
            Some(Found::Done(i))
        }
    }
    let mut l = Loader {
        read,
        options,
        out: Vec::new(),
        index: BTreeMap::new(),
        open: Vec::new(),
        back: Vec::new(),
        errors: Vec::new(),
    };
    let entry = normalize(entry);
    if l.visit(&entry).is_none() && l.errors.is_empty() {
        l.errors.push(Diagnostic {
            file: entry.clone(),
            line: 1,
            col: 1,
            message: "cannot read the entry module".into(),
        });
    }
    for (importer, spec, target) in std::mem::take(&mut l.back) {
        if let (Some(&i), Some(&t)) = (l.index.get(&importer), l.index.get(&target)) {
            l.out[i].imports.insert(spec, t);
        }
    }
    (l.out, l.errors)
}

/// Several modules written as one text, each starting at a line `// @file <path>`
/// (as tests write small apps): the files in order, or `None` without markers.
pub fn virtual_files(text: &str) -> Option<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.split_inclusive('\n') {
        if let Some(name) = line.trim().strip_prefix("// @file ") {
            out.push((name.trim().to_owned(), String::new()));
        } else if let Some((_, body)) = out.last_mut() {
            body.push_str(line);
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Loads and compiles an app given as [`virtual_files`] (the first is the entry).
pub fn build_virtual(files: &[(String, String)]) -> Result<Build, Vec<Diagnostic>> {
    build_virtual_with(files, None)
}

/// [`build_virtual`] with npm packages read from `packages` (a directory holding
/// them, as `node_modules` does), which `node_modules/…` paths name.
pub fn build_virtual_with(
    files: &[(String, String)],
    packages: Option<&std::path::Path>,
) -> Result<Build, Vec<Diagnostic>> {
    let map: BTreeMap<&str, &str> = files
        .iter()
        .map(|(n, t)| (n.as_str(), t.as_str()))
        .collect();
    let entry = files.first().map(|(n, _)| n.clone()).unwrap_or_default();
    let options = LoadOptions {
        aliases: Vec::new(),
        node_modules: packages.map(|_| "node_modules".to_owned()),
        env: Default::default(),
    };
    let mut read = |f: &str| {
        if let Some(s) = map.get(f) {
            return Some((*s).to_owned());
        }
        let rest = f.strip_prefix("node_modules/")?;
        std::fs::read_to_string(packages?.join(rest)).ok()
    };
    let (sources, errors) = load_with(&entry, &mut read, &options);
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(build_modules(&sources))
}

/// Compiles an app of several modules (from `load`) both ways.
pub fn build_modules(sources: &[Source]) -> Build {
    build_modules_with_island(sources, &[])
}

/// [`build_modules`], with the modules `files` names on the island from the start
/// (whether or not they compile), for measuring what the island costs.
pub fn build_modules_with_island(sources: &[Source], files: &[&str]) -> Build {
    let (js, js_errors) = match emit_js::emit_modules(sources) {
        Ok(js) => (Some(js), Vec::new()),
        Err(e) => (None, e),
    };
    let forced: Vec<usize> = (0..sources.len())
        .filter(|&i| files.contains(&sources[i].file.as_str()))
        .collect();
    let (mut ir, mut diagnostics, vm, outside) = lower_with_islands(sources, forced);
    // What would run on the island must be able to: else the app is React's.
    // App code the island cannot run refuses the build. A package's is only
    // noted: a library holds code for platforms and modes an app never reaches,
    // so what it needs is judged when it runs (a missing global throws there).
    let mut island_notes = Vec::new();
    if ir.is_some() {
        let mut refused = Vec::new();
        for (i, s) in sources.iter().enumerate() {
            if s.package {
                island_notes.extend(island_check::check(s));
            } else if vm.contains(&i) {
                refused.extend(island_check::check(s));
            }
        }
        if !refused.is_empty() {
            diagnostics.extend(outside.iter().cloned());
            diagnostics.extend(refused);
            ir = None;
        }
    }
    // Packages the compiled code imports, and the app's modules outside the
    // subset, run on the app's island.
    if let Some(island) = ir.as_mut().and_then(|m| m.island.as_mut()) {
        match emit_js::emit_island(sources, island, &vm) {
            Ok(script) => island.script = script,
            Err(e) => {
                diagnostics.extend(e);
                ir = None;
            }
        }
    }
    for e in &js_errors {
        if !diagnostics.contains(e) {
            diagnostics.push(e.clone());
        }
    }
    diagnostics.sort_by(|a, b| (&a.file, a.line, a.col).cmp(&(&b.file, b.line, b.col)));
    let island_modules = if ir.is_some() {
        vm.iter().map(|&i| sources[i].file.clone()).collect()
    } else {
        Vec::new()
    };
    Build {
        ir,
        js,
        diagnostics,
        js_errors,
        island_modules,
        outside,
        island_notes,
    }
}

const NEVER_RENDERS: &str = "the module never renders";

/// Whether a module's text calls one of React DOM's ways to render a root.
fn renders(text: &str) -> bool {
    ["createRoot(", "hydrateRoot(", ".render("]
        .iter()
        .any(|c| text.contains(c))
}

/// Lowers the app, moving each module with code outside the subset onto the
/// island (with any module it imports from later in the order, so the island's
/// modules never wait on compiled ones mid-cycle) until the rest lowers. An
/// entry on the island renders through the shim's `createRoot`. Returns the IR or the
/// diagnostics that stop it, the modules on the island, and the diagnostics the
/// island absorbed.
#[allow(clippy::type_complexity)]
fn lower_with_islands(
    sources: &[Source],
    forced: Vec<usize>,
) -> (
    Option<cw_ui::ir::Module>,
    Vec<Diagnostic>,
    Vec<usize>,
    Vec<Diagnostic>,
) {
    let code = code_modules(sources);
    let entry = sources.len().saturating_sub(1);
    let mut options = lower::LowerOptions {
        vm_modules: forced,
        ..lower::LowerOptions::default()
    };
    let mut outside: Vec<Diagnostic> = Vec::new();
    loop {
        close_over_cycles(sources, &mut options.vm_modules);
        let result = lower::lower_modules_with(sources, &options);
        if options.vm_modules.contains(&entry) && !renders(&sources[entry].text) {
            // A cycle took along an entry that cannot render from the island.
            let mut all = outside;
            all.extend(result.err().unwrap_or_default());
            all.push(Diagnostic {
                file: sources[entry].display_file(code),
                line: 1,
                col: 1,
                message: "the entry is in an import cycle with code outside the subset, and renders only compiled".into(),
            });
            return (None, all, Vec::new(), Vec::new());
        }
        let d = match result {
            Ok(m) => return (Some(m), Vec::new(), options.vm_modules, outside),
            Err(d) => d,
        };
        let mut add: Vec<usize> = Vec::new();
        let mut stuck = false;
        // With code outside the subset, the render call may not have been found
        // for that reason alone: it counts once the rest lowers.
        let others = d.iter().any(|x| !x.message.starts_with(NEVER_RENDERS));
        for x in &d {
            if x.message.starts_with(NEVER_RENDERS) {
                stuck |= !others;
                continue;
            }
            let m = (0..sources.len()).find(|&i| sources[i].display_file(code) == x.file);
            match m {
                // An entry goes to the island only if it renders there.
                Some(m) if m == entry && !renders(&sources[m].text) => stuck = true,
                Some(m) if !sources[m].package && !sources[m].is_ambient() => {
                    if !options.vm_modules.contains(&m) && !add.contains(&m) {
                        add.push(m);
                    }
                }
                _ => stuck = true,
            }
        }
        if stuck || add.is_empty() {
            let mut all = outside;
            all.extend(d);
            return (None, all, Vec::new(), Vec::new());
        }
        outside.extend(d);
        options.vm_modules.extend(add);
    }
}

/// Grows `vm` over import cycles: a module on the island runs when its place in
/// the order comes, so one that imports a module later in the order, or is
/// imported by a compiled module earlier in it (either way, across a cycle),
/// would be read before it runs. Both ends go to the island.
fn close_over_cycles(sources: &[Source], vm: &mut Vec<usize>) {
    loop {
        let mut more = Vec::new();
        for (c, src) in sources.iter().enumerate() {
            if src.package {
                continue;
            }
            for &m in src.imports.values() {
                if m <= c || sources[m].package {
                    continue;
                }
                // `c` imports `m`, which comes later: a back edge of a cycle.
                let (c_vm, m_vm) = (vm.contains(&c), vm.contains(&m));
                if c_vm && !m_vm && !more.contains(&m) {
                    more.push(m);
                }
                if m_vm && !c_vm && !more.contains(&c) {
                    more.push(c);
                }
            }
        }
        if more.is_empty() {
            break;
        }
        vm.extend(more);
    }
    vm.sort_unstable();
}

/// What `build` produced for one module.
#[derive(Clone, Debug)]
pub struct Build {
    /// The IR, when the whole module is inside the subset.
    pub ir: Option<cw_ui::ir::Module>,
    /// The fallback script, when the module parses and transforms.
    pub js: Option<String>,
    /// Why the IR (or the script) is missing; empty when both were produced.
    pub diagnostics: Vec<Diagnostic>,
    /// Reasons the script could not be produced (a subset of `diagnostics`).
    pub js_errors: Vec<Diagnostic>,
    /// The app's modules that run on the island because they are outside the
    /// subset (empty when the whole app compiled, or when it did not build).
    pub island_modules: Vec<String>,
    /// What put those modules outside the subset.
    pub outside: Vec<Diagnostic>,
    /// What the app's packages reference that the island lacks (not a refusal:
    /// it matters only if that code runs).
    pub island_notes: Vec<Diagnostic>,
}

/// Compiles a one-module app both ways.
pub fn build(source: &str, file_name: &str) -> Build {
    build_modules(&[Source::single(source, file_name)])
}
