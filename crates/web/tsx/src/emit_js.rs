//! The fallback: the same app as a plain classic script for React 18's UMD build.
//!
//! Types are stripped and JSX lowered to `React.createElement` by oxc's transformer;
//! imports from `react` and `react-dom` (`react-dom/client`) become destructurings of
//! the `React` and `ReactDOM` globals the UMD bundles define. An app of several
//! modules is bundled the way a bundler's scope-per-module output is: each module,
//! in dependency order, runs in its own function scope that returns its value exports
//! (`const __cw_mod_2 = (() => { …; return { people, default: App }; })();`), and its
//! imports read the exports of the modules before it (an imported type reads
//! `undefined` and is never used at run time). A one-module app is emitted without
//! the wrapper. No bundler is involved, so the page loads `react.production.min.js`,
//! `react-dom.production.min.js` and this script, in Chrome and on the engine's
//! `Realm` alike.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use oxc_allocator::{Allocator, TakeIn};
use oxc_ast::ast::{
    BindingIdentifier, BindingPattern, Declaration, ExportDefaultDeclarationKind, Expression,
    IdentifierName, ImportDeclarationSpecifier, ObjectProperty, Statement, VariableDeclarationKind,
    VariableDeclarator,
};
use oxc_ast::builder::AstBuilder;
use oxc_ast_visit::{walk_mut, VisitMut};
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_transformer::{JsxRuntime, TransformOptions, Transformer};

use crate::{Diagnostic, Source};

/// Compiles a one-module app to a classic script, or the reasons it cannot be.
pub fn emit(source: &str, file_name: &str) -> Result<String, Vec<Diagnostic>> {
    emit_modules(&[Source::single(source, file_name)])
}

