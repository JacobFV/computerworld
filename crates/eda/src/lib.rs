//! Electronics design automation engine behind the simulator's KiCad application.
//!
//! Pure and deterministic: no host clock, randomness or I/O. Geometry is integer —
//! schematic coordinates in mils, board coordinates in nanometres, as KiCad keeps them —
//! and floating point (the circuit simulator, footprints at arbitrary angles, the
//! router's path lengths) uses only IEEE arithmetic and the deterministic
//! functions in [`num`] so a waveform is bit-identical on every target.
//!
//! - [`symbols`], [`footprints`]: the part libraries.
//! - [`schematic`], [`connectivity`], [`erc`], [`netlist`]: schematic capture.
//! - [`spice`]: the circuit simulator.
//! - [`pcb`], [`drc`], [`zones`], [`router`]: board layout, ratsnest, design rules and
//!   the interactive router.
//! - [`gerber`], [`svg`], [`files`]: fabrication outputs and KiCad project files.
pub mod connectivity;
pub mod drc;
pub mod erc;
pub mod files;
pub mod font;
pub mod footprints;
pub mod geom;
pub mod gerber;
pub mod netlist;
pub mod num;
pub mod pcb;
pub mod router;
pub mod schematic;
pub mod sexpr;
pub mod spice;
pub mod svg;
pub mod symbols;
pub mod zones;
