//! The phone photo editors' non-destructive "look": the Adjust sliders and the filter
//! a photo carries until Done (iOS) or Save copy (Google Photos) writes it. The look is
//! a list of engine adjustments and filters, previewed on the view and baked into the
//! full-resolution image when saved.
use super::{Product, Studio};
use crate::AppEffect;
use cw_raster::adjust::{self, Adjustment, Channel};
use cw_raster::filter::{self, Filter};
use cw_raster::{Canvas, IRect};

/// iOS Photos' Adjust list, in the order the dial row shows it: id, label, symbol.
pub const IOS_ADJUST: &[(&str, &str, &str)] = &[
    ("auto", "Auto", "magic"),
    ("exposure", "Exposure", "sun"),
    ("brilliance", "Brilliance", "magic"),
    ("highlights", "Highlights", "contrast"),
    ("shadows", "Shadows", "contrast"),
    ("contrast", "Contrast", "contrast"),
    ("brightness", "Brightness", "sun"),
    ("black-point", "Black Point", "contrast"),
    ("saturation", "Saturation", "drop"),
    ("vibrance", "Vibrance", "drop"),
    ("warmth", "Warmth", "thermometer"),
    ("tint", "Tint", "drop"),
    ("sharpness", "Sharpness", "filters"),
    ("definition", "Definition", "filters"),
    ("noise-reduction", "Noise Reduction", "filters"),
    ("vignette", "Vignette", "dial"),
];
/// Google Photos' Adjust list. Skin tone, Blue tone and HDR need scene understanding
/// the simulator does not have, so they are not offered.
pub const GOOGLE_ADJUST: &[(&str, &str, &str)] = &[
    ("brightness", "Brightness", "sun"),
    ("contrast", "Contrast", "contrast"),
    ("white-point", "White point", "sun"),
    ("highlights", "Highlights", "contrast"),
    ("shadows", "Shadows", "contrast"),
    ("black-point", "Black point", "contrast"),
    ("saturation", "Saturation", "drop"),
    ("warmth", "Warmth", "thermometer"),
    ("tint", "Tint", "drop"),
    ("pop", "Pop", "magic"),
    ("sharpen", "Sharpen", "filters"),
    ("denoise", "Denoise", "filters"),
    ("vignette", "Vignette", "dial"),
];
pub const IOS_FILTERS: &[(&str, &str)] = &[
    ("original", "Original"),
    ("vivid", "Vivid"),
    ("vivid-warm", "Vivid Warm"),
    ("vivid-cool", "Vivid Cool"),
    ("dramatic", "Dramatic"),
    ("dramatic-warm", "Dramatic Warm"),
    ("dramatic-cool", "Dramatic Cool"),
    ("mono", "Mono"),
    ("silvertone", "Silvertone"),
    ("noir", "Noir"),
];
pub const GOOGLE_FILTERS: &[(&str, &str)] = &[
    ("original", "None"),
    ("vivid", "Vivid"),
    ("playa", "Playa"),
    ("honey", "Honey"),
    ("isla", "Isla"),
    ("desert", "Desert"),
    ("clay", "Clay"),
    ("palma", "Palma"),
    ("blush", "Blush"),
    ("alpaca", "Alpaca"),
    ("modena", "Modena"),
    ("metro", "Metro"),
    ("west", "West"),
    ("ollie", "Ollie"),
    ("onyx", "Onyx"),
    ("eiffel", "Eiffel"),
    ("vogue", "Vogue"),
    ("vista", "Vista"),
];
/// Google Photos' one-tap Suggestions that need no scene understanding.
pub const GOOGLE_SUGGESTIONS: &[(&str, &str)] = &[
    ("enhance", "Enhance"),
    ("dynamic", "Dynamic"),
    ("warm", "Warm"),
    ("cool", "Cool"),
];
/// Crop aspect ratios: id, label, width, height (0 for the photo's own).
pub const ASPECTS: &[(&str, &str, u32, u32)] = &[
    ("original", "Original", 0, 0),
    ("square", "Square", 1, 1),
    ("16x9", "16:9", 16, 9),
    ("4x3", "4:3", 4, 3),
    ("3x2", "3:2", 3, 2),
];

