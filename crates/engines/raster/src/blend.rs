//! Layer compositing: the W3C Compositing and Blending model (source-over with a
//! separable blend function), in exact integer arithmetic on straight alpha.
use crate::fmath::div255;
use crate::Rgba;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Overlay,
    /// Linear dodge: the channels are summed and clipped.
    Add,
    Darken,
    Lighten,
}
impl BlendMode {
    pub const ALL: [BlendMode; 7] = [
        Self::Normal,
        Self::Multiply,
        Self::Screen,
        Self::Overlay,
        Self::Add,
        Self::Darken,
        Self::Lighten,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Multiply => "multiply",
            Self::Screen => "screen",
            Self::Overlay => "overlay",
            Self::Add => "add",
            Self::Darken => "darken",
            Self::Lighten => "lighten",
        }
    }
    pub fn parse(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }
    /// The name most editors print. GIMP says "Addition" and Pixelmator "Linear Dodge"
    /// for `Add`; interfaces that differ pass their own label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Multiply => "Multiply",
            Self::Screen => "Screen",
            Self::Overlay => "Overlay",
            Self::Add => "Add",
            Self::Darken => "Darken",
            Self::Lighten => "Lighten",
        }
    }
    /// `B(Cb, Cs)`: the blended colour of backdrop `cb` and source `cs`, both 0..=255.
    #[inline]
    pub fn channel(self, cb: u32, cs: u32) -> u32 {
        match self {
            Self::Normal => cs,
            Self::Multiply => div255(cb * cs),
            Self::Screen => cb + cs - div255(cb * cs),
            // Overlay is hard light with the operands exchanged.
            Self::Overlay => {
                if 2 * cb <= 255 {
                    div255(2 * cb * cs)
                } else {
                    let s = 2 * cb - 255;
                    cs + s - div255(cs * s)
                }
            }
            Self::Add => (cb + cs).min(255),
            Self::Darken => cb.min(cs),
            Self::Lighten => cb.max(cs),
        }
    }
}

/// Composite `src` over `dst`, the source alpha scaled by `opacity` (0..=255).
///
/// With `αs` the effective source alpha and `αb` the backdrop's:
/// `Cs' = (1 - αb)·Cs + αb·B(Cb, Cs)`, `αo = αs + αb·(1 - αs)` and
/// `Co = (αs·Cs' + αb·Cb·(1 - αs)) / αo`, each rounded once at the end.
#[inline]
pub fn composite(dst: Rgba, src: Rgba, opacity: u8, mode: BlendMode) -> Rgba {
    let sa = div255(u32::from(src[3]) * u32::from(opacity));
    if sa == 0 {
        return dst;
    }
    let da = u32::from(dst[3]);
    if da == 0 {
        return [src[0], src[1], src[2], sa as u8];
    }
    let oa = sa + da - div255(sa * da);
    let denom = 255 * oa;
    let mut out = [0u8; 4];
    for c in 0..3 {
        let cs = u32::from(src[c]);
        let cb = u32::from(dst[c]);
        let mixed = (255 - da) * cs + da * mode.channel(cb, cs);
        let num = sa * mixed + da * cb * (255 - sa);
        out[c] = ((num + denom / 2) / denom).min(255) as u8;
    }
    out[3] = oa as u8;
    out
}

/// Reduce a pixel's alpha by `amount` (0..=255): what an eraser does.
#[inline]
pub fn erase(dst: Rgba, amount: u32) -> Rgba {
    let a = div255(u32::from(dst[3]) * (255 - amount.min(255)));
    if a == 0 {
        return [0, 0, 0, 0];
    }
    [dst[0], dst[1], dst[2], a as u8]
}

/// Linear mix of two pixels, `t` of the way from `a` to `b` (0..=255). Used to apply a
/// whole-layer result only where a selection covers it.
#[inline]
pub fn mix(a: Rgba, b: Rgba, t: u32) -> Rgba {
    if t == 0 {
        return a;
    }
    if t >= 255 {
        return b;
    }
    let mut out = [0u8; 4];
    for c in 0..4 {
        let (x, y) = (u32::from(a[c]), u32::from(b[c]));
        out[c] = div255(x * (255 - t) + y * t) as u8;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    const RED: Rgba = [200, 0, 0, 255];
    const GREY: Rgba = [100, 100, 100, 255];
    #[test]
    fn opaque_blend_modes_match_their_formulas() {
        let over = |mode| composite(GREY, RED, 255, mode);
        assert_eq!(over(BlendMode::Normal), RED);
        // 200*100/255 = 78.4 -> 78; 0*100 = 0.
        assert_eq!(over(BlendMode::Multiply), [78, 0, 0, 255]);
        // 100 + 200 - 78 = 222; 100 + 0 - 0 = 100.
        assert_eq!(over(BlendMode::Screen), [222, 100, 100, 255]);
        // Backdrop 100 <= 127: 2*100*200/255 = 156.9 -> 157; 2*100*0 = 0.
        assert_eq!(over(BlendMode::Overlay), [157, 0, 0, 255]);
        assert_eq!(over(BlendMode::Add), [255, 100, 100, 255]);
        assert_eq!(over(BlendMode::Darken), [100, 0, 0, 255]);
        assert_eq!(over(BlendMode::Lighten), [200, 100, 100, 255]);
        // Overlay over a bright backdrop screens: s = 2*220-255 = 185;
        // 200 + 185 - 200*185/255 (145.1 -> 145) = 240.
        assert_eq!(
            composite([220, 220, 220, 255], RED, 255, BlendMode::Overlay)[0],
            240
        );
    }
    #[test]
    fn alpha_and_opacity_compose_like_source_over() {
        // Half-opaque black over white is mid grey.
        assert_eq!(
            composite([255; 4], [0, 0, 0, 255], 128, BlendMode::Normal),
            [127, 127, 127, 255]
        );
        // Anything over transparency keeps its colour and takes its own alpha.
        assert_eq!(
            composite([0; 4], RED, 128, BlendMode::Multiply),
            [200, 0, 0, 128]
        );
        // Zero opacity changes nothing.
        assert_eq!(composite(GREY, RED, 0, BlendMode::Normal), GREY);
        // Translucent over translucent: αo = 128 + 128 - 64 = 192.
        let out = composite([0, 0, 255, 128], [255, 0, 0, 128], 255, BlendMode::Normal);
        assert_eq!(out, [170, 0, 85, 192]);
        assert_eq!(erase([10, 20, 30, 255], 255), [0, 0, 0, 0]);
        assert_eq!(erase([10, 20, 30, 200], 128), [10, 20, 30, 100]);
        assert_eq!(
            mix([0, 0, 0, 255], [255, 255, 255, 255], 51),
            [51, 51, 51, 255]
        );
        for mode in BlendMode::ALL {
            assert_eq!(BlendMode::parse(mode.id()), Some(mode));
        }
    }
}
