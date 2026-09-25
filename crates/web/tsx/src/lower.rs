//! Type-checks a TSX module against the subset and lowers it to the UI IR.
//!
//! One pass over the module collects its type declarations, React imports and
//! function signatures; a second lowers every global in order. Module functions are
//! lowered on first use as well, so a caller can see the return type a callee infers.
//! Every construct outside the subset leaves a `Diagnostic` and lowering carries on,
//! so one build reports every reason at once; any diagnostic means no IR.

use std::collections::{BTreeMap, BTreeSet};

use cw_ui::ir::*;
use oxc_allocator::Allocator;
use oxc_ast::ast::{self as ast, Expression as E, Statement as S};
use oxc_parser::Parser;
use oxc_span::{GetSpan, SourceType, Span};

use crate::types::{self, element, non_null, property, union, union_all, widen};
use crate::Diagnostic;

/// Lowers `source` to a module, or the reasons it is outside the subset.
pub fn lower(source: &str, file_name: &str) -> Result<Module, Vec<Diagnostic>> {
    lower_modules(&[crate::Source::single(source, file_name)])
}

/// Lowers an app of several modules, in dependency order (each after the modules it
/// imports, the entry last; see `crate::load`), to one IR module.
pub fn lower_modules(sources: &[crate::Source]) -> Result<Module, Vec<Diagnostic>> {
    let allocator = Allocator::default();
    let mut programs = Vec::with_capacity(sources.len());
    let mut errors = Vec::new();
    for src in sources {
        // A package's module is the island's, never lowered.
        let text = if src.package { "" } else { src.text.as_str() };
        let ret = Parser::new(&allocator, text, SourceType::tsx()).parse();
        for d in &ret.diagnostics {
            let mut d = Diagnostic::from_oxc(&src.text, d);
            d.file = src.display_file(crate::code_modules(sources));
            errors.push(d);
        }
        programs.push(ret.program);
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let mut l = Lowerer::new("");
    l.mods = sources
        .iter()
        .map(|s| ModNames {
            src: &s.text,
            file: s.display_file(crate::code_modules(sources)),
            ..ModNames::default()
        })
        .collect();
    l.ambient_mods = (0..sources.len())
        .filter(|i| sources[*i].is_ambient())
        .collect();
    l.package_mods = (0..sources.len()).filter(|i| sources[*i].package).collect();
    l.swap_current(0);
    let mut decls = Vec::with_capacity(programs.len());
    for (i, program) in programs.iter().enumerate() {
        l.enter_module(i);
        if sources[i].package {
            decls.push(Vec::new());
            continue;
        }
        decls.push(l.declare(&program.body, &sources[i].imports));
    }
    // Imports that close a cycle, now that every module is declared.
    for (i, import, m) in std::mem::take(&mut l.deferred_imports) {
        l.enter_module(i);
        l.local_import(import, m);
    }
    // Reassigned module `let`s make identity tracking unsafe across calls.
    l.mutable_globals = l.ginfo.iter().any(|g| g.reassigned);
    for (i, d) in decls.iter().enumerate() {
        l.enter_module(i);
        for stmt in d {
            l.module_statement(stmt);
        }
    }
    l.finish();
    let file_name = sources.last().map(|s| s.file.clone()).unwrap_or_default();
    if l.diags.is_empty() {
        Ok(Module {
            version: IR_VERSION,
            source: file_name,
            globals: l.globals,
            functions: l
                .functions
                .into_iter()
                .map(|f| f.expect("lowered"))
                .collect(),
            templates: l.templates,
            root: l.root,
            island: (!l.island_imports.is_empty()).then(|| Island {
                script: String::new(),
                imports: l.island_imports.clone(),
            }),
            mutates_shared: l.mutates_shared,
        })
    } else {
        let mut d = l.diags;
        d.sort_by(|a, b| (&a.file, a.line, a.col).cmp(&(&b.file, b.line, b.col)));
        d.dedup();
        Err(d)
    }
}

/// The local name of an anonymous or expression default export.
const DEFAULT_LOCAL: &str = "\u{0}default";

/// A name imported from `react` / `react-dom`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReactName {
    Hook(Hook),
    CreateContext,
    Fragment,
    StrictMode,
    Memo,
    ForwardRef,
    /// `useTransition()`: `[false, startTransition]` (no concurrent rendering).
    UseTransition,
    /// `useDeferredValue(v)`: `v`.
    UseDeferredValue,
    /// `useDebugValue(…)`: nothing.
    UseDebugValue,
    /// `startTransition(fn)`: runs `fn` now.
    StartTransition,
    /// `<Suspense fallback>`: its children (nothing compiled suspends).
    Suspense,
    CreateRoot,
    HydrateRoot,
    /// React 17's `ReactDOM.render(element, container)`.
    DomRender,
    /// `import React from 'react'` / `import * as React`.
    ReactNs,
    ReactDomNs,
    /// Anything else React exports (`useTransition`, `forwardRef`, ...).
    Other,
}

fn react_export(name: &str) -> ReactName {
    match name {
        "useState" => ReactName::Hook(Hook::State),
        "useReducer" => ReactName::Hook(Hook::Reducer),
        "useMemo" => ReactName::Hook(Hook::Memo),
        "useCallback" => ReactName::Hook(Hook::Callback),
        "useRef" => ReactName::Hook(Hook::Ref),
        "useEffect" => ReactName::Hook(Hook::Effect),
        "useLayoutEffect" => ReactName::Hook(Hook::LayoutEffect),
        "useContext" => ReactName::Hook(Hook::Context),
        "useId" => ReactName::Hook(Hook::Id),
        "useSyncExternalStore" => ReactName::Hook(Hook::SyncExternalStore),
        "createContext" => ReactName::CreateContext,
        "Fragment" => ReactName::Fragment,
        "StrictMode" => ReactName::StrictMode,
        "memo" => ReactName::Memo,
        "forwardRef" => ReactName::ForwardRef,
        "useImperativeHandle" => ReactName::Hook(Hook::ImperativeHandle),
        // Insertion effects run with the layout effects (nothing compiled reads
        // styles in between).
        "useInsertionEffect" => ReactName::Hook(Hook::LayoutEffect),
        "useTransition" => ReactName::UseTransition,
        "useDeferredValue" => ReactName::UseDeferredValue,
        "useDebugValue" => ReactName::UseDebugValue,
        "startTransition" => ReactName::StartTransition,
        "Suspense" => ReactName::Suspense,
        "createRoot" => ReactName::CreateRoot,
        "hydrateRoot" => ReactName::HydrateRoot,
        "render" => ReactName::DomRender,
        _ => ReactName::Other,
    }
}

struct Local {
    ty: Ty,
    /// Declared at its block's start (a `let`/`const`/function name), not yet
    /// initialised: reading it here is a use before its declaration, and a closure
    /// that reads it (`const f = () => f()`) shares it through a cell.
    pending: bool,
    /// Captured by a closure while `pending`: the block creates its cell when it
    /// starts, and the declaration assigns it.
    early: bool,
    captured: bool,
    /// Offset of an assignment after the declaration.
    reassigned: Option<u32>,
    /// Holds a value created in this frame (a literal, a `map` result), which
    /// mutating cannot affect anything that outlives the frame.
    fresh: bool,
}

struct FnCtx {
    kind: FunctionKind,
    locals: Vec<Local>,
    scopes: Vec<Vec<(String, u32)>>,
    /// Names declared later in each open block, for use-before-declaration.
    later: Vec<BTreeSet<String>>,
    captures: Vec<Capture>,
    capture_names: Vec<(String, Ty)>,
    /// Nesting inside conditionals and loops (hooks must be at depth 0).
    cond_depth: u32,
    has_depless_effect: bool,
    declared_ret: Option<Ty>,
    ret: Ty,
    is_async: bool,
}

impl FnCtx {
    fn new(kind: FunctionKind) -> FnCtx {
        FnCtx {
            kind,
            locals: Vec::new(),
            scopes: vec![Vec::new()],
            later: vec![BTreeSet::new()],
            captures: Vec::new(),
            capture_names: Vec::new(),
            cond_depth: 0,
            has_depless_effect: false,
            declared_ret: None,
            ret: Ty::Unknown,
            is_async: false,
        }
    }
    fn lookup(&self, name: &str) -> Option<u32> {
        for scope in self.scopes.iter().rev() {
            if let Some((_, slot)) = scope.iter().rev().find(|(n, _)| n == name) {
                return Some(*slot);
            }
        }
        None
    }
    fn declare(&mut self, name: &str, ty: Ty) -> u32 {
        // The binding its block declared in advance.
        if let Some(&(_, slot)) = self
            .scopes
            .last()
            .unwrap()
            .iter()
            .rev()
            .find(|(n, _)| n == name)
        {
            let l = &mut self.locals[slot as usize];
            if l.pending {
                l.fresh = false;
                l.pending = false;
                l.ty = ty;
                return slot;
            }
        }
        let slot = self.locals.len() as u32;
        self.locals.push(Local {
            ty,
            pending: false,
            early: false,
            captured: false,
            reassigned: None,
            fresh: false,
        });
        self.scopes
            .last_mut()
            .unwrap()
            .push((name.to_owned(), slot));
        if let Some(l) = self.later.last_mut() {
            l.remove(name);
        }
        slot
    }

    /// Declares `name` in the innermost scope ahead of its declaration.
    fn predeclare(&mut self, name: &str) -> u32 {
        let slot = self.locals.len() as u32;
        self.locals.push(Local {
            ty: Ty::Unknown,
            pending: true,
            early: false,
            captured: false,
            reassigned: None,
            fresh: false,
        });
        self.scopes
            .last_mut()
            .unwrap()
            .push((name.to_owned(), slot));
        slot
    }
}

/// What a module-level name is.
#[derive(Clone, Debug)]
struct GlobalInfo {
    slot: u32,
    ty: Ty,
    /// For a function: its index in `functions`.
    func: Option<u32>,
    kind: FunctionKind,
    reassigned: bool,
    /// For a generic function: its type parameters and its signature over them
    /// (`Ty::Param`), which each call instantiates.
    generic: Option<(Vec<String>, Ty)>,
}

/// A module function's declaration, lowered on demand.
struct PendingFn<'a> {
    name: String,
    type_params: Option<&'a ast::TSTypeParameterDeclaration<'a>>,
    /// The module it belongs to, and its global slot when it is a module function.
    module: usize,
    slot: Option<u32>,
    params: &'a ast::FormalParameters<'a>,
    body: FnBody<'a>,
    return_type: Option<&'a ast::TSTypeAnnotation<'a>>,
    span: Span,
    is_async: bool,
    generator: bool,
    kind: FunctionKind,
    /// `forwardRef(render)`: called with the element's `ref` as its second argument.
    forward_ref: bool,
}

#[derive(Clone, Copy)]
enum FnBody<'a> {
    Block(&'a ast::FunctionBody<'a>),
    Expr(&'a ast::Expression<'a>),
}

enum TypeDecl<'a> {
    Alias(
        &'a ast::TSType<'a>,
        Option<&'a ast::TSTypeParameterDeclaration<'a>>,
    ),
    Interface(&'a ast::TSInterfaceDeclaration<'a>),
}

struct TemplateBuilder {
    holes: Vec<Expr>,
    meta: Vec<Hole>,
}

/// The names one module sees. The lowerer works on one module at a time; the others'
/// tables wait in `Lowerer::mods` (see `enter_module`).
#[derive(Default)]
struct ModNames<'a> {
    src: &'a str,
    file: String,
    type_decls: BTreeMap<String, TypeDecl<'a>>,
    type_cache: BTreeMap<String, Ty>,
    react: BTreeMap<String, ReactName>,
    /// Module-level names (its own and the ones it imports) to global slots.
    global_names: BTreeMap<String, u32>,
    /// Imported type names: local name to (module, exported name).
    type_imports: BTreeMap<String, (usize, String)>,
    /// Exported names to the local names they export.
    exports: BTreeMap<String, String>,
}

struct Lowerer<'a> {
    src: &'a str,
    file: String,
    diags: Vec<Diagnostic>,
    type_decls: BTreeMap<String, TypeDecl<'a>>,
    type_cache: BTreeMap<String, Ty>,
    type_busy: BTreeSet<(usize, String)>,
    react: BTreeMap<String, ReactName>,
    globals: Vec<Global>,
    global_names: BTreeMap<String, u32>,
    type_imports: BTreeMap<String, (usize, String)>,
    exports: BTreeMap<String, String>,
    /// Per global slot.
    ginfo: Vec<GlobalInfo>,
    /// Every module's tables; the current module's are in the fields above.
    mods: Vec<ModNames<'a>>,
    /// Generic parameters in scope: each stands for its constraint.
    type_params: Vec<BTreeMap<String, Ty>>,
    /// The one `await` the statement being lowered may contain (see `ir::Expr::Await`).
    await_slot: Option<Span>,
    cur_mod: usize,
    functions: Vec<Option<Function>>,
    pending: BTreeMap<u32, PendingFn<'a>>,
    busy_fns: BTreeSet<u32>,
    templates: Vec<Template>,
    root: Option<Root>,
    mutates_shared: bool,
    fns: Vec<FnCtx>,
    /// Set while lowering a template hole when it reads something identity
    /// comparison cannot track.
    hole_always: Vec<bool>,
    /// Whether some module `let` is reassigned (then functions may read state that
    /// changes behind the frame's back).
    mutable_globals: bool,
    /// Declaration files (`.d.ts`): their types are visible to every module.
    ambient_mods: Vec<usize>,
    /// Constants they `declare` (the host's globals), to the module declaring them.
    ambient_values: BTreeMap<String, usize>,
    /// Imports of a module not yet declared (an import cycle): bound once every
    /// module is declared. Importer, declaration, imported module.
    deferred_imports: Vec<(usize, &'a ast::ImportDeclaration<'a>, usize)>,
    /// `import * as ns` of a module of the app, by (module, name): the module.
    namespaces: BTreeMap<(usize, String), usize>,
    /// Modules of npm packages (the island's).
    package_mods: Vec<usize>,
    /// What compiled code imports from the island: (specifier, name), each once.
    island_imports: Vec<(String, String)>,
    /// Module-level `const r = createRoot(container)`, by (module, name): the
    /// container's id.
    root_vars: BTreeMap<(usize, String), String>,
    /// Module-level `const el = document.getElementById(id)`, by (module, name).
    container_vars: BTreeMap<(usize, String), String>,
}

type Lowered = (Expr, Ty);

fn is_component_name(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_ascii_uppercase())
}

fn is_hook_name(name: &str) -> bool {
    name.len() > 3
        && name.starts_with("use")
        && name[3..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_uppercase())
}

fn fn_kind(name: &str) -> FunctionKind {
    if is_component_name(name) {
        FunctionKind::Component
    } else if is_hook_name(name) {
        FunctionKind::Hook
    } else {
        FunctionKind::Plain
    }
}

