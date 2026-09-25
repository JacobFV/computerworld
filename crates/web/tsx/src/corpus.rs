//! The held-out corpus evaluation: how much real React + TypeScript code, written
//! by other people and never used to tune the compiler, falls inside the compiled
//! subset. See `crates/web/tsx/corpus/manifest.json` (which `assemble.mjs` writes)
//! and docs/tsx-apps.md, "Held-out corpus".
//!
//! Units, per project of the manifest:
//!
//! * **module**: every `.ts`/`.tsx` file that is not a declaration file. It is
//!   *inside* when lowering it (as an entry, with the modules of the app it
//!   imports) reports nothing located in it, the one exception being "the module
//!   never renders", which a component file is not meant to do.
//! * **function**: every top-level function of a module (a declaration, or a
//!   `const` bound to an arrow, a function expression or a call wrapping one, like
//!   `memo(…)`/`forwardRef(…)`), and every top-level class. It is *inside* when no
//!   diagnostic of its module's lowering lies within it. A **component** is a
//!   function or class whose name starts with a capital letter.
//! * **app**: a project with an entry module (one that calls `createRoot(`,
//!   `hydrateRoot(` or `ReactDOM.render(`). It is *compiled* when `load` and
//!   `build_modules` of its entry report nothing at all: the page would run on
//!   `cw-ui` with no JS VM.
//!
//! Every diagnostic is put under a cause (its message with names that only
//! identify a place taken out). Two histograms: diagnostics per cause, and
//! functions per cause (a function blocked by a cause counts once for it).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use oxc_ast::ast;
use serde::{Deserialize, Serialize};

use crate::{Diagnostic, LoadOptions};

