//! Drawing shared by every face of the player: cover artwork, icon buttons and the
//! scrubber. Artwork is `cw_artwork`'s composition for what it depicts, drawn with the
//! renderer's own shapes, so an album has the same cover in a grid, on its page, in the
//! player and on the music sites, and never pretends to be a photograph.
use super::{clock, Live, SEEK_STEPS};
use crate::desktop_scene::{shared::Align, Painter};
use cw_artwork::{Rgba, Shape};
use cw_scene::{Color, Rect};

fn colour(c: Rgba) -> Color {
    Color(c.0, c.1, c.2, c.3)
}
/// The colour `key`'s artwork washes a header or a Now Playing screen with.
pub fn tint(key: &str) -> Color {
    colour(cw_artwork::artwork(key).tint)
}
pub fn mix(a: Color, b: Color, pct: u32) -> Color {
    let c = |a: u8, b: u8| ((u32::from(a) * (100 - pct) + u32::from(b) * pct) / 100) as u8;
    Color(c(a.0, b.0), c(a.1, b.1), c(a.2, b.2), a.3)
}
/// Square (or, with `radius` at half the side, round) artwork for `key`.
pub fn cover(p: &mut Painter, r: Rect, key: &str, radius: u32) {
    if r.width == 0 || r.height == 0 {
        return;
    }
    let art = cw_artwork::artwork(key);
    let mark = p.scene.nodes.len();
    let side = i64::from(r.width.min(r.height));
    p.gradient(
        r,
        colour(art.top),
        colour(art.bottom),
        (r.height / 5).clamp(4, 32),
    );
    let unit = i64::from(cw_artwork::UNIT);
    let sx = |v: i32| r.x + (i64::from(v) * i64::from(r.width) / unit) as i32;
    let sy = |v: i32| r.y + (i64::from(v) * i64::from(r.height) / unit) as i32;
    let len = |v: i32| (i64::from(v) * side / unit) as i32;
    // A thumbnail keeps the composition's big shapes; specks smaller than a few pixels
    // would only cost nodes.
    let floor = if side < 96 && art.shapes.len() > 40 {
        3
    } else {
        1
    };
    for shape in &art.shapes {
        match shape {
            Shape::Circle {
                cx,
                cy,
                r: rad,
                color,
            } => {
                let rad = len(*rad);
                if rad >= floor {
                    p.circle(sx(*cx), sy(*cy), rad as u32, colour(*color));
                }
            }
            Shape::Ring {
                cx,
                cy,
                r: rad,
                width,
                color,
            } => {
                let rad = len(*rad);
                if rad >= floor {
                    p.ring(
                        sx(*cx),
                        sy(*cy),
                        rad as u32,
                        len(*width).max(1) as u32,
                        colour(*color),
                    );
                }
            }
            Shape::Polygon { points, color } => p.path(
                points.iter().map(|(x, y)| (sx(*x), sy(*y))).collect(),
                colour(*color),
            ),
        }
    }
    // Shapes run off the square the way a print is trimmed: the cover is its frame.
    for n in &mut p.scene.nodes[mark..] {
        n.clip = Some(
            n.clip
                .map_or(Some(r), |c| c.intersection(r))
                .unwrap_or(Rect::new(r.x, r.y, 0, 0)),
        );
    }
    p.round_clip_since(mark, r, radius);
}
/// Round artwork for an artist: their cover, cut to a circle.
pub fn avatar(p: &mut Painter, r: Rect, key: &str, _name: &str) {
    cover(p, r, key, r.width.min(r.height) / 2);
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

/// A volume slider across `r` at `level` percent. Every step is a real control that sets
/// the level it sits at: `<target><pct>`, which is `music:volume:` for the player's own
/// volume and `shell:set:volume:` for the machine's.
#[allow(clippy::too_many_arguments)]
pub fn volume_slider(
    p: &mut Painter,
    r: Rect,
    level: u8,
    target: &str,
    thickness: u32,
    played: Color,
    rest: Color,
    knob: Option<(u32, Color)>,
) {
    let y = r.y + (r.height as i32 - thickness as i32) / 2;
    p.box_(Rect::new(r.x, y, r.width, thickness), rest, thickness / 2);
    let done = r.width * u32::from(level.min(100)) / 100;
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
    let steps = super::VOLUME_STEPS + 1;
    for i in 0..steps {
        let x0 = r.x + (r.width * i / steps) as i32;
        let x1 = r.x + (r.width * (i + 1) / steps) as i32;
        let pct = i * 100 / super::VOLUME_STEPS;
        p.region_above(
            Rect::new(x0, r.y, (x1 - x0).max(1) as u32, r.height),
            &format!("{target}{pct}"),
            &format!("Volume {pct}%"),
        );
    }
}

/// The level a volume control shows and the target prefix it sets, in `theme`'s idiom.
/// On a phone the player's slider is the phone's own volume while it plays there, and
/// the speaker's (the session's) once it plays elsewhere; on a desktop it is the
/// player's own volume, which the machine's master volume then scales.
pub fn volume_binding(
    app: &super::Music,
    theme: crate::desktop_scene::DesktopTheme,
    system: u8,
) -> (u8, &'static str) {
    let player = app.catalog.player.as_ref();
    let casting = player.is_some_and(|p| !p.device.is_empty());
    if theme.mobile() && !casting {
        (system, "shell:set:volume:")
    } else {
        (player.map_or(0, |p| p.audible()), "music:volume:")
    }
}

/// Colours a lyrics view uses: the line being sung, lines already sung, lines to come.
pub struct LyricInk {
    pub now: Color,
    pub sung: Color,
    pub ahead: Color,
}
/// The playing song's lyrics in `r`, `size` pixels tall, the sung line lit and kept a
/// third of the way down as the song moves on. Each line is a control that seeks to
/// where it is sung. Returns false when there are none to show.
pub fn lyrics_view(
    p: &mut Painter,
    app: &super::Music,
    clock_us: u64,
    r: Rect,
    size: u16,
    ink: &LyricInk,
    align: Align,
) -> bool {
    let Some(live) = app.live(clock_us) else {
        return false;
    };
    let Some(track) = app.catalog.track(&live.id) else {
        return false;
    };
    if track.lyrics.is_empty() {
        let why = if track.instrumental() {
            "This song is instrumental."
        } else {
            "Lyrics aren't available for this song."
        };
        p.label(
            r.x,
            r.y + r.height as i32 / 3,
            r.width,
            why,
            size.min(20),
            ink.ahead,
            true,
            align,
        );
        return false;
    }
    let at = track.sung(live.position_ms);
    let typeface = p.typeface();
    let line_h = i32::from(size) * 13 / 10;
    let gap = i32::from(size) * 2 / 3;
    // Wrap every line first, so the offset that keeps the sung one in view is exact.
    let blocks: Vec<Vec<String>> = track
        .lyrics
        .iter()
        .map(|(_, line)| {
            cw_scene::metrics::wrap(typeface, true, line, size, r.width)
                .into_iter()
                .map(|l| l.trim_end().to_owned())
                .collect()
        })
        .collect();
    let tops: Vec<i32> = blocks
        .iter()
        .scan(0, |y, b| {
            let top = *y;
            *y += b.len().max(1) as i32 * line_h + gap;
            Some(top)
        })
        .collect();
    let anchor = at.map_or(0, |i| tops[i]);
    let shift = (anchor - r.height as i32 / 3).max(0);
    let mark = p.scene.nodes.len();
    for (i, block) in blocks.iter().enumerate() {
        let top = r.y + tops[i] - shift;
        let height = block.len().max(1) as i32 * line_h;
        if top + height < r.y || top > r.y + r.height as i32 {
            continue;
        }
        let colour = match at {
            Some(a) if a == i => ink.now,
            Some(a) if i < a => ink.sung,
            _ => ink.ahead,
        };
        p.button(
            Rect::new(r.x, top, r.width, height as u32),
            Color::TRANSPARENT,
            6,
            &format!("music:lyric:{i}"),
            &track.lyrics[i].1,
        );
        for (j, piece) in block.iter().enumerate() {
            p.label(
                r.x,
                top + j as i32 * line_h,
                r.width,
                piece,
                size,
                colour,
                true,
                align,
            );
        }
    }
    for n in &mut p.scene.nodes[mark..] {
        n.clip = Some(
            n.clip
                .map_or(Some(r), |c| c.intersection(r))
                .unwrap_or(Rect::new(r.x, r.y, 0, 0)),
        );
    }
    true
}

/// The output picker: this device, then each of the account's speakers that speak the
/// platform's protocol — AirPlay on Apple's players, Cast on Android, DLNA ("Cast to
/// device") on Windows. A speaker is only offered once it has answered; one that has not
/// is shown disabled with why. On a phone the picker also holds the volume.
#[allow(clippy::too_many_arguments)]
pub fn output_picker(
    p: &mut Painter,
    app: &super::Music,
    theme: crate::desktop_scene::DesktopTheme,
    system_volume: u8,
    r: Rect,
    bounds: Rect,
    s: &Surface,
) {
    let Some((protocol, here)) = super::output_protocol(theme) else {
        return;
    };
    let current = app
        .catalog
        .player
        .as_ref()
        .map(|p| p.device.clone())
        .unwrap_or_default();
    let devices: Vec<&super::Device> = app
        .catalog
        .devices
        .iter()
        .filter(|d| d.protocols.iter().any(|p| p == protocol))
        .collect();
    let row = u32::from(s.size) * 2 + 14;
    let head = u32::from(s.size) + 22;
    let slider = if theme.mobile() { 44 } else { 0 };
    let height = head + slider + row * (devices.len() as u32 + 1) + 10;
    let r = Rect::new(
        r.x.min(bounds.x + bounds.width as i32 - r.width as i32 - 4)
            .max(bounds.x + 4),
        r.y.min(bounds.y + bounds.height as i32 - height as i32 - 4)
            .max(bounds.y + 4),
        r.width,
        height,
    );
    p.z += 2;
    p.region(bounds, "music:popup-close", "Close");
    p.drop_shadow(r, s.radius, 16, 70, 6);
    p.border(r, s.fill, s.radius, s.line);
    let title = match protocol {
        "airplay" => "AirPlay",
        "cast" => "Cast to",
        _ => "Cast to device",
    };
    p.label(
        r.x + 14,
        r.y + 10,
        r.width - 28,
        title,
        s.size + 1,
        s.ink,
        true,
        Align::Left,
    );
    let mut y = r.y + head as i32;
    if slider > 0 {
        let (level, target) = volume_binding(app, theme, system_volume);
        p.symbol("volume-mute", r.x + 14, y + 12, 16, s.muted);
        volume_slider(
            p,
            Rect::new(r.x + 40, y + 6, r.width - 80, 28),
            level,
            target,
            6,
            s.ink,
            s.line,
            Some((18, Color::WHITE)),
        );
        p.symbol("volume", r.x + r.width as i32 - 32, y + 12, 16, s.muted);
        y += slider as i32;
    }
    let item = |p: &mut Painter,
                y: i32,
                symbol: &str,
                name: &str,
                detail: &str,
                target: Option<String>,
                why: &str,
                on: bool| {
        let rr = Rect::new(r.x + 6, y, r.width - 12, row);
        match &target {
            Some(t) => p.button(
                rr,
                if on {
                    Color(s.accent.0, s.accent.1, s.accent.2, 30)
                } else {
                    Color::TRANSPARENT
                },
                s.radius.min(8),
                t,
                name,
            ),
            None => {
                p.box_(rr, Color::TRANSPARENT, 0);
                p.disabled(why);
            }
        }
        let ink = if target.is_some() { s.ink } else { s.muted };
        p.symbol(symbol, rr.x + 10, y + (row as i32 - 20) / 2, 20, ink);
        p.label(
            rr.x + 42,
            y + 6,
            rr.width - 80,
            name,
            s.size,
            ink,
            on,
            Align::Left,
        );
        p.label(
            rr.x + 42,
            y + 8 + i32::from(s.size) + 2,
            rr.width - 80,
            detail,
            s.size.saturating_sub(2).max(9),
            s.muted,
            false,
            Align::Left,
        );
        if on {
            p.symbol(
                "check",
                rr.x + rr.width as i32 - 28,
                y + (row as i32 - 16) / 2,
                16,
                s.accent,
            );
        }
    };
    let local = current.is_empty();
    item(
        p,
        y,
        if theme.mobile() {
            "cellular"
        } else {
            "desktop"
        },
        here,
        if local { "Playing here" } else { "" },
        Some("music:output:".into()),
        "",
        local,
    );
    y += row as i32;
    for d in devices {
        let on = d.id == current;
        let symbol = if d.kind == "tv" { "display" } else { "volume" };
        let (target, detail, why) = match app.reachable.get(&d.id) {
            _ if on => (Some(format!("music:output:{}", d.id)), "Playing", ""),
            Some(true) => (Some(format!("music:output:{}", d.id)), "", ""),
            Some(false) => (
                None,
                "Not available",
                "This device is not answering on the network",
            ),
            None => (None, "Looking…", "Still waiting for this device to answer"),
        };
        item(p, y, symbol, &d.name, detail, target, why, on);
        y += row as i32;
    }
    p.z -= 2;
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
