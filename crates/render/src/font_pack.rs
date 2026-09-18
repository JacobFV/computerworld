//! Outline sources for the fallback faces, and the on-demand font pack.
//!
//! Hebrew, Arabic, Thai, Devanagari, Bengali, Georgian and Armenian are embedded
//! everywhere. The CJK faces (Simplified Chinese, its bold, the Traditional Chinese,
//! Japanese and Korean locale faces, Hangul), the emoji faces (monochrome and colour)
//! and eight further scripts (Tamil, Gurmukhi, Lao, Khmer, Gujarati, Ethiopic,
//! Myanmar, Sinhala) are the font pack (`assets/fonts/pack/`):
//! the Wasm build leaves them out and a page installs each file with
//! [`install_font`] once it has fetched it; native builds embed them and never need
//! to. Layout does not wait for the pack — it shapes the always-embedded stubs (see
//! `cw_scene::text`) — so the only thing that changes when a pack file arrives is
//! that its glyphs stop drawing as `.notdef` boxes (or, for colour emoji, that the
//! monochrome glyphs turn into colour ones). Installed bytes must match the pinned
//! SHA-256 of the file this build was made with, so every installed pack is the same
//! pack and the same text renders the same pixels natively and in Wasm.
use cw_scene::text::FaceId;
use fontdue::{Font, FontSettings};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU32, Ordering};
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

const fn pack(face: FaceId, file: &'static str, sha256: &'static str, bytes: usize) -> PackFile {
    PackFile {
        face,
        file,
        sha256,
        bytes,
    }
}

pub const FONT_PACK: [PackFile; 20] = [
    pack(
        FaceId::Han,
        "noto-sans-sc.ttf",
        "34379072e545d67c4b22de7ccb2a60bdb02c82e8144eb705ab73356d1b65cec7",
        3_522_836,
    ),
    pack(
        FaceId::Hangul,
        "noto-sans-kr.ttf",
        "b7328d27e2cda3fd8c6198df10ba6a780102b7a5698ee67f6153005f26077c73",
        2_366_072,
    ),
    pack(
        FaceId::Emoji,
        "noto-emoji.ttf",
        "f2ca5cf2d5d68e2e98920db332ecb333a50c747d4bc2d151aa3357d2fe8b1ce8",
        862_788,
    ),
    pack(
        FaceId::HanBold,
        "noto-sans-sc-bold.ttf",
        "4d039b1401203d59c6b75da5e5d31b5b317a2651eb5f66d35d7347216dc42f4e",
        3_518_020,
    ),
    pack(
        FaceId::HangulBold,
        "noto-sans-kr-bold.ttf",
        "845529246ddc318926853f028279ebbcde1c6a72a8460d5ae11e2b34e6e9790e",
        2_365_908,
    ),
    pack(
        FaceId::HanTc,
        "noto-sans-tc.ttf",
        "ed345467119202ca3bde9eec4e11fb161a2ba480a2b23d30874f8e80c06e5f55",
        976_604,
    ),
    pack(
        FaceId::HanTcBold,
        "noto-sans-tc-bold.ttf",
        "ce6e458c0002c7233568a1f2510b31c6b46cb7008cb8de2d007e04fefe7ed6df",
        975_532,
    ),
    pack(
        FaceId::HanJp,
        "noto-sans-jp.ttf",
        "8fb8077988ce42be8590a7afd0a178fd173b06959244b2cfaa185ca893274b95",
        570_584,
    ),
    pack(
        FaceId::HanJpBold,
        "noto-sans-jp-bold.ttf",
        "87048d2bf6c4464e57de95aa7801016f72bf22ee767b8666875f2d08640db293",
        569_656,
    ),
    pack(
        FaceId::HanKr,
        "noto-sans-kr-han.ttf",
        "d31454e913aee276076e2b3473f24a2408ff9263c555369f80e6c448ad4d0def",
        980_500,
    ),
    pack(
        FaceId::HanKrBold,
        "noto-sans-kr-han-bold.ttf",
        "5fec17d1cea51433a462073554c7d6fd34016481327badb7f839ad8d049116d3",
        978_976,
    ),
    pack(
        FaceId::Tamil,
        "noto-tamil.ttf",
        "a7ec5158820dbb20b50224de63b3f409e0fc0eaa7c172f5964b9277ebcb27e2e",
        38_496,
    ),
    pack(
        FaceId::Gurmukhi,
        "noto-gurmukhi.ttf",
        "629fbe717f2546a74dc41f1f42c94ff667f1792231b29f16cedd1e479d2a6064",
        24_228,
    ),
    pack(
        FaceId::Lao,
        "noto-lao.ttf",
        "9ff6157c510f22389373aff217a7fe095aebffc1ec3e7055a5c2a1a368b8dcbb",
        20_704,
    ),
    pack(
        FaceId::Khmer,
        "noto-khmer.ttf",
        "67651a225bb763e44e64fb0ef91f74a06e5c7c183d946c1dc68287e770039145",
        70_744,
    ),
    pack(
        FaceId::Gujarati,
        "noto-gujarati.ttf",
        "17dab1ae66cb5ce757aa102d76acb0e19a2037aa9f0f98741d90648987fadad2",
        128_692,
    ),
    pack(
        FaceId::Ethiopic,
        "noto-ethiopic.ttf",
        "4974a303fb3575620db2ad0d1d343acd5de3bae52396998ef9b7fb4a56064600",
        76_924,
    ),
    pack(
        FaceId::Myanmar,
        "noto-myanmar.ttf",
        "0db45818c27e6d759341b65b3135e6949eb1ef3cbab57c85f7acad78fc81e776",
        127_552,
    ),
    pack(
        FaceId::Sinhala,
        "noto-sinhala.ttf",
        "1cff7973112e1c16d110b953aa7b12cdabf538374cce62b43ef2260e12385f93",
        196_940,
    ),
    pack(
        FaceId::ColorEmoji,
        "noto-color-emoji.ttf",
        "0ae57fe58645638523ba35f388d93739d292539a9acb84df5700c81b1e1a28d2",
        4_991_984,
    ),
];

