//! The table-driven proportional faces: the four platform UI families and the
//! thirteen web families, four styles each, and what the renderer does when a
//! family lacks a style.
//!
//! Every family has a slot for regular, bold, italic and bold italic
//! (`bold + 2 * italic`). A slot that is `None` is drawn from the nearest face the
//! family does have, in the order [`face_source`] fixes — the same order
//! `cw_scene::Typeface::web_face_index` measures with, so layout and pixels agree —
//! with the missing weight or slant synthesised on the rasterised coverage:
//! [`embolden`] smears each row one pixel to the right, [`oblique`] shears the bitmap
//! by about 12° about the baseline. Both are integer arithmetic over coverage bytes,
//! so a synthetic glyph is the same bits on every target. Every bundled family
//! currently ships all four files, so the synthetic path is exercised by its tests
//! rather than by any bundled face.
use cw_scene::Typeface;
use fontdue::Metrics;

/// Families in the face table, in slot order.
pub(crate) const FAMILIES: [Typeface; 17] = [
    Typeface::Inter,
    Typeface::OpenSans,
    Typeface::Ubuntu,
    Typeface::Roboto,
    Typeface::Arimo,
    Typeface::Tinos,
    Typeface::Cousine,
    Typeface::Gelasio,
    Typeface::Carlito,
    Typeface::Caladea,
    Typeface::Lato,
    Typeface::SourceSans,
    Typeface::SourceSerif,
    Typeface::Poppins,
    Typeface::Montserrat,
    Typeface::Playfair,
    Typeface::JetBrainsMono,
];
pub(crate) const FACE_COUNT: usize = FAMILIES.len() * 4;

macro_rules! face {
    ($name:literal) => {
        Some(include_bytes!(concat!("../assets/fonts/", $name)) as &[u8])
    };
}
/// Subsets of the families above, four per family (regular, bold, italic, bold
/// italic); `None` where a family has no such file. In a static so each face is in
/// the binary once. The platform families are Latin subsets; the web families are
/// subset to DejaVu's coverage (see `assets/build-fonts.py --web`).
pub(crate) static FACE_BYTES: [Option<&[u8]>; FACE_COUNT] = [
    face!("inter-regular.ttf"),
    face!("inter-bold.ttf"),
    face!("inter-italic.ttf"),
    face!("inter-bold-italic.ttf"),
    face!("opensans-regular.ttf"),
    face!("opensans-bold.ttf"),
    face!("opensans-italic.ttf"),
    face!("opensans-bold-italic.ttf"),
    face!("ubuntu-regular.ttf"),
    face!("ubuntu-bold.ttf"),
    face!("ubuntu-italic.ttf"),
    face!("ubuntu-bold-italic.ttf"),
    face!("roboto-regular.ttf"),
    face!("roboto-bold.ttf"),
    face!("roboto-italic.ttf"),
    face!("roboto-bold-italic.ttf"),
    face!("arimo-regular.ttf"),
    face!("arimo-bold.ttf"),
    face!("arimo-italic.ttf"),
    face!("arimo-bold-italic.ttf"),
    face!("tinos-regular.ttf"),
    face!("tinos-bold.ttf"),
    face!("tinos-italic.ttf"),
    face!("tinos-bold-italic.ttf"),
    face!("cousine-regular.ttf"),
    face!("cousine-bold.ttf"),
    face!("cousine-italic.ttf"),
    face!("cousine-bold-italic.ttf"),
    face!("gelasio-regular.ttf"),
    face!("gelasio-bold.ttf"),
    face!("gelasio-italic.ttf"),
    face!("gelasio-bold-italic.ttf"),
    face!("carlito-regular.ttf"),
    face!("carlito-bold.ttf"),
    face!("carlito-italic.ttf"),
    face!("carlito-bold-italic.ttf"),
    face!("caladea-regular.ttf"),
    face!("caladea-bold.ttf"),
    face!("caladea-italic.ttf"),
    face!("caladea-bold-italic.ttf"),
    face!("lato-regular.ttf"),
    face!("lato-bold.ttf"),
    face!("lato-italic.ttf"),
    face!("lato-bold-italic.ttf"),
    face!("sourcesans-regular.ttf"),
    face!("sourcesans-bold.ttf"),
    face!("sourcesans-italic.ttf"),
    face!("sourcesans-bold-italic.ttf"),
    face!("sourceserif-regular.ttf"),
    face!("sourceserif-bold.ttf"),
    face!("sourceserif-italic.ttf"),
    face!("sourceserif-bold-italic.ttf"),
    face!("poppins-regular.ttf"),
    face!("poppins-bold.ttf"),
    face!("poppins-italic.ttf"),
    face!("poppins-bold-italic.ttf"),
    face!("montserrat-regular.ttf"),
    face!("montserrat-bold.ttf"),
    face!("montserrat-italic.ttf"),
    face!("montserrat-bold-italic.ttf"),
    face!("playfair-regular.ttf"),
    face!("playfair-bold.ttf"),
    face!("playfair-italic.ttf"),
    face!("playfair-bold-italic.ttf"),
    face!("jetbrainsmono-regular.ttf"),
    face!("jetbrainsmono-bold.ttf"),
    face!("jetbrainsmono-italic.ttf"),
    face!("jetbrainsmono-bold-italic.ttf"),
];

