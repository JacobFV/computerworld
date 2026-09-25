//! Phase timing for the style, layout and paint pipeline.
//!
//! Off by default and free when off (one thread-local read per span). A harness
//! turns it on with [`set_clock`], passing a monotonic nanosecond clock (the engine
//! itself never reads a clock: it runs deterministically and on targets without
//! one), runs its interaction, and reads the accumulated times with [`take`].
//! Spans may nest; each phase accumulates its own inclusive time.

use std::cell::Cell;

/// A pipeline phase.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Collecting the effective sheets for a flush.
    Sheets,
    /// Building the cascade engine (selector index, parsed declarations) from sheets.
    EngineBuild,
    /// Working out which elements a batch of mutations or state changes restyles.
    Invalidation,
    /// Selector matching and cascade ordering (the winning declarations).
    Match,
    /// Computed-value resolution from the winning declarations.
    Compute,
    /// Everything else in a style flush (transition bookkeeping, caches).
    StyleOther,
    /// Box tree construction.
    BoxTree,
    /// Layout proper (formatting contexts to fragments).
    Layout,
    /// Painting a scene.
    Paint,
    /// Hit testing (`elementFromPoint`, pointer targets).
    HitTest,
}

pub const PHASES: [Phase; 10] = [
    Phase::Sheets,
    Phase::EngineBuild,
    Phase::Invalidation,
    Phase::Match,
    Phase::Compute,
    Phase::StyleOther,
    Phase::BoxTree,
    Phase::Layout,
    Phase::Paint,
    Phase::HitTest,
];

impl Phase {
    pub fn name(self) -> &'static str {
        match self {
            Phase::Sheets => "sheets",
            Phase::EngineBuild => "engine build",
            Phase::Invalidation => "invalidation",
            Phase::Match => "match",
            Phase::Compute => "compute",
            Phase::StyleOther => "style other",
            Phase::BoxTree => "box tree",
            Phase::Layout => "layout",
            Phase::Paint => "paint",
            Phase::HitTest => "hit test",
        }
    }
}

/// Accumulated nanoseconds and span counts per phase, in [`PHASES`] order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Times {
    pub nanos: [u64; 10],
    pub counts: [u64; 10],
}

impl Times {
    pub fn get(&self, p: Phase) -> (u64, u64) {
        (self.nanos[p as usize], self.counts[p as usize])
    }
}

thread_local! {
    static CLOCK: Cell<Option<fn() -> u64>> = const { Cell::new(None) };
    static TIMES: Cell<Times> = const { Cell::new(Times { nanos: [0; 10], counts: [0; 10] }) };
}

/// Turns timing on with `clock` (nanoseconds), or off with `None`.
pub fn set_clock(clock: Option<fn() -> u64>) {
    CLOCK.with(|c| c.set(clock));
}

/// The times accumulated since the last call, which resets them.
pub fn take() -> Times {
    TIMES.with(|t| t.replace(Times::default()))
}

/// A running span; its time is added to its phase when it drops.
pub struct Span {
    phase: Phase,
    start: u64,
    clock: fn() -> u64,
}

impl Drop for Span {
    fn drop(&mut self) {
        let d = (self.clock)().saturating_sub(self.start);
        TIMES.with(|t| {
            let mut v = t.get();
            v.nanos[self.phase as usize] += d;
            v.counts[self.phase as usize] += 1;
            t.set(v);
        });
    }
}

/// Starts a span of `phase` when timing is on.
#[inline]
pub fn span(phase: Phase) -> Option<Span> {
    let clock = CLOCK.with(|c| c.get())?;
    Some(Span {
        phase,
        start: clock(),
        clock,
    })
}

thread_local! {
    static VERIFY: Cell<Option<bool>> = const { Cell::new(None) };
}

/// Whether every incremental style and layout flush is checked against a
/// from-scratch pass (and panics on a difference). On when the environment sets
/// `CW_WEB_VERIFY_INCREMENTAL=1`, or after [`set_verify`]`(true)` on this thread.
pub fn verifying() -> bool {
    VERIFY.with(|v| match v.get() {
        Some(b) => b,
        None => {
            let on = std::env::var("CW_WEB_VERIFY_INCREMENTAL").is_ok_and(|s| s != "0");
            v.set(Some(on));
            on
        }
    })
}

/// Turns the incremental check on or off for this thread.
pub fn set_verify(on: bool) {
    VERIFY.with(|v| v.set(Some(on)));
}