fn slot(face: FaceId) -> Option<usize> {
    FONT_PACK.iter().position(|p| p.face == face)
}

macro_rules! pack_bytes {
    ($($name:literal),* $(,)?) => {
        [$(include_bytes!(concat!("../assets/fonts/pack/", $name)) as &[u8]),*]
    };
}
/// In statics rather than constants so each file is in the binary exactly once.
#[cfg(not(target_family = "wasm"))]
static EMBEDDED: [&[u8]; 20] = pack_bytes![
    "noto-sans-sc.ttf",
    "noto-sans-kr.ttf",
    "noto-emoji.ttf",
    "noto-sans-sc-bold.ttf",
    "noto-sans-kr-bold.ttf",
    "noto-sans-tc.ttf",
    "noto-sans-tc-bold.ttf",
    "noto-sans-jp.ttf",
    "noto-sans-jp-bold.ttf",
    "noto-sans-kr-han.ttf",
    "noto-sans-kr-han-bold.ttf",
    "noto-tamil.ttf",
    "noto-gurmukhi.ttf",
    "noto-lao.ttf",
    "noto-khmer.ttf",
    "noto-gujarati.ttf",
    "noto-ethiopic.ttf",
    "noto-myanmar.ttf",
    "noto-sinhala.ttf",
    "noto-color-emoji.ttf",
];
#[cfg(not(target_family = "wasm"))]
fn embedded(face: FaceId) -> Option<&'static [u8]> {
    slot(face).map(|i| EMBEDDED[i])
}
#[cfg(target_family = "wasm")]
fn embedded(_: FaceId) -> Option<&'static [u8]> {
    None
}

static INSTALLED: [OnceLock<&'static [u8]>; 20] = [const { OnceLock::new() }; 20];
/// Bit per pack slot: a renderer needed that face and did not have it.
static WANTED: AtomicU32 = AtomicU32::new(0);
static GENERATION: AtomicU32 = AtomicU32::new(0);

#[cfg(test)]
thread_local! {
    /// Lets tests exercise the Wasm "pack not yet fetched" path natively.
    pub(crate) static HIDE_PACK: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Hides only the colour emoji file, to exercise the monochrome fallback.
    pub(crate) static HIDE_COLOR: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Outline bytes for a face, if this process has them.
pub(crate) fn face_bytes(face: FaceId) -> Option<&'static [u8]> {
    if let Some(bytes) = face.embedded_bytes() {
        return Some(bytes);
    }
    #[cfg(test)]
    if HIDE_PACK.with(|h| h.get()) || (face == FaceId::ColorEmoji && HIDE_COLOR.with(|h| h.get())) {
        return None;
    }
    embedded(face).or_else(|| slot(face).and_then(|i| INSTALLED[i].get().copied()))
}

/// Parsed faces are immutable and shared by every renderer in the process.
pub(crate) fn face_font(face: FaceId) -> Option<&'static Font> {
    static FONTS: [OnceLock<Font>; FaceId::COUNT] = [const { OnceLock::new() }; FaceId::COUNT];
    let bytes = face_bytes(face)?;
    Some(FONTS[face as usize].get_or_init(|| {
        Font::from_bytes(bytes, FontSettings::default()).expect("bundled fallback face is valid")
    }))
}

/// The colour emoji face, parsed for shaping and for its colour tables.
pub(crate) fn color_emoji() -> Option<&'static rustybuzz::Face<'static>> {
    static FACE: OnceLock<rustybuzz::Face<'static>> = OnceLock::new();
    let bytes = face_bytes(FaceId::ColorEmoji)?;
    Some(FACE.get_or_init(|| {
        rustybuzz::Face::from_slice(bytes, 0).expect("bundled colour emoji face is valid")
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
        assert_eq!(FONT_PACK.len(), FaceId::PACK.len());
        for (file, face) in FONT_PACK.iter().zip(FaceId::PACK) {
            assert_eq!(file.face, face);
            let bytes = embedded(file.face).unwrap();
            assert_eq!(bytes.len(), file.bytes, "{}", file.file);
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), file.sha256);
            assert_eq!(file.face.file_name(), file.file);
            assert_eq!(install_font(bytes).unwrap(), file);
        }
        assert!(matches!(
            install_font(b"not a font"),
            Err(FontPackError::Unknown { .. })
        ));
        assert_eq!(font_pack_status().installed.len(), FONT_PACK.len());
    }
}
