//! The fallback: the same module as a plain classic script for React 18's UMD build.
//!
//! Types are stripped and JSX lowered to `React.createElement` by oxc's transformer;
//! imports from `react` and `react-dom` (`react-dom/client`) become destructurings of
//! the `React` and `ReactDOM` globals the UMD bundles define, and `export`s are
//! dropped (the script has nowhere to export to). No bundler is involved, so the page
//! loads `react.production.min.js`, `react-dom.production.min.js` and this script, in
//! Chrome and on the engine's `Realm` alike.

use oxc_allocator::{Allocator, TakeIn};
use oxc_ast::ast::{
    ExportDefaultDeclarationKind, ImportDeclarationSpecifier, ModuleExportName, Statement,
};
use oxc_codegen::{Codegen, CodegenOptions};
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use oxc_transformer::{JsxRuntime, TransformOptions, Transformer};

use crate::Diagnostic;

/// Compiles `source` to a classic script, or the reasons it cannot be.
pub fn emit(source: &str, file_name: &str) -> Result<String, Vec<Diagnostic>> {
    let allocator = Allocator::default();
    let source_type = SourceType::tsx();
    let ret = Parser::new(&allocator, source, source_type).parse();
    if !ret.diagnostics.is_empty() {
        return Err(ret
            .diagnostics
            .iter()
            .map(|d| Diagnostic::from_oxc(source, d))
            .collect());
    }
    let mut program = ret.program;
    let mut preamble = String::new();
    let mut errors = Vec::new();
    let alloc = &allocator;
    let body = program.body.take_in(&alloc);
    let mut kept = oxc_allocator::Vec::new_in(&alloc);
    for stmt in body {
        match stmt {
            Statement::ImportDeclaration(mut import) => {
                if import.import_kind.is_type() {
                    continue;
                }
                let module = import.source.value.as_str();
                let global = match module {
                    "react" => "React",
                    "react-dom" | "react-dom/client" => "ReactDOM",
                    other => {
                        errors.push(Diagnostic::at(
                            source,
                            import.span.start,
                            format!("import from `{other}`: only `react` and `react-dom` are available to a compiled app"),
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
                            let imported = match &s.imported {
                                ModuleExportName::IdentifierName(n) => n.name.as_str(),
                                ModuleExportName::IdentifierReference(n) => n.name.as_str(),
                                ModuleExportName::StringLiteral(n) => n.value.as_str(),
                            };
                            let local = s.local.name.as_str();
                            if imported == local {
                                named.push(local.to_owned());
                            } else {
                                named.push(format!("{imported}: {local}"));
                            }
                        }
                        ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                            if s.local.name.as_str() != global {
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
                kept.push(Statement::from(export.declaration.take_in(&alloc)));
            }
            // `export { a, b }`: the names stay declared; there is nowhere to export to.
            Statement::ExportNamedDeclaration(_) => {}
            Statement::ExportFromDeclaration(e) => {
                errors.push(Diagnostic::at(
                    source,
                    e.span.start,
                    "a re-export has nothing to re-export in a single-module app".into(),
                ));
            }
            Statement::ExportDefaultDeclaration(mut export) => {
                match export.declaration.take_in(&alloc) {
                    ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                        if f.id.is_some() {
                            kept.push(Statement::FunctionDeclaration(f));
                        }
                    }
                    ExportDefaultDeclarationKind::ClassDeclaration(c) if c.id.is_some() => {
                        kept.push(Statement::ClassDeclaration(c));
                    }
                    _ => {}
                }
            }
            Statement::ExportAllDeclaration(e) => {
                errors.push(Diagnostic::at(
                    source,
                    e.span.start,
                    "`export *` has nothing to re-export in a single-module app".into(),
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
    let path = std::path::Path::new(file_name);
    let ret =
        Transformer::new(&allocator, path, &options).build_with_scoping(scoping, &mut program);
    if !ret.diagnostics.is_empty() {
        return Err(ret
            .diagnostics
            .iter()
            .map(|d| Diagnostic::from_oxc(source, d))
            .collect());
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
    Ok(format!(
        "// Compiled by cw-tsx from {file_name}: types stripped, JSX as React.createElement.\n'use strict';\n{preamble}{code}"
    ))
}
