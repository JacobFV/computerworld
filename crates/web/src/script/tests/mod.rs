//! Tests of the script layer, by API area. Each runs a short script against a
//! parsed document through a `MemoryHost` and asserts DOM or console results.

mod support;
mod basics;
mod cssom;
mod dom;
mod elements;
mod events;
mod fixtures;
mod frameworks;
mod more;
mod window;

pub use support::*;
