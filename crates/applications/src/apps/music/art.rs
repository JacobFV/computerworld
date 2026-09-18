//! Drawing shared by every face of the player: stand-in artwork, icon buttons and the
//! scrubber. Artwork is a flat gradient chosen by what it depicts, so an album is the same
//! colour in a grid, on its page and in the player, and never pretends to be a photograph.
use super::{clock, Live, SEEK_STEPS};
use crate::desktop_scene::{shared::Align, Painter};
use cw_scene::{Color, Rect};

const PALETTE: [(u8, u8, u8); 10] = [
    (232, 76, 61),
    (241, 148, 64),
    (236, 196, 64),
    (88, 178, 104),
    (46, 160, 170),
    (66, 120, 216),
    (122, 92, 214),
    (196, 78, 160),
    (58, 62, 88),
    (150, 110, 84),
];

fn hash(key: &str) -> u64 {
    key.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    })
}
/// The base colour of `key`'s artwork.
pub fn tint(key: &str) -> Color {
    let (r, g, b) = PALETTE[(hash(key) % PALETTE.len() as u64) as usize];
    Color::rgb(r, g, b)
}
pub fn mix(a: Color, b: Color, pct: u32) -> Color {
    let c = |a: u8, b: u8| ((u32::from(a) * (100 - pct) + u32::from(b) * pct) / 100) as u8;
    Color(c(a.0, b.0), c(a.1, b.1), c(a.2, b.2), a.3)
}
/// Square (or, with `radius` at half the side, round) artwork for `key`.
pub fn cover(p: &mut Painter, r: Rect, key: &str, radius: u32) {
    let base = tint(key);
    let mark = p.scene.nodes.len();
    p.gradient(
        r,
        mix(base, Color::WHITE, 22),
        mix(base, Color::BLACK, 18),
        32,
    );
    p.round_clip_since(mark, r, radius);
    let glyph = (r.width.min(r.height) / 3).clamp(8, 64);
    p.symbol(
        "music",
        r.x + (r.width as i32 - glyph as i32) / 2,
        r.y + (r.height as i32 - glyph as i32) / 2,
        glyph,
        Color(255, 255, 255, 120),
    );
}
/// Round artwork for an artist: the same tint, with their initial.
pub fn avatar(p: &mut Painter, r: Rect, key: &str, name: &str) {
    let base = tint(key);
    p.box_(r, mix(base, Color::BLACK, 10), r.width / 2);
    let initial: String = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    let size = (r.height * 2 / 5).clamp(9, 60) as u16;
    p.label(
        r.x,
        r.y + (r.height as i32 - i32::from(size) * 3 / 2) / 2,
        r.width,
        &initial,
        size,
        Color::WHITE,
        true,
        Align::Center,
    );
}
/// A glyph that is a real button: the whole square takes the click.
#[allow(clippy::too_many_arguments)]
pub fn icon(
    p: &mut Painter,
    r: Rect,
    symbol: &str,
    size: u32,
    color: Color,
    target: &str,
    label: &str,
) {
    p.button(
        r,
        Color::TRANSPARENT,
        r.width.min(r.height) / 2,
        target,
        label,
    );
    p.symbol(
        symbol,
        r.x + (r.width as i32 - size as i32) / 2,
        r.y + (r.height as i32 - size as i32) / 2,
        size,
        color,
    );
}
/// A glyph whose control cannot act right now, announced disabled with the reason.
pub fn icon_off(p: &mut Painter, r: Rect, symbol: &str, size: u32, color: Color, why: &str) {
    p.symbol(
        symbol,
        r.x + (r.width as i32 - size as i32) / 2,
        r.y + (r.height as i32 - size as i32) / 2,
        size,
        Color(color.0, color.1, color.2, color.3 / 3),
    );
    p.disabled(why);
}
/// The scrubber: a bar of `thickness` across `r`, played part in `played`, and click
/// targets across it that seek to where they sit. `knob` draws the playhead.
#[allow(clippy::too_many_arguments)]
pub fn scrubber(
    p: &mut Painter,
    r: Rect,
    live: &Live,
    thickness: u32,
    played: Color,
    rest: Color,
    knob: Option<(u32, Color)>,
) {
    let y = r.y + (r.height as i32 - thickness as i32) / 2;
    p.box_(Rect::new(r.x, y, r.width, thickness), rest, thickness / 2);
    let done = (u64::from(r.width) * live.position_ms)
        .checked_div(live.duration_ms)
        .unwrap_or(0)
        .min(u64::from(r.width)) as u32;
    if done > 0 {
        p.box_(Rect::new(r.x, y, done, thickness), played, thickness / 2);
    }
    if let Some((size, colour)) = knob {
        p.circle(
            r.x + done as i32,
            y + thickness as i32 / 2,
            size / 2,
            colour,
        );
    }
    // One target per couple of pixels, capped: a click lands within half a step of where
    // it was aimed, and an agent can name any step it likes with `music:seek:<n>`.
    let steps = (r.width / 3).clamp(1, 200);
    for i in 0..steps {
        let x0 = r.x + (r.width * i / steps) as i32;
        let x1 = r.x + (r.width * (i + 1) / steps) as i32;
        let step = u64::from(i) * SEEK_STEPS / u64::from(steps);
        let at = live.duration_ms * step / SEEK_STEPS;
        p.region_above(
            Rect::new(x0, r.y, (x1 - x0).max(1) as u32, r.height),
            &format!("music:seek:{step}"),
            &format!("Seek to {}", clock(at)),
        );
    }
}
/// A tappable row: its background, and the click that opens or plays it.
pub fn row(p: &mut Painter, r: Rect, fill: Color, radius: u32, target: &str, label: &str) {
    p.button(r, fill, radius, target, label);
}

