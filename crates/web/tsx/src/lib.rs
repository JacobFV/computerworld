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
}

impl Source {
    /// A one-module app.
    pub fn single(text: &str, file: &str) -> Source {
        Source {
            file: file.to_owned(),
            text: text.to_owned(),
            imports: BTreeMap::new(),
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
    use oxc_ast::ast::Statement as S;
    let allocator = oxc_allocator::Allocator::default();
    let ret = oxc_parser::Parser::new(&allocator, text, oxc_span::SourceType::tsx()).parse();
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
    #[allow(clippy::too_many_arguments)]
    fn visit(
        file: &str,
        read: &mut dyn FnMut(&str) -> Option<String>,
        options: &LoadOptions,
        out: &mut Vec<Source>,
        index: &mut BTreeMap<String, usize>,
        stack: &mut Vec<String>,
        errors: &mut Vec<Diagnostic>,
    ) -> Option<usize> {
        if let Some(i) = index.get(file) {
            return Some(*i);
        }
        if let Some(pos) = stack.iter().position(|f| f == file) {
            let cycle: Vec<&str> = stack[pos..]
                .iter()
                .map(String::as_str)
                .chain([file])
                .collect();
            errors.push(Diagnostic {
                file: file.to_owned(),
                line: 1,
                col: 1,
                message: format!("import cycle: {}", cycle.join(" → ")),
            });
            return None;
        }
        let text = read(file)?;
        stack.push(file.to_owned());
        let dir = match file.rfind('/') {
            Some(i) => &file[..i],
            None => "",
        };
        // Declaration files a module references come before it, like its imports.
        for (path, at) in references(&text) {
            let target = normalize(&format!("{dir}/{path}"));
            if visit(&target, read, options, out, index, stack, errors).is_none() {
                let mut d = Diagnostic::at(&text, at, format!("cannot find `{path}`"));
                d.file = file.to_owned();
                errors.push(d);
            }
        }
        let mut imports = BTreeMap::new();
        for (spec, at) in specifiers(&text) {
            let base = if is_relative(&spec) {
                normalize(&format!("{dir}/{spec}"))
            } else if let Some((from, to)) = options
                .aliases
                .iter()
                .find(|(from, _)| spec.starts_with(from.as_str()))
            {
                normalize(&format!("{to}{}", &spec[from.len()..]))
            } else {
                continue;
            };
            let candidates = [
                base.clone(),
                format!("{base}.tsx"),
                format!("{base}.ts"),
                format!("{base}/index.tsx"),
                format!("{base}/index.ts"),
            ];
            let found = candidates.iter().find_map(|c| {
                if let Some(i) = index.get(c) {
                    return Some(*i);
                }
                if c.ends_with(".tsx") || c.ends_with(".ts") {
                    let probe = read(c)?;
                    drop(probe);
                    visit(c, read, options, out, index, stack, errors)
                } else {
                    None
                }
            });
            match found {
                Some(i) => {
                    imports.insert(spec, i);
                }
                None => {
                    let mut d = Diagnostic::at(&text, at, format!("cannot find module `{spec}`"));
                    d.file = file.to_owned();
                    errors.push(d);
                }
            }
        }
        stack.pop();
        let i = out.len();
        out.push(Source {
            file: file.to_owned(),
            text,
            imports,
        });
        index.insert(file.to_owned(), i);
        Some(i)
    }
    let mut out = Vec::new();
    let mut index = BTreeMap::new();
    let mut errors = Vec::new();
    let entry = normalize(entry);
    if visit(
        &entry,
        read,
        options,
        &mut out,
        &mut index,
        &mut Vec::new(),
        &mut errors,
    )
    .is_none()
        && errors.is_empty()
    {
        errors.push(Diagnostic {
            file: entry.clone(),
            line: 1,
            col: 1,
            message: "cannot read the entry module".into(),
        });
    }
    (out, errors)
}

/// Compiles an app of several modules (from `load`) both ways.
pub fn build_modules(sources: &[Source]) -> Build {
    let (js, js_errors) = match emit_js::emit_modules(sources) {
        Ok(js) => (Some(js), Vec::new()),
        Err(e) => (None, e),
    };
    let (ir, mut diagnostics) = match lower::lower_modules(sources) {
        Ok(m) => (Some(m), Vec::new()),
        Err(d) => (None, d),
    };
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