pub fn adjust_list(product: Product) -> &'static [(&'static str, &'static str, &'static str)] {
    match product {
        Product::IosPhotos => IOS_ADJUST,
        Product::GooglePhotos => GOOGLE_ADJUST,
        _ => &[],
    }
}
pub fn filter_list(product: Product) -> &'static [(&'static str, &'static str)] {
    match product {
        Product::IosPhotos => IOS_FILTERS,
        Product::GooglePhotos => GOOGLE_FILTERS,
        _ => &[],
    }
}

/// Range of a look parameter on this product, or `None` when it has no such slider.
pub fn range(product: Product, id: &str) -> Option<(i32, i32)> {
    if !matches!(product, Product::IosPhotos | Product::GooglePhotos) {
        return None;
    }
    if id == "straighten" {
        return Some((-45, 45));
    }
    if id == "filter-strength" {
        return Some((0, 100));
    }
    if !adjust_list(product).iter().any(|(k, _, _)| *k == id) {
        return None;
    }
    Some(match id {
        "auto" => (0, 1),
        "sharpness" | "definition" | "noise-reduction" | "sharpen" | "denoise" => (0, 100),
        _ => (-100, 100),
    })
}

/// A filter preset's adjustments.
fn preset_adjustments(id: &str) -> Vec<Adjustment> {
    let hs = |s: i32| Adjustment::HueSaturation {
        hue: 0,
        saturation: s,
        lightness: 0,
    };
    let bc = |b: i32, c: i32| Adjustment::BrightnessContrast {
        brightness: b,
        contrast: c,
    };
    let temp = |t: i32, tint: i32| Adjustment::Temperature {
        temperature: t,
        tint,
    };
    match id {
        "vivid" => vec![hs(30), bc(0, 12)],
        "vivid-warm" => vec![hs(25), bc(0, 12), temp(25, 0)],
        "vivid-cool" => vec![hs(25), bc(0, 12), temp(-25, 0)],
        "dramatic" => vec![bc(-8, 35), hs(-15)],
        "dramatic-warm" => vec![bc(-8, 35), hs(-10), temp(22, 0)],
        "dramatic-cool" => vec![bc(-8, 35), hs(-10), temp(-22, 0)],
        "mono" => vec![Adjustment::Grayscale],
        "silvertone" => vec![Adjustment::Grayscale, bc(10, -10)],
        "noir" => vec![Adjustment::Grayscale, bc(-5, 50)],
        "playa" => vec![temp(15, 5), bc(8, -5)],
        "honey" => vec![temp(30, 0), hs(-10)],
        "isla" => vec![temp(-10, -5), bc(5, 5)],
        "desert" => vec![temp(20, 0), hs(-25), bc(5, 0)],
        "clay" => vec![temp(10, 10), hs(-30)],
        "palma" => vec![temp(10, 0), hs(15), bc(5, 10)],
        "blush" => vec![temp(0, 25), bc(5, -5)],
        "alpaca" => vec![temp(12, 0), bc(6, -12)],
        "modena" => vec![temp(-15, 0), hs(-20), bc(0, 10)],
        "metro" => vec![temp(-20, 0), bc(-5, 20)],
        "west" => vec![temp(25, 0), hs(-35), bc(0, 15)],
        "ollie" => vec![temp(-8, 8), bc(8, 0)],
        "onyx" => vec![Adjustment::Grayscale, bc(-10, 30)],
        "eiffel" => vec![Adjustment::Grayscale, bc(5, 10)],
        "vogue" => vec![Adjustment::Grayscale, bc(12, -8)],
        "vista" => vec![temp(-5, 0), hs(20), bc(0, 20)],
        _ => vec![],
    }
}

