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
//! The parser is oxc (`oxc_parser`, with its transformer and code generator for the
//! fallback). It parses all of TypeScript and JSX, so the fallback exists for any
//! valid module, not only for the subset; it is pure Rust and builds for wasm32. See
//! docs/tsx-apps.md.

pub mod emit_js;
pub mod lower;
mod types;

use serde::{Deserialize, Serialize};

/// Something the compiler could not accept, at a 1-based line and column.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub line: u32,
    pub col: u32,
    pub message: String,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}:{}: {}", self.line, self.col, self.message)
    }
}

impl Diagnostic {
    /// A diagnostic at byte offset `offset` of `source`.
    pub fn at(source: &str, offset: u32, message: String) -> Diagnostic {
        let (line, col) = line_col(source, offset);
        Diagnostic { line, col, message }
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

/// Compiles `source` both ways.
pub fn build(source: &str, file_name: &str) -> Build {
    let (js, js_errors) = match emit_js::emit(source, file_name) {
        Ok(js) => (Some(js), Vec::new()),
        Err(e) => (None, e),
    };
    let (ir, mut diagnostics) = match lower::lower(source, file_name) {
        Ok(m) => (Some(m), Vec::new()),
        Err(d) => (None, d),
    };
    for e in &js_errors {
        if !diagnostics.contains(e) {
            diagnostics.push(e.clone());
        }
    }
    diagnostics.sort_by_key(|d| (d.line, d.col));
    Build {
        ir,
        js,
        diagnostics,
        js_errors,
    }
}