/// Slot of a family's face for `bold`/`italic`; `None` for DejaVu and the
/// monospace terminal face, which have their own fonts.
pub(crate) fn face_index(typeface: Typeface, bold: bool, italic: bool) -> Option<usize> {
    let family = FAMILIES.iter().position(|f| *f == typeface)?;
    Some(family * 4 + usize::from(bold) + 2 * usize::from(italic))
}

/// The file a requested slot is drawn from, and what must be synthesised on top:
/// `(slot of the file, synthetic bold, synthetic oblique)`. A present slot is drawn
/// as is; a missing one falls to the upright of the same weight, then the regular
/// of the same slant, then the regular — the order `Typeface::web_face_index`
/// measures with.
pub(crate) fn face_source(requested: usize) -> (usize, bool, bool) {
    for candidate in [requested, requested & !2, requested & !1, requested & !3] {
        if FACE_BYTES[candidate].is_some() {
            let missing = requested & !candidate;
            return (candidate, missing & 1 != 0, missing & 2 != 0);
        }
    }
    unreachable!("every family in FACE_BYTES has a regular face")
}

/// Synthetic bold: every coverage row is smeared one pixel to the right (each pixel
/// takes the greater of itself and its left neighbour), which thickens vertical
/// stems by one pixel and leaves horizontals as they were. The advance is not
/// changed — layout measured the regular face — so a synthetic bold word sets a
/// little tighter than a designed one would.
pub(crate) fn embolden(metrics: Metrics, alpha: &[u8]) -> (Metrics, Vec<u8>) {
    if metrics.width == 0 || metrics.height == 0 {
        return (metrics, alpha.to_vec());
    }
    let (w, h) = (metrics.width, metrics.height);
    let mut out = vec![0u8; (w + 1) * h];
    for y in 0..h {
        let row = &alpha[y * w..(y + 1) * w];
        let dst = &mut out[y * (w + 1)..(y + 1) * (w + 1)];
        for x in 0..=w {
            let here = if x < w { row[x] } else { 0 };
            let left = if x > 0 { row[x - 1] } else { 0 };
            dst[x] = here.max(left);
        }
    }
    (
        Metrics {
            width: w + 1,
            ..metrics
        },
        out,
    )
}

/// Slope of the synthetic oblique in 1/64 pixel of x per pixel of y: 14/64, close to
/// the 12° most italics lean at (tan 12° = 0.2126).
const SHEAR_64: i64 = 14;

