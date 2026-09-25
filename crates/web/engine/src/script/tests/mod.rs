//! Tests of the script layer, by API area. Each runs a short script against a
//! parsed document through a `MemoryHost` and asserts DOM or console results.

mod basics;
mod cssom;
mod dom;
mod elements;
mod events;
mod fixtures;
mod frameworks;
mod host_state;
mod more;
mod snapshots;
mod support;
mod window;

pub use support::*;
