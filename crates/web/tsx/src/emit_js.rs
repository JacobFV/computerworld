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

use std::collections::BTreeSet;

use oxc_allocator::{Allocator, TakeIn};
use oxc_ast::ast::{
    BindingPattern, Declaration, ExportDefaultDeclarationKind, ImportDeclarationSpecifier,
    Statement,
};
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{JsxRuntime, TransformOptions, Transformer};

use crate::{Diagnostic, Source};

/// Compiles a one-module app to a classic script, or the reasons it cannot be.
pub fn emit(source: &str, file_name: &str) -> Result<String, Vec<Diagnostic>> {
    emit_modules(&[Source::single(source, file_name)])
}

/// Compiles an app's modules (in dependency order, entry last) to one script.
pub fn emit_modules(sources: &[Source]) -> Result<String, Vec<Diagnostic>> {
    let bundled = sources.len() > 1;
    let entry = sources.last().map(|s| s.file.as_str()).unwrap_or("app.tsx");
    let mut out = format!(
        "// Compiled by cw-tsx from {entry}: types stripped, JSX as React.createElement.\n'use strict';\n"
    );
    let mut errors = Vec::new();
    for (i, src) in sources.iter().enumerate() {
        match emit_one(src, sources.len()) {
            Ok((code, exports)) => {
                if !bundled {
                    out.push_str(&code);
                } else if i + 1 == sources.len() {
                    out.push_str(&format!("// {}\n(() => {{\n{code}}})();\n", src.file));
                } else {
                    let fields: Vec<String> = exports
                        .iter()
                        .map(|(exported, local)| {
                            if exported == local {
                                exported.clone()
                            } else {
                                format!("{}: {local}", js_key(exported))
                            }
                        })
                        .collect();
                    out.push_str(&format!(
                        "// {}\nconst __cw_mod_{i} = (() => {{\n{code}return {{ {} }};\n}})();\n",
                        src.file,
                        fields.join(", ")
                    ));
                }
            }
            Err(e) => errors.extend(e),
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
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

/// One module's code (imports rewritten, exports stripped) and its value exports
/// as (exported name, local name).
#[allow(clippy::type_complexity)]
fn emit_one(
    src: &Source,
    modules: usize,
) -> Result<(String, Vec<(String, String)>), Vec<Diagnostic>> {
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
    let ret = Parser::new(&allocator, source, SourceType::tsx()).parse();
    if !ret.diagnostics.is_empty() {
        return Err(ret.diagnostics.iter().map(from_oxc).collect());
    }
    let mut program = ret.program;
    let values = top_level_values(&program.body);
    let mut preamble = String::new();
    let mut errors = Vec::new();
    let mut exports: Vec<(String, String)> = Vec::new();
    let alloc = &allocator;
    let body = program.body.take_in(&alloc);
    let mut kept = oxc_allocator::Vec::new_in(&alloc);
    for stmt in body {
        match stmt {
            Statement::ImportDeclaration(mut import) => {
                if import.import_kind.is_type() {
                    continue;
                }
                let module = import.source.value.to_string();
                let local_module = src.imports.get(&module).copied();
                let global = match (module.as_str(), local_module) {
                    (_, Some(m)) => format!("__cw_mod_{m}"),
                    ("react", _) => "React".to_owned(),
                    ("react-dom" | "react-dom/client", _) => "ReactDOM".to_owned(),
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
                            let imported = s.imported.name().to_string();
                            let local = s.local.name.to_string();
                            if imported == local {
                                named.push(local);
                            } else {
                                named.push(format!("{}: {local}", js_key(&imported)));
                            }
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            if local_module.is_some() {
                                preamble.push_str(&format!(
                                    "const {} = {global}.default;\n",
                                    s.local.name
                                ));
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
                    if spec.export_kind.is_type() || !values.contains(&local) {
                        continue;
                    }
                    exports.push((spec.exported.name().to_string(), local));
                }
            }
            Statement::ExportFromDeclaration(e) => {
                errors.push(at(
                    e.span.start,
                    "a re-export from another module is outside what cw-tsx bundles (import, then export)".into(),
                ));
            }
            Statement::ExportDefaultDeclaration(mut export) => {
                let span = export.span;
                match export.declaration.take_in(&alloc) {
                    ExportDefaultDeclarationKind::FunctionDeclaration(f) => match &f.id {
                        Some(id) => {
                            exports.push(("default".into(), id.name.to_string()));
                            kept.push(Statement::FunctionDeclaration(f));
                        }
                        None if bundled => errors.push(at(
                            span.start,
                            "an anonymous default export: name the function".into(),
                        )),
                        None => {}
                    },
                    ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                        if let Some(id) = &c.id {
                            exports.push(("default".into(), id.name.to_string()));
                            kept.push(Statement::ClassDeclaration(c));
                        }
                    }
                    ExportDefaultDeclarationKind::Identifier(id) => {
                        exports.push(("default".into(), id.name.to_string()));
                    }
                    ExportDefaultDeclarationKind::TSInterfaceDeclaration(_) => {}
                    _ if bundled => errors.push(at(
                        span.start,
                        "`export default` of an expression: export a named declaration".into(),
                    )),
                    _ => {}
                }
            }
            Statement::ExportAllDeclaration(e) => {
                errors.push(at(
                    e.span.start,
                    "`export *` is outside what cw-tsx bundles".into(),
                ));
            }
            other => kept.push(other),
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    program.body = kept;
    let semantic = SemanticBuilder::new().build(&program);
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
    Ok((format!("{preamble}{code}"), exports))
}
