//! A deterministic parametric CAD kernel.
//!
//! - [`sketch`]: FreeCAD-style Sketcher geometry and constraints, with a
//!   Levenberg–Marquardt solver that reports degrees of freedom and redundant or
//!   conflicting constraints.
//! - [`mesh`] and [`csg`]: solids as closed, consistently oriented triangle meshes whose
//!   triangles remember the analytic surface they lie on; booleans by BSP trees, healed
//!   back into watertight meshes.
//! - [`solid`]: extrusion and revolution of sketch profiles, and faces, edges and
//!   vertices recovered from the mesh for picking, measuring and dress-up features.
//! - [`document`]: a Part Design body — Pad, Pocket, Revolution, Groove, Fillet,
//!   Chamfer, Hole, Mirrored, Linear and Polar Pattern — recomputed from its parameters.
//! - [`io`]: STL (ASCII and binary), OBJ, DXF and SVG exchange.
//! - [`view`]: cameras, a z-buffer rasteriser and picking for the 3D view.
//!
//! Everything is a pure function of its inputs. No host clock, randomness or I/O, and
//! floating point only in operations IEEE-754 rounds identically on every target (see
//! [`math`]).
pub mod brep;
pub mod csg;
pub mod document;
pub mod io;
pub mod linalg;
pub mod math;
pub mod mesh;
pub mod sketch;
pub mod solid;
pub mod view;

pub use math::{v2, v3, Frame, Xform, V2, V3};
