//! Outline sources for the fallback faces, and the on-demand CJK/emoji font pack.
//!
//! The Hebrew, Arabic, Thai and Devanagari faces are embedded everywhere. The
//! CJK and emoji faces (`assets/fonts/pack/`) are 6.75 MB of outlines, so the Wasm
//! build leaves them out and a page installs them with [`install_font`] once it has
//! fetched them; native builds embed them and never need to. Layout does not wait
//! for the pack — it shapes the always-embedded stubs (see `cw_scene::text`) — so
//! the only thing that changes when a pack file arrives is that its glyphs stop
//! drawing as `.notdef` boxes. Installed bytes must match the pinned SHA-256 of the
//! file this build was made with, so every installed pack is the same pack and the
//! same text renders the same pixels natively and in Wasm.
use cw_scene::text::FaceId;
use fontdue::{Font, FontSettings};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
use std::sync::OnceLock;

/// One file of the font pack: where it is served from, relative to the package
/// (`fonts/<file>`), and the SHA-256 its bytes must have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackFile {
    pub face: FaceId,
    pub file: &'static str,
    pub sha256: &'static str,
    pub bytes: usize,
}

pub const FONT_PACK: [PackFile; 3] = [
    PackFile {
        face: FaceId::Han,
        file: "noto-sans-sc.ttf",
        sha256: "34379072e545d67c4b22de7ccb2a60bdb02c82e8144eb705ab73356d1b65cec7",
        bytes: 3_522_836,
    },
    PackFile {
        face: FaceId::Hangul,
        file: "noto-sans-kr.ttf",
        sha256: "b7328d27e2cda3fd8c6198df10ba6a780102b7a5698ee67f6153005f26077c73",
        bytes: 2_366_072,
    },
    PackFile {
        face: FaceId::Emoji,
        file: "noto-emoji.ttf",
        sha256: "f2ca5cf2d5d68e2e98920db332ecb333a50c747d4bc2d151aa3357d2fe8b1ce8",
        bytes: 862_788,
    },
];

fn slot(face: FaceId) -> Option<usize> {
    FONT_PACK.iter().position(|p| p.face == face)
}

/// In statics rather than constants so each file is in the binary exactly once.
#[cfg(not(target_family = "wasm"))]
static EMBEDDED: [&[u8]; 3] = [
    include_bytes!("../assets/fonts/pack/noto-sans-sc.ttf"),
    include_bytes!("../assets/fonts/pack/noto-sans-kr.ttf"),
    include_bytes!("../assets/fonts/pack/noto-emoji.ttf"),
];
#[cfg(not(target_family = "wasm"))]
fn embedded(face: FaceId) -> Option<&'static [u8]> {
    slot(face).map(|i| EMBEDDED[i])
}
#[cfg(target_family = "wasm")]
fn embedded(_: FaceId) -> Option<&'static [u8]> {
    None
}

static INSTALLED: [OnceLock<&'static [u8]>; 3] = [const { OnceLock::new() }; 3];
/// Bit per pack slot: a renderer needed that face and did not have it.
static WANTED: AtomicU8 = AtomicU8::new(0);
static GENERATION: AtomicU32 = AtomicU32::new(0);

#[cfg(test)]
thread_local! {
    /// Lets tests exercise the Wasm "pack not yet fetched" path natively.
    pub(crate) static HIDE_PACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Outline bytes for a face, if this process has them.
pub(crate) fn face_bytes(face: FaceId) -> Option<&'static [u8]> {
    if let Some(bytes) = face.embedded_bytes() {
        return Some(bytes);
    }
    #[cfg(test)]
    if HIDE_PACK.with(|h| h.get()) {
        return None;
    }
    embedded(face).or_else(|| slot(face).and_then(|i| INSTALLED[i].get().copied()))
}

/// Parsed faces are immutable and shared by every renderer in the process.
pub(crate) fn face_font(face: FaceId) -> Option<&'static Font> {
    static FONTS: [OnceLock<Font>; 11] = [const { OnceLock::new() }; 11];
    let bytes = face_bytes(face)?;
    Some(FONTS[face as usize].get_or_init(|| {
        Font::from_bytes(bytes, FontSettings::default()).expect("bundled fallback face is valid")
    }))
}

pub(crate) fn note_missing(face: FaceId) {
    if let Some(i) = slot(face) {
        WANTED.fetch_or(1 << i, Ordering::Relaxed);
    }
}

/// Changes whenever a pack file is installed; renderers drop cached text then.
pub(crate) fn generation() -> u32 {
    GENERATION.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FontPackError {
    /// The bytes are not any file of this build's pack.
    Unknown { sha256: String },
}
impl std::fmt::Display for FontPackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unknown { sha256 } => write!(
                f,
                "font data with SHA-256 {sha256} is not a file of this build's font pack"
            ),
        }
    }
}
impl std::error::Error for FontPackError {}

/// Install one pack file, identified by its SHA-256. Installing a file twice, or
/// installing into a native build that already embeds it, is a harmless no-op.
pub fn install_font(bytes: &[u8]) -> Result<&'static PackFile, FontPackError> {
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    let Some(i) = FONT_PACK.iter().position(|p| p.sha256 == sha256) else {
        return Err(FontPackError::Unknown { sha256 });
    };
    if embedded(FONT_PACK[i].face).is_none() && INSTALLED[i].get().is_none() {
        let leaked: &'static [u8] = Box::leak(bytes.to_vec().into_boxed_slice());
        if INSTALLED[i].set(leaked).is_ok() {
            GENERATION.fetch_add(1, Ordering::Relaxed);
        }
    }
    Ok(&FONT_PACK[i])
}

/// Which pack files this process can draw, and which ones renderers have needed
/// but not had (so a page can fetch exactly those).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontPackStatus {
    pub installed: Vec<&'static str>,
    pub missing: Vec<&'static str>,
}
pub fn font_pack_status() -> FontPackStatus {
    let wanted = WANTED.load(Ordering::Relaxed);
    let mut status = FontPackStatus {
        installed: Vec::new(),
        missing: Vec::new(),
    };
    for (i, file) in FONT_PACK.iter().enumerate() {
        if face_bytes(file.face).is_some() {
            status.installed.push(file.file);
        } else if wanted & (1 << i) != 0 {
            status.missing.push(file.file);
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pack_files_match_their_pinned_hashes() {
        for file in FONT_PACK {
            let bytes = embedded(file.face).unwrap();
            assert_eq!(bytes.len(), file.bytes, "{}", file.file);
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), file.sha256);
            assert_eq!(file.face.file_name(), file.file);
            assert_eq!(install_font(bytes).unwrap(), &file);
        }
        assert!(matches!(
            install_font(b"not a font"),
            Err(FontPackError::Unknown { .. })
        ));
        assert_eq!(font_pack_status().installed.len(), 3);
    }
}