/// Colours of a menu or sheet in one platform's idiom.
pub struct Surface {
    pub fill: Color,
    pub ink: Color,
    pub muted: Color,
    pub line: Color,
    pub accent: Color,
    pub radius: u32,
    pub size: u16,
}
/// The song menu, anchored at `(x, y)` and kept inside `bounds`. Every row is either a
/// real command or announced disabled with its reason.
pub fn menu(
    p: &mut Painter,
    app: &super::Music,
    theme: crate::desktop_scene::DesktopTheme,
    x: i32,
    y: i32,
    bounds: Rect,
    s: &Surface,
) {
    let items = app.menu_items(theme);
    if items.is_empty() {
        return;
    }
    let row = u32::from(s.size) + 14;
    let width = 230.min(bounds.width.saturating_sub(16));
    let height = row * items.len() as u32 + 12;
    let x = x
        .min(bounds.x + bounds.width as i32 - width as i32 - 8)
        .max(bounds.x + 8);
    let y = y
        .min(bounds.y + bounds.height as i32 - height as i32 - 8)
        .max(bounds.y + 8);
    let r = Rect::new(x, y, width, height);
    // A click anywhere else closes the menu, as it does on every platform.
    p.z += 2;
    p.region(bounds, "music:menu-close", "Close menu");
    p.drop_shadow(r, s.radius, 16, 70, 6);
    p.border(r, s.fill, s.radius, s.line);
    let mut cy = y + 6;
    for item in items {
        let rr = Rect::new(x + 5, cy, width - 10, row);
        match &item.target {
            Some(target) => {
                p.button(rr, Color::TRANSPARENT, s.radius.min(6), target, &item.label);
                p.left(
                    rr.x + 10,
                    rr.y + (row as i32 - i32::from(s.size) * 3 / 2) / 2,
                    rr.width - 20,
                    &item.label,
                    s.size,
                    if item.destructive { s.accent } else { s.ink },
                );
            }
            None => {
                p.left(
                    rr.x + 10,
                    rr.y + (row as i32 - i32::from(s.size) * 3 / 2) / 2,
                    rr.width - 20,
                    &item.label,
                    s.size,
                    s.muted,
                );
                p.disabled(item.why);
            }
        }
        cy += row as i32;
    }
    p.z -= 2;
}
/// The New Playlist sheet: a title field, Create and Cancel.
pub fn composer(p: &mut Painter, draft: &str, bounds: Rect, s: &Surface) {
    let width = 320.min(bounds.width.saturating_sub(24));
    let r = Rect::new(
        bounds.x + (bounds.width as i32 - width as i32) / 2,
        bounds.y + (bounds.height as i32 - 150) / 2,
        width,
        150,
    );
    p.z += 3;
    p.box_(bounds, Color(0, 0, 0, 60), 0);
    // The sheet is modal: a click beside it goes to its field, never to what is under it.
    p.region(bounds, "music:compose", "New Playlist");
    p.drop_shadow(r, s.radius, 20, 80, 8);
    p.border(r, s.fill, s.radius, s.line);
    p.label(
        r.x,
        r.y + 16,
        r.width,
        "New Playlist",
        s.size + 2,
        s.ink,
        true,
        Align::Center,
    );
    let field = Rect::new(r.x + 16, r.y + 50, r.width - 32, 32);
    p.border(field, Color(0, 0, 0, 0), 6, s.line);
    p.region(field, "music:compose", "Playlist title");
    p.left(
        field.x + 10,
        field.y + 8,
        field.width - 20,
        if draft.is_empty() { "Title" } else { draft },
        s.size,
        if draft.is_empty() { s.muted } else { s.ink },
    );
    let half = (r.width - 44) / 2;
    let cancel = Rect::new(r.x + 16, r.y + 102, half, 32);
    p.button(
        cancel,
        Color(128, 128, 128, 40),
        8,
        "music:cancel",
        "Cancel",
    );
    p.label(
        cancel.x,
        cancel.y + 8,
        cancel.width,
        "Cancel",
        s.size,
        s.ink,
        false,
        Align::Center,
    );
    let create = Rect::new(r.x + 28 + half as i32, r.y + 102, half, 32);
    if draft.trim().is_empty() {
        p.box_(create, Color(s.accent.0, s.accent.1, s.accent.2, 90), 8);
        p.disabled("A playlist needs a title");
        p.label(
            create.x,
            create.y + 8,
            create.width,
            "Create",
            s.size,
            Color::WHITE,
            true,
            Align::Center,
        );
    } else {
        p.button(create, s.accent, 8, "music:create", "Create");
        p.label(
            create.x,
            create.y + 8,
            create.width,
            "Create",
            s.size,
            Color::WHITE,
            true,
            Align::Center,
        );
    }
    p.z -= 3;
}