/// The whole look: colour first, then the spatial filters.
pub fn look(studio: &Studio) -> (Vec<Adjustment>, Vec<Filter>) {
    let v = |k: &str| studio.look.get(k).copied().unwrap_or(0);
    let mut adj = vec![];
    let mut filters = vec![];
    if let Some((id, _)) = filter_list(studio.product).get(v("filter").max(0) as usize) {
        adj.extend(preset_adjustments(id));
    }
    if v("auto") != 0 {
        adj.push(Adjustment::AutoLevels);
    }
    if v("exposure") != 0 {
        adj.push(Adjustment::Exposure {
            stops: v("exposure") * 2,
        });
    }
    let brilliance = v("brilliance");
    let (sh, hi) = (
        v("shadows") + brilliance * 6 / 10,
        v("highlights") - brilliance * 4 / 10,
    );
    if sh != 0 || hi != 0 {
        adj.push(Adjustment::ShadowsHighlights {
            shadows: sh.clamp(-100, 100),
            highlights: hi.clamp(-100, 100),
        });
    }
    if v("brightness") != 0 || v("contrast") != 0 {
        adj.push(Adjustment::BrightnessContrast {
            brightness: v("brightness") / 2,
            contrast: v("contrast") / 2,
        });
    }
    let black = v("black-point");
    let white = v("white-point");
    if black != 0 || white != 0 {
        adj.push(Adjustment::Levels {
            channel: Channel::Value,
            in_black: (black.max(0) * 60 / 100) as u8,
            in_white: (255 - white.max(0) * 60 / 100) as u8,
            gamma: 100,
            out_black: ((-black).max(0) * 60 / 100) as u8,
            out_white: (255 - (-white).max(0) * 60 / 100) as u8,
        });
    }
    if v("saturation") != 0 {
        adj.push(Adjustment::HueSaturation {
            hue: 0,
            saturation: v("saturation"),
            lightness: 0,
        });
    }
    if v("vibrance") != 0 {
        adj.push(Adjustment::Vibrance {
            amount: v("vibrance"),
        });
    }
    if v("warmth") != 0 || v("tint") != 0 {
        adj.push(Adjustment::Temperature {
            temperature: v("warmth"),
            tint: v("tint"),
        });
    }
    let sharp = v("sharpness") + v("sharpen");
    if sharp > 0 {
        filters.push(Filter::Sharpen {
            amount: sharp.min(100) as u32,
        });
    }
    let local = v("definition") + v("pop");
    if local != 0 {
        filters.push(Filter::UnsharpMask {
            radius: 8,
            amount: local.unsigned_abs().min(100),
            threshold: 0,
        });
    }
    let noise = v("noise-reduction") + v("denoise");
    if noise > 0 {
        filters.push(Filter::Median {
            radius: 1 + (noise as u32) / 50,
        });
    }
    if v("vignette") != 0 {
        filters.push(Filter::Vignette {
            amount: v("vignette"),
        });
    }
    (adj, filters)
}

/// Apply the look (and the straighten angle) to a canvas.
pub fn apply(studio: &Studio, src: &Canvas) -> Canvas {
    let (adj, filters) = look(studio);
    let mut out = src.clone();
    for a in &adj {
        adjust::apply(&mut out, a, None);
    }
    for f in &filters {
        filter::apply(&mut out, f, None);
    }
    let angle = studio.look.get("straighten").copied().unwrap_or(0);
    if angle != 0 {
        out = cw_raster::transform::straighten(&out, angle * 100);
    }
    out
}

/// Whether anything in the look would change the picture.
pub fn is_identity(studio: &Studio) -> bool {
    studio.look.values().all(|v| *v == 0)
}