/// Compiles an app's modules (in dependency order, entry last) to one script.
pub fn emit_modules(sources: &[Source]) -> Result<String, Vec<Diagnostic>> {
    let code_modules = crate::code_modules(sources);
    let bundled = code_modules > 1;
    let entry = sources.last().map(|s| s.file.as_str()).unwrap_or("app.tsx");
    let mut out = format!(
        "// Compiled by cw-tsx from {entry}: types stripped, JSX as React.createElement.\n'use strict';\n"
    );
    if sources.iter().any(|s| {
        s.imports.contains_key("react/jsx-runtime") || s.text.contains("react/jsx-runtime")
    }) {
        // The automatic JSX runtime a package was built for, over React's
        // createElement (a key travels in the props).
        out.push_str(JSX_RUNTIME);
    }
    let mut errors = Vec::new();
    if !bundled {
        for src in sources.iter().filter(|s| !s.is_ambient()) {
            match emit_one(src, code_modules, 0, &[], &[]) {
                Ok(code) => out.push_str(&code),
                Err(e) => errors.extend(e),
            }
        }
    } else {
        // Every module's exports object exists before any module runs, so a
        // module in an import cycle sees the other's (live) exports.
        let names = export_names(sources);
        out.push_str(&format!(
            "const __cw_m = [{}];\n",
            vec!["{}"; sources.len()].join(", ")
        ));
        if sources.iter().any(|s| s.commonjs) {
            out.push_str(MODULE_RUNTIME);
        }
        let cjs: Vec<usize> = (0..sources.len())
            .filter(|&i| sources[i].commonjs)
            .collect();
        for (i, src) in sources.iter().enumerate() {
            if src.is_ambient() {
                continue;
            }
            if src.commonjs {
                out.push_str(&emit_cjs(src, i));
                continue;
            }
            match emit_one(src, code_modules, i, &names, &cjs) {
                Ok(code) => {
                    out.push_str(&format!("// {}\n(() => {{\n{code}}})();\n", src.file));
                }
                Err(e) => errors.extend(e),
            }
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// What a bundle of several modules needs to run them: `__cw_get(i)` is module
/// `i`'s exports object, running it first if it is CommonJS and has not run;
/// `__cw_req` is a CommonJS module's `require`; `__cw_cjs` gives a CommonJS
/// module's exports the shape an ES import sees (`default` is `module.exports`,
/// or its `default` when it says `__esModule`; each own key a named export).
const MODULE_RUNTIME: &str = r#"var __cw_lazy = {};
var __cw_import_meta = { url: typeof location === 'undefined' ? '' : String(location.href) };
function __cw_get(i) { var l = __cw_lazy[i]; if (l) { __cw_lazy[i] = null; l(); } return __cw_m[i]; }
function __cw_req(map, s) {
  if (s === 'react') return React;
  if (s === 'react-dom' || s === 'react-dom/client') return ReactDOM;
  if (s === 'react/jsx-runtime' || s === 'react/jsx-dev-runtime') return __cw_jsx;
  var m = map[s];
  if (m === undefined) throw new Error("Cannot find module '" + s + "'");
  var ns = __cw_get(m);
  return Object.prototype.hasOwnProperty.call(ns, '__cw_cjs') ? ns.__cw_cjs : ns;
}
function __cw_cjs(ns, ex) {
  Object.defineProperty(ns, '__cw_cjs', { value: ex });
  var def = ex && ex.__esModule ? ex['default'] : ex;
  Object.defineProperty(ns, 'default', { enumerable: true, get: function () { return def; } });
  if (ex !== null && (typeof ex === 'object' || typeof ex === 'function')) {
    Object.keys(ex).forEach(function (k) {
      if (k !== 'default' && k !== '__cw_cjs') Object.defineProperty(ns, k, { enumerable: true, get: function () { return ex[k]; } });
    });
  }
}
"#;

/// A package's CommonJS module, run when first required or imported: its code in
/// Node's module wrapper, with `require` resolving what the loader resolved.
fn emit_cjs(src: &Source, index: usize) -> String {
    let map: Vec<String> = src
        .imports
        .iter()
        .map(|(spec, m)| format!("{}: {m}", js_key_quoted(spec)))
        .collect();
    let code = define_node_env_text(&src.text);
    format!(
        "// {file}\n__cw_lazy[{index}] = function () {{ var module = {{ exports: {{}} }}; var require = function (s) {{ return __cw_req({{ {map} }}, s); }}; (function (module, exports, require) {{\n{code}\n}}).call(module.exports, module, module.exports, require); __cw_cjs(__cw_m[{index}], module.exports); }};\n",
        file = src.file,
        map = map.join(", "),
    )
}

/// A string as an object key, always quoted.
fn js_key_quoted(s: &str) -> String {
    format!("{s:?}")
}

/// `process.env.NODE_ENV` as `"production"` in a script's text (for CommonJS
/// code, which is not re-emitted).
fn define_node_env_text(text: &str) -> String {
    if !text.contains("NODE_ENV") {
        return text.to_owned();
    }
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, text, oxc_span::SourceType::cjs()).parse();
    if !ret.diagnostics.is_empty() {
        return text.to_owned();
    }
    let mut program = ret.program;
    DefineNodeEnv {
        b: AstBuilder::new(&allocator),
    }
    .visit_program(&mut program);
    Codegen::new().build(&program).code
}

/// `react/jsx-runtime` for a bundle whose packages import it.
const JSX_RUNTIME: &str = "const __cw_jsx = (() => { const jsx = (type, props, key) => React.createElement(type, key === undefined ? props : Object.assign({}, props, { key })); return { jsx, jsxs: jsx, jsxDEV: jsx, Fragment: React.Fragment }; })();\n";

/// The island's script: the package modules, which run as it loads; the app's
/// modules outside the compiled subset (`vm`), each wrapped to run when
/// `__cw_run(i)` first asks, at its place in the module order; the exports of
/// compiled modules those import, as getters reading the compiled globals
/// (`__cw.g(slot)`); and `__cw_exports`, one function per `island.imports` entry
/// returning that value (or, for `"!run"`, running the module).
pub fn emit_island(
    sources: &[Source],
    island: &cw_ui::ir::Island,
    vm: &[usize],
) -> Result<String, Vec<Diagnostic>> {
    let names = export_names(sources);
    let mut out = String::from("// The island of an app compiled by cw-tsx.\n");
    out.push_str(&format!(
        "var __cw_m = [{}];\nvar __cw_init = {{}};\nvar __cw_done = {{}};\nfunction __cw_run(i) {{ if (!__cw_done[i]) {{ __cw_done[i] = true; __cw_init[i](); }} }}\n",
        vec!["{}"; sources.len()].join(", ")
    ));
    out.push_str(MODULE_RUNTIME);
    if sources.iter().enumerate().any(|(i, s)| {
        (s.package || vm.contains(&i))
            && (s.imports.contains_key("react/jsx-runtime") || s.text.contains("react/jsx-runtime"))
    }) {
        out.push_str("var __cw_jsx = globalThis.__cw_jsx;\n");
    }
    let file_index = |f: &str| sources.iter().position(|s| s.file == f);
    let cjs: Vec<usize> = (0..sources.len())
        .filter(|&i| sources[i].commonjs)
        .collect();
    // Compiled modules' exports, as the island's modules see them.
    let mut provided: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (file, name, slot) in &island.provides {
        if let Some(m) = file_index(file) {
            provided.entry(m).or_default().push(format!(
                "{}: {{ enumerable: true, get: () => __cw.g({slot}) }}",
                js_key(name)
            ));
        }
    }
    for (m, props) in &provided {
        out.push_str(&format!(
            "Object.defineProperties(__cw_m[{m}], {{ {} }});\n",
            props.join(", ")
        ));
    }
    let mut errors = Vec::new();
    for (i, src) in sources.iter().enumerate() {
        if src.package && src.commonjs {
            out.push_str(&emit_cjs(src, i));
        } else if src.package {
            match emit_one(src, 2, i, &names, &cjs) {
                Ok(code) => out.push_str(&format!("// {}\n(() => {{\n{code}}})();\n", src.file)),
                Err(e) => errors.extend(e),
            }
        } else if vm.contains(&i) {
            match emit_one(src, 2, i, &names, &cjs) {
                Ok(code) => out.push_str(&format!(
                    "// {}\n__cw_init[{i}] = () => {{\n{code}}};\n",
                    src.file
                )),
                Err(e) => errors.extend(e),
            }
        }
    }
    let mut exports = Vec::new();
    for (spec, name) in &island.imports {
        if spec == "!global" {
            // A global of the VM itself (`Intl`, the shim's `__cw_locale`).
            exports.push(format!("() => globalThis{}", js_member(name)));
            continue;
        }
        let module =
            file_index(spec).or_else(|| sources.iter().find_map(|s| s.imports.get(spec).copied()));
        let Some(m) = module else {
            errors.push(Diagnostic {
                file: String::new(),
                line: 1,
                col: 1,
                message: format!("cannot find package `{spec}`"),
            });
            continue;
        };
        exports.push(match name.as_str() {
            "!run" => format!("() => __cw_run({m})"),
            "!root" => "() => __cw.rendered".to_owned(),
            "*" | "" => format!("() => __cw_get({m})"),
            n => format!("() => __cw_get({m}){}", js_member(n)),
        });
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    out.push_str(&format!("var __cw_exports = [{}];\n", exports.join(", ")));
    Ok(out)
}

/// The names each module exports (values and types alike), `export *` resolved
/// through the modules before it.
fn export_names(sources: &[Source]) -> Vec<Vec<String>> {
    let mut out: Vec<Vec<String>> = Vec::new();
    for src in sources {
        let allocator = Allocator::default();
        let ret = Parser::new(&allocator, &src.text, crate::source_type(&src.file)).parse();
        let mut names = Vec::new();
        for stmt in &ret.program.body {
            match stmt {
                Statement::ExportDeclaration(e) => match &e.declaration {
                    Declaration::FunctionDeclaration(f) => {
                        names.extend(f.id.as_ref().map(|i| i.name.to_string()))
                    }
                    Declaration::ClassDeclaration(c) => {
                        names.extend(c.id.as_ref().map(|i| i.name.to_string()))
                    }
                    Declaration::VariableDeclaration(v) => {
                        for d in &v.declarations {
                            binding_idents(&d.id, &mut names);
                        }
                    }
                    Declaration::TSEnumDeclaration(e) => names.push(e.id.name.to_string()),
                    _ => {}
                },
                Statement::ExportNamedDeclaration(e) => {
                    for spec in &e.specifiers {
                        names.push(spec.exported.name().to_string());
                    }
                }
                Statement::ExportFromDeclaration(e) => {
                    for spec in &e.specifiers {
                        names.push(spec.exported.name().to_string());
                    }
                }
                Statement::ExportDefaultDeclaration(_) => names.push("default".into()),
                Statement::ExportAllDeclaration(e) => match &e.exported {
                    Some(n) => names.push(n.name().to_string()),
                    None => {
                        if let Some(&m) = src.imports.get(e.source.value.as_str()) {
                            if let Some(theirs) = out.get(m) {
                                names.extend(theirs.iter().filter(|n| *n != "default").cloned());
                            }
                        }
                    }
                },
                _ => {}
            }
        }
        names.sort();
        names.dedup();
        out.push(names);
    }
    out
}

fn js_key(name: &str) -> String {
    if name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        && !name.starts_with(|c: char| c.is_ascii_digit())
    {
        name.to_owned()
    } else {
        format!("{name:?}")
    }
}

fn binding_idents(p: &BindingPattern<'_>, out: &mut Vec<String>) {
    match p {
        BindingPattern::BindingIdentifier(id) => out.push(id.name.to_string()),
        BindingPattern::ObjectPattern(o) => {
            for prop in &o.properties {
                binding_idents(&prop.value, out);
            }
            if let Some(r) = &o.rest {
                binding_idents(&r.argument, out);
            }
        }
        BindingPattern::ArrayPattern(a) => {
            for el in a.elements.iter().flatten() {
                binding_idents(el, out);
            }
            if let Some(r) = &a.rest {
                binding_idents(&r.argument, out);
            }
        }
        BindingPattern::AssignmentPattern(a) => binding_idents(&a.left, out),
    }
}

/// Names the module binds to values at its top level.
fn top_level_values(body: &[Statement<'_>]) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    for stmt in body {
        let decl = match stmt {
            Statement::ExportDeclaration(e) => Some(&e.declaration),
            other => other.as_declaration(),
        };
        match decl {
            Some(Declaration::FunctionDeclaration(f)) => {
                values.extend(f.id.as_ref().map(|i| i.name.to_string()))
            }
            Some(Declaration::ClassDeclaration(c)) => {
                values.extend(c.id.as_ref().map(|i| i.name.to_string()))
            }
            Some(Declaration::VariableDeclaration(v)) => {
                for d in &v.declarations {
                    let mut n = Vec::new();
                    binding_idents(&d.id, &mut n);
                    values.extend(n);
                }
            }
            Some(Declaration::TSEnumDeclaration(e)) => {
                values.insert(e.id.name.to_string());
            }
            _ => {}
        }
        if let Statement::ImportDeclaration(i) = stmt {
            if !i.import_kind.is_type() {
                for spec in i.specifiers.iter().flatten() {
                    match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            if !s.import_kind.is_type() {
                                values.insert(s.local.name.to_string());
                            }
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            values.insert(s.local.name.to_string());
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            values.insert(s.local.name.to_string());
                        }
                    }
                }
            }
        }
    }
    values
}

/// One module's code. On its own (`modules == 1`) it is the module's code with its
/// React imports read from the globals. Bundled, it is the body of the module's
/// scope: its exports defined as getters on `__cw_m[index]` (live, as ES module
/// bindings are), then its code, in which every use of a name imported from
/// another module of the app reads that module's exports object
/// (`__cw_i3.Button`), so an import cycle sees the other module as it is when the
/// name is used, not when this module started.
fn emit_one(
    src: &Source,
    modules: usize,
    index: usize,
    names: &[Vec<String>],
    cjs: &[usize],
) -> Result<String, Vec<Diagnostic>> {
    let bundled = modules > 1;
    let source = src.text.as_str();
    let at = |offset: u32, msg: String| {
        let mut d = Diagnostic::at(source, offset, msg);
        d.file = src.display_file(modules);
        d
    };
    let from_oxc = |d: &oxc_diagnostics::OxcDiagnostic| {
        let mut d = Diagnostic::from_oxc(source, d);
        d.file = src.display_file(modules);
        d
    };
    let allocator = Allocator::default();
    let b = AstBuilder::new(&allocator);
    let ret = Parser::new(&allocator, source, crate::source_type(&src.file)).parse();
    if !ret.diagnostics.is_empty() {
        return Err(ret.diagnostics.iter().map(from_oxc).collect());
    }
    let mut program = ret.program;
    if source.contains("NODE_ENV") || source.contains("import.meta") {
        DefineNodeEnv {
            b: AstBuilder::new(&allocator),
        }
        .visit_program(&mut program);
    }
    let values = top_level_values(&program.body);
    let mut preamble = String::new();
    let mut errors = Vec::new();
    // Exported name, and the expression its getter returns.
    let mut exports: Vec<(String, String)> = Vec::new();
    // Names imported from modules of the app: module, name there.
    let mut imported: BTreeMap<String, (usize, String)> = BTreeMap::new();
    let mut used_modules: BTreeSet<usize> = BTreeSet::new();
    let module_of = |spec: &str| src.imports.get(spec).copied();
    let alloc = &allocator;
    let body = program.body.take_in(&alloc);
    let mut kept = oxc_allocator::Vec::new_in(&alloc);
    let default_name = alloc.alloc_str("__cw_default");
    for stmt in body {
        match stmt {
            Statement::ImportDeclaration(mut import) => {
                if import.import_kind.is_type() {
                    continue;
                }
                let module = import.source.value.to_string();
                let local_module = module_of(&module);
                if local_module.is_none()
                    && crate::is_stylesheet(&module)
                    && import.specifiers.as_ref().is_none_or(|s| s.is_empty())
                {
                    // A stylesheet: the page's, collected by `crate::stylesheet`.
                    continue;
                }
                if local_module.is_none() {
                    if let Some(url) = crate::asset_url(&src.file, &module) {
                        // A stylesheet the page loads, or a media file's URL.
                        for spec in import.specifiers.iter().flatten() {
                            match spec {
                                ImportDeclarationSpecifier::ImportDefaultSpecifier(s)
                                    if !crate::is_stylesheet(&module) =>
                                {
                                    preamble
                                        .push_str(&format!("const {} = {url:?};\n", s.local.name));
                                }
                                _ => errors.push(at(
                                    import.span.start,
                                    format!(
                                        "import of names from `{module}`, which is not a module"
                                    ),
                                )),
                            }
                        }
                        continue;
                    }
                }
                let global = match (module.as_str(), local_module) {
                    (_, Some(m)) => {
                        used_modules.insert(m);
                        format!("__cw_i{m}")
                    }
                    ("react", _) => "React".to_owned(),
                    ("react-dom" | "react-dom/client", _) => "ReactDOM".to_owned(),
                    ("react/jsx-runtime" | "react/jsx-dev-runtime", _) => "__cw_jsx".to_owned(),
                    (other, None) => {
                        errors.push(at(
                            import.span.start,
                            format!("import from `{other}`: only `react`, `react-dom` and the app's own modules are available to a compiled app"),
                        ));
                        continue;
                    }
                };
                let mut named = Vec::new();
                for spec in import.specifiers.take().into_iter().flatten() {
                    match spec {
                        ImportDeclarationSpecifier::ImportSpecifier(s) => {
                            if s.import_kind.is_type() {
                                continue;
                            }
                            let name = s.imported.name().to_string();
                            let local = s.local.name.to_string();
                            if let Some(m) = local_module {
                                imported.insert(local, (m, name));
                            } else if name == local {
                                named.push(local);
                            } else {
                                named.push(format!("{}: {local}", js_key(&name)));
                            }
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            if let Some(m) = local_module {
                                imported.insert(s.local.name.to_string(), (m, "default".into()));
                            } else if s.local.name.as_str() != global {
                                preamble.push_str(&format!("const {} = {global};\n", s.local.name));
                            }
                        }
                        ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                            if s.local.name.as_str() != global {
                                preamble.push_str(&format!("const {} = {global};\n", s.local.name));
                            }
                        }
                    }
                }
                if !named.is_empty() {
                    preamble.push_str(&format!("const {{ {} }} = {global};\n", named.join(", ")));
                }
            }
            Statement::ExportDeclaration(mut export) => {
                let decl = export.declaration.take_in(&alloc);
                let mut names = Vec::new();
                match &decl {
                    Declaration::FunctionDeclaration(f) => {
                        names.extend(f.id.as_ref().map(|i| i.name.to_string()))
                    }
                    Declaration::ClassDeclaration(c) => {
                        names.extend(c.id.as_ref().map(|i| i.name.to_string()))
                    }
                    Declaration::VariableDeclaration(v) => {
                        for d in &v.declarations {
                            binding_idents(&d.id, &mut names);
                        }
                    }
                    Declaration::TSEnumDeclaration(e) => names.push(e.id.name.to_string()),
                    _ => {}
                }
                exports.extend(names.into_iter().map(|n| (n.clone(), n)));
                kept.push(Statement::from(decl));
            }
            // `export { a, b as c }`: the names stay declared; types export nothing.
            Statement::ExportNamedDeclaration(e) => {
                if e.export_kind.is_type() {
                    continue;
                }
                for spec in &e.specifiers {
                    let local = spec.local.name().to_string();
                    if spec.export_kind.is_type() {
                        continue;
                    }
                    let exported = spec.exported.name().to_string();
                    if let Some((m, name)) = imported.get(&local) {
                        exports.push((exported, format!("__cw_i{m}{}", js_member(name))));
                    } else if values.contains(&local) {
                        exports.push((exported, local));
                    }
                }
            }
            Statement::ExportFromDeclaration(e) => {
                if e.export_kind.is_type() {
                    continue;
                }
                let Some(m) = module_of(e.source.value.as_str()) else {
                    errors.push(at(
                        e.span.start,
                        format!("import from `{}`: only `react`, `react-dom` and the app's own modules are available to a compiled app", e.source.value),
                    ));
                    continue;
                };
                used_modules.insert(m);
                for spec in &e.specifiers {
                    if spec.export_kind.is_type() {
                        continue;
                    }
                    let name = spec.local.name().to_string();
                    exports.push((
                        spec.exported.name().to_string(),
                        format!("__cw_i{m}{}", js_member(&name)),
                    ));
                }
            }
            Statement::ExportAllDeclaration(e) => {
                if e.export_kind.is_type() {
                    continue;
                }
                let Some(m) = module_of(e.source.value.as_str()) else {
                    errors.push(at(
                        e.span.start,
                        format!("import from `{}`: only `react`, `react-dom` and the app's own modules are available to a compiled app", e.source.value),
                    ));
                    continue;
                };
                used_modules.insert(m);
                match &e.exported {
                    Some(ns) => exports.push((ns.name().to_string(), format!("__cw_i{m}"))),
                    None => {
                        for n in names.get(m).into_iter().flatten() {
                            if n != "default" {
                                exports.push((n.clone(), format!("__cw_i{m}{}", js_member(n))));
                            }
                        }
                    }
                }
            }
            Statement::ExportDefaultDeclaration(mut export) => {
                let span = export.span;
                match export.declaration.take_in(&alloc) {
                    ExportDefaultDeclarationKind::FunctionDeclaration(mut f) => {
                        if f.id.is_none() {
                            f.id = Some(BindingIdentifier::new(span, default_name, &b));
                        }
                        let name =
                            f.id.as_ref()
                                .map(|i| i.name.to_string())
                                .unwrap_or_default();
                        exports.push(("default".into(), name));
                        kept.push(Statement::FunctionDeclaration(f));
                    }
                    ExportDefaultDeclarationKind::ClassDeclaration(mut c) => {
                        if c.id.is_none() {
                            c.id = Some(BindingIdentifier::new(span, default_name, &b));
                        }
                        let name =
                            c.id.as_ref()
                                .map(|i| i.name.to_string())
                                .unwrap_or_default();
                        exports.push(("default".into(), name));
                        kept.push(Statement::ClassDeclaration(c));
                    }
                    ExportDefaultDeclarationKind::Identifier(id) => {
                        let local = id.name.to_string();
                        let getter = match imported.get(&local) {
                            Some((m, name)) => format!("__cw_i{m}{}", js_member(name)),
                            None => local,
                        };
                        exports.push(("default".into(), getter));
                    }
                    ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => {}
                    other => {
                        // `export default <expression>`: `const __cw_default = …`.
                        let x = other.into_expression();
                        let decl = VariableDeclarator::new(
                            span,
                            BindingPattern::new_binding_identifier(span, default_name, &b),
                            None,
                            Some(x),
                            false,
                            &b,
                        );
                        kept.push(Statement::new_variable_declaration(
                            span,
                            VariableDeclarationKind::Const,
                            oxc_allocator::Vec::from_iter_in([decl], &alloc),
                            false,
                            &b,
                        ));
                        exports.push(("default".into(), "__cw_default".into()));
                    }
                }
            }
            other => kept.push(other),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    if !bundled && !exports.is_empty() {
        // A one-module app's exports are nobody's imports.
        exports.clear();
    }
    program.body = kept;
    let semantic = SemanticBuilder::new().with_enum_eval(true).build(&program);
    let scoping = semantic.semantic.into_scoping();
    let mut options = TransformOptions::default();
    options.jsx.runtime = JsxRuntime::Classic;
    options.jsx.pure = false;
    options.jsx.display_name_plugin = false;
    let path = std::path::Path::new(&src.file);
    let ret =
        Transformer::new(&allocator, path, &options).build_with_scoping(scoping, &mut program);
    if !ret.diagnostics.is_empty() {
        return Err(ret.diagnostics.iter().map(from_oxc).collect());
    }
    if !imported.is_empty() {
        // Uses of imported names (unbound now that the imports are gone) read the
        // exporting module's exports object.
        let semantic = SemanticBuilder::new().build(&program);
        let scoping = semantic.semantic.into_scoping();
        let mut refs: HashMap<oxc_semantic::ReferenceId, (usize, String)> = HashMap::new();
        for (name, ids) in scoping.root_unresolved_references() {
            if let Some(target) = imported.get(name.as_str()) {
                for id in ids {
                    refs.insert(*id, target.clone());
                }
            }
        }
        let mut rw = ImportUses {
            refs,
            b: AstBuilder::new(&allocator),
            alloc: &allocator,
        };
        rw.visit_program(&mut program);
    }
    let code = Codegen::new()
        .with_options(CodegenOptions {
            single_quote: true,
            comments: oxc_codegen::CommentOptions {
                annotation: false,
                ..oxc_codegen::CommentOptions::default()
            },
            ..CodegenOptions::default()
        })
        .build(&program)
        .code;
    if !bundled {
        return Ok(format!("{preamble}{code}"));
    }
    let mut head = String::new();
    for m in &used_modules {
        // A CommonJS module runs when first imported.
        if cjs.contains(m) {
            head.push_str(&format!("const __cw_i{m} = __cw_get({m});\n"));
        } else {
            head.push_str(&format!("const __cw_i{m} = __cw_m[{m}];\n"));
        }
    }
    if !exports.is_empty() {
        let mut seen = BTreeSet::new();
        let props: Vec<String> = exports
            .iter()
            .filter(|(n, _)| seen.insert(n.clone()))
            .map(|(n, g)| format!("{}: {{ enumerable: true, get: () => {g} }}", js_key(n)))
            .collect();
        head.push_str(&format!(
            "Object.defineProperties(__cw_m[{index}], {{ {} }});\n",
            props.join(", ")
        ));
    }
    Ok(format!("{head}{preamble}{code}"))
}

/// A property access of `name`: `.name` (keywords are fine there) or `["name"]`.
fn js_member(name: &str) -> String {
    let key = js_key(name);
    if key.starts_with('"') {
        format!("[{key}]")
    } else {
        format!(".{key}")
    }
}

/// Rewrites uses of imported names into reads of the exporting module.
struct ImportUses<'a> {
    refs: HashMap<oxc_semantic::ReferenceId, (usize, String)>,
    b: AstBuilder<'a>,
    alloc: &'a Allocator,
}

impl<'a> VisitMut<'a> for ImportUses<'a> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        if let Expression::Identifier(id) = it {
            if let Some((m, name)) = id.reference_id.get().and_then(|r| self.refs.get(&r)) {
                let span = id.span;
                let object = Expression::new_identifier(
                    span,
                    self.alloc.alloc_str(&format!("__cw_i{m}")),
                    &self.b,
                );
                *it = if js_key(name).starts_with('"') {
                    Expression::new_computed_member_expression(
                        span,
                        object,
                        Expression::new_string_literal(
                            span,
                            self.alloc.alloc_str(name),
                            None,
                            &self.b,
                        ),
                        false,
                        &self.b,
                    )
                } else {
                    Expression::new_static_member_expression(
                        span,
                        object,
                        IdentifierName::new(span, self.alloc.alloc_str(name), &self.b),
                        false,
                        &self.b,
                    )
                };
                return;
            }
        }
        walk_mut::walk_expression(self, it);
    }

    fn visit_object_property(&mut self, it: &mut ObjectProperty<'a>) {
        // `{ a }` of an imported `a` becomes `{ a: __cw_i1.a }`.
        if it.shorthand {
            if let Expression::Identifier(id) = &it.value {
                if id
                    .reference_id
                    .get()
                    .is_some_and(|r| self.refs.contains_key(&r))
                {
                    it.shorthand = false;
                }
            }
        }
        walk_mut::walk_object_property(self, it);
    }
}

/// `process.env.NODE_ENV` is `"production"`, as a bundler defines it for the page
/// (packages branch on it; neither a browser nor the island has `process`).
struct DefineNodeEnv<'a> {
    b: AstBuilder<'a>,
}

impl<'a> VisitMut<'a> for DefineNodeEnv<'a> {
    fn visit_expression(&mut self, it: &mut Expression<'a>) {
        // `import.meta`, which a classic script cannot say: the bundle's object for
        // it (webpack defines no `env` on it; `url` is the page's).
        if let Expression::ImportMeta(m) = it {
            *it = Expression::new_identifier(m.span, "__cw_import_meta", &self.b);
            return;
        }
        if let Expression::StaticMemberExpression(m) = it {
            if m.property.name == "NODE_ENV" {
                if let Expression::StaticMemberExpression(inner) = &m.object {
                    if inner.property.name == "env"
                        && matches!(&inner.object, Expression::Identifier(p) if p.name == "process")
                    {
                        *it = Expression::new_string_literal(m.span, "production", None, &self.b);
                        return;
                    }
                }
            }
        }
        walk_mut::walk_expression(self, it);
    }
}