/// Synthetic oblique: the coverage is sheared about the baseline, each row moved
/// right by `SHEAR_64/64` of its height above the baseline (and left below it),
/// with the fractional part of the shift spread over two pixels so the slant is
/// smooth. Everything is integer arithmetic on the coverage bytes.
pub(crate) fn oblique(metrics: Metrics, alpha: &[u8]) -> (Metrics, Vec<u8>) {
    if metrics.width == 0 || metrics.height == 0 {
        return (metrics, alpha.to_vec());
    }
    let (w, h) = (metrics.width, metrics.height);
    // Shift of a row, in 1/64 pixel, from the height of the row's centre above the
    // baseline: row 0 is the top of the bitmap, `ymin + height` pixels up.
    let shift = |row: usize| -> i64 {
        let centre_64 = (i64::from(metrics.ymin) + h as i64 - row as i64) * 64 - 32;
        centre_64 * SHEAR_64 / 64
    };
    let lowest = shift(h - 1).div_euclid(64);
    let highest = shift(0).div_euclid(64) + 1;
    let out_w = w + (highest - lowest) as usize + 1;
    let mut out = vec![0u8; out_w * h];
    for y in 0..h {
        let s = shift(y);
        let whole = (s.div_euclid(64) - lowest) as usize;
        let frac = s.rem_euclid(64) as u32;
        let row = &alpha[y * w..(y + 1) * w];
        let dst = &mut out[y * out_w..(y + 1) * out_w];
        for (x, &a) in row.iter().enumerate() {
            let a = u32::from(a);
            let near = (a * (64 - frac) + 32) / 64;
            let far = (a * frac + 32) / 64;
            dst[x + whole] = dst[x + whole].saturating_add(near as u8);
            dst[x + whole + 1] = dst[x + whole + 1].saturating_add(far as u8);
        }
    }
    (
        Metrics {
            xmin: metrics.xmin + lowest as i32,
            width: out_w,
            ..metrics
        },
        out,
    )
}

