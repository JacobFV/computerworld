//! cw-ui's test fixtures as generated programs.
//!
//! `build.rs` compiles every TSX app cw-ui's tests run — the sources in
//! `tests/react_semantics.rs` and `tests/cw_bridge.rs`, the
//! `framework-parity/tsx-*.tsx` fixtures and the agent-written apps in
//! `framework-parity/app-src/` — with `cw-tsx`, translates each IR to Rust with
//! `cw_tsx::emit_rust`, and includes the modules here. A test compiles its source as
//! before and finds the generated program of that IR by the IR's hash
//! ([`by_hash`]), so it can run the same app interpreted and generated.

include!(concat!(env!("OUT_DIR"), "/fixtures.rs"));

/// The generated program of the IR with this hash (`cw_ui::program::ir_hash`).
pub fn by_hash(hash: u64) -> Option<&'static cw_ui::GenProgram> {
    PROGRAMS.iter().copied().find(|p| p.hash == hash)
}

/// The generated program of `module`.
pub fn for_module(module: &cw_ui::ir::Module) -> Option<&'static cw_ui::GenProgram> {
    by_hash(cw_ui::program::ir_hash(module))
}