impl<'a> Lowerer<'a> {
    fn new(src: &'a str) -> Lowerer<'a> {
        Lowerer {
            src,
            file: String::new(),
            diags: Vec::new(),
            type_decls: BTreeMap::new(),
            type_cache: BTreeMap::new(),
            type_busy: BTreeSet::new(),
            react: BTreeMap::new(),
            globals: Vec::new(),
            global_names: BTreeMap::new(),
            type_imports: BTreeMap::new(),
            exports: BTreeMap::new(),
            ginfo: Vec::new(),
            mods: Vec::new(),
            type_params: Vec::new(),
            await_slot: None,
            cur_mod: 0,
            functions: Vec::new(),
            pending: BTreeMap::new(),
            busy_fns: BTreeSet::new(),
            templates: Vec::new(),
            root: None,
            mutates_shared: false,
            fns: Vec::new(),
            hole_always: Vec::new(),
            mutable_globals: false,
            ambient_mods: Vec::new(),
            ambient_values: BTreeMap::new(),
            deferred_imports: Vec::new(),
            namespaces: BTreeMap::new(),
            package_mods: Vec::new(),
            island_imports: Vec::new(),
            root_vars: BTreeMap::new(),
            container_vars: BTreeMap::new(),
        }
    }

    fn err(&mut self, span: Span, msg: impl Into<String>) {
        let mut d = Diagnostic::at(self.src, span.start, msg.into());
        d.file = self.file.clone();
        self.diags.push(d);
    }

    /// Makes module `m` the current one; returns the module that was.
    fn enter_module(&mut self, m: usize) -> usize {
        let prev = self.cur_mod;
        if m == prev {
            return prev;
        }
        self.swap_current(prev);
        self.swap_current(m);
        self.cur_mod = m;
        prev
    }

    /// Exchanges the current-module fields with `mods[m]`.
    fn swap_current(&mut self, m: usize) {
        let t = &mut self.mods[m];
        std::mem::swap(&mut self.src, &mut t.src);
        std::mem::swap(&mut self.file, &mut t.file);
        std::mem::swap(&mut self.type_decls, &mut t.type_decls);
        std::mem::swap(&mut self.type_cache, &mut t.type_cache);
        std::mem::swap(&mut self.react, &mut t.react);
        std::mem::swap(&mut self.global_names, &mut t.global_names);
        std::mem::swap(&mut self.type_imports, &mut t.type_imports);
        std::mem::swap(&mut self.exports, &mut t.exports);
    }

    /// Brings a function's generic parameters into scope (each as its constraint).
    fn push_type_params(&mut self, tp: Option<&'a ast::TSTypeParameterDeclaration<'a>>) {
        let mut scope = BTreeMap::new();
        self.type_params.push(BTreeMap::new());
        if let Some(tp) = tp {
            for p in &tp.params {
                let t = match &p.constraint {
                    Some(c) => self.ts_type(c),
                    None => Ty::Unknown,
                };
                scope.insert(p.name.name.to_string(), t.clone());
                self.type_params
                    .last_mut()
                    .unwrap()
                    .insert(p.name.name.to_string(), t);
            }
        }
        let _ = scope;
    }

    fn gname(&self, name: &str) -> Option<&GlobalInfo> {
        self.global_names
            .get(name)
            .map(|s| &self.ginfo[*s as usize])
    }

    fn gname_mut(&mut self, name: &str) -> Option<&mut GlobalInfo> {
        let s = *self.global_names.get(name)?;
        Some(&mut self.ginfo[s as usize])
    }

    fn add_global_name(&mut self, name: &str, info: GlobalInfo) {
        let slot = info.slot;
        debug_assert_eq!(slot as usize, self.ginfo.len());
        self.ginfo.push(info);
        self.global_names.insert(name.to_owned(), slot);
    }

    fn unsupported(&mut self, span: Span, what: &str) -> Lowered {
        self.err(span, format!("{what} is outside the compiled subset"));
        (Expr::Undefined, Ty::Unknown)
    }

    fn line(&self, span: Span) -> u32 {
        crate::line_col(self.src, span.start).0
    }

    fn cur(&mut self) -> &mut FnCtx {
        self.fns.last_mut().expect("inside a function")
    }

    // ------------------------------------------------------------------ module

    /// Passes 1 and 2 over the current module: its imports, type declarations and
    /// exports, then every global it declares. Returns the statements to lower.
    fn declare(
        &mut self,
        body: &'a oxc_allocator::Vec<'a, S<'a>>,
        imports: &BTreeMap<String, usize>,
    ) -> Vec<&'a S<'a>> {
        self.collect_exports(body, imports);
        // Pass 1: imports and type declarations; module declarations to lower.
        let mut decls: Vec<&'a S<'a>> = Vec::new();
        for stmt in body.iter() {
            match stmt {
                S::ImportDeclaration(import) => self.import(import, imports),
                S::TSTypeAliasDeclaration(t) => {
                    self.type_decls.insert(
                        t.id.name.to_string(),
                        TypeDecl::Alias(&t.type_annotation, t.type_parameters.as_deref()),
                    );
                }
                S::TSInterfaceDeclaration(i) => {
                    self.type_decls
                        .insert(i.id.name.to_string(), TypeDecl::Interface(i));
                }
                S::ExportDeclaration(e) => match &e.declaration {
                    ast::Declaration::TSTypeAliasDeclaration(t) => {
                        self.type_decls.insert(
                            t.id.name.to_string(),
                            TypeDecl::Alias(&t.type_annotation, t.type_parameters.as_deref()),
                        );
                    }
                    ast::Declaration::TSInterfaceDeclaration(i) => {
                        self.type_decls
                            .insert(i.id.name.to_string(), TypeDecl::Interface(i));
                    }
                    _ => decls.push(stmt),
                },
                S::ExportNamedDeclaration(_) => {}
                _ if self.ambient_mods.contains(&self.cur_mod) => match stmt {
                    S::VariableDeclaration(v) if v.declare => {
                        for d in &v.declarations {
                            if let ast::BindingPattern::BindingIdentifier(id) = &d.id {
                                self.ambient_values
                                    .insert(id.name.to_string(), self.cur_mod);
                            }
                        }
                    }
                    other => self.err(
                        other.span(),
                        "a declaration file for the compiled subset declares types and constants only",
                    ),
                },
                _ => decls.push(stmt),
            }
        }
        // Pass 2: declare every global (functions hoisted first, then the rest in
        // order) so functions can refer to globals declared after them.
        for stmt in &decls {
            if let Some(f) = self.function_decl_of(stmt) {
                self.declare_function(f);
            }
        }
        for stmt in &decls {
            self.declare_statement_globals(stmt);
        }
        decls
    }

    /// What the current module exports, by exported name to local name.
    fn collect_exports(
        &mut self,
        body: &'a oxc_allocator::Vec<'a, S<'a>>,
        imports: &BTreeMap<String, usize>,
    ) {
        for stmt in body.iter() {
            match stmt {
                S::ExportDeclaration(e) => {
                    let mut names = Vec::new();
                    match &e.declaration {
                        ast::Declaration::FunctionDeclaration(f) => {
                            names.extend(f.id.as_ref().map(|i| i.name.to_string()))
                        }
                        ast::Declaration::VariableDeclaration(v) => {
                            for d in &v.declarations {
                                let mut b = Vec::new();
                                binding_names(&d.id, &mut b);
                                names.extend(b.into_iter().map(|(n, _)| n));
                            }
                        }
                        ast::Declaration::TSTypeAliasDeclaration(t) => {
                            names.push(t.id.name.to_string())
                        }
                        ast::Declaration::TSInterfaceDeclaration(i) => {
                            names.push(i.id.name.to_string())
                        }
                        ast::Declaration::TSEnumDeclaration(e) => names.push(e.id.name.to_string()),
                        ast::Declaration::ClassDeclaration(c) => {
                            names.extend(c.id.as_ref().map(|i| i.name.to_string()))
                        }
                        _ => {}
                    }
                    for n in names {
                        self.exports.insert(n.clone(), n);
                    }
                }
                S::ExportDefaultDeclaration(e) => match &e.declaration {
                    ast::ExportDefaultDeclarationKind::FunctionDeclaration(f) => {
                        let local = match &f.id {
                            Some(id) => id.name.to_string(),
                            None => DEFAULT_LOCAL.to_owned(),
                        };
                        self.exports.insert("default".into(), local);
                    }
                    ast::ExportDefaultDeclarationKind::Identifier(id) => {
                        self.exports.insert("default".into(), id.name.to_string());
                    }
                    _ => {
                        self.exports
                            .insert("default".into(), DEFAULT_LOCAL.to_owned());
                    }
                },
                S::ExportNamedDeclaration(e) => {
                    for spec in &e.specifiers {
                        self.exports.insert(
                            spec.exported.name().to_string(),
                            spec.local.name().to_string(),
                        );
                    }
                }
                S::ExportFromDeclaration(e) => {
                    let from = e.source.value.as_str();
                    let Some(m) = self.reexported_module(imports, from, e.span) else {
                        continue;
                    };
                    for spec in &e.specifiers {
                        let exported = spec.exported.name().to_string();
                        let theirs = spec.local.name().to_string();
                        let local = format!("\u{0}re:{exported}");
                        let only_type = e.export_kind.is_type() || spec.export_kind.is_type();
                        self.bind_import(&local, m, &theirs, only_type, e.span, from);
                        self.exports.insert(exported, local);
                    }
                }
                S::ExportAllDeclaration(e) => {
                    let from = e.source.value.as_str();
                    let Some(m) = self.reexported_module(imports, from, e.span) else {
                        continue;
                    };
                    match &e.exported {
                        Some(ns) => {
                            let exported = ns.name().to_string();
                            let local = format!("\u{0}re:{exported}");
                            self.namespace_import(&local, m);
                            self.exports.insert(exported, local);
                        }
                        None => {
                            let names: Vec<String> = self.mods[m]
                                .exports
                                .keys()
                                .filter(|n| *n != "default")
                                .cloned()
                                .collect();
                            for n in names {
                                if self.exports.contains_key(&n) {
                                    continue;
                                }
                                let local = format!("\u{0}re:{n}");
                                self.bind_import(&local, m, &n, false, e.span, from);
                                self.exports.insert(n, local);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// The module of the app an `export … from` names (declared before this one).
    fn reexported_module(
        &mut self,
        imports: &BTreeMap<String, usize>,
        from: &str,
        span: Span,
    ) -> Option<usize> {
        match imports.get(from) {
            Some(&m) if m < self.cur_mod => Some(m),
            Some(_) => {
                self.err(
                    span,
                    "re-exporting from a module in an import cycle is outside the compiled subset",
                );
                None
            }
            None => {
                self.err(
                    span,
                    format!(
                        "import from `{from}`: a compiled app imports only `react` and `react-dom`"
                    ),
                );
                None
            }
        }
    }

    /// After every module: functions nobody called, and the render root.
    fn finish(&mut self) {
        // Functions nobody called yet.
        let rest: Vec<u32> = self.pending.keys().copied().collect();
        for f in rest {
            self.ensure_function(f);
        }
        if self.root.is_none() {
            self.diags.push(Diagnostic {
                file: String::new(),
                line: 1,
                col: 1,
                message: "the module never renders: expected `createRoot(document.getElementById(id)).render(<App />)`".into(),
            });
        }
    }

    fn import(
        &mut self,
        import: &'a ast::ImportDeclaration<'a>,
        imports: &BTreeMap<String, usize>,
    ) {
        let module = import.source.value.as_str();
        let resolved = imports.get(module).copied();
        if resolved.is_some_and(|m| self.package_mods.contains(&m))
            || (resolved.is_none()
                && !module.starts_with('.')
                && !matches!(module, "react" | "react-dom" | "react-dom/client"))
        {
            // A package: its values come from the island.
            self.island_import(import);
            return;
        }
        if resolved.is_none()
            && import.specifiers.as_ref().is_none_or(|s| s.is_empty())
            && [".css", ".scss", ".sass", ".less"]
                .iter()
                .any(|e| module.ends_with(e))
        {
            // `import './App.css'`: the page's stylesheet, loaded with the page.
            return;
        }
        if let Some(&m) = imports.get(module) {
            if m >= self.cur_mod {
                self.deferred_imports.push((self.cur_mod, import, m));
            } else {
                self.local_import(import, m);
            }
            return;
        }
        if import.import_kind.is_type() {
            return;
        }
        let is_dom = matches!(module, "react-dom" | "react-dom/client");
        if module != "react" && !is_dom {
            self.err(
                import.span,
                format!(
                    "import from `{module}`: a compiled app imports only `react` and `react-dom`"
                ),
            );
            return;
        }
        for spec in import.specifiers.iter().flatten() {
            match spec {
                ast::ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    if s.import_kind.is_type() {
                        continue;
                    }
                    let name = s.imported.name();
                    self.react
                        .insert(s.local.name.to_string(), react_export(name.as_str()));
                }
                ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    self.react.insert(
                        s.local.name.to_string(),
                        if is_dom {
                            ReactName::ReactDomNs
                        } else {
                            ReactName::ReactNs
                        },
                    );
                }
                ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    self.react.insert(
                        s.local.name.to_string(),
                        if is_dom {
                            ReactName::ReactDomNs
                        } else {
                            ReactName::ReactNs
                        },
                    );
                }
            }
        }
    }

    /// An import from a package: each name a global the island's export initialises.
    fn island_import(&mut self, import: &'a ast::ImportDeclaration<'a>) {
        if import.import_kind.is_type() {
            return;
        }
        let spec = import.source.value.to_string();
        let export = |l: &mut Self, name: &str| -> u32 {
            let key = (spec.clone(), name.to_owned());
            match l.island_imports.iter().position(|k| *k == key) {
                Some(i) => i as u32,
                None => {
                    l.island_imports.push(key);
                    l.island_imports.len() as u32 - 1
                }
            }
        };
        let specifiers = import.specifiers.as_ref();
        if specifiers.is_none_or(|s| s.is_empty()) {
            // `import 'pkg'`: run for its effects.
            export(self, "");
            return;
        }
        for s in specifiers.into_iter().flatten() {
            let (local, name) = match s {
                ast::ImportDeclarationSpecifier::ImportSpecifier(s) => {
                    if s.import_kind.is_type() {
                        continue;
                    }
                    (s.local.name.to_string(), s.imported.name().to_string())
                }
                ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    (s.local.name.to_string(), "default".to_owned())
                }
                ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    (s.local.name.to_string(), "*".to_owned())
                }
            };
            let k = export(self, &name);
            let slot = self.globals.len() as u32;
            self.globals.push(Global {
                name: local.clone(),
                init: GlobalInit::Island(k),
                ty: Ty::Unknown,
            });
            self.add_global_name(
                &local,
                GlobalInfo {
                    slot,
                    ty: Ty::Unknown,
                    func: None,
                    kind: FunctionKind::Plain,
                    reassigned: false,
                    generic: None,
                },
            );
        }
    }

    /// An import from another module of the app: its values become names for the
    /// same global slots, its types names that resolve there.
    fn local_import(&mut self, import: &'a ast::ImportDeclaration<'a>, m: usize) {
        let whole_type = import.import_kind.is_type();
        for spec in import.specifiers.iter().flatten() {
            let (local, exported, only_type) = match spec {
                ast::ImportDeclarationSpecifier::ImportSpecifier(s) => (
                    s.local.name.to_string(),
                    s.imported.name().to_string(),
                    whole_type || s.import_kind.is_type(),
                ),
                ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                    (s.local.name.to_string(), "default".to_string(), whole_type)
                }
                ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                    self.namespace_import(&s.local.name, m);
                    continue;
                }
            };
            self.bind_import(
                &local,
                m,
                &exported,
                only_type,
                import.span,
                &import.source.value,
            );
        }
    }

    /// Binds `local` in the current module to what module `m` exports as `exported`.
    fn bind_import(
        &mut self,
        local: &str,
        m: usize,
        exported: &str,
        only_type: bool,
        span: Span,
        from: &str,
    ) {
        let theirs = &self.mods[m];
        let Some(their_local) = theirs.exports.get(exported).cloned() else {
            self.err(span, format!("`{exported}` is not exported by `{from}`"));
            return;
        };
        let value = theirs.global_names.get(&their_local).copied();
        let is_type = theirs.type_decls.contains_key(&their_local)
            || theirs.type_imports.contains_key(&their_local);
        let ns = self.namespaces.get(&(m, their_local.clone())).copied();
        if let (Some(slot), false) = (value, only_type) {
            self.global_names.insert(local.to_owned(), slot);
        }
        if let Some(n) = ns {
            self.namespaces.insert((self.cur_mod, local.to_owned()), n);
        }
        if is_type {
            self.type_imports
                .insert(local.to_owned(), (m, exported.to_owned()));
        } else if value.is_none() {
            self.err(
                span,
                format!("`{exported}` from `{from}` is neither a value nor a type the compiled subset knows"),
            );
        }
    }

    /// `import * as ns` of module `m`: an object of its exported values (made when
    /// the module loads), and a name its types are reached through (`ns.Props`).
    fn namespace_import(&mut self, local: &str, m: usize) {
        let theirs = &self.mods[m];
        let mut props = Vec::new();
        for (exported, their_local) in &theirs.exports {
            if let Some(slot) = theirs.global_names.get(their_local) {
                props.push(Prop::KeyValue(exported.clone(), Expr::Global(*slot)));
            }
        }
        let slot = self.globals.len() as u32;
        self.globals.push(Global {
            name: local.to_owned(),
            init: GlobalInit::Expr(Expr::Object(props)),
            ty: Ty::Unknown,
        });
        self.add_global_name(
            local,
            GlobalInfo {
                slot,
                ty: Ty::Unknown,
                func: None,
                kind: FunctionKind::Plain,
                reassigned: false,
                generic: None,
            },
        );
        self.namespaces.insert((self.cur_mod, local.to_owned()), m);
    }

    fn function_decl_of(&self, stmt: &'a S<'a>) -> Option<&'a ast::Function<'a>> {
        match stmt {
            S::FunctionDeclaration(f) => Some(f),
            S::ExportDeclaration(e) => match &e.declaration {
                ast::Declaration::FunctionDeclaration(f) => Some(f),
                _ => None,
            },
            S::ExportDefaultDeclaration(e) => match &e.declaration {
                ast::ExportDefaultDeclarationKind::FunctionDeclaration(f) => Some(f),
                _ => None,
            },
            _ => None,
        }
    }

    fn declare_function(&mut self, f: &'a ast::Function<'a>) {
        // An anonymous `export default function` (a page, a component).
        let name = match &f.id {
            Some(id) => id.name.to_string(),
            None => DEFAULT_LOCAL.to_owned(),
        };
        let Some(body) = &f.body else {
            return;
        };
        self.add_function_global(
            &name,
            PendingFn {
                forward_ref: false,
                module: self.cur_mod,
                type_params: f.type_parameters.as_deref(),
                slot: None,
                name: name.clone(),
                params: &f.params,
                body: FnBody::Block(body),
                return_type: f.return_type.as_deref(),
                span: f.span,
                is_async: f.r#async,
                generator: f.generator,
                kind: if f.id.is_none() {
                    FunctionKind::Component
                } else {
                    fn_kind(&name)
                },
            },
        );
    }

    fn add_function_global(&mut self, name: &str, p: PendingFn<'a>) {
        let fidx = self.functions.len() as u32;
        self.functions.push(None);
        let slot = self.globals.len() as u32;
        let kind = p.kind;
        let ty = self.signature(&p);
        let generic = p.type_params.map(|tp| {
            let names: Vec<String> = tp.params.iter().map(|t| t.name.name.to_string()).collect();
            self.type_params.push(
                names
                    .iter()
                    .map(|n| (n.clone(), Ty::Param(n.clone())))
                    .collect(),
            );
            let sig = self.signature_inner(&p);
            self.type_params.pop();
            (names, sig)
        });
        self.globals.push(Global {
            name: name.to_owned(),
            init: GlobalInit::Function(fidx),
            ty: ty.clone(),
        });
        self.add_global_name(
            name,
            GlobalInfo {
                slot,
                ty,
                func: Some(fidx),
                kind,
                reassigned: false,
                generic,
            },
        );
        let mut p = p;
        p.slot = Some(slot);
        self.pending.insert(fidx, p);
    }

    /// A function's type from its annotations (return type `Unknown` until lowered
    /// when not annotated).
    fn signature(&mut self, p: &PendingFn<'a>) -> Ty {
        self.push_type_params(p.type_params);
        let t = self.signature_inner(p);
        self.type_params.pop();
        t
    }

    fn signature_inner(&mut self, p: &PendingFn<'a>) -> Ty {
        let params: Vec<Ty> = p
            .params
            .items
            .iter()
            .map(|param| match &param.type_annotation {
                Some(t) => self.ts_type(&t.type_annotation),
                None => Ty::Unknown,
            })
            .collect();
        let ret = match p.return_type {
            Some(t) => self.ts_type(&t.type_annotation),
            None if p.kind == FunctionKind::Component => Ty::Node,
            None => Ty::Unknown,
        };
        Ty::Function(params, Box::new(ret))
    }

    /// A module global that holds a value (not a function).
    fn add_value_global(&mut self, name: &str) -> u32 {
        let slot = self.globals.len() as u32;
        self.globals.push(Global {
            name: name.to_owned(),
            init: GlobalInit::Undefined,
            ty: Ty::Unknown,
        });
        self.add_global_name(
            name,
            GlobalInfo {
                slot,
                ty: Ty::Unknown,
                func: None,
                kind: FunctionKind::Plain,
                reassigned: false,
                generic: None,
            },
        );
        slot
    }

    fn declare_statement_globals(&mut self, stmt: &'a S<'a>) {
        let decl = match stmt {
            S::VariableDeclaration(d) => d,
            S::ExportDeclaration(e) => match &e.declaration {
                ast::Declaration::VariableDeclaration(d) => d,
                ast::Declaration::TSEnumDeclaration(en) => {
                    self.add_value_global(en.id.name.as_str());
                    return;
                }
                _ => return,
            },
            S::TSEnumDeclaration(en) => {
                self.add_value_global(en.id.name.as_str());
                return;
            }
            S::ExportDefaultDeclaration(e) => {
                if let Some(x) = e.declaration.as_expression() {
                    if !matches!(x, E::Identifier(_)) {
                        match self.function_value(DEFAULT_LOCAL, x) {
                            Some(mut p) => {
                                p.kind = FunctionKind::Component;
                                self.add_function_global(DEFAULT_LOCAL, p);
                            }
                            None => {
                                self.add_value_global(DEFAULT_LOCAL);
                            }
                        }
                    }
                }
                return;
            }
            _ => return,
        };
        for d in &decl.declarations {
            // `const Comp = (props: P) => ...` and `memo(...)` are functions.
            if let (ast::BindingPattern::BindingIdentifier(id), Some(init)) = (&d.id, &d.init) {
                let name = id.name.to_string();
                if let Some(p) = self.function_value(&name, init) {
                    self.add_function_global(&name, p);
                    continue;
                }
            }
            let mut names = Vec::new();
            binding_names(&d.id, &mut names);
            for (name, _) in names {
                let slot = self.globals.len() as u32;
                self.globals.push(Global {
                    name: name.clone(),
                    init: GlobalInit::Undefined,
                    ty: Ty::Unknown,
                });
                self.add_global_name(
                    &name,
                    GlobalInfo {
                        slot,
                        ty: Ty::Unknown,
                        func: None,
                        kind: FunctionKind::Plain,
                        reassigned: false,
                        generic: None,
                    },
                );
            }
        }
        // Module `let`s assigned anywhere after their declaration.
        if decl.kind == ast::VariableDeclarationKind::Let {
            // Detected when lowering assignments; see `assign_target`.
        }
    }

    /// A module `const` whose value is a function (an arrow, a function expression,
    /// or one wrapped in `memo`).
    fn function_value(&self, name: &str, init: &'a E<'a>) -> Option<PendingFn<'a>> {
        match strip(init) {
            E::ArrowFunctionExpression(a) => Some(PendingFn {
                forward_ref: false,
                module: self.cur_mod,
                type_params: a.type_parameters.as_deref(),
                slot: None,
                name: name.to_owned(),
                params: &a.params,
                body: match &a.body {
                    ast::ArrowFunctionBody::FunctionBody(b) => FnBody::Block(b),
                    other => FnBody::Expr(other.as_expression().expect("expression body")),
                },
                return_type: a.return_type.as_deref(),
                span: a.span,
                is_async: a.r#async,
                generator: false,
                kind: fn_kind(name),
            }),
            E::FunctionExpression(f) => Some(PendingFn {
                forward_ref: false,
                module: self.cur_mod,
                type_params: f.type_parameters.as_deref(),
                slot: None,
                name: name.to_owned(),
                params: &f.params,
                body: FnBody::Block(f.body.as_ref()?),
                return_type: f.return_type.as_deref(),
                span: f.span,
                is_async: f.r#async,
                generator: f.generator,
                kind: fn_kind(name),
            }),
            E::CallExpression(c) => {
                let callee = match strip(&c.callee) {
                    E::Identifier(id) => self.react.get(id.name.as_str()).copied(),
                    E::StaticMemberExpression(m) => match strip(&m.object) {
                        E::Identifier(id)
                            if self.react.get(id.name.as_str()) == Some(&ReactName::ReactNs) =>
                        {
                            Some(react_export(m.property.name.as_str()))
                        }
                        _ => None,
                    },
                    _ => None,
                };
                let inner = c.arguments.first().and_then(|a| a.as_expression());
                match (callee, inner) {
                    // `memo(C)` is `C` (renders are pure; only render counts differ).
                    (Some(ReactName::Memo), Some(x)) => self.function_value(name, x),
                    (Some(ReactName::ForwardRef), Some(x)) => {
                        let mut p = self.function_value(name, x)?;
                        p.forward_ref = true;
                        p.kind = FunctionKind::Component;
                        Some(p)
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }

    fn module_statement(&mut self, stmt: &'a S<'a>) {
        match stmt {
            S::ImportDeclaration(_) | S::EmptyStatement(_) => {}
            S::FunctionDeclaration(_) => {}
            S::TSTypeAliasDeclaration(_) | S::TSInterfaceDeclaration(_) => {}
            S::ExportDefaultDeclaration(e) => match &e.declaration {
                ast::ExportDefaultDeclarationKind::FunctionDeclaration(_) => {}
                ast::ExportDefaultDeclarationKind::Identifier(_) => {}
                ast::ExportDefaultDeclarationKind::ClassDeclaration(c) => {
                    self.err(c.span, "class components are outside the compiled subset");
                }
                other => {
                    let x = other.as_expression().expect("an expression");
                    if self.gname(DEFAULT_LOCAL).is_some_and(|g| g.func.is_none()) {
                        self.module_value(DEFAULT_LOCAL, x, e.span);
                    }
                }
            },
            S::ExportDeclaration(e) => match &e.declaration {
                ast::Declaration::VariableDeclaration(d) => self.module_var(d),
                ast::Declaration::TSEnumDeclaration(en) => self.enum_decl(en),
                ast::Declaration::ClassDeclaration(c) => {
                    self.err(c.span, "class components are outside the compiled subset");
                }
                _ => {}
            },
            S::ExportNamedDeclaration(_)
            | S::ExportFromDeclaration(_)
            | S::ExportAllDeclaration(_) => {}
            S::VariableDeclaration(d) => self.module_var(d),
            S::ExpressionStatement(e) => {
                if !self.render_call(&e.expression) {
                    self.run_statement(stmt);
                }
            }
            S::TSEnumDeclaration(e) => self.enum_decl(e),
            S::ClassDeclaration(c) => {
                self.err(c.span, "class components are outside the compiled subset");
            }
            S::TSGlobalDeclaration(_) | S::TSExternalModuleDeclaration(_) => {}
            S::TSNamespaceDeclaration(n) if n.declare => {}
            S::TSImportEqualsDeclaration(i) => {
                self.err(
                    i.span,
                    "`import … = require(…)` is outside the compiled subset",
                );
            }
            // Any other statement (`if`, a loop, `try`, a block) runs when the
            // module loads, in order with the module's declarations.
            _ => self.run_statement(stmt),
        }
    }

    /// A module-level statement that runs when the module loads: a function of
    /// its own, run at its place in the globals' initialisation order.
    fn run_statement(&mut self, stmt: &'a S<'a>) {
        let fidx = self.functions.len() as u32;
        self.functions.push(None);
        self.fns.push(FnCtx::new(FunctionKind::Plain));
        let mut body = Vec::new();
        self.cur().scopes.push(Vec::new());
        self.statement(stmt, &mut body);
        let ctx = self.fns.pop().unwrap();
        self.finish_run(fidx, ctx, body, stmt.span());
    }

    /// Records the load-time function `fidx` made of `body` in frame `ctx`.
    fn finish_run(&mut self, fidx: u32, ctx: FnCtx, body: Vec<Stmt>, span: Span) {
        let boxed: Vec<u32> = ctx
            .locals
            .iter()
            .enumerate()
            .filter(|(_, l)| l.captured && l.reassigned.is_some())
            .map(|(i, _)| i as u32)
            .collect();
        self.functions[fidx as usize] = Some(Function {
            name: "<module>".into(),
            kind: FunctionKind::Plain,
            params: Vec::new(),
            n_locals: ctx.locals.len() as u32,
            captures: Vec::new(),
            body,
            local_types: ctx.locals.iter().map(|l| l.ty.clone()).collect(),
            ret: Ty::Void,
            line: self.line(span),
            has_depless_effect: false,
            boxed,
            is_async: false,
            rest: None,
            forward_ref: false,
        });
        self.globals.push(Global {
            name: "<module>".into(),
            init: GlobalInit::Run(fidx),
            ty: Ty::Void,
        });
        self.ginfo.push(GlobalInfo {
            slot: self.globals.len() as u32 - 1,
            ty: Ty::Void,
            func: None,
            kind: FunctionKind::Plain,
            reassigned: false,
            generic: None,
        });
    }

    /// A module global `name` initialised with `x` when the module loads.
    fn module_value(&mut self, name: &str, x: &'a E<'a>, span: Span) {
        self.fns.push(FnCtx::new(FunctionKind::Plain));
        let (init, ty) = self.expr(x, None);
        let ctx = self.fns.pop().unwrap();
        let g = self.gname(name).expect("declared").slot;
        self.ginfo[g as usize].ty = ty.clone();
        self.globals[g as usize].ty = ty;
        if ctx.locals.is_empty() && ctx.captures.is_empty() {
            self.globals[g as usize].init = GlobalInit::Expr(init);
        } else {
            let fidx = self.functions.len() as u32;
            self.functions.push(None);
            let body = vec![Stmt::Expr(Expr::Assign(
                Box::new(LValue::Global(g)),
                None,
                Box::new(init),
            ))];
            self.finish_run(fidx, ctx, body, span);
        }
    }

    /// `enum E { A, B = 5, C = 'c' }`: an object of its members, with the reverse
    /// mapping TypeScript gives numeric members (`E[5] === 'B'`).
    fn enum_decl(&mut self, en: &'a ast::TSEnumDeclaration<'a>) {
        let mut props = Vec::new();
        let mut next = Some(0.0);
        self.fns.push(FnCtx::new(FunctionKind::Plain));
        for m in &en.body.members {
            let name = match &m.id {
                ast::TSEnumMemberName::Identifier(i) => i.name.to_string(),
                ast::TSEnumMemberName::String(s) => s.value.to_string(),
                other => {
                    self.err(
                        other.span(),
                        "this enum member name is outside the compiled subset",
                    );
                    continue;
                }
            };
            let value = match m.initializer.as_ref().map(strip) {
                None => match next {
                    Some(n) => Expr::Num(n),
                    None => {
                        self.err(m.span, "an enum member after a string member needs a value");
                        continue;
                    }
                },
                Some(E::NumericLiteral(n)) => Expr::Num(n.value),
                Some(E::UnaryExpression(u))
                    if u.operator == ast::UnaryOperator::UnaryNegation
                        && matches!(strip(&u.argument), E::NumericLiteral(_)) =>
                {
                    match strip(&u.argument) {
                        E::NumericLiteral(n) => Expr::Num(-n.value),
                        _ => unreachable!(),
                    }
                }
                Some(E::StringLiteral(s)) => Expr::Str(s.value.to_string()),
                Some(other) => self.expr(other, None).0,
            };
            next = match &value {
                Expr::Num(n) => Some(n + 1.0),
                _ => None,
            };
            if let Expr::Num(n) = &value {
                props.push(Prop::KeyValue(name.clone(), value.clone()));
                props.push(Prop::KeyValue(num_key(*n), Expr::Str(name)));
            } else {
                props.push(Prop::KeyValue(name, value));
            }
        }
        self.fns.pop();
        let g = self.gname(en.id.name.as_str()).expect("declared").slot as usize;
        self.globals[g].init = GlobalInit::Expr(Expr::Object(props));
    }

    fn module_var(&mut self, d: &'a ast::VariableDeclaration<'a>) {
        for decl in &d.declarations {
            if let (ast::BindingPattern::BindingIdentifier(id), Some(init)) = (&decl.id, &decl.init)
            {
                let name = id.name.to_string();
                if self.gname(&name).is_some_and(|g| g.func.is_some()) {
                    continue;
                }
                // `const root = createRoot(container)`: the render root, rendered
                // into by `root.render(<App />)` below.
                if let Some(container) = self.create_root_container(init) {
                    if let Some(id) = container {
                        self.root_vars.insert((self.cur_mod, name), id);
                    }
                    continue;
                }
                if let Some(cid) = self.container_id(init) {
                    self.container_vars.insert((self.cur_mod, name), cid);
                }
            }
            // Module initialisers run in a frame of their own.
            self.fns.push(FnCtx::new(FunctionKind::Plain));
            let declared = decl
                .type_annotation
                .as_ref()
                .map(|t| self.ts_type(&t.type_annotation));
            let (init, ty) = match &decl.init {
                Some(e) => {
                    // `createContext<T>(default)`.
                    if let Some((def, t)) = self.create_context(e, declared.as_ref()) {
                        self.fns.pop();
                        if let ast::BindingPattern::BindingIdentifier(id) = &decl.id {
                            let name = id.name.to_string();
                            let g = self.gname_mut(&name).expect("declared");
                            g.ty = t.clone();
                            let slot = g.slot as usize;
                            self.globals[slot].init = GlobalInit::Context(def);
                            self.globals[slot].ty = t;
                        }
                        continue;
                    }
                    let (x, t) = self.expr(e, declared.as_ref());
                    (Some(x), declared.clone().unwrap_or(t))
                }
                None => (None, declared.clone().unwrap_or(Ty::Undefined)),
            };
            let mut frame = self.fns.pop().unwrap();
            let Some(init) = init else {
                continue;
            };
            let destructures = !matches!(decl.id, ast::BindingPattern::BindingIdentifier(_));
            let mut effects = false;
            walk_expr(&init, &mut |e| {
                if matches!(
                    e,
                    Expr::Call(..) | Expr::Method { .. } | Expr::Invoke { .. } | Expr::Builtin(..)
                ) {
                    effects = true;
                }
            });
            if !frame.locals.is_empty() || !frame.captures.is_empty() || (destructures && effects) {
                // An initialiser with variables of its own (or one to destructure
                // with effects, evaluated once): it runs as a function, whose
                // destructuring assigns the globals.
                let tmp = frame.locals.len() as u32;
                frame.locals.push(Local {
                    ty: ty.clone(),
                    pending: false,
                    early: false,
                    captured: false,
                    reassigned: None,
                    fresh: false,
                });
                let mut body = vec![Stmt::Let(Pattern::Local(tmp), Some(init))];
                let mut names = Vec::new();
                binding_names(&decl.id, &mut names);
                for (name, path) in names {
                    let g = self.gname(&name).expect("declared").slot;
                    let mut value = Expr::Local(tmp);
                    for step in path {
                        value = match step {
                            PathStep::Key(k) => Expr::Member(Box::new(value), k, false),
                            PathStep::Index(i) => {
                                Expr::Index(Box::new(value), Box::new(Expr::Num(i as f64)), false)
                            }
                        };
                    }
                    body.push(Stmt::Expr(Expr::Assign(
                        Box::new(LValue::Global(g)),
                        None,
                        Box::new(value),
                    )));
                }
                let fidx = self.functions.len() as u32;
                self.functions.push(None);
                self.finish_run(fidx, frame, body, decl.span);
                continue;
            }
            let ty = if d.kind == ast::VariableDeclarationKind::Let {
                widen(&ty)
            } else {
                ty
            };
            // One global per bound name; destructuring reads a path into the value.
            let mut names = Vec::new();
            binding_names(&decl.id, &mut names);
            for (name, path) in names {
                let g = self.gname(&name).expect("declared").clone();
                let mut value = init.clone();
                let mut t = ty.clone();
                for step in path {
                    match step {
                        PathStep::Key(k) => {
                            t = property(&t, &k).unwrap_or(Ty::Unknown);
                            value = Expr::Member(Box::new(value), k, false);
                        }
                        PathStep::Index(i) => {
                            t = types::index(&t, &Ty::NumLit(i as f64)).unwrap_or(Ty::Unknown);
                            value =
                                Expr::Index(Box::new(value), Box::new(Expr::Num(i as f64)), false);
                        }
                    }
                }
                self.globals[g.slot as usize].init = GlobalInit::Expr(value);
                self.globals[g.slot as usize].ty = t.clone();
                self.gname_mut(&name).unwrap().ty = t;
            }
        }
    }

    fn create_context(&mut self, e: &'a E<'a>, declared: Option<&Ty>) -> Option<(Expr, Ty)> {
        let E::CallExpression(c) = strip(e) else {
            return None;
        };
        let is_create = match strip(&c.callee) {
            E::Identifier(id) => {
                self.react.get(id.name.as_str()) == Some(&ReactName::CreateContext)
            }
            E::StaticMemberExpression(m) => {
                m.property.name == "createContext"
                    && matches!(strip(&m.object), E::Identifier(id) if self.react.get(id.name.as_str()) == Some(&ReactName::ReactNs))
            }
            _ => false,
        };
        if !is_create {
            return None;
        }
        let explicit = c
            .type_arguments
            .as_ref()
            .and_then(|t| t.params.first())
            .map(|t| self.ts_type(t));
        let inner = match (explicit, declared) {
            (Some(t), _) => Some(t),
            (None, Some(Ty::Context(t))) => Some((**t).clone()),
            _ => None,
        };
        let (def, t) = match c.arguments.first().and_then(|a| a.as_expression()) {
            Some(a) => self.expr(a, inner.as_ref()),
            None => (Expr::Undefined, Ty::Undefined),
        };
        Some((
            def,
            Ty::Context(Box::new(inner.unwrap_or_else(|| widen(&t)))),
        ))
    }

    /// The render call: `createRoot(container).render(<App />)` (or through a
    /// `const root = createRoot(container)`), `hydrateRoot(container, <App />)`,
    /// or React 17's `ReactDOM.render(<App />, container)`; the container is
    /// `document.getElementById(id)`, `document.querySelector('#id')` or a module
    /// constant holding one.
    fn render_call(&mut self, e: &'a E<'a>) -> bool {
        let E::CallExpression(call) = strip(e) else {
            return false;
        };
        let args: Vec<&'a E<'a>> = call
            .arguments
            .iter()
            .filter_map(|a| a.as_expression())
            .collect();
        let (container, element) = match strip(&call.callee) {
            E::StaticMemberExpression(m) if m.property.name == "render" => {
                if let Some(c) = self.create_root_container(&m.object) {
                    (c, args.first().copied())
                } else if let E::Identifier(id) = strip(&m.object) {
                    if let Some(c) = self.root_vars.get(&(self.cur_mod, id.name.to_string())) {
                        (Some(c.clone()), args.first().copied())
                    } else if self.react.get(id.name.as_str()) == Some(&ReactName::ReactDomNs) {
                        // `ReactDOM.render(element, container)`.
                        (
                            args.get(1).and_then(|c| self.container_id(c)),
                            args.first().copied(),
                        )
                    } else {
                        return false;
                    }
                } else {
                    return false;
                }
            }
            callee if self.dom_function(callee) == Some(ReactName::HydrateRoot) => (
                args.first().and_then(|c| self.container_id(c)),
                args.get(1).copied(),
            ),
            callee if self.dom_function(callee) == Some(ReactName::DomRender) => (
                args.get(1).and_then(|c| self.container_id(c)),
                args.first().copied(),
            ),
            _ => return false,
        };
        let Some(container_id) = container else {
            self.err(
                call.span,
                "the render container must be `document.getElementById('<id>')`",
            );
            return true;
        };
        let Some(element) = element else {
            self.err(call.span, "`render` needs an element");
            return true;
        };
        let fidx = self.functions.len() as u32;
        self.functions.push(None);
        self.fns.push(FnCtx::new(FunctionKind::Plain));
        let (x, _) = self.expr(element, Some(&Ty::Node));
        let ctx = self.fns.pop().unwrap();
        self.functions[fidx as usize] = Some(Function {
            name: "<root>".into(),
            kind: FunctionKind::Plain,
            params: Vec::new(),
            n_locals: ctx.locals.len() as u32,
            captures: Vec::new(),
            body: vec![Stmt::Return(Some(x))],
            local_types: ctx.locals.iter().map(|l| l.ty.clone()).collect(),
            ret: Ty::Node,
            line: self.line(call.span),
            has_depless_effect: false,
            boxed: Vec::new(),
            is_async: false,
            rest: None,
            forward_ref: false,
        });
        if self.root.is_some() {
            self.err(call.span, "the module renders twice");
        }
        self.root = Some(Root {
            container_id,
            element: fidx,
        });
        true
    }

    /// Which react-dom function `callee` names (`createRoot`, `ReactDOM.render`).
    fn dom_function(&self, callee: &E<'a>) -> Option<ReactName> {
        match strip(callee) {
            E::Identifier(id) if self.resolve_is_free(id.name.as_str()) => {
                self.react.get(id.name.as_str()).copied()
            }
            E::StaticMemberExpression(m) => match strip(&m.object) {
                E::Identifier(id)
                    if self.react.get(id.name.as_str()) == Some(&ReactName::ReactDomNs) =>
                {
                    match m.property.name.as_str() {
                        "createRoot" => Some(ReactName::CreateRoot),
                        "hydrateRoot" => Some(ReactName::HydrateRoot),
                        "render" => Some(ReactName::DomRender),
                        _ => None,
                    }
                }
                _ => None,
            },
            _ => None,
        }
    }

    /// For `createRoot(container)`: `Some` with the container's id (`None` when
    /// the container is not one the compiler can find).
    fn create_root_container(&self, e: &'a E<'a>) -> Option<Option<String>> {
        let E::CallExpression(c) = strip(e) else {
            return None;
        };
        if self.dom_function(&c.callee) != Some(ReactName::CreateRoot) {
            return None;
        }
        Some(
            c.arguments
                .first()
                .and_then(|a| a.as_expression())
                .and_then(|c| self.container_id(c)),
        )
    }

    /// `document.getElementById('id')`, `document.querySelector('#id')`, or a
    /// module constant holding one: the id.
    fn container_id(&self, e: &E<'a>) -> Option<String> {
        match strip(e) {
            E::Identifier(id) => self
                .container_vars
                .get(&(self.cur_mod, id.name.to_string()))
                .cloned(),
            E::CallExpression(g) => {
                let E::StaticMemberExpression(gm) = strip(&g.callee) else {
                    return None;
                };
                if !matches!(strip(&gm.object), E::Identifier(d) if d.name == "document") {
                    return None;
                }
                let arg = match g
                    .arguments
                    .first()
                    .and_then(|a| a.as_expression())
                    .map(strip)
                {
                    Some(E::StringLiteral(s)) => s.value.to_string(),
                    _ => return None,
                };
                match gm.property.name.as_str() {
                    "getElementById" => Some(arg),
                    "querySelector" => arg
                        .strip_prefix('#')
                        .filter(|r| {
                            r.chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
                        })
                        .map(str::to_owned),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    // ------------------------------------------------------------------ types

    fn ts_type(&mut self, t: &'a ast::TSType<'a>) -> Ty {
        use ast::TSType as T;
        match t {
            // Types are hints: a value of type any is resolved when it runs.
            T::TSAnyKeyword(_) => Ty::Unknown,
            T::TSStringKeyword(_) => Ty::String,
            T::TSNumberKeyword(_) => Ty::Number,
            T::TSBooleanKeyword(_) => Ty::Boolean,
            T::TSNullKeyword(_) => Ty::Null,
            T::TSUndefinedKeyword(_) => Ty::Undefined,
            T::TSVoidKeyword(_) => Ty::Void,
            T::TSUnknownKeyword(_) => Ty::Unknown,
            T::TSNeverKeyword(_) => Ty::Unknown,
            T::TSArrayType(a) => Ty::Array(Box::new(self.ts_type(&a.element_type))),
            T::TSTupleType(tt) => Ty::Tuple(
                tt.element_types
                    .iter()
                    .map(|e| self.tuple_element(e))
                    .collect(),
            ),
            T::TSUnionType(u) => {
                let mut out = Ty::Unknown;
                for t in &u.types {
                    let t = self.ts_type(t);
                    out = union(out, t);
                }
                out
            }
            T::TSParenthesizedType(p) => self.ts_type(&p.type_annotation),
            T::TSLiteralType(l) => match &l.literal {
                ast::TSLiteral::StringLiteral(s) => Ty::Lit(s.value.to_string()),
                ast::TSLiteral::NumericLiteral(n) => Ty::NumLit(n.value),
                ast::TSLiteral::BooleanLiteral(_) => Ty::Boolean,
                ast::TSLiteral::TemplateLiteral(_) => Ty::String,
                _ => Ty::Unknown,
            },
            T::TSTypeLiteral(lit) => self.members(&lit.members),
            T::TSFunctionType(f) => {
                let ps = f
                    .params
                    .items
                    .iter()
                    .map(|p| match &p.type_annotation {
                        Some(t) => self.ts_type(&t.type_annotation),
                        None => Ty::Unknown,
                    })
                    .collect();
                let r = self.ts_type(&f.return_type.type_annotation);
                Ty::Function(ps, Box::new(r))
            }
            T::TSTypeOperatorType(op) => {
                let inner = self.ts_type(&op.type_annotation);
                match op.operator {
                    ast::TSTypeOperatorOperator::Keyof => types::keys_of(&inner),
                    _ => inner,
                }
            }
            T::TSIntersectionType(i) => {
                let mut out = Ty::Unknown;
                for t in &i.types {
                    let t = self.ts_type(t);
                    out = types::intersect(out, t);
                }
                out
            }
            T::TSTypeQuery(q) => {
                let name = match &q.expr_name {
                    ast::TSTypeQueryExprName::IdentifierReference(id) => Some(id.name.to_string()),
                    _ => None,
                };
                name.and_then(|n| self.value_type_of(&n))
                    .unwrap_or(Ty::Unknown)
            }
            T::TSIndexedAccessType(ia) => {
                let o = self.ts_type(&ia.object_type);
                let k = self.ts_type(&ia.index_type);
                types::index(&o, &k).unwrap_or(Ty::Unknown)
            }
            T::TSTypeReference(r) => self.type_reference(r),
            T::TSTemplateLiteralType(_) => Ty::String,
            T::TSObjectKeyword(_) => Ty::Unknown,
            // Mapped, conditional, `infer`, constructor and other type-level
            // constructs: only hints, and the values they describe are resolved
            // when the code runs.
            _ => Ty::Unknown,
        }
    }

    fn tuple_element(&mut self, e: &'a ast::TSTupleElement<'a>) -> Ty {
        match e {
            ast::TSTupleElement::TSOptionalType(o) => {
                union(self.ts_type(&o.type_annotation), Ty::Undefined)
            }
            ast::TSTupleElement::TSRestType(_) => Ty::Unknown,
            other => match other.as_ts_type() {
                Some(ast::TSType::TSNamedTupleMember(m)) => self.tuple_element(&m.element_type),
                Some(t) => self.ts_type(t),
                None => Ty::Unknown,
            },
        }
    }

    fn members(&mut self, members: &'a oxc_allocator::Vec<'a, ast::TSSignature<'a>>) -> Ty {
        let mut fields = Vec::new();
        for m in members {
            match m {
                ast::TSSignature::TSPropertySignature(p) => {
                    let Some(name) = property_key_name(&p.key) else {
                        continue;
                    };
                    let t = match &p.type_annotation {
                        Some(t) => self.ts_type(&t.type_annotation),
                        None => Ty::Unknown,
                    };
                    fields.push((name, t, p.optional));
                }
                ast::TSSignature::TSIndexSignature(i) => {
                    return Ty::Dict(Box::new(self.ts_type(&i.type_annotation.type_annotation)));
                }
                ast::TSSignature::TSMethodSignature(ms) => {
                    let Some(name) = property_key_name(&ms.key) else {
                        continue;
                    };
                    self.push_type_params(ms.type_parameters.as_deref());
                    let ps = ms
                        .params
                        .items
                        .iter()
                        .map(|p| match &p.type_annotation {
                            Some(t) => self.ts_type(&t.type_annotation),
                            None => Ty::Unknown,
                        })
                        .collect();
                    let r = match &ms.return_type {
                        Some(t) => self.ts_type(&t.type_annotation),
                        None => Ty::Void,
                    };
                    self.type_params.pop();
                    fields.push((name, Ty::Function(ps, Box::new(r)), ms.optional));
                }
                // Call and construct signatures: hints only.
                _ => {}
            }
        }
        Ty::Object(fields)
    }

    fn type_args(&mut self, r: &'a ast::TSTypeReference<'a>) -> Vec<Ty> {
        match &r.type_arguments {
            Some(a) => a.params.iter().map(|t| self.ts_type(t)).collect(),
            None => Vec::new(),
        }
    }

    fn type_reference(&mut self, r: &'a ast::TSTypeReference<'a>) -> Ty {
        let full = type_name(&r.type_name);
        let name = full.rsplit('.').next().unwrap_or(&full).to_owned();
        if !full.contains('.') {
            if let Some(t) = self.generic_instance(&name, r) {
                return t;
            }
            if let Some(t) = self.named_type(&name, r.span) {
                return t;
            }
        }
        let args = self.type_args(r);
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Ty::Unknown);
        match name.as_str() {
            "Array" | "ReadonlyArray" => Ty::Array(Box::new(arg(0))),
            "Record" => Ty::Dict(Box::new(arg(1))),
            "Partial" => match arg(0) {
                Ty::Object(fs) => {
                    Ty::Object(fs.into_iter().map(|(n, t, _)| (n, t, true)).collect())
                }
                t => t,
            },
            "Readonly" | "NonNullable" => non_null(&arg(0)),
            "Pick" | "Omit" => {
                let keys = types::literal_names(&arg(1));
                match arg(0) {
                    Ty::Object(fs) => Ty::Object(
                        fs.into_iter()
                            .filter(|(n, _, _)| keys.contains(n) == (name == "Pick"))
                            .collect(),
                    ),
                    t => t,
                }
            }
            "Exclude" | "Extract" => {
                let drop = arg(1);
                let keep = |t: &Ty| {
                    let hit = match &drop {
                        Ty::Union(ds) => ds.contains(t),
                        d => d == t,
                    };
                    hit == (name == "Extract")
                };
                match arg(0) {
                    Ty::Union(ts) => union_all(ts.into_iter().filter(|t| keep(t))),
                    t if keep(&t) => t,
                    _ => Ty::Unknown,
                }
            }
            "Set" | "ReadonlySet" => Ty::Set(Box::new(arg(0))),
            "Map" | "ReadonlyMap" => Ty::Map(Box::new(arg(0)), Box::new(arg(1))),
            "RegExp" => Ty::Regex,
            "Date" => Ty::Date,
            "Error" | "TypeError" | "RangeError" | "SyntaxError" => Ty::Error,
            "ReactNode" | "ReactElement" | "Element" | "ReactChild" | "ReactPortal" => Ty::Node,
            "PropsWithChildren" => match arg(0) {
                Ty::Object(mut fs) => {
                    fs.push(("children".into(), Ty::Node, true));
                    Ty::Object(fs)
                }
                t => t,
            },
            "FormEvent" | "ChangeEvent" | "MouseEvent" | "KeyboardEvent" | "FocusEvent"
            | "SyntheticEvent" | "PointerEvent" | "WheelEvent" | "UIEvent" | "Event"
            | "InputEvent" | "DragEvent" => Ty::Event,
            "SetStateAction" => union(arg(0), Ty::Function(vec![arg(0)], Box::new(arg(0)))),
            "Dispatch" => match arg(0) {
                Ty::Union(ts) if ts.len() == 2 && matches!(ts[1], Ty::Function(..)) => {
                    Ty::Setter(Box::new(ts[0].clone()))
                }
                t => Ty::Dispatch(Box::new(t)),
            },
            "RefObject" | "MutableRefObject" => Ty::Ref(Box::new(arg(0))),
            "Context" => Ty::Context(Box::new(arg(0))),
            "Promise" => Ty::Promise(Box::new(arg(0))),
            "Response" => Ty::Response,
            "CwResponse" => Ty::CwResponse,
            "Headers" => Ty::Headers,
            "CSSProperties" => Ty::Dict(Box::new(union(Ty::String, Ty::Number))),
            n if n.starts_with("HTML") && n.ends_with("Element") => Ty::DomNode,
            "Node" | "EventTarget" => Ty::DomNode,
            "FC" | "FunctionComponent" => Ty::Function(vec![arg(0)], Box::new(Ty::Node)),
            // A type the compiler does not model (a package's, `Date`, React's
            // prop helpers): values of it are resolved when the code runs.
            _ => Ty::Unknown,
        }
    }

    fn interface_type(&mut self, i: &'a ast::TSInterfaceDeclaration<'a>) -> Ty {
        let mut base = Vec::new();
        for h in &i.extends {
            let n = type_name(&h.type_name);
            if let Some(Ty::Object(fs)) = self.named_type(&n, h.span) {
                base.extend(fs);
            }
        }
        match self.members(&i.body.body) {
            Ty::Object(fs) => {
                base.extend(fs);
                Ty::Object(base)
            }
            t => t,
        }
    }

    /// The declaration file declaring type `name`, if one does.
    fn ambient_decl(&self, name: &str) -> Option<usize> {
        self.ambient_mods
            .iter()
            .copied()
            .find(|m| *m != self.cur_mod && self.mods[*m].type_decls.contains_key(name))
    }

    /// Where the type declaration `name` refers to is: its module and local name.
    fn find_type_decl(&self, name: &str) -> Option<(usize, String)> {
        if self.type_decls.contains_key(name) {
            return Some((self.cur_mod, name.to_owned()));
        }
        if let Some((m, exported)) = self.type_imports.get(name) {
            let theirs = &self.mods[*m];
            let local = theirs.exports.get(exported)?;
            return theirs
                .type_decls
                .contains_key(local)
                .then(|| (*m, local.clone()));
        }
        self.ambient_decl(name).map(|m| (m, name.to_owned()))
    }

    /// A reference to a generic alias or interface: its declaration resolved with
    /// the parameters bound to the arguments (to their defaults or constraints when
    /// left out). `None` when `name` is not a generic declaration.
    fn generic_instance(&mut self, name: &str, r: &'a ast::TSTypeReference<'a>) -> Option<Ty> {
        if self.type_params.iter().any(|s| s.contains_key(name)) {
            return None;
        }
        let (m, local) = self.find_type_decl(name)?;
        let decls = if m == self.cur_mod {
            &self.type_decls
        } else {
            &self.mods[m].type_decls
        };
        let (params, decl) = match decls.get(&local)? {
            TypeDecl::Alias(t, Some(tp)) => (*tp, TypeDecl::Alias(t, Some(tp))),
            TypeDecl::Interface(i) => (i.type_parameters.as_deref()?, TypeDecl::Interface(i)),
            TypeDecl::Alias(_, None) => return None,
        };
        let args = self.type_args(r);
        if !self.type_busy.insert((m, local.clone())) {
            return Some(Ty::Unknown);
        }
        let prev = self.enter_module(m);
        let saved = std::mem::replace(&mut self.type_params, vec![BTreeMap::new()]);
        for (i, p) in params.params.iter().enumerate() {
            let t = match (args.get(i), &p.default, &p.constraint) {
                (Some(a), _, _) => a.clone(),
                (None, Some(d), _) => self.ts_type(d),
                (None, None, Some(c)) => self.ts_type(c),
                (None, None, None) => Ty::Unknown,
            };
            self.type_params[0].insert(p.name.name.to_string(), t);
        }
        let t = match decl {
            TypeDecl::Alias(t, _) => self.ts_type(t),
            TypeDecl::Interface(i) => self.interface_type(i),
        };
        self.type_params = saved;
        self.enter_module(prev);
        self.type_busy.remove(&(m, local));
        Some(t)
    }

    fn named_type(&mut self, name: &str, _span: Span) -> Option<Ty> {
        for scope in self.type_params.iter().rev() {
            if let Some(t) = scope.get(name) {
                return Some(t.clone());
            }
        }
        if let Some(t) = self.type_cache.get(name) {
            return Some(t.clone());
        }
        if !self.type_decls.contains_key(name) {
            // A type imported from another module of the app: resolved there.
            let Some((m, exported)) = self.type_imports.get(name).cloned() else {
                // Or one a declaration file declares.
                let m = self.ambient_decl(name)?;
                let prev = self.enter_module(m);
                let t = self.named_type(name, _span);
                self.enter_module(prev);
                return t;
            };
            let prev = self.enter_module(m);
            let local = self.exports.get(&exported).cloned();
            let t = match local {
                Some(local) => self.named_type(&local, _span),
                None => None,
            };
            self.enter_module(prev);
            // A value imported as a type (`typeof`-less class or re-export): a hint.
            return Some(t.unwrap_or(Ty::Unknown));
        }
        if !self.type_busy.insert((self.cur_mod, name.to_owned())) {
            return Some(Ty::Unknown);
        }
        let t = match self.type_decls.get(name) {
            Some(TypeDecl::Alias(t, _)) => {
                let t: &'a ast::TSType<'a> = t;
                self.ts_type(t)
            }
            Some(TypeDecl::Interface(i)) => {
                let i: &'a ast::TSInterfaceDeclaration<'a> = i;
                self.interface_type(i)
            }
            None => Ty::Unknown,
        };
        self.type_busy.remove(&(self.cur_mod, name.to_owned()));
        self.type_cache.insert(name.to_owned(), t.clone());
        Some(t)
    }

    // ------------------------------------------------------------------ functions

    /// Lowers a module function (once).
    fn ensure_function(&mut self, fidx: u32) {
        if self.functions[fidx as usize].is_some() || self.busy_fns.contains(&fidx) {
            return;
        }
        let Some(p) = self.pending.remove(&fidx) else {
            return;
        };
        self.busy_fns.insert(fidx);
        let saved_fns = std::mem::take(&mut self.fns);
        let saved_holes = std::mem::take(&mut self.hole_always);
        let prev_mod = self.enter_module(p.module);
        let sig = p
            .slot
            .map(|s| self.ginfo[s as usize].ty.clone())
            .unwrap_or(Ty::Unknown);
        let (params, declared_ret) = match &sig {
            Ty::Function(ps, r) => (ps.clone(), (**r).clone()),
            _ => (Vec::new(), Ty::Unknown),
        };
        // A module function's own annotations are not a context: an unannotated
        // parameter there is implicitly `any`.
        let params: Vec<Ty> = params
            .into_iter()
            .map(|t| match t {
                Ty::Unknown => Ty::Param(String::new()),
                t => t,
            })
            .collect();
        let f = self.function(&p, &params, declared_ret, None);
        let ret = f.ret.clone();
        self.fns = saved_fns;
        self.hole_always = saved_holes;
        self.enter_module(prev_mod);
        if let Some(g) = p.slot.map(|s| &mut self.ginfo[s as usize]) {
            if let Ty::Function(_, r) = &mut g.ty {
                if matches!(**r, Ty::Unknown) {
                    **r = ret;
                }
            }
            let slot = g.slot as usize;
            let t = g.ty.clone();
            self.globals[slot].ty = t;
        }
        self.functions[fidx as usize] = Some(f);
        self.busy_fns.remove(&fidx);
    }

    /// Lowers a function body. `param_hint` types unannotated parameters from the
    /// context (a callback's), `declared_ret` is the annotated return type (or
    /// `Unknown`).
    fn function(
        &mut self,
        p: &PendingFn<'a>,
        param_hint: &[Ty],
        declared_ret: Ty,
        outer: Option<()>,
    ) -> Function {
        self.push_type_params(p.type_params);
        let f = self.function_inner(p, param_hint, declared_ret, outer);
        self.type_params.pop();
        f
    }

    fn function_inner(
        &mut self,
        p: &PendingFn<'a>,
        param_hint: &[Ty],
        declared_ret: Ty,
        _outer: Option<()>,
    ) -> Function {
        if p.generator {
            self.err(p.span, "generators are outside the compiled subset");
        }
        let mut ctx = FnCtx::new(p.kind);
        ctx.is_async = p.is_async;
        // An async function's body returns what its promise resolves to.
        let declared_ret = match (p.is_async, declared_ret) {
            (true, Ty::Promise(t)) => *t,
            (_, t) => t,
        };
        if !matches!(declared_ret, Ty::Unknown) {
            ctx.declared_ret = Some(declared_ret.clone());
        }
        self.fns.push(ctx);
        let mut params = Vec::new();
        for (i, param) in p.params.items.iter().enumerate() {
            let annotated = param
                .type_annotation
                .as_ref()
                .map(|t| self.ts_type(&t.type_annotation));
            let ty = match annotated {
                Some(t) => t,
                // Typed by context when there is one; an implicitly `any`
                // parameter is resolved when the code runs.
                None => match param_hint.get(i) {
                    Some(t) if !matches!(t, Ty::Param(n) if n.is_empty()) => t.clone(),
                    _ => Ty::Unknown,
                },
            };
            let ty = if param.optional {
                union(ty, Ty::Undefined)
            } else {
                ty
            };
            let pat = match &param.initializer {
                Some(init) => {
                    let (d, _) = self.expr(init, Some(&ty));
                    let inner = self.bind_pattern(&param.pattern, &non_null(&ty));
                    Pattern::Default(Box::new(inner), d)
                }
                None => self.bind_pattern(&param.pattern, &ty),
            };
            params.push(pat);
        }
        let rest = p.params.rest.as_ref().map(|r| {
            let t = match &r.type_annotation {
                Some(t) => self.ts_type(&t.type_annotation),
                None => Ty::Array(Box::new(Ty::Unknown)),
            };
            self.bind_pattern(&r.rest.argument, &t)
        });
        let body = match p.body {
            FnBody::Block(b) => {
                for d in &b.directives {
                    let _ = d;
                }
                self.block(&b.statements)
            }
            FnBody::Expr(e) => {
                let want = self.fns.last().unwrap().declared_ret.clone();
                let (x, t) = self.expr(e, want.as_ref());
                let c = self.cur();
                c.ret = union(c.ret.clone(), t);
                vec![Stmt::Return(Some(x))]
            }
        };
        let ctx = self.fns.pop().unwrap();
        // Captured and reassigned: shared through a cell.
        let boxed: Vec<u32> = ctx
            .locals
            .iter()
            .enumerate()
            .filter(|(_, l)| l.captured && l.reassigned.is_some())
            .map(|(i, _)| i as u32)
            .collect();
        let ret = match ctx.declared_ret.clone() {
            Some(t) => t,
            None if matches!(ctx.ret, Ty::Unknown) => Ty::Void,
            None => ctx.ret.clone(),
        };
        let ret = if p.is_async {
            Ty::Promise(Box::new(ret))
        } else {
            ret
        };
        Function {
            name: p.name.clone(),
            kind: p.kind,
            params,
            n_locals: ctx.locals.len() as u32,
            captures: ctx.captures.clone(),
            body,
            local_types: ctx.locals.iter().map(|l| l.ty.clone()).collect(),
            ret,
            line: self.line(p.span),
            has_depless_effect: ctx.has_depless_effect,
            boxed,
            is_async: p.is_async,
            rest,
            forward_ref: p.forward_ref,
        }
    }

    /// A closure: an arrow or function expression inside a function.
    fn closure(&mut self, p: PendingFn<'a>, want: Option<&Ty>) -> Lowered {
        let hint: Vec<Ty> = match want.map(function_member) {
            Some(Some(Ty::Function(ps, _))) => ps,
            _ => Vec::new(),
        };
        let declared_ret = match p.return_type {
            Some(t) => self.ts_type(&t.type_annotation),
            None => Ty::Unknown,
        };
        let fidx = self.functions.len() as u32;
        self.functions.push(None);
        let f = self.function(&p, &hint, declared_ret, Some(()));
        let ty = Ty::Function(
            f.local_types
                .iter()
                .take(p.params.items.len())
                .cloned()
                .collect(),
            Box::new(f.ret.clone()),
        );
        // Parameter types are the first locals only for plain identifiers; use the
        // hint or annotations as the function type's parameters instead.
        let ptys: Vec<Ty> = p
            .params
            .items
            .iter()
            .enumerate()
            .map(|(i, param)| match &param.type_annotation {
                Some(_) => f.local_types.get(i).cloned().unwrap_or(Ty::Unknown),
                None => hint.get(i).cloned().unwrap_or(Ty::Unknown),
            })
            .collect();
        let ty = match ty {
            Ty::Function(_, r) => Ty::Function(ptys, r),
            t => t,
        };
        self.functions[fidx as usize] = Some(f);
        (Expr::Closure(fidx), ty)
    }

    // ------------------------------------------------------------------ names

    /// Resolves a name to a variable read.
    fn resolve(&mut self, name: &str, span: Span) -> Option<Lowered> {
        let depth = self.fns.len();
        for level in (0..depth).rev() {
            if let Some(slot) = self.fns[level].lookup(name) {
                let ty = self.fns[level].locals[slot as usize].ty.clone();
                if self.fns[level].locals[slot as usize].pending {
                    if level == depth - 1 {
                        self.err(
                            span,
                            format!("`{name}` is used before its declaration (declare it above this use)"),
                        );
                        return Some((Expr::Undefined, Ty::Unknown));
                    }
                    // A closure made before the variable is initialised: it
                    // shares the variable through a cell, which the declaration
                    // then assigns.
                    let l = &mut self.fns[level].locals[slot as usize];
                    l.reassigned = Some(span.start);
                    l.early = true;
                }
                if level == depth - 1 {
                    return Some((Expr::Local(slot), ty));
                }
                self.fns[level].locals[slot as usize].captured = true;
                // Thread a capture through every function between.
                let mut cap = Capture::Local(slot);
                for l in level + 1..depth {
                    let ctx = &mut self.fns[l];
                    let idx = match ctx.capture_names.iter().position(|(n, _)| n == name) {
                        Some(i) => i as u32,
                        None => {
                            ctx.captures.push(cap);
                            ctx.capture_names.push((name.to_owned(), ty.clone()));
                            (ctx.captures.len() - 1) as u32
                        }
                    };
                    cap = Capture::Capture(idx);
                }
                let Capture::Capture(idx) = cap else {
                    unreachable!()
                };
                return Some((Expr::Capture(idx), ty));
            }
            if self.fns[level].later.iter().any(|l| l.contains(name)) {
                self.err(
                    span,
                    format!("`{name}` is used before its declaration (declare it above this use)"),
                );
                return Some((Expr::Undefined, Ty::Unknown));
            }
        }
        if let Some(g) = self.gname(name).cloned() {
            if let Some(f) = g.func {
                self.ensure_function(f);
            }
            let g = self.gname(name).cloned().unwrap();
            if g.reassigned {
                self.mark_always();
            }
            return Some((Expr::Global(g.slot), g.ty));
        }
        None
    }

    /// The static type of a variable, for `typeof x` in a type.
    fn value_type_of(&mut self, name: &str) -> Option<Ty> {
        for f in self.fns.iter().rev() {
            if let Some(slot) = f.lookup(name) {
                return Some(f.locals[slot as usize].ty.clone());
            }
            if let Some((_, t)) = f.capture_names.iter().find(|(n, _)| n == name) {
                return Some(t.clone());
            }
        }
        let g = self.gname(name)?.clone();
        if let Some(f) = g.func {
            self.ensure_function(f);
        }
        self.gname(name).map(|g| g.ty.clone())
    }

    fn mark_always(&mut self) {
        if let Some(a) = self.hole_always.last_mut() {
            *a = true;
        }
    }

    // ------------------------------------------------------------------ patterns

    fn bind_pattern(&mut self, p: &'a ast::BindingPattern<'a>, ty: &Ty) -> Pattern {
        match p {
            ast::BindingPattern::BindingIdentifier(id) => {
                let slot = self.cur().declare(id.name.as_str(), ty.clone());
                Pattern::Local(slot)
            }
            ast::BindingPattern::ArrayPattern(a) => {
                let mut items = Vec::new();
                for (i, el) in a.elements.iter().enumerate() {
                    match el {
                        Some(el) => {
                            let t = types::index(ty, &Ty::NumLit(i as f64))
                                .map(|t| match ty {
                                    Ty::Tuple(_) => t,
                                    _ => non_null(&t),
                                })
                                .unwrap_or(Ty::Unknown);
                            items.push(Some(self.bind_pattern(el, &t)));
                        }
                        None => items.push(None),
                    }
                }
                let rest = a.rest.as_ref().map(|r| {
                    let t = match ty {
                        Ty::Tuple(ts) => {
                            Ty::Array(Box::new(union_all(ts.iter().skip(items.len()).cloned())))
                        }
                        t => t.clone(),
                    };
                    Box::new(self.bind_pattern(&r.argument, &t))
                });
                Pattern::Array { items, rest }
            }
            ast::BindingPattern::ObjectPattern(o) => {
                let mut props = Vec::new();
                for prop in &o.properties {
                    let Some(key) = property_key_name(&prop.key).filter(|_| !prop.computed) else {
                        self.err(prop.span, "a computed key in a destructuring pattern is outside the compiled subset");
                        continue;
                    };
                    let t = property(ty, &key).unwrap_or(Ty::Unknown);
                    // `{a = 1}` narrows away `undefined`.
                    let t = if matches!(prop.value, ast::BindingPattern::AssignmentPattern(_)) {
                        non_null(&t)
                    } else {
                        t
                    };
                    props.push((key, self.bind_pattern(&prop.value, &t)));
                }
                let rest = o.rest.as_ref().map(|r| {
                    let t = match ty {
                        Ty::Object(fs) => Ty::Object(
                            fs.iter()
                                .filter(|(n, _, _)| !props.iter().any(|(k, _)| k == n))
                                .cloned()
                                .collect(),
                        ),
                        t => t.clone(),
                    };
                    Box::new(self.bind_pattern(&r.argument, &t))
                });
                Pattern::Object { props, rest }
            }
            ast::BindingPattern::AssignmentPattern(a) => {
                let (d, _) = self.expr(&a.right, Some(ty));
                let inner = self.bind_pattern(&a.left, &non_null(ty));
                Pattern::Default(Box::new(inner), d)
            }
        }
    }

    // ------------------------------------------------------------------ statements

    fn block(&mut self, stmts: &'a oxc_allocator::Vec<'a, S<'a>>) -> Vec<Stmt> {
        // `let`/`const` names and function declarations are bindings of the whole
        // block: a closure may use one declared below it.
        let later = BTreeSet::new();
        let mut ahead = Vec::new();
        for s in stmts.iter() {
            match s {
                S::VariableDeclaration(d) if d.kind != ast::VariableDeclarationKind::Var => {
                    for decl in &d.declarations {
                        let mut names = Vec::new();
                        binding_names(&decl.id, &mut names);
                        ahead.extend(names.into_iter().map(|(n, _)| n));
                    }
                }
                S::FunctionDeclaration(f) => {
                    if let Some(id) = &f.id {
                        ahead.push(id.name.to_string());
                    }
                }
                _ => {}
            }
        }
        let slots: Vec<u32> = {
            let c = self.cur();
            c.scopes.push(Vec::new());
            c.later.push(later);
            ahead.iter().map(|n| c.predeclare(n)).collect()
        };
        let mut out = Vec::new();
        // Function declarations are initialised when the block starts.
        for s in stmts.iter() {
            if matches!(s, S::FunctionDeclaration(_)) {
                self.statement(s, &mut out);
            }
        }
        for s in stmts.iter() {
            if !matches!(s, S::FunctionDeclaration(_)) {
                self.statement(s, &mut out);
            }
        }
        let c = self.cur();
        c.scopes.pop();
        c.later.pop();
        // The cells of variables closures captured before their declaration.
        let early: Vec<Stmt> = slots
            .into_iter()
            .filter(|s| c.locals[*s as usize].early)
            .map(|s| Stmt::Let(Pattern::Local(s), None))
            .collect();
        if early.is_empty() {
            out
        } else {
            early.into_iter().chain(out).collect()
        }
    }

    fn body_of(&mut self, s: &'a S<'a>) -> Vec<Stmt> {
        match s {
            S::BlockStatement(b) => self.block(&b.body),
            other => {
                let c = self.cur();
                c.scopes.push(Vec::new());
                c.later.push(BTreeSet::new());
                let mut out = Vec::new();
                self.statement(other, &mut out);
                let c = self.cur();
                c.scopes.pop();
                c.later.pop();
                out
            }
        }
    }

    fn statement(&mut self, s: &'a S<'a>, out: &mut Vec<Stmt>) {
        match s {
            S::EmptyStatement(_) => {}
            S::ExpressionStatement(e) => {
                // `await x;` and `v = await x;` are places an async function stops.
                match strip(&e.expression) {
                    E::AwaitExpression(a) => self.await_slot = Some(a.span),
                    E::AssignmentExpression(asg)
                        if asg.operator == ast::AssignmentOperator::Assign =>
                    {
                        if let E::AwaitExpression(a) = strip(&asg.right) {
                            self.await_slot = Some(a.span);
                        }
                    }
                    _ => {}
                }
                let (x, _) = self.expr(&e.expression, None);
                self.await_slot = None;
                out.push(Stmt::Expr(x));
            }
            S::VariableDeclaration(d) => self.var_decl(d, out),
            S::FunctionDeclaration(f) => {
                let Some(id) = &f.id else { return };
                let Some(body) = &f.body else { return };
                let name = id.name.to_string();
                let p = PendingFn {
                    forward_ref: false,
                    module: self.cur_mod,
                    type_params: f.type_parameters.as_deref(),
                    slot: None,
                    name: name.clone(),
                    params: &f.params,
                    body: FnBody::Block(body),
                    return_type: f.return_type.as_deref(),
                    span: f.span,
                    is_async: f.r#async,
                    generator: f.generator,
                    kind: FunctionKind::Plain,
                };
                // Declared before its body is lowered, so it can call itself: the
                // closure captures its own slot, which is assigned once.
                let (x, ty) = self.closure(p, None);
                let slot = self.cur().declare(&name, ty);
                if self.cur().locals[slot as usize].early {
                    out.push(Stmt::Expr(Expr::Assign(
                        Box::new(LValue::Local(slot)),
                        None,
                        Box::new(x),
                    )));
                } else {
                    out.push(Stmt::Let(Pattern::Local(slot), Some(x)));
                }
            }
            S::ReturnStatement(r) => {
                let want = self.fns.last().unwrap().declared_ret.clone();
                let kind = self.fns.last().unwrap().kind;
                let want = want.or(if kind == FunctionKind::Component {
                    Some(Ty::Node)
                } else {
                    None
                });
                if let Some(E::AwaitExpression(aw)) = r.argument.as_ref().map(strip) {
                    self.await_slot = Some(aw.span);
                }
                let x = r.argument.as_ref().map(|a| {
                    let (x, t) = self.expr(a, want.as_ref());
                    let c = self.cur();
                    c.ret = union(c.ret.clone(), t);
                    x
                });
                if x.is_none() {
                    let c = self.cur();
                    c.ret = union(c.ret.clone(), Ty::Undefined);
                }
                out.push(Stmt::Return(x));
            }
            S::IfStatement(i) => {
                let (test, _) = self.expr(&i.test, None);
                self.cur().cond_depth += 1;
                let then = self.body_of(&i.consequent);
                let els = match &i.alternate {
                    Some(a) => self.body_of(a),
                    None => Vec::new(),
                };
                self.cur().cond_depth -= 1;
                out.push(Stmt::If(test, then, els));
            }
            S::BlockStatement(b) => {
                let inner = self.block(&b.body);
                out.push(Stmt::Block(inner));
            }
            S::ForOfStatement(f) => {
                if f.r#await {
                    self.err(f.span, "`for await` is outside the compiled subset");
                    return;
                }
                let (iter, ity) = self.expr(&f.right, None);
                let ety = element(&ity).unwrap_or(Ty::Unknown);
                self.cur().cond_depth += 1;
                self.cur().scopes.push(Vec::new());
                let pat = match &f.left {
                    ast::ForStatementLeft::VariableDeclaration(d) if d.declarations.len() == 1 => {
                        self.bind_pattern(&d.declarations[0].id, &ety)
                    }
                    other => {
                        self.err(other.span(), "a `for...of` target must be a declaration");
                        Pattern::Ignore
                    }
                };
                let body = self.body_of(&f.body);
                self.cur().scopes.pop();
                self.cur().cond_depth -= 1;
                out.push(Stmt::ForOf(pat, iter, body));
            }
            S::ForInStatement(f) => {
                // The object's enumerable keys, as strings, in order.
                let (obj, _) = self.expr(&f.right, None);
                let keys = Expr::Builtin(Builtin::ForInKeys, vec![ArrayItem::Item(obj)]);
                self.cur().cond_depth += 1;
                self.cur().scopes.push(Vec::new());
                let pat = match &f.left {
                    ast::ForStatementLeft::VariableDeclaration(d)
                        if d.declarations.len() == 1
                            && d.kind != ast::VariableDeclarationKind::Var =>
                    {
                        self.bind_pattern(&d.declarations[0].id, &Ty::String)
                    }
                    other => {
                        self.err(
                            other.span(),
                            "a `for...in` target must be a `const` or `let` declaration",
                        );
                        Pattern::Ignore
                    }
                };
                let body = self.body_of(&f.body);
                self.cur().scopes.pop();
                self.cur().cond_depth -= 1;
                out.push(Stmt::ForOf(pat, keys, body));
            }
            S::ForStatement(f) => {
                self.cur().cond_depth += 1;
                self.cur().scopes.push(Vec::new());
                let mut init = Vec::new();
                match &f.init {
                    Some(ast::ForStatementInit::VariableDeclaration(d)) => {
                        self.var_decl(d, &mut init)
                    }
                    Some(other) => {
                        if let Some(e) = other.as_expression() {
                            let (x, _) = self.expr(e, None);
                            init.push(Stmt::Expr(x));
                        }
                    }
                    None => {}
                }
                let test = f.test.as_ref().map(|t| self.expr(t, None).0);
                let update = f.update.as_ref().map(|u| self.expr(u, None).0);
                // Each iteration of a `for (let …)` has its own binding: a
                // closure made in the body keeps that iteration's value. The
                // body reads a copy made when the iteration starts.
                let loop_vars: Vec<String> = match &f.init {
                    Some(ast::ForStatementInit::VariableDeclaration(d))
                        if d.kind != ast::VariableDeclarationKind::Var =>
                    {
                        let mut names = Vec::new();
                        for decl in &d.declarations {
                            binding_names(&decl.id, &mut names);
                        }
                        names.into_iter().map(|(n, _)| n).collect()
                    }
                    _ => Vec::new(),
                };
                let used = crate::scan::NameUse::of_statement(&loop_vars, &f.body);
                let mut copies = Vec::new();
                self.cur().scopes.push(Vec::new());
                for name in &used.captured {
                    if used.assigned.contains(name) {
                        self.err(
                            f.span,
                            format!("`{name}` is kept by a closure and reassigned in the loop body: outside the compiled subset"),
                        );
                        continue;
                    }
                    let outer = self.cur().lookup(name).expect("loop variable");
                    let ty = self.cur().locals[outer as usize].ty.clone();
                    let inner = self.cur().declare(name, ty);
                    copies.push(Stmt::Let(Pattern::Local(inner), Some(Expr::Local(outer))));
                }
                let mut body = self.body_of(&f.body);
                self.cur().scopes.pop();
                if !copies.is_empty() {
                    body = copies.into_iter().chain(body).collect();
                }
                self.cur().scopes.pop();
                self.cur().cond_depth -= 1;
                out.push(Stmt::For {
                    init,
                    test,
                    update,
                    body,
                });
            }
            S::WhileStatement(w) => {
                self.cur().cond_depth += 1;
                let (test, _) = self.expr(&w.test, None);
                let body = self.body_of(&w.body);
                self.cur().cond_depth -= 1;
                out.push(Stmt::For {
                    init: Vec::new(),
                    test: Some(test),
                    update: None,
                    body,
                });
            }
            S::BreakStatement(b) => {
                if b.label.is_some() {
                    self.err(b.span, "labelled `break` is outside the compiled subset");
                }
                out.push(Stmt::Break);
            }
            S::ContinueStatement(c) => {
                if c.label.is_some() {
                    self.err(c.span, "labelled `continue` is outside the compiled subset");
                }
                out.push(Stmt::Continue);
            }
            S::SwitchStatement(sw) => {
                let (d, dty) = self.expr(&sw.discriminant, None);
                self.cur().cond_depth += 1;
                self.cur().scopes.push(Vec::new());
                let mut cases = Vec::new();
                for c in &sw.cases {
                    let test = c.test.as_ref().map(|t| self.expr(t, Some(&dty)).0);
                    let mut body = Vec::new();
                    for s in &c.consequent {
                        self.statement(s, &mut body);
                    }
                    cases.push((test, body));
                }
                self.cur().scopes.pop();
                self.cur().cond_depth -= 1;
                out.push(Stmt::Switch(d, cases));
            }
            S::ThrowStatement(t) => {
                let (x, _) = self.expr(&t.argument, None);
                out.push(Stmt::Throw(x));
            }
            S::TryStatement(t) => {
                self.cur().cond_depth += 1;
                let block = self.block(&t.block.body);
                let (param, handler) = match &t.handler {
                    Some(h) => {
                        self.cur().scopes.push(Vec::new());
                        let param = h.param.as_ref().map(|cp| {
                            let ty = cp
                                .type_annotation
                                .as_ref()
                                .map(|a| self.ts_type(&a.type_annotation))
                                .unwrap_or(Ty::Unknown);
                            self.bind_pattern(&cp.pattern, &ty)
                        });
                        let body = self.block(&h.body.body);
                        self.cur().scopes.pop();
                        (param, Some(body))
                    }
                    None => (None, None),
                };
                let finalizer = t.finalizer.as_ref().map(|f| self.block(&f.body));
                self.cur().cond_depth -= 1;
                out.push(Stmt::Try {
                    block,
                    param,
                    handler,
                    finalizer,
                });
            }
            S::TSTypeAliasDeclaration(_) | S::TSInterfaceDeclaration(_) => {}
            other => {
                self.err(
                    other.span(),
                    "this statement is outside the compiled subset",
                );
            }
        }
    }

    /// `p` with its early-captured slots (see `Local::early`) replaced by fresh
    /// temporaries, and the assignments of the temporaries into them.
    fn split_early(&mut self, p: Pattern, assigns: &mut Vec<Stmt>) -> Pattern {
        match p {
            Pattern::Local(slot) if self.cur().locals[slot as usize].early => {
                let ty = self.cur().locals[slot as usize].ty.clone();
                let tmp = self.cur().locals.len() as u32;
                self.cur().locals.push(Local {
                    ty,
                    pending: false,
                    early: false,
                    captured: false,
                    reassigned: None,
                    fresh: false,
                });
                assigns.push(Stmt::Expr(Expr::Assign(
                    Box::new(LValue::Local(slot)),
                    None,
                    Box::new(Expr::Local(tmp)),
                )));
                Pattern::Local(tmp)
            }
            Pattern::Array { items, rest } => Pattern::Array {
                items: items
                    .into_iter()
                    .map(|i| i.map(|p| self.split_early(p, assigns)))
                    .collect(),
                rest: rest.map(|r| Box::new(self.split_early(*r, assigns))),
            },
            Pattern::Object { props, rest } => Pattern::Object {
                props: props
                    .into_iter()
                    .map(|(k, p)| (k, self.split_early(p, assigns)))
                    .collect(),
                rest: rest.map(|r| Box::new(self.split_early(*r, assigns))),
            },
            Pattern::Default(inner, d) => {
                Pattern::Default(Box::new(self.split_early(*inner, assigns)), d)
            }
            p => p,
        }
    }

    fn var_decl(&mut self, d: &'a ast::VariableDeclaration<'a>, out: &mut Vec<Stmt>) {
        if d.kind == ast::VariableDeclarationKind::Var {
            self.err(
                d.span,
                "`var` is outside the compiled subset (use `let` or `const`)",
            );
        }
        for decl in &d.declarations {
            let declared = decl
                .type_annotation
                .as_ref()
                .map(|t| self.ts_type(&t.type_annotation));
            let (init, ty) = match &decl.init {
                Some(e) => {
                    let fresh = is_fresh_init(e);
                    if let E::AwaitExpression(aw) = strip(e) {
                        self.await_slot = Some(aw.span);
                    }
                    let (x, t) = self.expr(e, declared.as_ref());
                    self.await_slot = None;
                    let t = declared.clone().unwrap_or(t);
                    let t = if d.kind == ast::VariableDeclarationKind::Let {
                        widen(&t)
                    } else {
                        t
                    };
                    let pat = self.bind_pattern(&decl.id, &t);
                    if !matches!(pat, Pattern::Local(_)) {
                        // Destructured bindings a closure already shares: bound to
                        // temporaries, then assigned into their cells.
                        let mut assigns = Vec::new();
                        let pat = self.split_early(pat, &mut assigns);
                        out.push(Stmt::Let(pat, Some(x)));
                        out.extend(assigns);
                        continue;
                    }
                    if let Pattern::Local(slot) = pat {
                        if self.cur().locals[slot as usize].early {
                            // Its cell exists already: assign it.
                            out.push(Stmt::Expr(Expr::Assign(
                                Box::new(LValue::Local(slot)),
                                None,
                                Box::new(x),
                            )));
                            continue;
                        }
                        if fresh {
                            self.cur().locals[slot as usize].fresh = true;
                        }
                    }
                    out.push(Stmt::Let(pat, Some(x)));
                    continue;
                }
                None => (None::<Expr>, declared.clone().unwrap_or(Ty::Undefined)),
            };
            let pat = self.bind_pattern(&decl.id, &ty);
            if let Pattern::Local(slot) = pat {
                if self.cur().locals[slot as usize].early {
                    continue;
                }
            }
            out.push(Stmt::Let(pat, init));
        }
    }

    // ------------------------------------------------------------------ expressions

    fn exprs_args(
        &mut self,
        args: &'a oxc_allocator::Vec<'a, ast::Argument<'a>>,
        want: &[Ty],
    ) -> (Vec<ArrayItem>, Vec<Ty>) {
        let mut items = Vec::new();
        let mut tys = Vec::new();
        for (i, a) in args.iter().enumerate() {
            match a {
                ast::Argument::SpreadElement(s) => {
                    let (x, t) = self.expr(&s.argument, None);
                    items.push(ArrayItem::Spread(x));
                    tys.push(element(&t).unwrap_or(Ty::Unknown));
                }
                other => {
                    let e = other.as_expression().expect("argument");
                    let (x, t) = self.expr(e, want.get(i));
                    items.push(ArrayItem::Item(x));
                    tys.push(t);
                }
            }
        }
        (items, tys)
    }

    fn arg_exprs(
        &mut self,
        args: &'a oxc_allocator::Vec<'a, ast::Argument<'a>>,
        want: &[Ty],
    ) -> Vec<Lowered> {
        let mut out = Vec::new();
        for (i, a) in args.iter().enumerate() {
            match a.as_expression() {
                Some(e) => out.push(self.expr(e, want.get(i))),
                None => {
                    self.err(
                        a.span(),
                        "a spread argument is outside the compiled subset here",
                    );
                    out.push((Expr::Undefined, Ty::Unknown));
                }
            }
        }
        out
    }

    fn expr(&mut self, e: &'a E<'a>, want: Option<&Ty>) -> Lowered {
        match e {
            E::BooleanLiteral(b) => (Expr::Bool(b.value), Ty::Boolean),
            E::NullLiteral(_) => (Expr::Null, Ty::Null),
            E::NumericLiteral(n) => (Expr::Num(n.value), Ty::NumLit(n.value)),
            E::StringLiteral(s) => (Expr::Str(s.value.to_string()), Ty::Lit(s.value.to_string())),
            E::TemplateLiteral(t) => {
                let quasis = t
                    .quasis
                    .iter()
                    .map(|q| {
                        q.value
                            .cooked
                            .as_ref()
                            .map(|c| c.to_string())
                            .unwrap_or_else(|| q.value.raw.to_string())
                    })
                    .collect();
                let exprs = t.expressions.iter().map(|x| self.expr(x, None).0).collect();
                (Expr::Template(quasis, exprs), Ty::String)
            }
            E::Identifier(id) => self.identifier(id.name.as_str(), id.span),
            E::ArrayExpression(a) => self.array(a, want),
            E::ObjectExpression(o) => self.object(o, want),
            E::ArrowFunctionExpression(a) => {
                let p = PendingFn {
                    forward_ref: false,
                    module: self.cur_mod,
                    type_params: a.type_parameters.as_deref(),
                    slot: None,
                    name: "<arrow>".into(),
                    params: &a.params,
                    body: match &a.body {
                        ast::ArrowFunctionBody::FunctionBody(b) => FnBody::Block(b),
                        other => FnBody::Expr(other.as_expression().expect("expression body")),
                    },
                    return_type: a.return_type.as_deref(),
                    span: a.span,
                    is_async: a.r#async,
                    generator: false,
                    kind: FunctionKind::Plain,
                };
                self.closure(p, want)
            }
            E::FunctionExpression(f) => {
                let Some(body) = &f.body else {
                    return self.unsupported(f.span, "a function without a body");
                };
                let p = PendingFn {
                    forward_ref: false,
                    module: self.cur_mod,
                    type_params: f.type_parameters.as_deref(),
                    slot: None,
                    name: f
                        .id
                        .as_ref()
                        .map(|i| i.name.to_string())
                        .unwrap_or_else(|| "<function>".into()),
                    params: &f.params,
                    body: FnBody::Block(body),
                    return_type: f.return_type.as_deref(),
                    span: f.span,
                    is_async: f.r#async,
                    generator: f.generator,
                    kind: FunctionKind::Plain,
                };
                self.closure(p, want)
            }
            E::ParenthesizedExpression(p) => self.expr(&p.expression, want),
            E::TSAsExpression(a) => {
                if let ast::TSType::TSTypeReference(r) = &a.type_annotation {
                    if matches!(&r.type_name, ast::TSTypeName::IdentifierReference(id) if id.name == "const")
                    {
                        // `as const`: the value's literal types, arrays as tuples.
                        let (x, t) = self.expr(&a.expression, None);
                        return (x, const_type(&a.expression).unwrap_or(t));
                    }
                }
                let t = self.ts_type(&a.type_annotation);
                let (x, _) = self.expr(&a.expression, Some(&t));
                (x, t)
            }
            E::TSSatisfiesExpression(a) => {
                let t = self.ts_type(&a.type_annotation);
                self.expr(&a.expression, Some(&t))
            }
            E::TSTypeAssertion(a) => {
                let t = self.ts_type(&a.type_annotation);
                let (x, _) = self.expr(&a.expression, Some(&t));
                (x, t)
            }
            E::TSNonNullExpression(n) => {
                let (x, t) = self.expr(&n.expression, want);
                (x, non_null(&t))
            }
            E::TSInstantiationExpression(i) => self.expr(&i.expression, want),
            E::SequenceExpression(s) => {
                let mut xs = Vec::new();
                let mut last = Ty::Undefined;
                for x in &s.expressions {
                    let (x, t) = self.expr(x, None);
                    xs.push(x);
                    last = t;
                }
                (Expr::Seq(xs), last)
            }
            E::ConditionalExpression(c) => {
                let (test, _) = self.expr(&c.test, None);
                self.cur().cond_depth += 1;
                let (a, at) = self.expr(&c.consequent, want);
                let (b, bt) = self.expr(&c.alternate, want);
                self.cur().cond_depth -= 1;
                let t = union(at, bt);
                (Expr::Cond(Box::new(test), Box::new(a), Box::new(b)), t)
            }
            E::LogicalExpression(l) => {
                let (a, at) = self.expr(&l.left, want);
                self.cur().cond_depth += 1;
                let (b, bt) = self.expr(&l.right, want);
                self.cur().cond_depth -= 1;
                let (op, t) = match l.operator {
                    ast::LogicalOperator::And => (LogicalOp::And, union(falsy_part(&at), bt)),
                    ast::LogicalOperator::Or => (LogicalOp::Or, union(non_null(&at), bt)),
                    ast::LogicalOperator::Coalesce => {
                        (LogicalOp::Nullish, union(non_null(&at), bt))
                    }
                };
                (Expr::Logical(op, Box::new(a), Box::new(b)), t)
            }
            E::BinaryExpression(b) => self.binary(b),
            E::UnaryExpression(u) => {
                let (x, t) = self.expr(&u.argument, None);
                use ast::UnaryOperator as U;
                match u.operator {
                    U::LogicalNot => (Expr::Unary(UnaryOp::Not, Box::new(x)), Ty::Boolean),
                    U::UnaryNegation => match x {
                        Expr::Num(n) => (Expr::Num(-n), Ty::NumLit(-n)),
                        x => (Expr::Unary(UnaryOp::Neg, Box::new(x)), Ty::Number),
                    },
                    U::UnaryPlus => (Expr::Unary(UnaryOp::Plus, Box::new(x)), Ty::Number),
                    U::BitwiseNot => (Expr::Unary(UnaryOp::BitNot, Box::new(x)), Ty::Number),
                    U::Void => (Expr::Unary(UnaryOp::Void, Box::new(x)), Ty::Undefined),
                    U::Typeof => {
                        let _ = t;
                        (Expr::TypeOf(Box::new(x)), Ty::String)
                    }
                    U::Delete => {
                        // `delete o.k` / `delete o[k]`: `true`, as for any own key.
                        let (obj, key) = match &x {
                            Expr::Member(o, k, _) => ((**o).clone(), Expr::Str(k.clone())),
                            Expr::Index(o, k, _) => ((**o).clone(), (**k).clone()),
                            _ => {
                                return self
                                    .unsupported(u.span, "`delete` of anything but a property")
                            }
                        };
                        self.note_mutation(&obj);
                        (
                            Expr::Builtin(
                                Builtin::Delete,
                                vec![ArrayItem::Item(obj), ArrayItem::Item(key)],
                            ),
                            Ty::Boolean,
                        )
                    }
                }
            }
            E::UpdateExpression(u) => {
                let target = self.simple_target(&u.argument);
                let delta = match u.operator {
                    ast::UpdateOperator::Increment => 1.0,
                    ast::UpdateOperator::Decrement => -1.0,
                };
                match target {
                    Some((lv, _)) => (Expr::Update(Box::new(lv), u.prefix, delta), Ty::Number),
                    None => (Expr::Undefined, Ty::Unknown),
                }
            }
            E::AssignmentExpression(a) => self.assignment(a),
            E::CallExpression(c) => self.call(c, want),
            E::ChainExpression(c) => {
                let (x, t) = match &c.expression {
                    ast::ChainElement::CallExpression(call) => self.call(call, want),
                    ast::ChainElement::TSNonNullExpression(n) => {
                        let (x, t) = self.expr(&n.expression, want);
                        (x, non_null(&t))
                    }
                    other => match other.as_member_expression() {
                        Some(m) => self.member(m),
                        None => self.unsupported(c.span, "this optional chain"),
                    },
                };
                (Expr::Chain(Box::new(x)), union(t, Ty::Undefined))
            }
            E::StaticMemberExpression(_)
            | E::ComputedMemberExpression(_)
            | E::PrivateFieldExpression(_) => {
                let m = e.as_member_expression().expect("member");
                self.member(m)
            }
            E::JSXElement(el) => self.jsx_element(el),
            E::JSXFragment(f) => {
                let children = self.jsx_children_exprs(&f.children, false);
                (
                    Expr::Element(Box::new(ElementExpr::Fragment {
                        children,
                        key: None,
                    })),
                    Ty::Node,
                )
            }
            E::NewExpression(n) => {
                if let E::Identifier(id) = strip(&n.callee) {
                    if matches!(id.name.as_str(), "Set" | "Map")
                        && self.resolve_is_free(id.name.as_str())
                    {
                        let targs: Vec<Ty> = match &n.type_arguments {
                            Some(t) => t.params.iter().map(|t| self.ts_type(t)).collect(),
                            None => Vec::new(),
                        };
                        let (args, tys) = self.exprs_args(&n.arguments, &[]);
                        let wanted = want.map(non_null);
                        let is_set = id.name == "Set";
                        let t = if is_set {
                            let e = targs
                                .first()
                                .cloned()
                                .or_else(|| match &wanted {
                                    Some(Ty::Set(e)) => Some((**e).clone()),
                                    _ => None,
                                })
                                .or_else(|| tys.first().and_then(element))
                                .unwrap_or(Ty::Unknown);
                            Ty::Set(Box::new(e))
                        } else {
                            let (k, v) = match (targs.first(), targs.get(1), &wanted) {
                                (Some(k), Some(v), _) => (k.clone(), v.clone()),
                                (_, _, Some(Ty::Map(k, v))) => ((**k).clone(), (**v).clone()),
                                _ => match tys.first().and_then(element) {
                                    Some(Ty::Tuple(kv)) if kv.len() == 2 => {
                                        (kv[0].clone(), kv[1].clone())
                                    }
                                    _ => (Ty::Unknown, Ty::Unknown),
                                },
                            };
                            Ty::Map(Box::new(k), Box::new(v))
                        };
                        let b = if is_set {
                            Builtin::NewSet
                        } else {
                            Builtin::NewMap
                        };
                        return (Expr::Builtin(b, args), t);
                    }
                    if id.name == "Date" && self.resolve_is_free("Date") {
                        let (args, _) = self.exprs_args(&n.arguments, &[]);
                        return (Expr::Builtin(Builtin::NewDate, args), Ty::Date);
                    }
                    if id.name == "Array" && self.resolve_is_free("Array") {
                        let (args, _) = self.exprs_args(&n.arguments, &[]);
                        return (
                            Expr::Builtin(Builtin::NewArray, args),
                            Ty::Array(Box::new(Ty::Unknown)),
                        );
                    }
                    if matches!(id.name.as_str(), "Error" | "TypeError")
                        && self.resolve_is_free(id.name.as_str())
                    {
                        let (args, _) = self.exprs_args(&n.arguments, &[Ty::String]);
                        let b = if id.name == "Error" {
                            Builtin::Error
                        } else {
                            Builtin::TypeError
                        };
                        return (Expr::Builtin(b, args), Ty::Error);
                    }
                    if id.name == "Promise" && self.resolve_is_free("Promise") {
                        let t = match &n.type_arguments {
                            Some(ta) => ta
                                .params
                                .first()
                                .map(|t| self.ts_type(t))
                                .unwrap_or(Ty::Unknown),
                            None => match want.map(non_null) {
                                Some(Ty::Promise(t)) => *t,
                                _ => Ty::Unknown,
                            },
                        };
                        let exec = Ty::Function(
                            vec![
                                Ty::Function(vec![t.clone()], Box::new(Ty::Void)),
                                Ty::Function(vec![Ty::Unknown], Box::new(Ty::Void)),
                            ],
                            Box::new(Ty::Void),
                        );
                        let (args, _) = self.exprs_args(&n.arguments, &[exec]);
                        return (
                            Expr::Builtin(Builtin::NewPromise, args),
                            Ty::Promise(Box::new(t)),
                        );
                    }
                }
                self.unsupported(n.span, "`new`")
            }
            E::AwaitExpression(a) => {
                if !self.fns.last().is_some_and(|f| f.is_async) {
                    return self.unsupported(a.span, "`await` outside an async function");
                }
                if self.await_slot != Some(a.span) {
                    self.err(a.span, "`await` inside an expression is outside the compiled subset (await into a variable first)");
                    return (Expr::Undefined, Ty::Unknown);
                }
                self.await_slot = None;
                let (x, t) = self.expr(&a.argument, None);
                let t = match non_null(&t) {
                    Ty::Promise(inner) => *inner,
                    _ => t,
                };
                (Expr::Await(Box::new(x)), t)
            }
            E::ThisExpression(t) => self.unsupported(t.span, "`this`"),
            E::ClassExpression(c) => self.unsupported(c.span, "a class"),
            E::RegExpLiteral(r) => (
                Expr::Regex(r.regex.pattern.text.to_string(), r.regex.flags.to_string()),
                Ty::Regex,
            ),
            E::BigIntLiteral(b) => self.unsupported(b.span, "a BigInt"),
            E::TaggedTemplateExpression(t) => self.unsupported(t.span, "a tagged template"),
            other => self.unsupported(other.span(), "this expression"),
        }
    }

    fn identifier(&mut self, name: &str, span: Span) -> Lowered {
        if name == "undefined" {
            return (Expr::Undefined, Ty::Undefined);
        }
        if let Some(r) = self.resolve(name, span) {
            return r;
        }
        let as_value = |b: Builtin, t: Ty| (Expr::BuiltinFn(b), t);
        match name {
            "Infinity" => (Expr::Builtin(Builtin::Infinity, vec![]), Ty::Number),
            "NaN" => (Expr::Builtin(Builtin::NaN, vec![]), Ty::Number),
            // Built-in functions as values: `xs.filter(Boolean)`, `.map(Number)`.
            "Boolean" => as_value(
                Builtin::Boolean,
                Ty::Function(vec![Ty::Unknown], Box::new(Ty::Boolean)),
            ),
            "Number" => as_value(
                Builtin::Number,
                Ty::Function(vec![Ty::Unknown], Box::new(Ty::Number)),
            ),
            "String" => as_value(
                Builtin::String,
                Ty::Function(vec![Ty::Unknown], Box::new(Ty::String)),
            ),
            "parseInt" => as_value(
                Builtin::ParseInt,
                Ty::Function(vec![Ty::Unknown, Ty::Unknown], Box::new(Ty::Number)),
            ),
            "parseFloat" => as_value(
                Builtin::ParseFloat,
                Ty::Function(vec![Ty::Unknown], Box::new(Ty::Number)),
            ),
            "isNaN" => as_value(
                Builtin::IsNaN,
                Ty::Function(vec![Ty::Unknown], Box::new(Ty::Boolean)),
            ),
            _ => {
                if self.react.contains_key(name) {
                    self.err(
                        span,
                        format!("`{name}` from React cannot be used as a value here"),
                    );
                } else {
                    self.err(span, format!("unknown name `{name}`"));
                }
                (Expr::Undefined, Ty::Unknown)
            }
        }
    }

    fn array(&mut self, a: &'a ast::ArrayExpression<'a>, want: Option<&Ty>) -> Lowered {
        let want = want.map(non_null);
        let tuple_want = match &want {
            Some(Ty::Tuple(ts)) => Some(ts.clone()),
            Some(Ty::Union(us)) => us.iter().find_map(|u| match u {
                Ty::Tuple(ts) if ts.len() == a.elements.len() => Some(ts.clone()),
                _ => None,
            }),
            _ => None,
        };
        let elem_want = want.as_ref().and_then(element);
        let mut items = Vec::new();
        let mut tys = Vec::new();
        for (i, el) in a.elements.iter().enumerate() {
            match el {
                ast::ArrayExpressionElement::SpreadElement(s) => {
                    let w = want.clone();
                    let (x, t) = self.expr(&s.argument, w.as_ref());
                    items.push(ArrayItem::Spread(x));
                    tys.push(element(&t).unwrap_or(Ty::Unknown));
                }
                ast::ArrayExpressionElement::Elision(el) => {
                    self.err(el.span, "array holes are outside the compiled subset");
                }
                other => {
                    let e = other.as_expression().expect("element");
                    let w = match &tuple_want {
                        Some(ts) => ts.get(i).cloned(),
                        None => elem_want.clone(),
                    };
                    let (x, t) = self.expr(e, w.as_ref());
                    items.push(ArrayItem::Item(x));
                    tys.push(t);
                }
            }
        }
        let t = match (tuple_want, &want) {
            (Some(ts), _) => Ty::Tuple(ts),
            (None, Some(w)) if types::is_array(w) => w.clone(),
            _ => Ty::Array(Box::new(match union_all(tys.iter().map(widen)) {
                Ty::Unknown if tys.is_empty() => Ty::Unknown,
                t => t,
            })),
        };
        (Expr::Array(items), t)
    }

    fn object(&mut self, o: &'a ast::ObjectExpression<'a>, want: Option<&Ty>) -> Lowered {
        let want = want.map(non_null);
        let mut props = Vec::new();
        let mut fields: Vec<(String, Ty, bool)> = Vec::new();
        let mut dict: Option<Ty> = None;
        for p in &o.properties {
            match p {
                ast::ObjectPropertyKind::ObjectProperty(p) => {
                    if p.kind != ast::PropertyKind::Init {
                        self.err(
                            p.span,
                            "getters and setters are outside the compiled subset",
                        );
                        continue;
                    }
                    if p.computed {
                        let key = p.key.as_expression().expect("computed key");
                        let (k, _) = self.expr(key, None);
                        let vw = match &want {
                            Some(Ty::Dict(v)) => Some((**v).clone()),
                            _ => None,
                        };
                        let (v, vt) = self.expr(&p.value, vw.as_ref());
                        props.push(Prop::Computed(k, v));
                        dict = Some(union(dict.unwrap_or(Ty::Unknown), vt));
                        continue;
                    }
                    let Some(name) = property_key_name(&p.key) else {
                        self.err(p.span, "this property key is outside the compiled subset");
                        continue;
                    };
                    let vw = want
                        .as_ref()
                        .and_then(|w| property(w, &name))
                        .map(|t| non_null_keep(&t));
                    let (v, vt) = self.expr(&p.value, vw.as_ref());
                    fields.retain(|(n, _, _)| *n != name);
                    fields.push((name.clone(), vt, false));
                    props.push(Prop::KeyValue(name, v));
                }
                ast::ObjectPropertyKind::SpreadProperty(s) => {
                    let (x, t) = self.expr(&s.argument, want.as_ref());
                    match non_null(&t) {
                        Ty::Object(fs) => {
                            for (n, t, opt) in fs {
                                fields.retain(|(m, _, _)| *m != n);
                                fields.push((n, t, opt));
                            }
                        }
                        Ty::Dict(v) => dict = Some(union(dict.unwrap_or(Ty::Unknown), *v)),
                        _ => dict = Some(Ty::Unknown),
                    }
                    props.push(Prop::Spread(x));
                }
            }
        }
        let t = match (dict, &want) {
            (_, Some(w @ (Ty::Object(_) | Ty::Dict(_)))) => w.clone(),
            (Some(v), _) => Ty::Dict(Box::new(union_all(
                fields.iter().map(|(_, t, _)| t.clone()).chain([v]),
            ))),
            (None, _) => Ty::Object(fields),
        };
        (Expr::Object(props), t)
    }

    fn binary(&mut self, b: &'a ast::BinaryExpression<'a>) -> Lowered {
        use ast::BinaryOperator as B;
        if b.operator == B::Instanceof {
            let ctor = match strip(&b.right) {
                E::Identifier(id) if self.resolve_is_free(id.name.as_str()) => id.name.as_str(),
                _ => "",
            };
            if matches!(
                ctor,
                "Date" | "Array" | "Map" | "Set" | "RegExp" | "Promise" | "Object" | "Function"
            ) {
                let (l, _) = self.expr(&b.left, None);
                return (
                    Expr::Builtin(
                        Builtin::IsInstance,
                        vec![ArrayItem::Item(l), ArrayItem::Item(Expr::Str(ctor.into()))],
                    ),
                    Ty::Boolean,
                );
            }
            if !matches!(ctor, "Error" | "TypeError" | "RangeError" | "SyntaxError") {
                return self.unsupported(b.span, "`instanceof` of anything but a built-in class");
            }
            let (l, _) = self.expr(&b.left, None);
            return (
                Expr::Builtin(
                    Builtin::IsError,
                    vec![ArrayItem::Item(l), ArrayItem::Item(Expr::Str(ctor.into()))],
                ),
                Ty::Boolean,
            );
        }
        let (l, lt) = self.expr(&b.left, None);
        let (r, rt) = self.expr(&b.right, None);
        let (op, t) = match b.operator {
            B::Addition => {
                let t = if types::is_numeric(&lt) && types::is_numeric(&rt) {
                    Ty::Number
                } else if types::is_stringy(&lt) || types::is_stringy(&rt) {
                    Ty::String
                } else {
                    union(Ty::String, Ty::Number)
                };
                (BinaryOp::Add, t)
            }
            B::Subtraction => (BinaryOp::Sub, Ty::Number),
            B::Multiplication => (BinaryOp::Mul, Ty::Number),
            B::Division => (BinaryOp::Div, Ty::Number),
            B::Remainder => (BinaryOp::Rem, Ty::Number),
            B::Exponential => (BinaryOp::Exp, Ty::Number),
            B::Equality => (BinaryOp::Eq, Ty::Boolean),
            B::Inequality => (BinaryOp::NotEq, Ty::Boolean),
            B::StrictEquality => (BinaryOp::StrictEq, Ty::Boolean),
            B::StrictInequality => (BinaryOp::StrictNotEq, Ty::Boolean),
            B::LessThan => (BinaryOp::Lt, Ty::Boolean),
            B::LessEqualThan => (BinaryOp::LtEq, Ty::Boolean),
            B::GreaterThan => (BinaryOp::Gt, Ty::Boolean),
            B::GreaterEqualThan => (BinaryOp::GtEq, Ty::Boolean),
            B::BitwiseAnd => (BinaryOp::BitAnd, Ty::Number),
            B::BitwiseOR => (BinaryOp::BitOr, Ty::Number),
            B::BitwiseXOR => (BinaryOp::BitXor, Ty::Number),
            B::ShiftLeft => (BinaryOp::Shl, Ty::Number),
            B::ShiftRight => (BinaryOp::Shr, Ty::Number),
            B::ShiftRightZeroFill => (BinaryOp::UShr, Ty::Number),
            B::In => (BinaryOp::In, Ty::Boolean),
            B::Instanceof => unreachable!("lowered above"),
        };
        (Expr::Binary(op, Box::new(l), Box::new(r)), t)
    }

    /// An assignment target that is a variable or a member.
    fn simple_target(&mut self, t: &'a ast::SimpleAssignmentTarget<'a>) -> Option<(LValue, Ty)> {
        match t {
            ast::SimpleAssignmentTarget::AssignmentTargetIdentifier(id) => {
                self.assign_name(id.name.as_str(), id.span)
            }
            ast::SimpleAssignmentTarget::TSNonNullExpression(n) => match strip(&n.expression) {
                E::Identifier(id) => self.assign_name(id.name.as_str(), id.span),
                _ => {
                    self.err(
                        n.span,
                        "this assignment target is outside the compiled subset",
                    );
                    None
                }
            },
            other => match other.as_member_expression() {
                Some(m) => self.member_target(m),
                None => {
                    self.err(
                        other.span(),
                        "this assignment target is outside the compiled subset",
                    );
                    None
                }
            },
        }
    }

    fn assign_name(&mut self, name: &str, span: Span) -> Option<(LValue, Ty)> {
        let depth = self.fns.len();
        if let Some(slot) = self.fns[depth - 1].lookup(name) {
            let l = &mut self.fns[depth - 1].locals[slot as usize];
            l.reassigned = Some(span.start);
            l.fresh = false;
            return Some((LValue::Local(slot), l.ty.clone()));
        }
        for level in (0..depth - 1).rev() {
            if let Some(slot) = self.fns[level].lookup(name) {
                // A variable of an enclosing function: it becomes shared (boxed).
                let l = &mut self.fns[level].locals[slot as usize];
                l.reassigned = Some(span.start);
                l.fresh = false;
                let ty = l.ty.clone();
                return match self.resolve(name, span) {
                    Some((Expr::Capture(idx), _)) => Some((LValue::Capture(idx), ty)),
                    _ => None,
                };
            }
        }
        if let Some(g) = self.gname(name).cloned() {
            if g.func.is_some() {
                self.err(span, format!("assigning to function `{name}`"));
                return None;
            }
            self.ginfo[g.slot as usize].reassigned = true;
            self.mutable_globals = true;
            return Some((LValue::Global(g.slot), g.ty));
        }
        self.err(span, format!("unknown name `{name}`"));
        None
    }

    fn member_target(&mut self, m: &'a ast::MemberExpression<'a>) -> Option<(LValue, Ty)> {
        match m {
            ast::MemberExpression::StaticMemberExpression(s) => {
                let (o, ot) = self.expr(&s.object, None);
                let name = s.property.name.to_string();
                self.note_mutation(&o);
                if let E::Identifier(id) = strip(&s.object) {
                    if id.name == "document" && name == "title" {
                        return Some((
                            LValue::Member(Expr::Builtin(Builtin::DocumentTitle, vec![]), name),
                            Ty::String,
                        ));
                    }
                }
                let t = property(&ot, &name).unwrap_or(Ty::Unknown);
                Some((LValue::Member(o, name), t))
            }
            ast::MemberExpression::ComputedMemberExpression(c) => {
                let (o, ot) = self.expr(&c.object, None);
                let (k, kt) = self.expr(&c.expression, None);
                self.note_mutation(&o);
                let t = types::index(&ot, &kt).unwrap_or(Ty::Unknown);
                Some((LValue::Index(o, k), t))
            }
            ast::MemberExpression::PrivateFieldExpression(p) => {
                self.err(p.span, "private fields are outside the compiled subset");
                None
            }
        }
    }

    /// A mutation of the value `recv` evaluates to: fine on a fresh local, otherwise
    /// the module mutates shared values.
    fn note_mutation(&mut self, recv: &Expr) {
        if let Expr::Local(slot) = recv {
            if self.cur().locals[*slot as usize].fresh {
                return;
            }
        }
        self.mutates_shared = true;
    }

    /// A frame slot of its own for an intermediate value.
    fn temp(&mut self, ty: Ty) -> u32 {
        let c = self.cur();
        let slot = c.locals.len() as u32;
        c.locals.push(Local {
            ty,
            pending: false,
            early: false,
            captured: false,
            reassigned: None,
            fresh: false,
        });
        slot
    }

    /// `x` in a temporary unless it reads a variable (so it is evaluated once).
    fn stable(&mut self, x: Expr, prelude: &mut Vec<Expr>) -> Expr {
        match x {
            Expr::Local(_)
            | Expr::Capture(_)
            | Expr::Global(_)
            | Expr::Num(_)
            | Expr::Str(_)
            | Expr::Bool(_) => x,
            other => {
                let t = self.temp(Ty::Unknown);
                prelude.push(Expr::Assign(
                    Box::new(LValue::Local(t)),
                    None,
                    Box::new(other),
                ));
                Expr::Local(t)
            }
        }
    }

    /// An assignment target read and written through stable parts.
    fn stable_lvalue(&mut self, lv: LValue, prelude: &mut Vec<Expr>) -> (Expr, LValue) {
        match lv {
            LValue::Local(n) => (Expr::Local(n), LValue::Local(n)),
            LValue::Capture(n) => (Expr::Capture(n), LValue::Capture(n)),
            LValue::Global(n) => (Expr::Global(n), LValue::Global(n)),
            LValue::Member(o, k) => {
                let o = self.stable(o, prelude);
                (
                    Expr::Member(Box::new(o.clone()), k.clone(), false),
                    LValue::Member(o, k),
                )
            }
            LValue::Index(o, k) => {
                let o = self.stable(o, prelude);
                let k = self.stable(k, prelude);
                (
                    Expr::Index(Box::new(o.clone()), Box::new(k.clone()), false),
                    LValue::Index(o, k),
                )
            }
        }
    }

    /// `[a, b = 1, ...rest] = v` / `({ a, b: c, ...rest } = v)`: each target
    /// assigned from `value` (a stable expression), in order.
    fn destructure_assign(
        &mut self,
        target: &'a ast::AssignmentTarget<'a>,
        value: Expr,
        out: &mut Vec<Expr>,
    ) {
        match target {
            ast::AssignmentTarget::ArrayAssignmentTarget(arr) => {
                // Iterated into an array first, as destructuring iterates.
                let items = self.temp(Ty::Unknown);
                out.push(Expr::Assign(
                    Box::new(LValue::Local(items)),
                    None,
                    Box::new(Expr::Builtin(
                        Builtin::ArrayFrom,
                        vec![ArrayItem::Item(value)],
                    )),
                ));
                for (i, el) in arr.elements.iter().enumerate() {
                    let Some(el) = el else { continue };
                    let at = Expr::Index(
                        Box::new(Expr::Local(items)),
                        Box::new(Expr::Num(i as f64)),
                        false,
                    );
                    self.assign_maybe_default(el, at, out);
                }
                if let Some(rest) = &arr.rest {
                    let tail = Expr::Method {
                        recv: Box::new(Expr::Local(items)),
                        method: Method::ArraySlice,
                        args: vec![ArrayItem::Item(Expr::Num(arr.elements.len() as f64))],
                        optional: false,
                    };
                    self.assign_to(&rest.target, tail, out);
                }
            }
            ast::AssignmentTarget::ObjectAssignmentTarget(obj) => {
                let mut taken = Vec::new();
                for p in &obj.properties {
                    match p {
                        ast::AssignmentTargetProperty::AssignmentTargetPropertyIdentifier(id) => {
                            let name = id.binding.name.to_string();
                            let v = Expr::Member(Box::new(value.clone()), name.clone(), false);
                            let v = match &id.init {
                                Some(d) => self.with_default(v, d, out),
                                None => v,
                            };
                            if let Some((lv, _)) = self.assign_name(&name, id.binding.span) {
                                out.push(Expr::Assign(Box::new(lv), None, Box::new(v)));
                            }
                            taken.push(name);
                        }
                        ast::AssignmentTargetProperty::AssignmentTargetPropertyProperty(pp) => {
                            let v = if pp.computed {
                                let key = pp.name.as_expression().expect("computed key");
                                let (k, _) = self.expr(key, None);
                                Expr::Index(Box::new(value.clone()), Box::new(k), false)
                            } else {
                                let Some(name) = property_key_name(&pp.name) else {
                                    self.err(
                                        pp.span,
                                        "this property key is outside the compiled subset",
                                    );
                                    continue;
                                };
                                taken.push(name.clone());
                                Expr::Member(Box::new(value.clone()), name, false)
                            };
                            self.assign_maybe_default(&pp.binding, v, out);
                        }
                    }
                }
                if let Some(rest) = &obj.rest {
                    let mut props = vec![Prop::Spread(value.clone())];
                    // The named keys are left out by overwriting then deleting.
                    let rest_obj = self.temp(Ty::Unknown);
                    props.shrink_to_fit();
                    out.push(Expr::Assign(
                        Box::new(LValue::Local(rest_obj)),
                        None,
                        Box::new(Expr::Object(props)),
                    ));
                    for k in taken {
                        out.push(Expr::Builtin(
                            Builtin::Delete,
                            vec![
                                ArrayItem::Item(Expr::Local(rest_obj)),
                                ArrayItem::Item(Expr::Str(k)),
                            ],
                        ));
                    }
                    self.assign_to(&rest.target, Expr::Local(rest_obj), out);
                }
            }
            other => match other.as_simple_assignment_target() {
                Some(t) => {
                    if let Some((lv, _)) = self.simple_target(t) {
                        out.push(Expr::Assign(Box::new(lv), None, Box::new(value)));
                    }
                }
                None => {
                    self.err(
                        other.span(),
                        "this assignment target is outside the compiled subset",
                    );
                }
            },
        }
    }

    fn assign_to(&mut self, t: &'a ast::AssignmentTarget<'a>, v: Expr, out: &mut Vec<Expr>) {
        match t {
            ast::AssignmentTarget::ArrayAssignmentTarget(_)
            | ast::AssignmentTarget::ObjectAssignmentTarget(_) => {
                let tmp = self.temp(Ty::Unknown);
                out.push(Expr::Assign(
                    Box::new(LValue::Local(tmp)),
                    None,
                    Box::new(v),
                ));
                self.destructure_assign(t, Expr::Local(tmp), out);
            }
            other => self.destructure_assign(other, v, out),
        }
    }

    fn assign_maybe_default(
        &mut self,
        el: &'a ast::AssignmentTargetMaybeDefault<'a>,
        v: Expr,
        out: &mut Vec<Expr>,
    ) {
        match el {
            ast::AssignmentTargetMaybeDefault::AssignmentTargetWithDefault(d) => {
                let v = self.with_default(v, &d.init, out);
                self.assign_to(&d.binding, v, out);
            }
            other => {
                let t = other.as_assignment_target().expect("a target");
                self.assign_to(t, v, out);
            }
        }
    }

    /// `v === undefined ? default : v`, with `v` evaluated once.
    fn with_default(&mut self, v: Expr, d: &'a E<'a>, out: &mut Vec<Expr>) -> Expr {
        let t = self.temp(Ty::Unknown);
        out.push(Expr::Assign(Box::new(LValue::Local(t)), None, Box::new(v)));
        let (dx, _) = self.expr(d, None);
        Expr::Cond(
            Box::new(Expr::Binary(
                BinaryOp::StrictEq,
                Box::new(Expr::Local(t)),
                Box::new(Expr::Undefined),
            )),
            Box::new(dx),
            Box::new(Expr::Local(t)),
        )
    }

    fn assignment(&mut self, a: &'a ast::AssignmentExpression<'a>) -> Lowered {
        use ast::AssignmentOperator as A;
        if matches!(
            a.left,
            ast::AssignmentTarget::ArrayAssignmentTarget(_)
                | ast::AssignmentTarget::ObjectAssignmentTarget(_)
        ) {
            // Destructuring: the right side once, then each target in order; the
            // expression's value is the right side.
            let (v, vt) = self.expr(&a.right, None);
            let tmp = self.temp(vt.clone());
            let mut out = vec![Expr::Assign(
                Box::new(LValue::Local(tmp)),
                None,
                Box::new(v),
            )];
            self.destructure_assign(&a.left, Expr::Local(tmp), &mut out);
            out.push(Expr::Local(tmp));
            return (Expr::Seq(out), vt);
        }
        let target = match &a.left {
            ast::AssignmentTarget::AssignmentTargetIdentifier(id) => {
                self.assign_name(id.name.as_str(), id.span)
            }
            other => match other.as_simple_assignment_target() {
                Some(t) => self.simple_target(t),
                None => {
                    self.err(
                        a.span,
                        "this assignment target is outside the compiled subset",
                    );
                    None
                }
            },
        };
        let Some((lv, t)) = target else {
            return (Expr::Undefined, Ty::Unknown);
        };
        if matches!(a.operator, A::LogicalAnd | A::LogicalOr | A::LogicalNullish) {
            // `a ??= b`: the target's parts once; `b` only when it is needed.
            let mut prelude = Vec::new();
            let (read, write) = self.stable_lvalue(lv, &mut prelude);
            let (v, vt) = self.expr(&a.right, Some(&t));
            let op = match a.operator {
                A::LogicalAnd => LogicalOp::And,
                A::LogicalOr => LogicalOp::Or,
                _ => LogicalOp::Nullish,
            };
            prelude.push(Expr::Logical(
                op,
                Box::new(read),
                Box::new(Expr::Assign(Box::new(write), None, Box::new(v))),
            ));
            return (Expr::Seq(prelude), union(t, vt));
        }
        let (v, vt) = self.expr(&a.right, Some(&t));
        let op = match a.operator {
            A::Assign => None,
            A::Addition => Some(BinaryOp::Add),
            A::Subtraction => Some(BinaryOp::Sub),
            A::Multiplication => Some(BinaryOp::Mul),
            A::Division => Some(BinaryOp::Div),
            A::Remainder => Some(BinaryOp::Rem),
            A::Exponential => Some(BinaryOp::Exp),
            A::BitwiseOR => Some(BinaryOp::BitOr),
            A::BitwiseAnd => Some(BinaryOp::BitAnd),
            A::BitwiseXOR => Some(BinaryOp::BitXor),
            A::ShiftLeft => Some(BinaryOp::Shl),
            A::ShiftRight => Some(BinaryOp::Shr),
            A::ShiftRightZeroFill => Some(BinaryOp::UShr),
            _ => return self.unsupported(a.span, "logical assignment (`&&=`, `||=`, `??=`)"),
        };
        (Expr::Assign(Box::new(lv), op, Box::new(v)), vt)
    }

    fn member(&mut self, m: &'a ast::MemberExpression<'a>) -> Lowered {
        match m {
            ast::MemberExpression::StaticMemberExpression(s) => {
                let name = s.property.name.as_str();
                if let E::Identifier(id) = strip(&s.object) {
                    if self.resolve_is_free(id.name.as_str()) {
                        if let Some(r) = self.namespace_member(id.name.as_str(), name, s.span) {
                            return r;
                        }
                    }
                }
                if let Some(ns) = self.window_member(&s.object) {
                    if let Some(r) = self.namespace_member(ns, name, s.span) {
                        return r;
                    }
                }
                let (o, ot) = self.expr(&s.object, None);
                if matches!(ot, Ty::Unknown) {
                    return (
                        Expr::Member(Box::new(o), name.to_owned(), s.optional),
                        Ty::Unknown,
                    );
                }
                let base = if s.optional {
                    non_null(&ot)
                } else {
                    ot.clone()
                };
                if matches!(non_null(&base), Ty::Ref(_)) && name == "current" {
                    self.mark_always();
                }
                if matches!(non_null(&base), Ty::DomNode) {
                    self.mark_always();
                }
                match property(&base, name) {
                    Some(t) => (Expr::Member(Box::new(o), name.to_owned(), s.optional), t),
                    // A host object's property the runtime does not model.
                    None if is_host_type(&non_null(&base)) => {
                        self.err(
                            s.property.span,
                            format!(
                                "property `{name}` does not exist on type {}",
                                types::show(&ot)
                            ),
                        );
                        (Expr::Undefined, Ty::Unknown)
                    }
                    // Not in the static type (a union the compiler does not narrow,
                    // a type it does not model): read when it runs.
                    None => {
                        self.mark_always();
                        (
                            Expr::Member(Box::new(o), name.to_owned(), s.optional),
                            Ty::Unknown,
                        )
                    }
                }
            }
            ast::MemberExpression::ComputedMemberExpression(c) => {
                let (o, ot) = self.expr(&c.object, None);
                let (k, kt) = self.expr(&c.expression, None);
                let base = if c.optional {
                    non_null(&ot)
                } else {
                    ot.clone()
                };
                let t = match types::index(&base, &kt) {
                    Some(t) => t,
                    None => {
                        self.mark_always();
                        Ty::Unknown
                    }
                };
                (Expr::Index(Box::new(o), Box::new(k), c.optional), t)
            }
            ast::MemberExpression::PrivateFieldExpression(p) => {
                self.unsupported(p.span, "a private field")
            }
        }
    }

    /// `window.X` / `globalThis.X` for a host namespace `X` (`localStorage`,
    /// `location`, `navigator`, `document`, …).
    fn window_member(&self, e: &E<'a>) -> Option<&'static str> {
        let E::StaticMemberExpression(m) = strip(e) else {
            return None;
        };
        let E::Identifier(w) = strip(&m.object) else {
            return None;
        };
        if !matches!(w.name.as_str(), "window" | "globalThis" | "self")
            || !self.resolve_is_free(w.name.as_str())
        {
            return None;
        }
        [
            "localStorage",
            "sessionStorage",
            "location",
            "navigator",
            "document",
            "console",
            "Math",
            "JSON",
        ]
        .into_iter()
        .find(|n| *n == m.property.name.as_str())
    }

    /// Whether `name` is not a variable (so `Math` means the built-in).
    fn resolve_is_free(&self, name: &str) -> bool {
        !self.fns.iter().any(|f| f.lookup(name).is_some()) && !self.global_names.contains_key(name)
    }

    /// `Math.PI`, `Number.MAX_SAFE_INTEGER`, `document.title`, ...
    fn namespace_member(&mut self, ns: &str, name: &str, span: Span) -> Option<Lowered> {
        if ns == "cw" && self.ambient_values.contains_key("cw") {
            return Some(self.cw_member(name, span));
        }
        let r = match (ns, name) {
            ("Math", "PI") => (Expr::Builtin(Builtin::MathPi, vec![]), Ty::Number),
            ("Number", "MAX_SAFE_INTEGER") => (Expr::Num(9007199254740991.0), Ty::Number),
            ("Number", "MIN_SAFE_INTEGER") => (Expr::Num(-9007199254740991.0), Ty::Number),
            ("Number", "POSITIVE_INFINITY") => {
                (Expr::Builtin(Builtin::Infinity, vec![]), Ty::Number)
            }
            ("Number", "NaN") => (Expr::Builtin(Builtin::NaN, vec![]), Ty::Number),
            ("Number", "NEGATIVE_INFINITY") => (Expr::Num(f64::NEG_INFINITY), Ty::Number),
            ("Number", "EPSILON") => (Expr::Num(f64::EPSILON), Ty::Number),
            ("Number", "MAX_VALUE") => (Expr::Num(f64::MAX), Ty::Number),
            ("Number", "MIN_VALUE") => (Expr::Num(5e-324), Ty::Number),
            ("Math", "E") => (Expr::Num(std::f64::consts::E), Ty::Number),
            ("Math", "LN2") => (Expr::Num(std::f64::consts::LN_2), Ty::Number),
            ("Math", "LN10") => (Expr::Num(std::f64::consts::LN_10), Ty::Number),
            ("Math", "LOG2E") => (Expr::Num(std::f64::consts::LOG2_E), Ty::Number),
            ("Math", "LOG10E") => (Expr::Num(std::f64::consts::LOG10_E), Ty::Number),
            ("Math", "SQRT2") => (Expr::Num(std::f64::consts::SQRT_2), Ty::Number),
            ("Math", "SQRT1_2") => (Expr::Num(std::f64::consts::FRAC_1_SQRT_2), Ty::Number),
            // Built-in functions as values (`xs.map(Math.round)`).
            (ns, n) if builtin_value(ns, n).is_some() => {
                (Expr::BuiltinFn(builtin_value(ns, n).unwrap()), Ty::Unknown)
            }
            ("document", "title") => {
                self.mark_always();
                (Expr::Builtin(Builtin::DocumentTitle, vec![]), Ty::String)
            }
            ("document", "activeElement") => {
                self.mark_always();
                (
                    Expr::Builtin(Builtin::ActiveElement, vec![]),
                    union(Ty::DomNode, Ty::Null),
                )
            }
            ("document", "body") => (Expr::Builtin(Builtin::DocumentBody, vec![]), Ty::DomNode),
            ("localStorage" | "sessionStorage", "length") => {
                self.mark_always();
                (
                    Expr::Builtin(
                        Builtin::StorageLength,
                        vec![ArrayItem::Item(Expr::Num(storage_area(ns)))],
                    ),
                    Ty::Number,
                )
            }
            (
                "location",
                p @ ("href" | "pathname" | "search" | "hash" | "origin" | "host" | "hostname"
                | "protocol" | "port"),
            ) => {
                self.mark_always();
                (
                    Expr::Builtin(
                        Builtin::LocationPart,
                        vec![ArrayItem::Item(Expr::Str(p.into()))],
                    ),
                    Ty::String,
                )
            }
            ("navigator", p) if navigator_constant(p).is_some() => (
                navigator_constant(p).unwrap(),
                match p {
                    "onLine" | "cookieEnabled" | "webdriver" => Ty::Boolean,
                    "hardwareConcurrency" | "maxTouchPoints" | "deviceMemory" => Ty::Number,
                    "languages" => Ty::Array(Box::new(Ty::String)),
                    _ => Ty::String,
                },
            ),
            ("document", "documentElement") => {
                (Expr::Builtin(Builtin::DocumentElement, vec![]), Ty::DomNode)
            }
            ("window", "scrollX" | "pageXOffset") => {
                self.mark_always();
                (Expr::Builtin(Builtin::ScrollX, vec![]), Ty::Number)
            }
            ("window", "scrollY" | "pageYOffset") => {
                self.mark_always();
                (Expr::Builtin(Builtin::ScrollY, vec![]), Ty::Number)
            }
            ("window", "innerWidth") => {
                self.mark_always();
                (Expr::Builtin(Builtin::InnerWidth, vec![]), Ty::Number)
            }
            ("window", "innerHeight") => {
                self.mark_always();
                (Expr::Builtin(Builtin::InnerHeight, vec![]), Ty::Number)
            }
            (
                "Math" | "Number" | "JSON" | "Object" | "Array" | "console" | "Date" | "window"
                | "document" | "Promise" | "String",
                _,
            ) => {
                self.err(
                    span,
                    format!("`{ns}.{name}` is outside the compiled subset"),
                );
                (Expr::Undefined, Ty::Unknown)
            }
            _ => return None,
        };
        Some(r)
    }

    // ------------------------------------------------------------------ calls

    fn call(&mut self, c: &'a ast::CallExpression<'a>, want: Option<&Ty>) -> Lowered {
        let callee = strip(&c.callee);
        match callee {
            E::Identifier(id) => {
                let name = id.name.as_str();
                if self.resolve_is_free(name) {
                    if let Some(r) = self.react.get(name).copied() {
                        return self.react_call(r, c, want);
                    }
                    if let Some(r) = self.global_call(name, c) {
                        return r;
                    }
                }
                // A custom hook: same placement rules as React's.
                if is_hook_name(name) {
                    if let Some(g) = self.gname(name) {
                        if g.kind == FunctionKind::Hook {
                            self.check_hook_position(c.span, name);
                        }
                    }
                }
                let generic = if self.fns.iter().any(|f| f.lookup(name).is_some()) {
                    None
                } else {
                    self.gname(name).and_then(|g| g.generic.clone())
                };
                let (f, ft) = self.identifier(name, id.span);
                match generic {
                    Some((params, sig)) => self.generic_call(f, ft, &params, &sig, c),
                    None => self.value_call(f, ft, c, want),
                }
            }
            E::StaticMemberExpression(m) => {
                let prop = m.property.name.as_str();
                if let E::StaticMemberExpression(inner) = strip(&m.object) {
                    if let E::Identifier(root) = strip(&inner.object) {
                        if root.name == "cw"
                            && matches!(inner.property.name.as_str(), "state" | "fs" | "window")
                            && self.ambient_values.contains_key("cw")
                            && self.resolve_is_free("cw")
                        {
                            return self.cw_call(inner.property.name.as_str(), prop, c);
                        }
                    }
                }
                if let E::Identifier(ns) = strip(&m.object) {
                    let nsn = ns.name.as_str();
                    if self.resolve_is_free(nsn) {
                        if self.react.get(nsn) == Some(&ReactName::ReactNs) {
                            return self.react_call(react_export(prop), c, want);
                        }
                        if let Some(r) = self.namespace_call(nsn, prop, c) {
                            return r;
                        }
                    }
                }
                // `window.localStorage.getItem(k)` is `localStorage.getItem(k)`.
                if let Some(ns) = self.window_member(&m.object) {
                    if let Some(r) = self.namespace_call(ns, prop, c) {
                        return r;
                    }
                }
                let (recv, rt) = self.expr(&m.object, None);
                self.method_call(recv, rt, prop, m.optional, c, want)
            }
            _ => {
                let (f, ft) = self.expr(&c.callee, None);
                self.value_call(f, ft, c, want)
            }
        }
    }

    /// A call of a generic module function: its type parameters bound from the
    /// explicit type arguments, then from the arguments' types.
    fn generic_call(
        &mut self,
        f: Expr,
        ft: Ty,
        params: &[String],
        sig: &Ty,
        c: &'a ast::CallExpression<'a>,
    ) -> Lowered {
        let Ty::Function(ps, ret) = sig else {
            return self.value_call(f, ft, c, None);
        };
        let mut bound = BTreeMap::new();
        if let Some(ta) = &c.type_arguments {
            for (i, t) in ta.params.iter().enumerate() {
                if let Some(n) = params.get(i) {
                    let t = self.ts_type(t);
                    bound.insert(n.clone(), t);
                }
            }
        }
        let wants: Vec<Ty> = ps.iter().map(|p| types::subst(p, &bound)).collect();
        let (args, tys) = self.exprs_args(&c.arguments, &wants);
        for (p, a) in ps.iter().zip(&tys) {
            types::unify(p, a, &mut bound);
        }
        let mut t = types::subst(ret, &bound);
        if matches!(t, Ty::Unknown) {
            if let Ty::Function(_, r) = non_null(&ft) {
                t = *r;
            }
        }
        if self.mutable_globals && matches!(f, Expr::Global(_)) {
            self.mark_always();
        }
        (Expr::Call(Box::new(f), args, c.optional), t)
    }

    fn value_call(
        &mut self,
        f: Expr,
        ft: Ty,
        c: &'a ast::CallExpression<'a>,
        _want: Option<&Ty>,
    ) -> Lowered {
        let fnt = non_null(&ft);
        let (params, ret) = match &fnt {
            Ty::Function(ps, r) => (ps.clone(), (**r).clone()),
            Ty::Setter(t) => (
                vec![union(
                    (**t).clone(),
                    Ty::Function(vec![(**t).clone()], t.clone()),
                )],
                Ty::Void,
            ),
            Ty::Dispatch(a) => (vec![(**a).clone()], Ty::Void),
            // Called as it is when it runs (a `TypeError` if it is no function).
            _ => (Vec::new(), Ty::Unknown),
        };
        let (args, _) = self.exprs_args(&c.arguments, &params);
        if self.mutable_globals && matches!(f, Expr::Global(_)) {
            self.mark_always();
        }
        (Expr::Call(Box::new(f), args, c.optional), ret)
    }

    /// `parseInt(x)`, `setTimeout(f, ms)`, `fetch(url)`, ...
    fn global_call(&mut self, name: &str, c: &'a ast::CallExpression<'a>) -> Option<Lowered> {
        let (b, want, ret) = match name {
            "parseInt" => (Builtin::ParseInt, vec![Ty::String, Ty::Number], Ty::Number),
            "parseFloat" => (Builtin::ParseFloat, vec![Ty::String], Ty::Number),
            "isNaN" => (Builtin::IsNaN, vec![Ty::Number], Ty::Boolean),
            "Number" => (Builtin::Number, vec![Ty::Unknown], Ty::Number),
            "String" => (Builtin::String, vec![Ty::Unknown], Ty::String),
            "Boolean" => (Builtin::Boolean, vec![Ty::Unknown], Ty::Boolean),
            "setTimeout" => (
                Builtin::SetTimeout,
                vec![Ty::Function(vec![], Box::new(Ty::Void)), Ty::Number],
                Ty::Number,
            ),
            "setInterval" => (
                Builtin::SetInterval,
                vec![Ty::Function(vec![], Box::new(Ty::Void)), Ty::Number],
                Ty::Number,
            ),
            "clearTimeout" => (Builtin::ClearTimeout, vec![Ty::Number], Ty::Void),
            "clearInterval" => (Builtin::ClearInterval, vec![Ty::Number], Ty::Void),
            "fetch" => (
                Builtin::Fetch,
                vec![Ty::String],
                Ty::Promise(Box::new(Ty::Response)),
            ),
            "Array" => (Builtin::NewArray, vec![], Ty::Array(Box::new(Ty::Unknown))),
            "alert" | "confirm" | "prompt" => {
                // What the page's `alert` does: the host records `kind: text`;
                // `confirm` answers true and `prompt` null.
                let (mut args, _) = self.exprs_args(&c.arguments, &[]);
                args.insert(0, ArrayItem::Item(Expr::Str(name.into())));
                return Some((
                    Expr::Builtin(Builtin::Alert, args),
                    match name {
                        "confirm" => Ty::Boolean,
                        "prompt" => union(Ty::String, Ty::Null),
                        _ => Ty::Void,
                    },
                ));
            }
            _ => return None,
        };
        let (args, _) = self.exprs_args(&c.arguments, &want);
        if matches!(
            b,
            Builtin::SetTimeout | Builtin::SetInterval | Builtin::Fetch
        ) {
            self.mark_always();
        }
        Some((Expr::Builtin(b, args), ret))
    }

    /// `Math.max(...)`, `Object.keys(o)`, `JSON.stringify(v)`, `console.log(...)`.
    fn namespace_call(
        &mut self,
        ns: &str,
        name: &str,
        c: &'a ast::CallExpression<'a>,
    ) -> Option<Lowered> {
        if ns == "cw" && self.ambient_values.contains_key("cw") {
            return Some(self.cw_call("", name, c));
        }
        let num = Ty::Number;
        let (b, want, ret): (Builtin, Vec<Ty>, Ty) = match (ns, name) {
            ("Math", "max") => (Builtin::MathMax, vec![], num),
            ("Math", "min") => (Builtin::MathMin, vec![], num),
            ("Math", "round") => (Builtin::MathRound, vec![], num),
            ("Math", "floor") => (Builtin::MathFloor, vec![], num),
            ("Math", "ceil") => (Builtin::MathCeil, vec![], num),
            ("Math", "abs") => (Builtin::MathAbs, vec![], num),
            ("Math", "trunc") => (Builtin::MathTrunc, vec![], num),
            ("Math", "sign") => (Builtin::MathSign, vec![], num),
            ("Math", "sqrt") => (Builtin::MathSqrt, vec![], num),
            ("Math", "pow") => (Builtin::MathPow, vec![], num),
            ("Math", "random") => {
                self.mark_always();
                (Builtin::MathRandom, vec![], num)
            }
            ("Number", "isNaN") => (Builtin::NumberIsNaN, vec![], Ty::Boolean),
            ("Number", "isInteger") => (Builtin::NumberIsInteger, vec![], Ty::Boolean),
            ("Number", "isFinite") => (Builtin::NumberIsFinite, vec![], Ty::Boolean),
            ("Number", "parseInt") => (Builtin::ParseInt, vec![], num),
            ("Number", "parseFloat") => (Builtin::ParseFloat, vec![], num),
            ("Array", "isArray") => (Builtin::ArrayIsArray, vec![], Ty::Boolean),
            ("Array", "from") => {
                // The source first (an iterable or `{ length }`), then the mapper,
                // typed by the source's elements.
                let src = c.arguments.first().and_then(|a| a.as_expression());
                let (sx, st) = match src {
                    Some(e) => self.expr(e, None),
                    None => (Expr::Undefined, Ty::Undefined),
                };
                let elem = if types::is_stringy(&st) {
                    Ty::String
                } else {
                    element(&st).unwrap_or(Ty::Undefined)
                };
                let mut args = vec![ArrayItem::Item(sx)];
                let mut t = elem.clone();
                if let Some(f) = c.arguments.get(1).and_then(|a| a.as_expression()) {
                    let fw = Ty::Function(vec![elem, Ty::Number], Box::new(Ty::Unknown));
                    let (fx, ft) = self.expr(f, Some(&fw));
                    if let Ty::Function(_, r) = ft {
                        t = *r;
                    }
                    args.push(ArrayItem::Item(fx));
                }
                return Some((
                    Expr::Builtin(Builtin::ArrayFrom, args),
                    Ty::Array(Box::new(t)),
                ));
            }
            ("Array", "of") => {
                let (args, tys) = self.exprs_args(&c.arguments, &[]);
                return Some((
                    Expr::Builtin(Builtin::ArrayOf, args),
                    Ty::Array(Box::new(union_all(tys))),
                ));
            }
            ("Object", "keys") => (Builtin::ObjectKeys, vec![], Ty::Array(Box::new(Ty::String))),
            ("Object", "is") => (Builtin::ObjectIs, vec![], Ty::Boolean),
            ("window" | "document", "addEventListener" | "removeEventListener") => {
                let b = match (ns, name) {
                    ("window", "addEventListener") => Builtin::WindowAddListener,
                    ("window", _) => Builtin::WindowRemoveListener,
                    (_, "addEventListener") => Builtin::DocumentAddListener,
                    _ => Builtin::DocumentRemoveListener,
                };
                let listener = Ty::Function(vec![Ty::Event], Box::new(Ty::Void));
                let options = union(
                    Ty::Boolean,
                    Ty::Object(vec![("capture".into(), Ty::Boolean, true)]),
                );
                (b, vec![Ty::String, listener, options], Ty::Void)
            }
            ("document", "getElementById") => {
                self.mark_always();
                (
                    Builtin::GetElementById,
                    vec![Ty::String],
                    union(Ty::DomNode, Ty::Null),
                )
            }
            ("document", "querySelector") => {
                self.mark_always();
                (
                    Builtin::QuerySelector,
                    vec![Ty::String],
                    union(Ty::DomNode, Ty::Null),
                )
            }
            (
                "localStorage" | "sessionStorage",
                m @ ("getItem" | "setItem" | "removeItem" | "clear" | "key"),
            ) => {
                let b = match m {
                    "getItem" => Builtin::StorageGet,
                    "setItem" => Builtin::StorageSet,
                    "removeItem" => Builtin::StorageRemove,
                    "clear" => Builtin::StorageClear,
                    _ => Builtin::StorageKey,
                };
                let ret = match m {
                    "getItem" | "key" => union(Ty::String, Ty::Null),
                    _ => Ty::Void,
                };
                self.mark_always();
                let (mut args, _) = self.exprs_args(&c.arguments, &[]);
                args.insert(0, ArrayItem::Item(Expr::Num(storage_area(ns))));
                return Some((Expr::Builtin(b, args), ret));
            }
            ("document", "querySelectorAll") => {
                self.mark_always();
                (
                    Builtin::QuerySelectorAll,
                    vec![Ty::String],
                    Ty::Array(Box::new(Ty::DomNode)),
                )
            }
            ("window", "scrollTo" | "scroll") => (Builtin::WindowScrollTo, vec![], Ty::Void),
            ("window", "scrollBy") => (Builtin::WindowScrollBy, vec![], Ty::Void),
            ("Object", "values") | ("Object", "entries") => {
                let (args, tys) = self.exprs_args(&c.arguments, &[]);
                let v = match tys.first().map(non_null) {
                    Some(Ty::Object(fs)) => union_all(fs.into_iter().map(|(_, t, _)| t)),
                    Some(Ty::Dict(v)) => *v,
                    Some(Ty::Array(e)) => *e,
                    _ => Ty::Unknown,
                };
                return Some(if name == "values" {
                    (
                        Expr::Builtin(Builtin::ObjectValues, args),
                        Ty::Array(Box::new(v)),
                    )
                } else {
                    (
                        Expr::Builtin(Builtin::ObjectEntries, args),
                        Ty::Array(Box::new(Ty::Tuple(vec![Ty::String, v]))),
                    )
                });
            }
            ("Object", "assign") => {
                let (args, tys) = self.exprs_args(&c.arguments, &[]);
                self.mutates_shared = true;
                return Some((
                    Expr::Builtin(Builtin::ObjectAssign, args),
                    tys.into_iter().next().unwrap_or(Ty::Unknown),
                ));
            }
            ("Object", "fromEntries") => {
                let (args, tys) = self.exprs_args(&c.arguments, &[]);
                let v = tys
                    .first()
                    .and_then(element)
                    .and_then(|e| types::index(&e, &Ty::NumLit(1.0)))
                    .unwrap_or(Ty::Unknown);
                return Some((
                    Expr::Builtin(Builtin::ObjectFromEntries, args),
                    Ty::Dict(Box::new(v)),
                ));
            }
            ("JSON", "stringify") => (Builtin::JsonStringify, vec![], Ty::String),
            ("JSON", "parse") => (Builtin::JsonParse, vec![Ty::String], Ty::Unknown),
            ("Date", "now") => {
                self.mark_always();
                (Builtin::DateNow, vec![], num)
            }
            ("Date", "UTC") => (Builtin::DateUTC, vec![], num),
            ("Date", "parse") => (Builtin::DateParse, vec![Ty::String], num),
            ("console", "log") | ("console", "info") | ("console", "debug") => {
                (Builtin::ConsoleLog, vec![], Ty::Void)
            }
            ("console", "warn") => (Builtin::ConsoleWarn, vec![], Ty::Void),
            ("console", "error") => (Builtin::ConsoleError, vec![], Ty::Void),
            ("window", "setTimeout") => {
                let (args, _) = self.exprs_args(
                    &c.arguments,
                    &[Ty::Function(vec![], Box::new(Ty::Void)), num],
                );
                return Some((Expr::Builtin(Builtin::SetTimeout, args), Ty::Number));
            }
            ("window", "clearTimeout") => (Builtin::ClearTimeout, vec![], Ty::Void),
            ("window", "setInterval") => {
                let (args, _) = self.exprs_args(
                    &c.arguments,
                    &[Ty::Function(vec![], Box::new(Ty::Void)), num],
                );
                return Some((Expr::Builtin(Builtin::SetInterval, args), Ty::Number));
            }
            ("window", "clearInterval") => (Builtin::ClearInterval, vec![], Ty::Void),
            ("Promise", "all") => {
                let (args, tys) = self.exprs_args(&c.arguments, &[]);
                let unwrap = |t: Ty| match t {
                    Ty::Promise(i) => *i,
                    t => t,
                };
                let t = match tys.into_iter().next().map(|t| non_null(&t)) {
                    Some(Ty::Tuple(ts)) => Ty::Tuple(ts.into_iter().map(unwrap).collect()),
                    Some(Ty::Array(e)) => Ty::Array(Box::new(unwrap(*e))),
                    _ => Ty::Unknown,
                };
                return Some((
                    Expr::Builtin(Builtin::PromiseAll, args),
                    Ty::Promise(Box::new(t)),
                ));
            }
            ("Promise", "reject") => {
                let (args, _) = self.exprs_args(&c.arguments, &[]);
                return Some((
                    Expr::Builtin(Builtin::PromiseReject, args),
                    Ty::Promise(Box::new(Ty::Unknown)),
                ));
            }
            ("Promise", "resolve") => {
                let (args, tys) = self.exprs_args(&c.arguments, &[]);
                return Some((
                    Expr::Builtin(Builtin::PromiseResolve, args),
                    Ty::Promise(Box::new(tys.into_iter().next().unwrap_or(Ty::Undefined))),
                ));
            }
            (
                "Math" | "Number" | "JSON" | "Object" | "Array" | "console" | "Date" | "window"
                | "document" | "Promise" | "String",
                _,
            ) => {
                self.err(
                    c.span,
                    format!("`{ns}.{name}` is outside the compiled subset"),
                );
                return Some((Expr::Undefined, Ty::Unknown));
            }
            _ => return None,
        };
        let (args, _) = self.exprs_args(&c.arguments, &want);
        Some((Expr::Builtin(b, args), ret))
    }

    /// A type a declaration file declares, or `unknown`.
    fn ambient_type(&mut self, name: &str, span: Span) -> Ty {
        self.named_type(name, span).unwrap_or(Ty::Unknown)
    }

    /// `cw.kind`, `cw.argument`, `cw.env`: the desktop host's global (see `cw_ui::cw`).
    fn cw_member(&mut self, name: &str, span: Span) -> Lowered {
        self.mark_always();
        match name {
            "kind" => (Expr::Builtin(Builtin::CwKind, vec![]), Ty::String),
            "argument" => (Expr::Builtin(Builtin::CwArgument, vec![]), Ty::String),
            "env" => (
                Expr::Builtin(Builtin::CwEnv, vec![]),
                self.ambient_type("CwEnv", span),
            ),
            _ => {
                self.err(
                    span,
                    format!("`cw.{name}` is outside the compiled subset (call its methods)"),
                );
                (Expr::Undefined, Ty::Unknown)
            }
        }
    }

    /// A call of one of `cw`'s methods: `cw.now()`, `cw.fs.readFile(path)`, ...
    fn cw_call(&mut self, group: &str, name: &str, c: &'a ast::CallExpression<'a>) -> Lowered {
        self.mark_always();
        let promise = |t: Ty| Ty::Promise(Box::new(t));
        let s = Ty::String;
        let (b, want, ret) = match (group, name) {
            ("", "onEnv") => {
                let env = self.ambient_type("CwEnv", c.span);
                (
                    Builtin::CwOnEnv,
                    vec![Ty::Function(vec![env], Box::new(Ty::Void))],
                    Ty::Function(vec![], Box::new(Ty::Void)),
                )
            }
            ("", "now") => (Builtin::CwNow, vec![], Ty::Number),
            ("", "fetch") => {
                let init = self.ambient_type("CwFetchInit", c.span);
                (Builtin::CwFetch, vec![s, init], promise(Ty::CwResponse))
            }
            ("", "launch") => (Builtin::CwLaunch, vec![s.clone(), s], promise(Ty::Void)),
            ("", "emit") => (Builtin::CwEmit, vec![s, Ty::Unknown], promise(Ty::Void)),
            ("", "refuse") => (Builtin::CwRefuse, vec![s], Ty::Void),
            ("state", "get") => {
                let t = match c.type_arguments.as_ref().and_then(|a| a.params.first()) {
                    Some(t) => self.ts_type(t),
                    None => Ty::Unknown,
                };
                (Builtin::CwStateGet, vec![], union(t, Ty::Null))
            }
            ("state", "set") => (Builtin::CwStateSet, vec![Ty::Unknown], Ty::Void),
            ("fs", "readFile") => (Builtin::CwReadFile, vec![s.clone()], promise(s)),
            ("fs", "writeFile") => (Builtin::CwWriteFile, vec![s.clone(), s], promise(Ty::Void)),
            ("fs", "list") => (
                Builtin::CwList,
                vec![s.clone()],
                promise(Ty::Array(Box::new(s))),
            ),
            ("fs", "mkdir") => (Builtin::CwMkdir, vec![s], promise(Ty::Void)),
            ("window", "set") => {
                let facts = self.ambient_type("CwWindowFacts", c.span);
                (Builtin::CwWindowSet, vec![facts], Ty::Void)
            }
            _ => {
                let path = if group.is_empty() {
                    format!("cw.{name}")
                } else {
                    format!("cw.{group}.{name}")
                };
                self.err(c.span, format!("`{path}` is outside the compiled subset"));
                return (Expr::Undefined, Ty::Unknown);
            }
        };
        let (args, _) = self.exprs_args(&c.arguments, &want);
        (Expr::Builtin(b, args), ret)
    }

    fn check_hook_position(&mut self, span: Span, name: &str) {
        let ctx = self.fns.last().unwrap();
        let ok = matches!(ctx.kind, FunctionKind::Component | FunctionKind::Hook)
            && ctx.cond_depth == 0
            && self.hole_always.is_empty();
        if !ok {
            self.err(
                span,
                format!("`{name}` must be called at the top level of a component or hook"),
            );
        }
    }

    fn react_call(
        &mut self,
        r: ReactName,
        c: &'a ast::CallExpression<'a>,
        want: Option<&Ty>,
    ) -> Lowered {
        let arg0 = c.arguments.first().and_then(|a| a.as_expression());
        match r {
            ReactName::Memo => {
                // `memo(C)` is `C`.
                return match arg0 {
                    Some(x) => self.expr(x, want),
                    None => (Expr::Undefined, Ty::Undefined),
                };
            }
            ReactName::ForwardRef => {
                let Some(x) = arg0 else {
                    return (Expr::Undefined, Ty::Undefined);
                };
                return match self.function_value("<forwardRef>", x) {
                    Some(mut p) => {
                        p.forward_ref = true;
                        p.kind = FunctionKind::Component;
                        self.closure(p, None)
                    }
                    None => self.unsupported(c.span, "`forwardRef` of anything but a function"),
                };
            }
            ReactName::UseTransition => {
                self.check_hook_position(c.span, "useTransition");
                return (
                    Expr::Array(vec![
                        ArrayItem::Item(Expr::Bool(false)),
                        ArrayItem::Item(Expr::BuiltinFn(Builtin::StartTransition)),
                    ]),
                    Ty::Tuple(vec![
                        Ty::Boolean,
                        Ty::Function(
                            vec![Ty::Function(vec![], Box::new(Ty::Void))],
                            Box::new(Ty::Void),
                        ),
                    ]),
                );
            }
            ReactName::UseDeferredValue => {
                self.check_hook_position(c.span, "useDeferredValue");
                return match arg0 {
                    Some(x) => self.expr(x, want),
                    None => (Expr::Undefined, Ty::Undefined),
                };
            }
            ReactName::UseDebugValue => {
                self.check_hook_position(c.span, "useDebugValue");
                let (args, _) = self.exprs_args(&c.arguments, &[]);
                return (
                    Expr::Seq(
                        args.into_iter()
                            .map(|a| match a {
                                ArrayItem::Item(e) | ArrayItem::Spread(e) => e,
                            })
                            .chain([Expr::Undefined])
                            .collect(),
                    ),
                    Ty::Void,
                );
            }
            ReactName::StartTransition => {
                let (args, _) =
                    self.exprs_args(&c.arguments, &[Ty::Function(vec![], Box::new(Ty::Void))]);
                return (Expr::Builtin(Builtin::StartTransition, args), Ty::Void);
            }
            _ => {}
        }
        let ReactName::Hook(hook) = r else {
            let what = match r {
                ReactName::CreateContext => "`createContext` inside a function",
                ReactName::CreateRoot => "`createRoot` outside the render call",
                _ => "this React API",
            };
            return self.unsupported(c.span, what);
        };
        self.check_hook_position(c.span, &format!("{hook:?}"));
        let targ = c
            .type_arguments
            .as_ref()
            .and_then(|t| t.params.first())
            .map(|t| self.ts_type(t));
        match hook {
            Hook::State => {
                let init_want = targ
                    .clone()
                    .map(|t| union(t.clone(), Ty::Function(vec![], Box::new(t))));
                let args = self.arg_exprs(&c.arguments, &init_want.into_iter().collect::<Vec<_>>());
                let (init, it) = args
                    .into_iter()
                    .next()
                    .unwrap_or((Expr::Undefined, Ty::Undefined));
                let st = match targ {
                    Some(t) => t,
                    None => match &it {
                        Ty::Function(ps, r) if ps.is_empty() => widen(r),
                        t => widen(t),
                    },
                };
                (
                    Expr::Hook(Hook::State, vec![init]),
                    Ty::Tuple(vec![st.clone(), Ty::Setter(Box::new(st))]),
                )
            }
            Hook::Reducer => {
                let args = self.arg_exprs(&c.arguments, &[]);
                let mut it = args.into_iter();
                let (reducer, rt) = it.next().unwrap_or((Expr::Undefined, Ty::Unknown));
                let (init, initt) = it.next().unwrap_or((Expr::Undefined, Ty::Undefined));
                let init_fn = it.next();
                let (st, at) = match &rt {
                    Ty::Function(ps, _) => (
                        ps.first().cloned().unwrap_or(Ty::Unknown),
                        ps.get(1).cloned().unwrap_or(Ty::Undefined),
                    ),
                    _ => (widen(&initt), Ty::Unknown),
                };
                let mut hargs = vec![reducer, init];
                if let Some((f, _)) = init_fn {
                    hargs.push(f);
                }
                (
                    Expr::Hook(Hook::Reducer, hargs),
                    Ty::Tuple(vec![st, Ty::Dispatch(Box::new(at))]),
                )
            }
            Hook::Memo | Hook::Callback => {
                let fwant = match (&targ, hook) {
                    (Some(t), Hook::Memo) => Ty::Function(vec![], Box::new(t.clone())),
                    (Some(t), _) => t.clone(),
                    (None, _) => Ty::Unknown,
                };
                let args = self.arg_exprs(&c.arguments, &[fwant]);
                let mut it = args.into_iter();
                let (f, ft) = it.next().unwrap_or((Expr::Undefined, Ty::Unknown));
                let (deps, _) = it.next().unwrap_or((Expr::Undefined, Ty::Undefined));
                let t = match (hook, &ft) {
                    (Hook::Memo, Ty::Function(_, r)) => (**r).clone(),
                    (Hook::Memo, _) => Ty::Unknown,
                    _ => ft.clone(),
                };
                (Expr::Hook(hook, vec![f, deps]), t)
            }
            Hook::Ref => {
                let args = self.arg_exprs(&c.arguments, &targ.iter().cloned().collect::<Vec<_>>());
                let (init, it) = args
                    .into_iter()
                    .next()
                    .unwrap_or((Expr::Undefined, Ty::Undefined));
                let t = match targ {
                    Some(Ty::DomNode) => union(Ty::DomNode, Ty::Null),
                    Some(t) => t,
                    None => widen(&it),
                };
                (Expr::Hook(Hook::Ref, vec![init]), Ty::Ref(Box::new(t)))
            }
            Hook::Effect | Hook::LayoutEffect => {
                let fwant = Ty::Function(
                    vec![],
                    Box::new(union(Ty::Void, Ty::Function(vec![], Box::new(Ty::Void)))),
                );
                let args = self.arg_exprs(&c.arguments, &[fwant]);
                let mut it = args.into_iter();
                let (f, _) = it.next().unwrap_or((Expr::Undefined, Ty::Unknown));
                let deps = it.next().map(|(d, _)| d);
                let mut hargs = vec![f];
                match deps {
                    Some(d) => hargs.push(d),
                    None => self.cur().has_depless_effect = true,
                }
                (Expr::Hook(hook, hargs), Ty::Void)
            }
            Hook::ImperativeHandle => {
                // `useImperativeHandle(ref, create, deps?)`: a layout effect that
                // sets the ref to `create()` (and back to null on cleanup).
                let args = self.arg_exprs(
                    &c.arguments,
                    &[Ty::Unknown, Ty::Function(vec![], Box::new(Ty::Unknown))],
                );
                let mut hargs: Vec<Expr> = args.into_iter().take(3).map(|(e, _)| e).collect();
                if hargs.len() < 3 {
                    self.cur().has_depless_effect = true;
                }
                hargs.resize(2.max(hargs.len()), Expr::Undefined);
                (Expr::Hook(Hook::ImperativeHandle, hargs), Ty::Void)
            }
            Hook::Context => {
                let args = self.arg_exprs(&c.arguments, &[]);
                let (ctx, ct) = args
                    .into_iter()
                    .next()
                    .unwrap_or((Expr::Undefined, Ty::Unknown));
                let t = match ct {
                    Ty::Context(t) => *t,
                    _ => Ty::Unknown,
                };
                // A context read cannot be tracked by frame identity.
                (Expr::Hook(Hook::Context, vec![ctx]), t)
            }
            Hook::Id => {
                let _ = want;
                (Expr::Hook(Hook::Id, vec![]), Ty::String)
            }
            Hook::SyncExternalStore => {
                let unsubscribe = Ty::Function(vec![], Box::new(Ty::Void));
                let subscribe = Ty::Function(
                    vec![Ty::Function(vec![], Box::new(Ty::Void))],
                    Box::new(unsubscribe),
                );
                let args = self.arg_exprs(&c.arguments, &[subscribe]);
                let t = match args.get(1).map(|(_, t)| non_null_keep(t)) {
                    Some(Ty::Function(_, r)) => *r,
                    _ => {
                        self.err(c.span, "`useSyncExternalStore` needs a snapshot function");
                        Ty::Unknown
                    }
                };
                let exprs = args.into_iter().take(2).map(|(e, _)| e).collect();
                (Expr::Hook(Hook::SyncExternalStore, exprs), t)
            }
        }
    }

    fn method_call(
        &mut self,
        recv: Expr,
        rt: Ty,
        name: &str,
        optional: bool,
        c: &'a ast::CallExpression<'a>,
        want: Option<&Ty>,
    ) -> Lowered {
        let base = non_null(&rt);
        // A function-valued property: `props.onToggle(id)`.
        if let Ty::Object(_) = &base {
            if let Some(ft) = property(&base, name) {
                let callee = Expr::Member(Box::new(recv), name.to_owned(), optional);
                return self.value_call(callee, ft, c, want);
            }
        }
        let is_arr = types::is_array(&base);
        let is_str = types::is_stringy(&base);
        let elem = element(&base).unwrap_or(Ty::Unknown);
        let arr = || Ty::Array(Box::new(elem.clone()));
        let num = Ty::Number;
        let s = Ty::String;
        use Method as M;
        let cb = |ret: Ty| Ty::Function(vec![elem.clone(), Ty::Number, arr()], Box::new(ret));
        let (m, want_args, ret): (Method, Vec<Ty>, Ty) = if is_arr {
            match name {
                "map" => {
                    let args = self.arg_exprs(&c.arguments, &[cb(Ty::Unknown)]);
                    let r = match args.first().map(|a| &a.1) {
                        Some(Ty::Function(_, r)) => (**r).clone(),
                        _ => Ty::Unknown,
                    };
                    let args = args.into_iter().map(|(x, _)| ArrayItem::Item(x)).collect();
                    return (
                        Expr::Method {
                            recv: Box::new(recv),
                            method: M::ArrayMap,
                            args,
                            optional,
                        },
                        Ty::Array(Box::new(r)),
                    );
                }
                "flatMap" => {
                    let args = self.arg_exprs(&c.arguments, &[cb(Ty::Unknown)]);
                    let r = match args.first().map(|a| &a.1) {
                        Some(Ty::Function(_, r)) => element(r).unwrap_or((**r).clone()),
                        _ => Ty::Unknown,
                    };
                    let args = args.into_iter().map(|(x, _)| ArrayItem::Item(x)).collect();
                    return (
                        Expr::Method {
                            recv: Box::new(recv),
                            method: M::ArrayFlatMap,
                            args,
                            optional,
                        },
                        Ty::Array(Box::new(r)),
                    );
                }
                "reduce" => {
                    // The accumulator is typed by the initial value.
                    let init = c.arguments.get(1).and_then(|a| a.as_expression());
                    let (init_x, init_t) = match init {
                        Some(e) => {
                            let (x, t) = self.expr(e, None);
                            (Some(x), widen(&t))
                        }
                        None => (None, elem.clone()),
                    };
                    let acc = match init {
                        Some(E::ArrayExpression(a)) if a.elements.is_empty() => {
                            // `[]` alone says nothing: type from `want`.
                            want.cloned().unwrap_or(init_t)
                        }
                        _ => init_t,
                    };
                    let fw = Ty::Function(
                        vec![acc.clone(), elem.clone(), Ty::Number],
                        Box::new(acc.clone()),
                    );
                    let f = c.arguments.first().and_then(|a| a.as_expression());
                    let (fx, _) = match f {
                        Some(f) => self.expr(f, Some(&fw)),
                        None => (Expr::Undefined, Ty::Unknown),
                    };
                    let mut args = vec![ArrayItem::Item(fx)];
                    if let Some(i) = init_x {
                        args.push(ArrayItem::Item(i));
                    }
                    return (
                        Expr::Method {
                            recv: Box::new(recv),
                            method: M::ArrayReduce,
                            args,
                            optional,
                        },
                        acc,
                    );
                }
                "filter" => (M::ArrayFilter, vec![cb(Ty::Boolean)], arr()),
                "find" => (
                    M::ArrayFind,
                    vec![cb(Ty::Boolean)],
                    union(elem.clone(), Ty::Undefined),
                ),
                "findLast" => (
                    M::ArrayFindLast,
                    vec![cb(Ty::Boolean)],
                    union(elem.clone(), Ty::Undefined),
                ),
                "findIndex" => (M::ArrayFindIndex, vec![cb(Ty::Boolean)], num),
                "some" => (M::ArraySome, vec![cb(Ty::Boolean)], Ty::Boolean),
                "every" => (M::ArrayEvery, vec![cb(Ty::Boolean)], Ty::Boolean),
                "forEach" => (M::ArrayForEach, vec![cb(Ty::Void)], Ty::Void),
                "slice" => (M::ArraySlice, vec![num.clone(), num], arr()),
                "concat" => (M::ArrayConcat, vec![], arr()),
                "includes" => (M::ArrayIncludes, vec![elem.clone()], Ty::Boolean),
                "indexOf" => (M::ArrayIndexOf, vec![elem.clone()], num),
                "join" => (M::ArrayJoin, vec![s.clone()], s),
                "sort" | "toSorted" => {
                    if name == "sort" {
                        self.note_mutation(&recv);
                    }
                    (
                        if name == "sort" {
                            M::ArraySort
                        } else {
                            M::ArrayToSorted
                        },
                        vec![Ty::Function(
                            vec![elem.clone(), elem.clone()],
                            Box::new(num),
                        )],
                        arr(),
                    )
                }
                "reverse" => {
                    self.note_mutation(&recv);
                    (M::ArrayReverse, vec![], arr())
                }
                "toReversed" => (M::ArrayToReversed, vec![], arr()),
                "push" => {
                    self.note_mutation(&recv);
                    (
                        M::ArrayPush,
                        vec![elem.clone(), elem.clone(), elem.clone()],
                        num,
                    )
                }
                "unshift" => {
                    self.note_mutation(&recv);
                    (M::ArrayUnshift, vec![elem.clone(), elem.clone()], num)
                }
                "pop" => {
                    self.note_mutation(&recv);
                    (M::ArrayPop, vec![], union(elem.clone(), Ty::Undefined))
                }
                "shift" => {
                    self.note_mutation(&recv);
                    (M::ArrayShift, vec![], union(elem.clone(), Ty::Undefined))
                }
                "splice" => {
                    self.note_mutation(&recv);
                    (
                        M::ArraySplice,
                        vec![num.clone(), num, elem.clone(), elem.clone()],
                        arr(),
                    )
                }
                "fill" => {
                    self.note_mutation(&recv);
                    (M::ArrayFill, vec![elem.clone()], arr())
                }
                "flat" => (
                    M::ArrayFlat,
                    vec![],
                    Ty::Array(Box::new(element(&elem).unwrap_or(elem.clone()))),
                ),
                "at" => (M::ArrayAt, vec![num], union(elem.clone(), Ty::Undefined)),
                "keys" => (M::ArrayKeys, vec![], Ty::Array(Box::new(num))),
                "entries" => (
                    M::ArrayEntries,
                    vec![],
                    Ty::Array(Box::new(Ty::Tuple(vec![num, elem.clone()]))),
                ),
                "with" => (M::ArrayWith, vec![num, elem.clone()], arr()),
                "toString" => (M::ToString, vec![], s),
                _ => return self.dynamic_invoke(recv, name, optional, c),
            }
        } else if is_str {
            match name {
                "trim" => (M::StrTrim, vec![], s),
                "trimStart" => (M::StrTrimStart, vec![], s),
                "trimEnd" => (M::StrTrimEnd, vec![], s),
                "toUpperCase" | "toLocaleUpperCase" => (M::StrToUpperCase, vec![], s),
                "toLowerCase" | "toLocaleLowerCase" => (M::StrToLowerCase, vec![], s),
                "includes" => (M::StrIncludes, vec![s], Ty::Boolean),
                "startsWith" => (M::StrStartsWith, vec![s], Ty::Boolean),
                "endsWith" => (M::StrEndsWith, vec![s], Ty::Boolean),
                "indexOf" => (M::StrIndexOf, vec![s], num),
                "lastIndexOf" => (M::StrLastIndexOf, vec![s], num),
                "slice" => (M::StrSlice, vec![num.clone(), num], s),
                "substring" => (M::StrSubstring, vec![num.clone(), num], s),
                "split" => (M::StrSplit, vec![s], Ty::Array(Box::new(Ty::String))),
                "replace" | "replaceAll" => {
                    // The replacement: a string or a replacer (match first).
                    let replacer = union(
                        s.clone(),
                        Ty::Function(vec![s.clone()], Box::new(s.clone())),
                    );
                    (
                        if name == "replace" {
                            M::StrReplace
                        } else {
                            M::StrReplaceAll
                        },
                        vec![union(s.clone(), Ty::Regex), replacer],
                        s,
                    )
                }
                "repeat" => (M::StrRepeat, vec![num], s),
                "padStart" => (M::StrPadStart, vec![num, s.clone()], s),
                "padEnd" => (M::StrPadEnd, vec![num, s.clone()], s),
                "charAt" => (M::StrCharAt, vec![num], s),
                "charCodeAt" => (M::StrCharCodeAt, vec![num.clone()], num),
                "codePointAt" => (
                    M::StrCodePointAt,
                    vec![num.clone()],
                    union(num, Ty::Undefined),
                ),
                "at" => (M::StrAt, vec![num], union(s, Ty::Undefined)),
                "localeCompare" => (M::StrLocaleCompare, vec![s], num),
                "concat" => (M::StrConcat, vec![], s),
                "match" => (
                    M::StrMatch,
                    vec![Ty::Regex],
                    union(
                        Ty::Array(Box::new(union(Ty::String, Ty::Undefined))),
                        Ty::Null,
                    ),
                ),
                "search" => (M::StrSearch, vec![Ty::Regex], num),
                "toString" => (M::ToString, vec![], s),
                _ => return self.dynamic_invoke(recv, name, optional, c),
            }
        } else {
            match (&base, name) {
                (Ty::Number | Ty::NumLit(_), "toFixed") => (M::NumToFixed, vec![num], s),
                (Ty::Number | Ty::NumLit(_), "toString") => (M::NumToString, vec![num], s),
                (Ty::Boolean, "toString") => (M::ToString, vec![], s),
                (Ty::Promise(t), "then") => {
                    let args = self.arg_exprs(
                        &c.arguments,
                        &[
                            Ty::Function(vec![(**t).clone()], Box::new(Ty::Unknown)),
                            Ty::Function(vec![Ty::Unknown], Box::new(Ty::Unknown)),
                        ],
                    );
                    let r = match args.first().map(|a| &a.1) {
                        Some(Ty::Function(_, r)) => match &**r {
                            Ty::Promise(inner) => (**inner).clone(),
                            other => other.clone(),
                        },
                        _ => Ty::Unknown,
                    };
                    let args = args.into_iter().map(|(x, _)| ArrayItem::Item(x)).collect();
                    return (
                        Expr::Method {
                            recv: Box::new(recv),
                            method: M::PromiseThen,
                            args,
                            optional,
                        },
                        Ty::Promise(Box::new(r)),
                    );
                }
                (Ty::Promise(t), "catch") => (
                    M::PromiseCatch,
                    vec![Ty::Function(vec![Ty::String], Box::new(Ty::Unknown))],
                    Ty::Promise(t.clone()),
                ),
                (Ty::Promise(t), "finally") => (
                    M::PromiseFinally,
                    vec![Ty::Function(vec![], Box::new(Ty::Void))],
                    Ty::Promise(t.clone()),
                ),
                (Ty::Response | Ty::CwResponse, "json") => {
                    // `json<T>()` promises a T, as TypeScript's typings let it.
                    let t = match c.type_arguments.as_ref().and_then(|a| a.params.first()) {
                        Some(t) => self.ts_type(t),
                        None => Ty::Unknown,
                    };
                    (M::ResponseJson, vec![], Ty::Promise(Box::new(t)))
                }
                (Ty::Response | Ty::CwResponse, "text") => {
                    (M::ResponseText, vec![], Ty::Promise(Box::new(Ty::String)))
                }
                (Ty::Headers, "get") => {
                    (M::HeadersGet, vec![Ty::String], union(Ty::String, Ty::Null))
                }
                (Ty::Headers, "has") => (M::HeadersHas, vec![Ty::String], Ty::Boolean),
                (Ty::Regex, "test") => (M::RegexTest, vec![s.clone()], Ty::Boolean),
                (Ty::Regex, "exec") => (
                    M::RegexExec,
                    vec![s.clone()],
                    union(
                        Ty::Array(Box::new(union(Ty::String, Ty::Undefined))),
                        Ty::Null,
                    ),
                ),
                (
                    Ty::Set(e),
                    m @ ("has" | "add" | "delete" | "clear" | "forEach" | "values" | "keys"
                    | "entries"),
                ) => {
                    if matches!(m, "add" | "delete" | "clear") {
                        self.note_mutation(&recv);
                    }
                    let e = (**e).clone();
                    match m {
                        "has" => (M::SetHas, vec![e], Ty::Boolean),
                        "add" => (M::SetAdd, vec![e.clone()], Ty::Set(Box::new(e))),
                        "delete" => (M::SetDelete, vec![e], Ty::Boolean),
                        "clear" => (M::SetClear, vec![], Ty::Void),
                        "forEach" => (
                            M::CollectionForEach,
                            vec![Ty::Function(vec![e.clone(), e], Box::new(Ty::Void))],
                            Ty::Void,
                        ),
                        "entries" => (
                            M::CollectionEntries,
                            vec![],
                            Ty::Array(Box::new(Ty::Tuple(vec![e.clone(), e]))),
                        ),
                        _ => (M::CollectionValues, vec![], Ty::Array(Box::new(e))),
                    }
                }
                (
                    Ty::Map(k, v),
                    m @ ("get" | "set" | "has" | "delete" | "clear" | "forEach" | "values" | "keys"
                    | "entries"),
                ) => {
                    if matches!(m, "set" | "delete" | "clear") {
                        self.note_mutation(&recv);
                    }
                    let (k, v) = ((**k).clone(), (**v).clone());
                    match m {
                        "get" => (M::MapGet, vec![k], union(v, Ty::Undefined)),
                        "set" => (
                            M::MapSet,
                            vec![k.clone(), v.clone()],
                            Ty::Map(Box::new(k), Box::new(v)),
                        ),
                        "has" => (M::SetHas, vec![k], Ty::Boolean),
                        "delete" => (M::SetDelete, vec![k], Ty::Boolean),
                        "clear" => (M::SetClear, vec![], Ty::Void),
                        "forEach" => (
                            M::CollectionForEach,
                            vec![Ty::Function(vec![v, k], Box::new(Ty::Void))],
                            Ty::Void,
                        ),
                        "keys" => (M::CollectionKeys, vec![], Ty::Array(Box::new(k))),
                        "values" => (M::CollectionValues, vec![], Ty::Array(Box::new(v))),
                        _ => (
                            M::CollectionEntries,
                            vec![],
                            Ty::Array(Box::new(Ty::Tuple(vec![k, v]))),
                        ),
                    }
                }
                (Ty::DomNode, "focus") => (M::NodeFocus, vec![], Ty::Void),
                (Ty::DomNode, "blur") => (M::NodeBlur, vec![], Ty::Void),
                (Ty::DomNode, "select") => (M::NodeSelect, vec![], Ty::Void),
                (Ty::DomNode, "setSelectionRange") => (
                    M::NodeSetSelectionRange,
                    vec![Ty::Number, Ty::Number],
                    Ty::Void,
                ),
                (Ty::DomNode, n)
                    if cw_ui::ir::method_by_name(cw_ui::ir::MethodKind::Node, n).is_some() =>
                {
                    let m = cw_ui::ir::method_by_name(cw_ui::ir::MethodKind::Node, n).unwrap();
                    let node_or_null = union(Ty::DomNode, Ty::Null);
                    let ret = match m {
                        M::NodeGetBoundingClientRect => dom_rect(),
                        M::NodeGetClientRects => Ty::Array(Box::new(dom_rect())),
                        M::NodeContains | M::NodeMatches | M::NodeHasAttribute => Ty::Boolean,
                        M::NodeClosest | M::NodeQuerySelector => node_or_null,
                        M::NodeQuerySelectorAll => Ty::Array(Box::new(Ty::DomNode)),
                        M::NodeGetAttribute => union(Ty::String, Ty::Null),
                        M::ToString => Ty::String,
                        _ => Ty::Void,
                    };
                    if matches!(m, M::NodeGetBoundingClientRect | M::NodeGetClientRects) {
                        self.mark_always();
                    }
                    (m, vec![], ret)
                }
                (Ty::Date, n)
                    if cw_ui::ir::method_by_name(cw_ui::ir::MethodKind::Date, n).is_some() =>
                {
                    let m = cw_ui::ir::method_by_name(cw_ui::ir::MethodKind::Date, n).unwrap();
                    let ret = match m {
                        M::DateToISOString
                        | M::DateToString
                        | M::DateToDateString
                        | M::DateToTimeString
                        | M::DateToUTCString
                        | M::ToString => Ty::String,
                        M::DateToJSON => union(Ty::String, Ty::Null),
                        _ => Ty::Number,
                    };
                    if matches!(
                        m,
                        M::DateSetFullYear
                            | M::DateSetMonth
                            | M::DateSetDate
                            | M::DateSetHours
                            | M::DateSetMinutes
                            | M::DateSetSeconds
                            | M::DateSetMilliseconds
                            | M::DateSetTime
                    ) {
                        self.note_mutation(&recv);
                    }
                    (m, vec![], ret)
                }
                (Ty::Event, "preventDefault") => (M::EventPreventDefault, vec![], Ty::Void),
                (Ty::Event, "stopPropagation") => (M::EventStopPropagation, vec![], Ty::Void),
                // A host object's method the runtime does not model.
                (t, _) if is_host_type(t) => {
                    self.err(
                        c.span,
                        format!(
                            "method `{name}` on type {} is outside the compiled subset",
                            types::show(t)
                        ),
                    );
                    return (Expr::Undefined, Ty::Unknown);
                }
                // A receiver whose type does not say which method this is.
                _ => return self.dynamic_invoke(recv, name, optional, c),
            }
        };
        let (args, _) = self.exprs_args(&c.arguments, &want_args);
        (
            Expr::Method {
                recv: Box::new(recv),
                method: m,
                args,
                optional,
            },
            ret,
        )
    }

    /// `recv.name(args)` looked up when it runs (`Expr::Invoke`).
    fn dynamic_invoke(
        &mut self,
        recv: Expr,
        name: &str,
        optional: bool,
        c: &'a ast::CallExpression<'a>,
    ) -> Lowered {
        if cw_ui::ir::unimplemented_builtin(name) {
            self.err(
                c.span,
                format!("built-in method `{name}` is outside the compiled subset"),
            );
            return (Expr::Undefined, Ty::Unknown);
        }
        if matches!(
            name,
            "push"
                | "pop"
                | "shift"
                | "unshift"
                | "splice"
                | "sort"
                | "reverse"
                | "fill"
                | "copyWithin"
                | "set"
                | "add"
                | "delete"
                | "clear"
        ) {
            self.note_mutation(&recv);
        }
        let (args, _) = self.exprs_args(&c.arguments, &[]);
        (
            Expr::Invoke {
                recv: Box::new(recv),
                name: name.to_owned(),
                args,
                optional,
            },
            Ty::Unknown,
        )
    }

    // ------------------------------------------------------------------ JSX

    fn jsx_element(&mut self, el: &'a ast::JSXElement<'a>) -> Lowered {
        let name = &el.opening_element.name;
        match self.jsx_kind(name) {
            JsxKind::Host(tag) => {
                let mut tb = TemplateBuilder {
                    holes: Vec::new(),
                    meta: Vec::new(),
                };
                let mut key = None;
                let node = self.host_node(el, &tag, &mut tb, Some(&mut key));
                let tid = self.templates.len() as u32;
                self.templates.push(Template {
                    root: node,
                    holes: tb.meta,
                });
                (
                    Expr::Element(Box::new(ElementExpr::Template {
                        template: tid,
                        holes: tb.holes,
                        key,
                    })),
                    Ty::Node,
                )
            }
            JsxKind::Fragment => {
                let mut key = None;
                for a in &el.opening_element.attributes {
                    if let ast::JSXAttributeItem::Attribute(a) = a {
                        if jsx_attr_name(&a.name) == "key" {
                            key = self.jsx_attr_value(a, None).map(|x| x.0);
                        } else {
                            self.err(a.span, "a Fragment takes only `key`");
                        }
                    }
                }
                let children = self.jsx_children_exprs(&el.children, false);
                (
                    Expr::Element(Box::new(ElementExpr::Fragment { children, key })),
                    Ty::Node,
                )
            }
            JsxKind::Suspense => {
                // Nothing compiled suspends, so Suspense renders its children; the
                // fallback is lowered (it is code) but never shown.
                let mut key = None;
                for a in &el.opening_element.attributes {
                    if let ast::JSXAttributeItem::Attribute(a) = a {
                        match jsx_attr_name(&a.name).as_str() {
                            "key" => key = self.jsx_attr_value(a, None).map(|x| x.0),
                            _ => {
                                let _ = self.jsx_attr_value(a, None);
                            }
                        }
                    }
                }
                let children = self.jsx_children_exprs(&el.children, false);
                (
                    Expr::Element(Box::new(ElementExpr::Fragment { children, key })),
                    Ty::Node,
                )
            }
            JsxKind::Provider(ctx, vt) => {
                let mut key = None;
                let mut value = None;
                for a in &el.opening_element.attributes {
                    match a {
                        ast::JSXAttributeItem::Attribute(a) => {
                            match jsx_attr_name(&a.name).as_str() {
                                "key" => key = self.jsx_attr_value(a, None).map(|x| x.0),
                                "value" => value = self.jsx_attr_value(a, Some(&vt)).map(|x| x.0),
                                _ => self.err(a.span, "a Provider takes only `value` and `key`"),
                            }
                        }
                        ast::JSXAttributeItem::SpreadAttribute(s) => {
                            self.err(
                                s.span,
                                "spread props on a Provider are outside the compiled subset",
                            );
                        }
                    }
                }
                let children = self.jsx_children_exprs(&el.children, false);
                (
                    Expr::Element(Box::new(ElementExpr::Provider {
                        context: ctx,
                        value: value.unwrap_or(Expr::Undefined),
                        children,
                        key,
                    })),
                    Ty::Node,
                )
            }
            JsxKind::Component(callee, props_ty) => {
                let mut props = Vec::new();
                let mut key = None;
                for a in &el.opening_element.attributes {
                    match a {
                        ast::JSXAttributeItem::Attribute(a) => {
                            let name = jsx_attr_name(&a.name);
                            if name == "key" {
                                key = self.jsx_attr_value(a, None).map(|x| x.0);
                                continue;
                            }
                            // `ref` goes to the element, not the props: a `forwardRef`
                            // component receives it as its second argument.
                            let want = props_ty.as_ref().and_then(|t| property(t, &name));
                            if let Some((x, _)) = self.jsx_attr_value(a, want.as_ref()) {
                                props.push(Prop::KeyValue(name, x));
                            }
                        }
                        ast::JSXAttributeItem::SpreadAttribute(s) => {
                            let (x, _) = self.expr(&s.argument, props_ty.as_ref());
                            props.push(Prop::Spread(x));
                        }
                    }
                }
                // A component's children are whatever it makes of them (a render
                // function, an object): only host children must be renderable.
                let mut kids = self.jsx_children_exprs(&el.children, true);
                let children = match kids.len() {
                    0 => None,
                    1 => kids.pop(),
                    _ => Some(Expr::Array(kids.into_iter().map(ArrayItem::Item).collect())),
                };
                (
                    Expr::Element(Box::new(ElementExpr::Component {
                        callee,
                        props,
                        children,
                        key,
                    })),
                    Ty::Node,
                )
            }
            JsxKind::Invalid => (Expr::Undefined, Ty::Unknown),
        }
    }

    fn jsx_kind(&mut self, name: &'a ast::JSXElementName<'a>) -> JsxKind {
        match name {
            ast::JSXElementName::Identifier(id) => JsxKind::Host(id.name.to_string()),
            ast::JSXElementName::IdentifierReference(id) => {
                let n = id.name.as_str();
                if self.resolve_is_free(n) {
                    match self.react.get(n) {
                        Some(ReactName::Fragment | ReactName::StrictMode) => {
                            return JsxKind::Fragment
                        }
                        Some(ReactName::Suspense) => return JsxKind::Suspense,
                        Some(_) => {
                            self.err(id.span, format!("`<{n}>` is outside the compiled subset"));
                            return JsxKind::Invalid;
                        }
                        None => {}
                    }
                }
                let (callee, ty) = self.identifier(n, id.span);
                let props = match &ty {
                    Ty::Function(ps, _) => ps.first().cloned(),
                    _ => None,
                };
                JsxKind::Component(callee, props.filter(|p| !matches!(p, Ty::Unknown)))
            }
            ast::JSXElementName::MemberExpression(m) => {
                let prop = m.property.name.as_str();
                if let ast::JSXMemberExpressionObject::IdentifierReference(obj) = &m.object {
                    let on = obj.name.as_str();
                    if self.resolve_is_free(on) && self.react.get(on) == Some(&ReactName::ReactNs) {
                        return match prop {
                            "Fragment" | "StrictMode" => JsxKind::Fragment,
                            "Suspense" => JsxKind::Suspense,
                            _ => {
                                self.err(
                                    m.span,
                                    format!("`<React.{prop}>` is outside the compiled subset"),
                                );
                                JsxKind::Invalid
                            }
                        };
                    }
                    if prop == "Provider" {
                        let (ctx, ct) = self.identifier(on, obj.span);
                        return match ct {
                            Ty::Context(t) => JsxKind::Provider(ctx, *t),
                            other => {
                                self.err(
                                    m.span,
                                    format!(
                                        "`<{on}.Provider>` on a value of type {}",
                                        types::show(&other)
                                    ),
                                );
                                JsxKind::Invalid
                            }
                        };
                    }
                }
                // `<Menu.Item>`, `<ns.Comp>`: a component reached through members.
                match self.jsx_member_callee(m) {
                    Some(callee) => JsxKind::Component(callee, None),
                    None => {
                        self.err(m.span, "this element name is outside the compiled subset");
                        JsxKind::Invalid
                    }
                }
            }
            ast::JSXElementName::NamespacedName(n) => {
                self.err(
                    n.span,
                    "namespaced element names are outside the compiled subset",
                );
                JsxKind::Invalid
            }
            ast::JSXElementName::ThisExpression(t) => {
                self.err(t.span, "`this` is outside the compiled subset");
                JsxKind::Invalid
            }
        }
    }

    /// The value a member element name (`A.B.C`) reads.
    fn jsx_member_callee(&mut self, m: &'a ast::JSXMemberExpression<'a>) -> Option<Expr> {
        let object = match &m.object {
            ast::JSXMemberExpressionObject::IdentifierReference(id) => {
                self.identifier(id.name.as_str(), id.span).0
            }
            ast::JSXMemberExpressionObject::MemberExpression(inner) => {
                self.jsx_member_callee(inner)?
            }
            ast::JSXMemberExpressionObject::ThisExpression(_) => return None,
        };
        Some(Expr::Member(
            Box::new(object),
            m.property.name.to_string(),
            false,
        ))
    }

    /// An attribute's value: a string, an expression, or `true` when absent.
    fn jsx_attr_value(
        &mut self,
        a: &'a ast::JSXAttribute<'a>,
        want: Option<&Ty>,
    ) -> Option<Lowered> {
        match &a.value {
            None => Some((Expr::Bool(true), Ty::Boolean)),
            Some(ast::JSXAttributeValue::StringLiteral(s)) => {
                let v = decode_entities(s.value.as_str());
                Some((Expr::Str(v.clone()), Ty::Lit(v)))
            }
            Some(ast::JSXAttributeValue::ExpressionContainer(c)) => match &c.expression {
                ast::JSXExpression::EmptyExpression(e) => {
                    self.err(e.span, "an empty attribute expression");
                    None
                }
                other => Some(self.expr(other.as_expression().expect("expression"), want)),
            },
            Some(ast::JSXAttributeValue::Element(e)) => Some(self.jsx_element(e)),
            Some(ast::JSXAttributeValue::Fragment(f)) => {
                let children = self.jsx_children_exprs(&f.children, false);
                Some((
                    Expr::Element(Box::new(ElementExpr::Fragment {
                        children,
                        key: None,
                    })),
                    Ty::Node,
                ))
            }
        }
    }

    /// Children as a list of expressions (for components, fragments and providers).
    fn jsx_children_exprs(
        &mut self,
        children: &'a oxc_allocator::Vec<'a, ast::JSXChild<'a>>,
        of_component: bool,
    ) -> Vec<Expr> {
        let mut out = Vec::new();
        for c in children {
            match c {
                ast::JSXChild::Text(t) => {
                    if let Some(s) = clean_jsx_text(t.value.as_str()) {
                        out.push(Expr::Str(s));
                    }
                }
                ast::JSXChild::Element(e) => out.push(self.jsx_element(e).0),
                ast::JSXChild::Fragment(f) => {
                    let children = self.jsx_children_exprs(&f.children, false);
                    out.push(Expr::Element(Box::new(ElementExpr::Fragment {
                        children,
                        key: None,
                    })));
                }
                ast::JSXChild::ExpressionContainer(ec) => match &ec.expression {
                    ast::JSXExpression::EmptyExpression(_) => {}
                    other => {
                        let e = other.as_expression().expect("expression");
                        let (x, t) =
                            self.expr(e, if of_component { None } else { Some(&Ty::Node) });
                        if !of_component {
                            self.check_renderable(&t, e.span());
                        }
                        out.push(x);
                    }
                },
                ast::JSXChild::Spread(s) => {
                    self.err(s.span, "spread children are outside the compiled subset");
                }
            }
        }
        out
    }

    fn check_renderable(&mut self, t: &Ty, span: Span) {
        if !matches!(t, Ty::Unknown) && !types::is_renderable(t) {
            self.err(
                span,
                format!("a value of type {} cannot be rendered", types::show(t)),
            );
        }
    }

    /// Lowers a host element into template nodes, adding holes to `tb`.
    fn host_node(
        &mut self,
        el: &'a ast::JSXElement<'a>,
        tag: &str,
        tb: &mut TemplateBuilder,
        key_out: Option<&mut Option<Expr>>,
    ) -> TNode {
        let mut attrs = Vec::new();
        let mut key = None;
        for a in &el.opening_element.attributes {
            match a {
                ast::JSXAttributeItem::Attribute(a) => {
                    let name = jsx_attr_name(&a.name);
                    if name == "key" {
                        key = self.jsx_attr_value(a, None).map(|x| x.0);
                        continue;
                    }
                    if name == "dangerouslySetInnerHTML" {
                        self.err(
                            a.span,
                            "`dangerouslySetInnerHTML` is outside the compiled subset",
                        );
                        continue;
                    }
                    if name == "ref" {
                        let hole = self.hole(tb, |l| {
                            l.jsx_attr_value(a, None)
                                .unwrap_or((Expr::Undefined, Ty::Unknown))
                        });
                        attrs.push(TAttr::Ref(hole));
                        continue;
                    }
                    // A string that React writes as-is: a static attribute.
                    if let Some(ast::JSXAttributeValue::StringLiteral(s)) = &a.value {
                        if let Some(dom) = static_attr_name(&name) {
                            attrs.push(TAttr::Static(dom, decode_entities(s.value.as_str())));
                            continue;
                        }
                    }
                    let want = prop_want(&name);
                    let hole = self.hole(tb, |l| {
                        l.jsx_attr_value(a, want.as_ref())
                            .unwrap_or((Expr::Undefined, Ty::Unknown))
                    });
                    attrs.push(TAttr::Dynamic(name, hole));
                }
                ast::JSXAttributeItem::SpreadAttribute(s) => {
                    let hole = self.hole(tb, |l| l.expr(&s.argument, None));
                    attrs.push(TAttr::Spread(hole));
                }
            }
        }
        match key_out {
            Some(k) => *k = key,
            None => {
                // A key below the root of a template changes nothing React renders.
            }
        }
        let mut children = Vec::new();
        for c in &el.children {
            match c {
                ast::JSXChild::Text(t) => {
                    if let Some(s) = clean_jsx_text(t.value.as_str()) {
                        children.push(TNode::Text(s));
                    }
                }
                ast::JSXChild::Element(e) => match self.jsx_kind(&e.opening_element.name) {
                    JsxKind::Host(t) => children.push(self.host_node(e, &t, tb, None)),
                    _ => {
                        let hole = self.hole(tb, |l| l.jsx_element(e));
                        children.push(TNode::Hole(hole));
                    }
                },
                ast::JSXChild::Fragment(f) => {
                    let hole = self.hole(tb, |l| {
                        let children = l.jsx_children_exprs(&f.children, false);
                        (
                            Expr::Element(Box::new(ElementExpr::Fragment {
                                children,
                                key: None,
                            })),
                            Ty::Node,
                        )
                    });
                    children.push(TNode::Hole(hole));
                }
                ast::JSXChild::ExpressionContainer(ec) => match &ec.expression {
                    ast::JSXExpression::EmptyExpression(_) => {}
                    ast::JSXExpression::StringLiteral(s) => {
                        if !s.value.is_empty() {
                            children.push(TNode::Text(s.value.to_string()));
                        }
                    }
                    other => {
                        let e = other.as_expression().expect("expression");
                        let hole = self.hole(tb, |l| {
                            let (x, t) = l.expr(e, Some(&Ty::Node));
                            l.check_renderable(&t, e.span());
                            (x, t)
                        });
                        children.push(TNode::Hole(hole));
                    }
                },
                ast::JSXChild::Spread(s) => {
                    self.err(s.span, "spread children are outside the compiled subset");
                }
            }
        }
        if matches!(tag, "textarea") && !children.is_empty() {
            self.err(
                el.span,
                "`<textarea>` children: use `value` or `defaultValue`",
            );
        }
        TNode::Element {
            tag: tag.to_owned(),
            attrs,
            children,
        }
    }

    /// Lowers one hole expression and records what it depends on.
    fn hole(&mut self, tb: &mut TemplateBuilder, f: impl FnOnce(&mut Self) -> Lowered) -> u32 {
        self.hole_always.push(false);
        let (x, t) = f(self);
        let always = self.hole_always.pop().unwrap();
        let mut deps = BTreeSet::new();
        let mut hook = false;
        self.free_slots(&x, &mut deps, &mut hook);
        if hook {
            self.err(Span::default(), "a hook called inside JSX");
        }
        let idx = tb.holes.len() as u32;
        tb.holes.push(x);
        tb.meta.push(Hole {
            deps: deps
                .into_iter()
                .map(|(k, n)| {
                    if k == 0 {
                        Capture::Local(n)
                    } else {
                        Capture::Capture(n)
                    }
                })
                .collect(),
            always,
            ty: t,
        });
        // A nested hole leaves its parent hole's flag set too.
        if always {
            self.mark_always();
        }
        idx
    }

    /// The frame slots `x` reads: `(0, slot)` for locals, `(1, idx)` for captures.
    fn free_slots(&self, x: &Expr, out: &mut BTreeSet<(u8, u32)>, hook: &mut bool) {
        walk_expr(x, &mut |e| match e {
            Expr::Local(n) => {
                out.insert((0, *n));
            }
            Expr::Capture(n) => {
                out.insert((1, *n));
            }
            Expr::Closure(f) => {
                if let Some(Some(func)) = self.functions.get(*f as usize) {
                    for c in &func.captures {
                        match c {
                            Capture::Local(n) => out.insert((0, *n)),
                            Capture::Capture(n) => out.insert((1, *n)),
                        };
                    }
                }
            }
            Expr::Hook(..) => *hook = true,
            _ => {}
        });
    }
}

enum JsxKind {
    Host(String),
    Fragment,
    /// `<Suspense fallback>`: its children.
    Suspense,
    Provider(Expr, Ty),
    Component(Expr, Option<Ty>),
    Invalid,
}

enum PathStep {
    Key(String),
    Index(usize),
}

/// Every name a binding pattern binds, with the path from the bound value.
fn binding_names(p: &ast::BindingPattern<'_>, out: &mut Vec<(String, Vec<PathStep>)>) {
    fn go(
        p: &ast::BindingPattern<'_>,
        path: &mut Vec<(bool, String, usize)>,
        out: &mut Vec<(String, Vec<PathStep>)>,
    ) {
        match p {
            ast::BindingPattern::BindingIdentifier(id) => out.push((
                id.name.to_string(),
                path.iter()
                    .map(|(is_key, k, i)| {
                        if *is_key {
                            PathStep::Key(k.clone())
                        } else {
                            PathStep::Index(*i)
                        }
                    })
                    .collect(),
            )),
            ast::BindingPattern::ArrayPattern(a) => {
                for (i, el) in a.elements.iter().enumerate() {
                    if let Some(el) = el {
                        path.push((false, String::new(), i));
                        go(el, path, out);
                        path.pop();
                    }
                }
            }
            ast::BindingPattern::ObjectPattern(o) => {
                for prop in &o.properties {
                    if let Some(k) = property_key_name(&prop.key) {
                        path.push((true, k, 0));
                        go(&prop.value, path, out);
                        path.pop();
                    }
                }
            }
            ast::BindingPattern::AssignmentPattern(a) => go(&a.left, path, out),
        }
    }
    go(p, &mut Vec::new(), out)
}

fn property_key_name(k: &ast::PropertyKey<'_>) -> Option<String> {
    match k {
        ast::PropertyKey::StaticIdentifier(id) => Some(id.name.to_string()),
        ast::PropertyKey::StringLiteral(s) => Some(s.value.to_string()),
        ast::PropertyKey::NumericLiteral(n) => Some(crate::lower::num_key(n.value)),
        _ => None,
    }
}

pub(crate) fn num_key(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

fn type_name(n: &ast::TSTypeName<'_>) -> String {
    match n {
        ast::TSTypeName::IdentifierReference(id) => id.name.to_string(),
        ast::TSTypeName::QualifiedName(q) => format!("{}.{}", type_name(&q.left), q.right.name),
        ast::TSTypeName::ThisExpression(_) => "this".into(),
    }
}

/// Unwraps parentheses and TypeScript-only wrappers.
fn strip<'b, 'a>(e: &'b E<'a>) -> &'b E<'a> {
    match e {
        E::ParenthesizedExpression(p) => strip(&p.expression),
        E::TSAsExpression(a) => strip(&a.expression),
        E::TSSatisfiesExpression(a) => strip(&a.expression),
        E::TSNonNullExpression(a) => strip(&a.expression),
        E::TSTypeAssertion(a) => strip(&a.expression),
        E::TSInstantiationExpression(a) => strip(&a.expression),
        e => e,
    }
}

/// Whether an initialiser creates a new array or object this frame owns.
fn is_fresh_init(e: &E<'_>) -> bool {
    match strip(e) {
        E::ArrayExpression(_) | E::ObjectExpression(_) | E::NewExpression(_) => true,
        E::CallExpression(c) => match strip(&c.callee) {
            E::StaticMemberExpression(m) => matches!(
                m.property.name.as_str(),
                "map"
                    | "filter"
                    | "slice"
                    | "concat"
                    | "toSorted"
                    | "toReversed"
                    | "flat"
                    | "flatMap"
                    | "split"
                    | "keys"
                    | "values"
                    | "entries"
                    | "from"
            ),
            _ => false,
        },
        _ => false,
    }
}

/// A `DOMRect` (a plain object of its readable fields).
fn dom_rect() -> Ty {
    Ty::Object(
        [
            "x", "y", "width", "height", "top", "right", "bottom", "left",
        ]
        .iter()
        .map(|k| (k.to_string(), Ty::Number, false))
        .collect(),
    )
}

/// `localStorage` is area 0, `sessionStorage` area 1 (the Realm's numbering).
fn storage_area(ns: &str) -> f64 {
    if ns == "sessionStorage" {
        1.0
    } else {
        0.0
    }
}

/// `navigator`'s constant properties, as the page's prelude defines them.
fn navigator_constant(p: &str) -> Option<Expr> {
    Some(match p {
        "userAgent" => Expr::Str("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Computerworld/1.0".into()),
        "language" => Expr::Str("en-US".into()),
        "languages" => Expr::Array(vec![
            ArrayItem::Item(Expr::Str("en-US".into())),
            ArrayItem::Item(Expr::Str("en".into())),
        ]),
        "platform" => Expr::Str("Linux x86_64".into()),
        "onLine" | "cookieEnabled" => Expr::Bool(true),
        "webdriver" => Expr::Bool(false),
        "hardwareConcurrency" => Expr::Num(4.0),
        "maxTouchPoints" => Expr::Num(0.0),
        "deviceMemory" => Expr::Num(8.0),
        _ => return None,
    })
}

/// The built-in function `ns.name`, for using it as a value.
fn builtin_value(ns: &str, name: &str) -> Option<Builtin> {
    Some(match (ns, name) {
        ("Math", "max") => Builtin::MathMax,
        ("Math", "min") => Builtin::MathMin,
        ("Math", "round") => Builtin::MathRound,
        ("Math", "floor") => Builtin::MathFloor,
        ("Math", "ceil") => Builtin::MathCeil,
        ("Math", "abs") => Builtin::MathAbs,
        ("Math", "trunc") => Builtin::MathTrunc,
        ("Math", "sign") => Builtin::MathSign,
        ("Math", "sqrt") => Builtin::MathSqrt,
        ("Math", "pow") => Builtin::MathPow,
        ("Number", "isNaN") => Builtin::NumberIsNaN,
        ("Number", "isInteger") => Builtin::NumberIsInteger,
        ("Number", "isFinite") => Builtin::NumberIsFinite,
        ("Number", "parseInt") => Builtin::ParseInt,
        ("Number", "parseFloat") => Builtin::ParseFloat,
        ("Array", "isArray") => Builtin::ArrayIsArray,
        ("Object", "keys") => Builtin::ObjectKeys,
        ("Object", "values") => Builtin::ObjectValues,
        ("Object", "entries") => Builtin::ObjectEntries,
        ("JSON", "stringify") => Builtin::JsonStringify,
        ("console", "log" | "info" | "debug") => Builtin::ConsoleLog,
        ("console", "warn") => Builtin::ConsoleWarn,
        ("console", "error") => Builtin::ConsoleError,
        _ => return None,
    })
}

/// A value the runtime models as a host object with a fixed set of properties
/// (reading another one would not be what JavaScript reads).
fn is_host_type(t: &Ty) -> bool {
    matches!(
        t,
        Ty::Response
            | Ty::Date
            | Ty::CwResponse
            | Ty::Headers
            | Ty::Event
            | Ty::DomNode
            | Ty::Regex
            | Ty::Error
            | Ty::Set(_)
            | Ty::Map(_, _)
            | Ty::Promise(_)
            | Ty::Ref(_)
    )
}

/// The function type inside a (possibly union) expected type.
fn function_member(t: &Ty) -> Option<Ty> {
    match t {
        Ty::Function(..) => Some(t.clone()),
        Ty::Union(ts) => ts.iter().find_map(function_member),
        Ty::Setter(inner) => Some(Ty::Function(vec![(**inner).clone()], inner.clone())),
        Ty::Dispatch(a) => Some(Ty::Function(vec![(**a).clone()], Box::new(Ty::Void))),
        _ => None,
    }
}

fn non_null_keep(t: &Ty) -> Ty {
    match non_null(t) {
        Ty::Unknown => t.clone(),
        n => n,
    }
}

/// The falsy values a `&&` can yield from its left side.
fn falsy_part(t: &Ty) -> Ty {
    match t {
        Ty::Boolean => Ty::Boolean,
        Ty::Number | Ty::NumLit(_) => Ty::Number,
        Ty::String | Ty::Lit(_) => Ty::String,
        Ty::Null | Ty::Undefined | Ty::Void => t.clone(),
        Ty::Union(ts) => union_all(ts.iter().map(falsy_part)),
        _ => Ty::Unknown,
    }
}

fn jsx_attr_name(n: &ast::JSXAttributeName<'_>) -> String {
    match n {
        ast::JSXAttributeName::Identifier(id) => id.name.to_string(),
        ast::JSXAttributeName::NamespacedName(n) => format!("{}:{}", n.namespace.name, n.name.name),
    }
}

/// The expected type of a host element prop, for typing inline handlers.
fn prop_want(name: &str) -> Option<Ty> {
    if name.len() > 2 && name.starts_with("on") && name.as_bytes()[2].is_ascii_uppercase() {
        return Some(Ty::Function(vec![Ty::Event], Box::new(Ty::Void)));
    }
    match name {
        "style" => Some(Ty::Dict(Box::new(union(Ty::String, Ty::Number)))),
        _ => None,
    }
}

/// The DOM attribute a string-valued JSX prop becomes when React writes it with
/// `setAttribute` unchanged; `None` for props React treats specially.
pub fn static_attr_name(name: &str) -> Option<String> {
    if name.starts_with("on") && name.len() > 2 && name.as_bytes()[2].is_ascii_uppercase() {
        return None;
    }
    match name {
        "value"
        | "defaultValue"
        | "checked"
        | "defaultChecked"
        | "style"
        | "children"
        | "autoFocus"
        | "suppressContentEditableWarning"
        | "suppressHydrationWarning"
        | "selected"
        | "multiple"
        | "muted"
        | "innerHTML" => None,
        n => Some(cw_ui::dom_attr_name(n)),
    }
}

/// Babel's JSX text cleanup: lines trimmed, blank lines dropped, lines joined by a
/// space; `None` when nothing is left.
pub fn clean_jsx_text(raw: &str) -> Option<String> {
    let lines: Vec<&str> = raw
        .split(['\n'])
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    // Babel starts this at 0: a whitespace-only single line keeps its text.
    let last_non_empty = lines
        .iter()
        .rposition(|l| l.chars().any(|c| c != ' ' && c != '\t'))
        .unwrap_or(0);
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        let first = i == 0;
        let last = i == lines.len() - 1;
        let mut t: String = line.replace('\t', " ");
        if !first {
            t = t.trim_start_matches(' ').to_owned();
        }
        if !last {
            t = t.trim_end_matches(' ').to_owned();
        }
        if !t.is_empty() {
            if i != last_non_empty {
                t.push(' ');
            }
            out.push_str(&t);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(decode_entities(&out))
    }
}

/// HTML character references in JSX text and attribute strings.
pub fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[1..].find(';').map(|e| e + 1);
        let decoded = end.and_then(|e| {
            let name = &rest[1..e];
            let c = if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
                u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)
            } else if let Some(dec) = name.strip_prefix('#') {
                dec.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                match name {
                    "amp" => Some('&'),
                    "lt" => Some('<'),
                    "gt" => Some('>'),
                    "quot" => Some('"'),
                    "apos" => Some('\''),
                    "nbsp" => Some('\u{a0}'),
                    "copy" => Some('©'),
                    "reg" => Some('®'),
                    "hellip" => Some('…'),
                    "mdash" => Some('—'),
                    "ndash" => Some('–'),
                    "middot" => Some('·'),
                    "times" => Some('×'),
                    "rarr" => Some('→'),
                    "larr" => Some('←'),
                    "bull" => Some('•'),
                    "lsquo" => Some('‘'),
                    "rsquo" => Some('’'),
                    "ldquo" => Some('“'),
                    "rdquo" => Some('”'),
                    _ => None,
                }
            };
            c.map(|c| (c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Visits `x` and every expression inside it (not into other functions' bodies).
pub(crate) fn walk_expr(x: &Expr, f: &mut impl FnMut(&Expr)) {
    f(x);
    let items = |items: &Vec<ArrayItem>, f: &mut dyn FnMut(&Expr)| {
        for i in items {
            match i {
                ArrayItem::Item(e) | ArrayItem::Spread(e) => f(e),
            }
        }
    };
    match x {
        Expr::Template(_, es) | Expr::Seq(es) | Expr::Hook(_, es) => {
            for e in es {
                walk_expr(e, f);
            }
        }
        Expr::Array(xs) | Expr::Builtin(_, xs) => items(xs, &mut |e| walk_expr(e, f)),
        Expr::Object(ps) => {
            for p in ps {
                match p {
                    Prop::KeyValue(_, v) | Prop::Spread(v) => walk_expr(v, f),
                    Prop::Computed(k, v) => {
                        walk_expr(k, f);
                        walk_expr(v, f);
                    }
                }
            }
        }
        Expr::Member(o, _, _)
        | Expr::Unary(_, o)
        | Expr::TypeOf(o)
        | Expr::Chain(o)
        | Expr::Await(o) => walk_expr(o, f),
        Expr::Index(o, k, _) => {
            walk_expr(o, f);
            walk_expr(k, f);
        }
        Expr::Call(c, args, _) => {
            walk_expr(c, f);
            items(args, &mut |e| walk_expr(e, f));
        }
        Expr::Method { recv, args, .. } | Expr::Invoke { recv, args, .. } => {
            walk_expr(recv, f);
            items(args, &mut |e| walk_expr(e, f));
        }
        Expr::Binary(_, a, b) | Expr::Logical(_, a, b) => {
            walk_expr(a, f);
            walk_expr(b, f);
        }
        Expr::Cond(a, b, c) => {
            walk_expr(a, f);
            walk_expr(b, f);
            walk_expr(c, f);
        }
        Expr::Assign(lv, _, v) => {
            walk_lvalue(lv, f);
            walk_expr(v, f);
        }
        Expr::Update(lv, _, _) => walk_lvalue(lv, f),
        Expr::Element(el) => match &**el {
            ElementExpr::Template { holes, key, .. } => {
                for h in holes {
                    walk_expr(h, f);
                }
                if let Some(k) = key {
                    walk_expr(k, f);
                }
            }
            ElementExpr::Component {
                callee,
                props,
                children,
                key,
            } => {
                walk_expr(callee, f);
                for p in props {
                    match p {
                        Prop::KeyValue(_, v) | Prop::Spread(v) => walk_expr(v, f),
                        Prop::Computed(k, v) => {
                            walk_expr(k, f);
                            walk_expr(v, f);
                        }
                    }
                }
                if let Some(c) = children {
                    walk_expr(c, f);
                }
                if let Some(k) = key {
                    walk_expr(k, f);
                }
            }
            ElementExpr::Fragment { children, key } => {
                for c in children {
                    walk_expr(c, f);
                }
                if let Some(k) = key {
                    walk_expr(k, f);
                }
            }
            ElementExpr::Provider {
                context,
                value,
                children,
                key,
            } => {
                walk_expr(context, f);
                walk_expr(value, f);
                for c in children {
                    walk_expr(c, f);
                }
                if let Some(k) = key {
                    walk_expr(k, f);
                }
            }
        },
        _ => {}
    }
}

fn walk_lvalue(lv: &LValue, f: &mut impl FnMut(&Expr)) {
    match lv {
        LValue::Local(n) => f(&Expr::Local(*n)),
        LValue::Capture(n) => f(&Expr::Capture(*n)),
        LValue::Global(_) => {}
        LValue::Member(o, _) => walk_expr(o, f),
        LValue::Index(o, k) => {
            walk_expr(o, f);
            walk_expr(k, f);
        }
    }
}

/// The type an `as const` gives a literal expression: literal types, arrays as
/// tuples, objects member by member; `None` for anything else.
fn const_type(e: &E<'_>) -> Option<Ty> {
    match strip(e) {
        E::StringLiteral(s) => Some(Ty::Lit(s.value.to_string())),
        E::NumericLiteral(n) => Some(Ty::NumLit(n.value)),
        E::BooleanLiteral(_) => Some(Ty::Boolean),
        E::NullLiteral(_) => Some(Ty::Null),
        E::UnaryExpression(u) if u.operator == ast::UnaryOperator::UnaryNegation => {
            match strip(&u.argument) {
                E::NumericLiteral(n) => Some(Ty::NumLit(-n.value)),
                _ => None,
            }
        }
        E::ArrayExpression(a) => {
            let mut out = Vec::new();
            for el in &a.elements {
                out.push(const_type(el.as_expression()?)?);
            }
            Some(Ty::Tuple(out))
        }
        E::ObjectExpression(o) => {
            let mut fields = Vec::new();
            for p in &o.properties {
                let ast::ObjectPropertyKind::ObjectProperty(p) = p else {
                    return None;
                };
                if p.computed {
                    return None;
                }
                fields.push((property_key_name(&p.key)?, const_type(&p.value)?, false));
            }
            Some(Ty::Object(fields))
        }
        _ => None,
    }
}