#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub seed: String,
    pub sources: Vec<ManifestSource>,
    pub projects: Vec<ManifestProject>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ManifestSource {
    pub id: String,
    pub repository: String,
    pub commit: String,
    pub license: String,
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ManifestProject {
    pub id: String,
    pub source: String,
    pub root: String,
    pub split: String,
    pub files: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Count {
    pub total: usize,
    pub inside: usize,
}

impl Count {
    fn add(&mut self, inside: bool) {
        self.total += 1;
        self.inside += inside as usize;
    }
    fn merge(&mut self, o: &Count) {
        self.total += o.total;
        self.inside += o.inside;
    }
    pub fn percent(&self) -> f64 {
        if self.total == 0 {
            0.0
        } else {
            100.0 * self.inside as f64 / self.total as f64
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ProjectResult {
    pub id: String,
    pub modules: Count,
    pub functions: Count,
    pub components: Count,
    /// `Some(compiled)` for a project with an entry.
    pub app: Option<bool>,
    /// The entry module, for an app.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// The first few diagnostics of the app's build (or of its modules).
    pub sample: Vec<String>,
    /// Each function outside the subset (`file: name`) with its causes.
    #[serde(default)]
    pub outside: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Report {
    pub split: String,
    pub projects: usize,
    pub modules: Count,
    pub functions: Count,
    pub components: Count,
    pub apps: Count,
    pub diagnostics: usize,
    pub diagnostics_by_cause: BTreeMap<String, usize>,
    pub functions_by_cause: BTreeMap<String, usize>,
    /// Packages imported (`import … from 'x'`, not `react`/`react-dom`), by the
    /// number of modules importing each.
    pub packages: BTreeMap<String, usize>,
    pub per_project: Vec<ProjectResult>,
}

impl Report {
    /// The summary as text: totals, then the top `top` causes.
    pub fn summary(&self, top: usize) -> String {
        let mut s = String::new();
        let line = |s: &mut String, name: &str, c: &Count| {
            s.push_str(&format!(
                "{name:<11} {:>5} / {:<5} {:>5.1}%\n",
                c.inside,
                c.total,
                c.percent()
            ));
        };
        s.push_str(&format!(
            "split {}: {} projects, {} diagnostics\n",
            self.split, self.projects, self.diagnostics
        ));
        line(&mut s, "modules", &self.modules);
        line(&mut s, "functions", &self.functions);
        line(&mut s, "components", &self.components);
        line(&mut s, "apps", &self.apps);
        fn ranked(m: &BTreeMap<String, usize>, top: usize) -> Vec<(&String, &usize)> {
            let mut v: Vec<(&String, &usize)> = m.iter().collect();
            v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            v.into_iter().take(top).collect()
        }
        s.push_str("functions blocked, by cause:\n");
        for (k, n) in ranked(&self.functions_by_cause, top) {
            s.push_str(&format!("  {n:>5}  {k}\n"));
        }
        s.push_str("diagnostics, by cause:\n");
        for (k, n) in ranked(&self.diagnostics_by_cause, top) {
            s.push_str(&format!("  {n:>5}  {k}\n"));
        }
        s
    }
}

/// The cause a diagnostic message is counted under, given the names the module
/// imports from packages (and from modules that do not resolve), by local name.
pub fn cause_in(message: &str, imported: &BTreeMap<String, &'static str>) -> String {
    for prefix in ["unknown name `", "type `"] {
        if let Some(rest) = message.strip_prefix(prefix) {
            let name = rest.split(['`', '.', '<']).next().unwrap_or("");
            if let Some(from) = imported.get(name) {
                return format!(
                    "{} imported from {from}",
                    if prefix == "type `" { "type" } else { "value" }
                );
            }
        }
    }
    cause(message)
}

/// The cause a diagnostic message is counted under.
pub fn cause(message: &str) -> String {
    let m = message;
    let ticked = |s: &str| -> Option<String> {
        let a = s.find('`')?;
        let b = s[a + 1..].find('`')?;
        Some(s[a + 1..a + 1 + b].to_owned())
    };
    let ty_class = |t: &str| -> &'static str {
        let t = t.trim();
        if t == "any" {
            "any"
        } else if t == "unknown" {
            "unknown"
        } else if t.contains('|') {
            "a union"
        } else {
            "another type"
        }
    };
    if let Some(rest) = m.strip_prefix("import from `") {
        let spec = rest.split('`').next().unwrap_or("");
        return if spec.starts_with('.') {
            if spec.ends_with(".css") || spec.ends_with(".scss") {
                "import of a stylesheet".into()
            } else if spec.ends_with(".json") {
                "import of a JSON file".into()
            } else if [".svg", ".png", ".jpg", ".jpeg", ".gif", ".webp"]
                .iter()
                .any(|e| spec.ends_with(e))
            {
                "import of an image".into()
            } else {
                "relative import that does not resolve".into()
            }
        } else {
            "import from a package".into()
        };
    }
    if m.starts_with("cannot find module") {
        return "relative import that does not resolve".into();
    }
    if m.starts_with("import cycle") {
        return "import cycle".into();
    }
    if m.starts_with("parameter `") && m.contains("implicit type any") {
        return "parameter with implicit type any".into();
    }
    if m.starts_with("value of type any") {
        return "value of type any".into();
    }
    if m.starts_with("calling `.") && m.ends_with("on a value of type unknown") {
        return "method call on a value of unknown type".into();
    }
    for verb in [
        "cannot index",
        "cannot call",
        "cannot spread",
        "cannot iterate",
    ] {
        if let Some(rest) = m.strip_prefix(&format!("{verb} a value of type ")) {
            return format!("{verb} a value of {}", ty_class(rest));
        }
    }
    if m.starts_with("property `") {
        if let Some(i) = m.find("does not exist on type ") {
            return format!(
                "property does not exist on {}",
                ty_class(&m[i + "does not exist on type ".len()..])
            );
        }
    }
    if let Some(rest) = m.strip_prefix("method `") {
        // "method `x` on type T is outside the compiled subset"
        if let Some((name, tail)) = rest.split_once("` on type ") {
            let ty = tail.trim_end_matches(" is outside the compiled subset");
            return format!("method `{name}` on {ty}");
        }
    }
    if m.starts_with("unknown name `") {
        return format!("unknown name `{}`", ticked(m).unwrap_or_default());
    }
    if m.starts_with("the component has no prop") {
        return "the component has no such prop".into();
    }
    if m.ends_with("is not exported by") || m.contains("` is not exported by `") {
        return "name not exported by the imported module".into();
    }
    if m.contains("is neither a value nor a type the compiled subset knows") {
        return "imported name the subset cannot use".into();
    }
    if m.contains("is used before its declaration") {
        return "name used before its declaration".into();
    }
    if m.contains("must be called at the top level of a component or hook") {
        return format!("`{}` not at the top level", ticked(m).unwrap_or_default());
    }
    if m.starts_with("a value of type ") && m.ends_with("cannot be rendered") {
        return "a value that cannot be rendered".into();
    }
    if let Some(rest) = m.strip_prefix("type `") {
        let name = rest.split('`').next().unwrap_or("");
        let head = name.split('<').next().unwrap_or(name);
        return format!("type `{head}`");
    }
    if m.starts_with("recursive type") {
        return "recursive type".into();
    }
    if m.starts_with("assigning to function") {
        return "assigning to a function".into();
    }
    if m.starts_with("`useContext` of a value") {
        return "`useContext` of a value of another type".into();
    }
    // The rest name a construct, not a place: keep them as they are.
    m.to_owned()
}

/// A top-level function or class of a module.
struct Unit {
    name: String,
    start: (u32, u32),
    end: (u32, u32),
    component: bool,
}

fn units(text: &str) -> Vec<Unit> {
    use ast::Statement as S;
    let allocator = oxc_allocator::Allocator::default();
    let ret = oxc_parser::Parser::new(&allocator, text, oxc_span::SourceType::tsx()).parse();
    let mut out = Vec::new();
    let mut push = |name: &str, span: oxc_span::Span| {
        out.push(Unit {
            name: name.to_owned(),
            start: crate::line_col(text, span.start),
            end: crate::line_col(text, span.end),
            component: name.starts_with(|c: char| c.is_ascii_uppercase()),
        });
    };
    fn wraps_function(e: &ast::Expression<'_>) -> bool {
        use ast::Expression as E;
        match e {
            E::ArrowFunctionExpression(_) | E::FunctionExpression(_) => true,
            E::CallExpression(c) => c.arguments.iter().any(|a| match a.as_expression() {
                Some(e) => wraps_function(e),
                None => false,
            }),
            E::ParenthesizedExpression(p) => wraps_function(&p.expression),
            E::TSAsExpression(a) => wraps_function(&a.expression),
            E::TSSatisfiesExpression(a) => wraps_function(&a.expression),
            _ => false,
        }
    }
    let decl = |d: &ast::Declaration<'_>, push: &mut dyn FnMut(&str, oxc_span::Span)| match d {
        ast::Declaration::FunctionDeclaration(f) => {
            if let Some(id) = &f.id {
                push(id.name.as_str(), f.span);
            }
        }
        ast::Declaration::ClassDeclaration(c) => {
            if let Some(id) = &c.id {
                push(id.name.as_str(), c.span);
            }
        }
        ast::Declaration::VariableDeclaration(v) => {
            for d in &v.declarations {
                if let (Some(init), Some(name)) = (&d.init, d.id.get_identifier_name()) {
                    if wraps_function(init) {
                        push(name.as_str(), d.span);
                    }
                }
            }
        }
        _ => {}
    };
    for stmt in &ret.program.body {
        match stmt {
            S::FunctionDeclaration(f) => {
                if let Some(id) = &f.id {
                    push(id.name.as_str(), f.span);
                }
            }
            S::ClassDeclaration(c) => {
                if let Some(id) = &c.id {
                    push(id.name.as_str(), c.span);
                }
            }
            S::VariableDeclaration(v) => {
                for d in &v.declarations {
                    if let (Some(init), Some(name)) = (&d.init, d.id.get_identifier_name()) {
                        if wraps_function(init) {
                            push(name.as_str(), d.span);
                        }
                    }
                }
            }
            S::ExportDeclaration(e) => decl(&e.declaration, &mut push),
            S::ExportDefaultDeclaration(e) => match &e.declaration {
                ast::ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                    let name = f.id.as_ref().map(|i| i.name.as_str()).unwrap_or("Default");
                    push(name, f.span);
                }
                ast::ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                    let name = c.id.as_ref().map(|i| i.name.as_str()).unwrap_or("Default");
                    push(name, c.span);
                }
                other => {
                    if let Some(e) = other.as_expression() {
                        if wraps_function(e) {
                            push("Default", e.span());
                        }
                    }
                }
            },
            _ => {}
        }
    }
    out
}

use oxc_span::GetSpan;

fn is_entry(text: &str) -> bool {
    text.contains("createRoot(")
        || text.contains("hydrateRoot(")
        || text.contains("ReactDOM.render(")
}

fn is_code(file: &str) -> bool {
    (file.ends_with(".tsx") || file.ends_with(".ts")) && !file.ends_with(".d.ts")
}

/// Packages a module imports (not `react`, `react-dom`, relative or aliased paths).
fn packages(text: &str, aliases: &[(String, String)]) -> BTreeSet<String> {
    crate::specifiers(text)
        .into_iter()
        .map(|(s, _)| s)
        .filter(|s| {
            !crate::is_relative(s) && !aliases.iter().any(|(a, _)| s.starts_with(a.as_str()))
        })
        .filter(|s| {
            !matches!(
                s.as_str(),
                "react" | "react-dom" | "react-dom/client" | "react/jsx-runtime"
            )
        })
        .map(|s| {
            // `@scope/name/sub` → `@scope/name`, `name/sub` → `name`.
            let mut parts = s.split('/');
            let first = parts.next().unwrap_or("");
            if first.starts_with('@') {
                format!("{first}/{}", parts.next().unwrap_or(""))
            } else {
                first.to_owned()
            }
        })
        .collect()
}

/// Local names a module binds by importing from a package, or from a module of the
/// app that did not resolve.
fn imported_names(
    text: &str,
    resolved: &BTreeMap<String, usize>,
) -> BTreeMap<String, &'static str> {
    use ast::Statement as S;
    let allocator = oxc_allocator::Allocator::default();
    let ret = oxc_parser::Parser::new(&allocator, text, oxc_span::SourceType::tsx()).parse();
    let mut out = BTreeMap::new();
    for stmt in &ret.program.body {
        let S::ImportDeclaration(i) = stmt else {
            continue;
        };
        let spec = i.source.value.as_str();
        if resolved.contains_key(spec) || matches!(spec, "react" | "react-dom" | "react-dom/client")
        {
            continue;
        }
        let from = if crate::is_relative(spec) {
            "a module that does not resolve"
        } else {
            "a package"
        };
        for s in i.specifiers.iter().flatten() {
            out.insert(s.local().name.to_string(), from);
        }
    }
    out
}

const NEVER_RENDERS: &str = "the module never renders";

/// Evaluates the projects of `split` (`"dev"`, `"test"` or `"all"`) of the corpus in
/// `dir` (holding `manifest.json` and `src/`).
pub fn evaluate(dir: &Path, split: &str) -> Report {
    let manifest: Manifest = serde_json::from_str(
        &std::fs::read_to_string(dir.join("manifest.json")).expect("corpus manifest"),
    )
    .expect("manifest parse");
    let mut report = Report {
        split: split.to_owned(),
        ..Report::default()
    };
    for p in &manifest.projects {
        if split != "all" && p.split != split {
            continue;
        }
        let source = manifest
            .sources
            .iter()
            .find(|s| s.id == p.source)
            .expect("project's source");
        let root: PathBuf = dir.join("src").join(&source.id);
        let options = LoadOptions {
            aliases: source
                .aliases
                .iter()
                .map(|(a, b)| (a.clone(), b.clone()))
                .collect(),
        };
        let result = evaluate_project(&root, p, &options, &mut report);
        report.modules.merge(&result.modules);
        report.functions.merge(&result.functions);
        report.components.merge(&result.components);
        if let Some(c) = result.app {
            report.apps.add(c);
        }
        report.projects += 1;
        report.per_project.push(result);
    }
    report
}

fn evaluate_project(
    root: &Path,
    p: &ManifestProject,
    options: &LoadOptions,
    report: &mut Report,
) -> ProjectResult {
    let mut result = ProjectResult {
        id: p.id.clone(),
        ..ProjectResult::default()
    };
    let read_file = |rel: &str| std::fs::read_to_string(root.join(rel)).ok();
    for file in p.files.iter().filter(|f| is_code(f)) {
        let Some(text) = read_file(file) else {
            continue;
        };
        for pkg in packages(&text, &options.aliases) {
            *report.packages.entry(pkg).or_default() += 1;
        }
        let mut read = |rel: &str| read_file(rel);
        let (sources, load_errors) = crate::load_with(file, &mut read, options);
        let mut diags: Vec<Diagnostic> = Vec::new();
        // Load errors of this module (its own imports that do not resolve).
        diags.extend(load_errors.into_iter().filter(|d| d.file == *file));
        if !sources.is_empty() {
            let multi = crate::code_modules(&sources) > 1;
            match crate::lower::lower_modules(&sources) {
                Ok(_) => {}
                Err(ds) => {
                    for d in ds {
                        let own = if multi || sources.iter().any(|s| s.is_ambient()) {
                            d.file == *file
                        } else {
                            d.file.is_empty() || d.file == *file
                        };
                        if own {
                            diags.push(d);
                        }
                    }
                }
            }
        }
        diags.retain(|d| !d.message.starts_with(NEVER_RENDERS));
        let empty = BTreeMap::new();
        let resolved = sources
            .iter()
            .find(|s| s.file == *file)
            .map(|s| &s.imports)
            .unwrap_or(&empty);
        let imported = imported_names(&text, resolved);
        let cause = |m: &str| cause_in(m, &imported);
        diags.dedup();
        for d in &diags {
            *report
                .diagnostics_by_cause
                .entry(cause(&d.message))
                .or_default() += 1;
            report.diagnostics += 1;
        }
        result.modules.add(diags.is_empty());
        for u in units(&text) {
            let within: BTreeSet<String> = diags
                .iter()
                .filter(|d| (d.line, d.col) >= u.start && (d.line, d.col) < u.end)
                .map(|d| cause(&d.message))
                .collect();
            let inside = within.is_empty();
            if !inside {
                let causes: Vec<&str> = within.iter().map(String::as_str).collect();
                result
                    .outside
                    .push(format!("{file}: {}: {}", u.name, causes.join("; ")));
            }
            result.functions.add(inside);
            if u.component {
                result.components.add(inside);
            }
            for c in within {
                *report.functions_by_cause.entry(c).or_default() += 1;
            }
        }
        if result.sample.len() < 5 {
            for d in diags.iter().take(5 - result.sample.len()) {
                result.sample.push(format!("{file}: {d}"));
            }
        }
    }
    // The app, whole, by today's rules: strict loading, every module inside.
    let entry = p
        .files
        .iter()
        .filter(|f| is_code(f))
        .find(|f| read_file(f).is_some_and(|t| is_entry(&t)));
    if let Some(entry) = entry {
        let mut read = |rel: &str| read_file(rel);
        let (sources, errors) = crate::load_with(entry, &mut read, options);
        let compiled = errors.is_empty() && crate::build_modules(&sources).diagnostics.is_empty();
        result.app = Some(compiled);
        result.entry = Some(entry.clone());
    }
    result
}
