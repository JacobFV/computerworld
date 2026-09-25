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
}

impl Source {
    /// A one-module app.
    pub fn single(text: &str, file: &str) -> Source {
        Source {
            file: file.to_owned(),
            text: text.to_owned(),
            imports: BTreeMap::new(),
            package: false,
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
    out
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

/// The target of a package.json `exports` entry under the conditions an ES module
/// bundle for a browser uses.
fn export_target(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Object(o) => {
            for c in ["browser", "import", "module", "default", "require"] {
                if let Some(t) = o.get(c).and_then(export_target) {
                    return Some(t);
                }
            }
            None
        }
        serde_json::Value::Array(a) => a.iter().find_map(export_target),
        _ => None,
    }
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
    if let Some(exports) = pkg.get("exports") {
        let entry = match exports {
            serde_json::Value::Object(o) if o.keys().any(|k| k.starts_with('.')) => {
                match o.get(&key) {
                    Some(e) => export_target(e),
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
            for (spec, at) in specifiers_of(&text, file) {
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
    let (js, js_errors) = match emit_js::emit_modules(sources) {
        Ok(js) => (Some(js), Vec::new()),
        Err(e) => (None, e),
    };
    let (mut ir, mut diagnostics) = match lower::lower_modules(sources) {
        Ok(m) => (Some(m), Vec::new()),
        Err(d) => (None, d),
    };
    // Packages the compiled code imports run on the app's island.
    if let Some(island) = ir.as_mut().and_then(|m| m.island.as_mut()) {
        match emit_js::emit_island(sources, &island.imports) {
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
    Build {
        ir,
        js,
        diagnostics,
        js_errors,
    }
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
}

/// Compiles a one-module app both ways.
pub fn build(source: &str, file_name: &str) -> Build {
    build_modules(&[Source::single(source, file_name)])
}