/// Choose a filter preset by id.
pub fn preset(studio: &mut Studio, id: &str) -> Result<(), String> {
    let list = filter_list(studio.product);
    if let Some(i) = list.iter().position(|(k, _)| *k == id) {
        studio.look.insert("filter".into(), i as i32);
        return Ok(());
    }
    if studio.product == Product::GooglePhotos {
        let set: &[(&str, i32)] = match id {
            "enhance" => &[("auto-enhance", 1), ("brightness", 10), ("saturation", 15)],
            "dynamic" => &[("contrast", 30), ("saturation", 25)],
            "warm" => &[("warmth", 35)],
            "cool" => &[("warmth", -35)],
            _ => return Err("unknown suggestion".into()),
        };
        for (k, v) in set {
            if *k == "auto-enhance" {
                continue;
            }
            studio.look.insert((*k).into(), *v);
        }
        studio.look.insert("suggestion".into(), 0);
        return Ok(());
    }
    Err("unknown filter".into())
}

/// Look commands: `look:reset`, `look:aspect:<id>`, `look:rotate`, `look:flip`,
/// `look:done` (write the edit).
pub fn command(studio: &mut Studio, window: u64, rest: &str) -> Result<Vec<AppEffect>, String> {
    let (op, arg) = rest.split_once(':').unwrap_or((rest, ""));
    match op {
        "reset" => {
            studio.look.clear();
            Ok(vec![])
        }
        "aspect" => {
            let (_, _, aw, ah) = ASPECTS
                .iter()
                .find(|a| a.0 == arg)
                .ok_or("unknown aspect ratio")?;
            let doc = studio.doc.as_ref().ok_or("no photo is open")?;
            let (w, h) = (doc.width(), doc.height());
            if *aw == 0 {
                return Ok(vec![]);
            }
            // The largest centred rectangle of that shape.
            let (cw, ch) = if u64::from(w) * u64::from(*ah) > u64::from(h) * u64::from(*aw) {
                ((u64::from(h) * u64::from(*aw) / u64::from(*ah)) as u32, h)
            } else {
                (w, (u64::from(w) * u64::from(*ah) / u64::from(*aw)) as u32)
            };
            let r = IRect::new(
                ((w - cw) / 2) as i32,
                ((h - ch) / 2) as i32,
                cw.max(1),
                ch.max(1),
            );
            studio.doc.as_mut().ok_or("no photo is open")?.crop(r)?;
            studio.modified = true;
            Ok(vec![])
        }
        "rotate" => {
            // Phones rotate counter-clockwise, a quarter at a time.
            studio
                .doc
                .as_mut()
                .ok_or("no photo is open")?
                .rotate_quarter(3)?;
            studio.modified = true;
            Ok(vec![])
        }
        "flip" => {
            studio.doc.as_mut().ok_or("no photo is open")?.flip(true)?;
            studio.modified = true;
            Ok(vec![])
        }
        "done" => done(studio, window, &[]),
        _ => Err(format!("unknown edit command {op}")),
    }
}

/// Write the edited photo. iOS saves over a PNG original (the only format written
/// here) and beside anything else; Google Photos always saves a copy. `taken` is the
/// folder's listing, so a copy never overwrites a file already there.
pub fn done(studio: &Studio, window: u64, taken: &[String]) -> Result<Vec<AppEffect>, String> {
    let doc = studio.doc.as_ref().ok_or("no photo is open")?;
    let baked = apply(studio, &doc.composite());
    let folder = super::folder_of(&studio.path);
    let path = if studio.product == Product::GooglePhotos {
        super::join(folder, &studio.suggested_name(taken, "-edited"))
    } else if studio.path.to_ascii_lowercase().ends_with(".png") {
        studio.path.clone()
    } else {
        super::join(folder, &studio.suggested_name(taken, ""))
    };
    Ok(vec![AppEffect::WriteImage {
        window,
        path,
        width: baked.width(),
        height: baked.height(),
        rgba: baked.into_pixels(),
    }])
}