/// SHA-256 of each web face, as `assets/build-fonts.py --web` produced it from the
/// masters `fetch-web-sources.py` pins.
#[cfg(test)]
const WEB_FACE_SHA256: [(&str, &str); 52] = [
    (
        "arimo-regular.ttf",
        "70dcd8763e2228dc7145ce884c72a8ffc7898a1db47acf73f46609e76280e242",
    ),
    (
        "arimo-bold.ttf",
        "0683379ddad7590bd4763b72a0dfba9f89be717f108ffb1d5d62fde3d869f19b",
    ),
    (
        "arimo-italic.ttf",
        "ad72095f9681f967a586d4890c2b6b3737cb8938e2613180971a8a227f6d55e6",
    ),
    (
        "arimo-bold-italic.ttf",
        "cdbbe7dcc527edd7af0bf918ad710721ccf1c1b041ffc936040ed5d236d05560",
    ),
    (
        "tinos-regular.ttf",
        "dcd3bab0226cac708b45f742cfdb97f3d4ac202627331a9251507aeee3449f8f",
    ),
    (
        "tinos-bold.ttf",
        "d639db1d835936a49be831505b1e3bd9dcc7e3f1483cc4da98ad7c0179026c68",
    ),
    (
        "tinos-italic.ttf",
        "6b263217478ad26ece3dfba570055dcf1dd1476eaeb511d3f65af39c255f0284",
    ),
    (
        "tinos-bold-italic.ttf",
        "4d1adbf9fb465916bde8316ec935a7c1763dc684352d93c975ded49c83ed8fd1",
    ),
    (
        "cousine-regular.ttf",
        "a1d0144bcfe6120a4b11e82eeab061b7a049cb5eebe4f044c226d1e68dcbfbfe",
    ),
    (
        "cousine-bold.ttf",
        "2accafad403122e06bd8fa690e74656b9ca7c720ded9e9908e25db9731f846f1",
    ),
    (
        "cousine-italic.ttf",
        "46ab107e120f5209f74959321cda0b7583e683556e8a68d4a01ce601abfef024",
    ),
    (
        "cousine-bold-italic.ttf",
        "ae061225465ab112a0a1ffa75c60f9d1b5c70ffe353ad27ade3bc7962782e0f2",
    ),
    (
        "gelasio-regular.ttf",
        "34fa275a3e930f262edc2f5987efa6c27916bdc2571d800665757b438c72ea16",
    ),
    (
        "gelasio-bold.ttf",
        "f15a5a6ea36a83114c41cf56bd6bbfed25b94041afff3860c0ebe877449656b9",
    ),
    (
        "gelasio-italic.ttf",
        "ab07e560003c02362c43deed83169ce6604a8be054bbab2dbfe2da7ca67d76d9",
    ),
    (
        "gelasio-bold-italic.ttf",
        "01feed9aaaa9ebb1d70378398b1efce0b1e6efcefc9618ff04b5d448dffd6ab3",
    ),
    (
        "carlito-regular.ttf",
        "c08912314e500f890270292bb37f5105bf526f45051bc9a7d9b2e516a5d79362",
    ),
    (
        "carlito-bold.ttf",
        "238acea90008784c975c551cb3c9a209b682734e3e905dd145bcdb043793dcdf",
    ),
    (
        "carlito-italic.ttf",
        "980295ff41c22839e913aa9efb7b40fbd1a94882df9c9d934dadd638b749907f",
    ),
    (
        "carlito-bold-italic.ttf",
        "6d19b5a717134973dc813e3e585358c775c52e2fa18380bd0ae6fd703550c685",
    ),
    (
        "caladea-regular.ttf",
        "73aed07063e394c26c4191734c1cdfe1635f3477ecca8c8846f900633042d2ed",
    ),
    (
        "caladea-bold.ttf",
        "b37dbe406c9521be3e7a3ac8659e2122e9ee37e15db72f02c05bd55b12c6e5e6",
    ),
    (
        "caladea-italic.ttf",
        "823f09701aa46cbd574454ee6f6a30385c50253f5c3dc60bc5294deffd9b11bd",
    ),
    (
        "caladea-bold-italic.ttf",
        "f3ba5cbed8ff96ac97625d7e0caeeb7598ecd3157de1a44cdb3d12b1448796f9",
    ),
    (
        "lato-regular.ttf",
        "941732d0e17b555e33f9419b79946b44c581570338c81beab630cd05da4cb7f3",
    ),
    (
        "lato-bold.ttf",
        "2d242a248875a177bffdcbba190cd68fdd757c5593be41585d39a68748bf65c6",
    ),
    (
        "lato-italic.ttf",
        "35316c7c4a3ec9c95d628a7dd501ae0c1732eee74deb99714ed47131a771acb3",
    ),
    (
        "lato-bold-italic.ttf",
        "73b9950a14ce9a4cf51d13429abb0af0a35492d91057aa3a2031a5ba1ec64062",
    ),
    (
        "sourcesans-regular.ttf",
        "6d298bf953417a0edf749d4a6c0f191a03db6f4c2d9386ff6e8b20c4f9385f3a",
    ),
    (
        "sourcesans-bold.ttf",
        "b17d1e2432f971a0de6543dc5c9a2c01a642db1d9956a7de328a099bcae0c6f8",
    ),
    (
        "sourcesans-italic.ttf",
        "cff86d85b26ca009c37424021ec1b8ff82951e8d52f485bc58becf7717029eec",
    ),
    (
        "sourcesans-bold-italic.ttf",
        "6376317d7322559c48ff09399213c8a73b4e5fcb5b80fd47877983d0009e717e",
    ),
    (
        "sourceserif-regular.ttf",
        "41b60f4994cb845f62c89e788bd60f7b222d546833679922b6ecf40b20b537ce",
    ),
    (
        "sourceserif-bold.ttf",
        "863b70d1505f9e6d5cb99798edea121b256f8072fed71b4de6cb081f0a106a2d",
    ),
    (
        "sourceserif-italic.ttf",
        "f7ffefde5e4b8c8ec971058606f6bfd371f47063a3233e186011c2b59d0d86d5",
    ),
    (
        "sourceserif-bold-italic.ttf",
        "4dd017e45f5541a46b263272b19e37aa2ae8510a398ab78a21c1fd8ba5a46406",
    ),
    (
        "poppins-regular.ttf",
        "66cee03e736e7f73cac6a33981f1eaaa714f1d68791addb7ab1760dd1e497039",
    ),
    (
        "poppins-bold.ttf",
        "089129a08f51802c640afecd165143bef4777e565913f5e78c0cf0c7337b941d",
    ),
    (
        "poppins-italic.ttf",
        "5a3c1942c729456c519ecc05722186aa3f8fc74a6e87f47243c0f8292ac32210",
    ),
    (
        "poppins-bold-italic.ttf",
        "adab280043d64523e8b67bb031751c5f62aa2a88904a5aa269d26d37e8713d45",
    ),
    (
        "montserrat-regular.ttf",
        "de8f30409a8aed69800710ec2ada6d222b8075acf401a6b873e85b075b7dec23",
    ),
    (
        "montserrat-bold.ttf",
        "34e7655e96d030fe9b1097bd2006bdafa151f8b4ecdd82bcda30fe0b00457636",
    ),
    (
        "montserrat-italic.ttf",
        "f7f9720116523918af28bb96599e075bb02dc0c0682be3920215c208252a8331",
    ),
    (
        "montserrat-bold-italic.ttf",
        "b926921121f5f38c130a516b3de99d0863304cee3f0fe9f1d1fedd8d97344964",
    ),
    (
        "playfair-regular.ttf",
        "0e913357adeb53e8a39376be7aae0e9dc255d70c1aeadb9d94cc73c6ff62f68e",
    ),
    (
        "playfair-bold.ttf",
        "000eaa508636b6561557d5efafbb880a57aa7bc08c4e11b0e89b06656f2b4704",
    ),
    (
        "playfair-italic.ttf",
        "5520209f2f20e65695070fed4541513b54be1bbf46239636c625412664a13832",
    ),
    (
        "playfair-bold-italic.ttf",
        "5766ce86b2c384ee82c02e3ef846d586df827a366f37c4f3a6a1e04044c4a63a",
    ),
    (
        "jetbrainsmono-regular.ttf",
        "446bbd98d41627d4237ca94ddf53c93f11f9f8cb415bbb292e400a640db51d2a",
    ),
    (
        "jetbrainsmono-bold.ttf",
        "511558e6fb9502152395d0d5e121a0f5487b563d768f61570c8f78bff7496bad",
    ),
    (
        "jetbrainsmono-italic.ttf",
        "db8d5e552dca9780d2f6fec87409a62847e3dcb197f640b6283c12d7d2c4534b",
    ),
    (
        "jetbrainsmono-bold-italic.ttf",
        "45d3e13aa22236d17fe604e1baed57ce1ca4a171368f060e5f714ff6bea2f9e7",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    use fontdue::{Font, FontSettings};
    use sha2::{Digest, Sha256};

    #[test]
    fn web_faces_match_their_pinned_hashes_and_carry_their_names() {
        assert_eq!(WEB_FACE_SHA256.len(), Typeface::WEB.len() * 4);
        for (i, (file, sha256)) in WEB_FACE_SHA256.iter().enumerate() {
            let typeface = Typeface::WEB[i / 4];
            let slot = face_index(typeface, i % 4 == 1 || i % 4 == 3, i % 4 >= 2).unwrap();
            let bytes = FACE_BYTES[slot].unwrap_or_else(|| panic!("{file} is bundled"));
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), *sha256, "{file}");
            // The file keeps its name table, so it still says what it is.
            let face = rustybuzz::ttf_parser::Face::parse(bytes, 0).unwrap();
            let names: Vec<String> = face
                .names()
                .into_iter()
                .map(|n| {
                    if n.is_unicode() {
                        let units: Vec<u16> = n
                            .name
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|&b| u16::from_be_bytes(b))
                            .collect();
                        String::from_utf16_lossy(&units)
                    } else {
                        n.name.iter().map(|&b| char::from(b)).collect()
                    }
                })
                .collect();
            let family = typeface.family_name();
            assert!(
                names.iter().any(|n| n.starts_with(family)),
                "{file}: {names:?} lacks {family}"
            );
            assert!(
                names.iter().any(|n| n.contains("SIL Open Font License")
                    || n.contains("http://scripts.sil.org/OFL")
                    || n.contains("openfontlicense.org")),
                "{file}: no licence in the name table: {names:?}"
            );
            assert!(face.is_monospaced() == typeface.is_monospace(), "{file}");
            assert_eq!(face.is_italic() || face.is_oblique(), i % 4 >= 2, "{file}");
            // Instanced faces carry their weight in OS/2 (the instancer does not set
            // the style bits), static ones in both.
            let bold = i % 4 == 1 || i % 4 == 3;
            assert_eq!(face.weight().to_number() >= 600, bold, "{file}");
        }
    }
    #[test]
    fn every_family_has_a_regular_face_and_present_slots_draw_themselves() {
        for (family, typeface) in FAMILIES.iter().enumerate() {
            assert!(FACE_BYTES[family * 4].is_some(), "{typeface:?} regular");
            for (slot, bytes) in FACE_BYTES.iter().enumerate().skip(family * 4).take(4) {
                if bytes.is_some() {
                    assert_eq!(face_source(slot), (slot, false, false));
                }
                // What the renderer picks is what layout measured with.
                if let Some(i) = typeface.web_face_index(slot % 2 == 1, slot % 4 >= 2) {
                    assert_eq!(face_source(slot).0, family * 4 + i, "{typeface:?}");
                }
            }
        }
        assert_eq!(face_index(Typeface::DejaVu, true, true), None);
        assert_eq!(face_index(Typeface::Mono, false, false), None);
        assert_eq!(face_index(Typeface::Inter, false, false), Some(0));
        assert_eq!(face_index(Typeface::Arimo, true, true), Some(19));
    }
    #[test]
    fn synthetic_bold_thickens_stems_and_synthetic_oblique_leans_right() {
        let font = Font::from_bytes(FACE_BYTES[16].unwrap(), FontSettings::default()).unwrap();
        let (metrics, alpha) = font.rasterize('l', 24.0);
        let ink = |a: &[u8]| a.iter().map(|&v| u32::from(v)).sum::<u32>();
        let (bold, bold_alpha) = embolden(metrics, &alpha);
        assert_eq!(
            (bold.width, bold.height),
            (metrics.width + 1, metrics.height)
        );
        assert!(ink(&bold_alpha) > ink(&alpha) * 5 / 4, "bold has more ink");
        assert_eq!(embolden(bold, &bold_alpha).0.width, metrics.width + 2);
        let (slant, slant_alpha) = oblique(metrics, &alpha);
        assert_eq!(slant.height, metrics.height);
        assert!(slant.width > metrics.width);
        // The shear conserves ink (to rounding) and moves the top of the stem right
        // of its foot.
        let total = ink(&slant_alpha);
        assert!(total.abs_diff(ink(&alpha)) <= metrics.height as u32 * 2);
        let centre = |row: usize| {
            let r = &slant_alpha[row * slant.width..(row + 1) * slant.width];
            let weight: u32 = r.iter().map(|&v| u32::from(v)).sum();
            r.iter()
                .enumerate()
                .map(|(x, &v)| x as u32 * u32::from(v))
                .sum::<u32>()
                * 64
                / weight.max(1)
        };
        let lean = centre(0) as i64 - centre(metrics.height - 1) as i64;
        let expected = (metrics.height as i64 - 1) * SHEAR_64;
        assert!(
            (lean - expected).abs() <= 64,
            "lean {lean} expected {expected}"
        );
        // Deterministic: the same input is the same bits.
        assert_eq!(oblique(metrics, &alpha), (slant, slant_alpha));
        // Empty glyphs pass through.
        let (space, none) = font.rasterize(' ', 24.0);
        assert_eq!(embolden(space, &none).1, none);
        assert_eq!(oblique(space, &none).1, none);
    }
}
