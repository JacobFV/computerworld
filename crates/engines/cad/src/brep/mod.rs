//! The exact solid kernel: boundary representations on analytic and traced geometry,
//! with booleans, blends, mass properties and tessellation.
pub mod blend;
pub mod boolean;
pub mod build;
pub mod geom;
pub mod intersect;
pub mod mass;
pub mod num;
pub mod refine;
pub mod tess;
pub mod topo;
pub mod uv;

pub use geom::{Curve, Surface};
pub use topo::{Coedge, Edge, Face, Solid, Vertex, TOL};

#[cfg(test)]
mod tests;
