//! A deterministic video editing engine: the one model every video editor in the
//! simulator (Clipchamp, iMovie, Kdenlive and the Android editor) is an interface over.
//!
//! - [`apng`] and [`wav`] read and write the media: Animated PNG for moving pictures,
//!   RIFF WAVE linear PCM for sound. Both are public standards any player opens.
//! - [`media`] is what an editor keeps of an imported file.
//! - [`project`] is the edit itself and the saved project format.
//! - [`compose`] and [`audio`] turn the edit into frames and samples.
//! - [`export`] renders the edit back to files, a few frames per step.
//! - [`samples`] generates the sample clips the reference world ships with.
//!
//! Everything is integer or built from the raster engine's deterministic arithmetic, so
//! a native build and a Wasm build produce identical frames, samples and files.
pub mod apng;
pub mod audio;
pub mod blob;
pub mod compose;
pub mod export;
pub mod media;
pub mod project;
pub mod qoi;
pub mod samples;
pub mod wav;

pub use blob::Blob;
pub use compose::{Compositor, Masks, TextMask};
pub use export::Export;
pub use media::{Media, MediaKind};
pub use project::{
    Clip, Crop, Ease, Edge, Grade, Key, Library, Param, Project, Source, Title, TitlePosition,
    Track, TrackKind, Transition, TransitionKind,
};
