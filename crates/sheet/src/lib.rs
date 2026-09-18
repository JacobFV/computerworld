//! A pure, deterministic spreadsheet engine.
//!
//! Cells hold values or formulas; formulas are parsed into an AST with Excel's
//! precedence, their references tracked in a dependency graph, and recalculated
//! incrementally in dependency order with circular references detected. Numbers are
//! IEEE doubles, and every transcendental function goes through
//! `cw_determinism::math`, so results are bit-identical on every target. Workbooks are
//! read and written as CSV, XLSX (Office Open XML, through the crate's own ZIP and
//! DEFLATE) and ODS.
pub mod address;
pub mod csv;
pub mod date;
pub mod eval;
pub mod format;
pub mod functions;
pub mod ods;
pub mod parser;
pub mod value;
pub mod workbook;
pub mod xlsx;
mod xml;
pub mod zip;

pub use address::{column_index, column_name, Cell, CellRef, Range, MAX_COLS, MAX_ROWS};
pub use value::{ErrorKind, Value};
pub use workbook::{
    Align, Chart, ChartData, ChartKind, Clip, Input, Series, Sheet, Stats, Style, Workbook,
};
