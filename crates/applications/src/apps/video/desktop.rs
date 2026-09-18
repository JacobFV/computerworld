//! The three desktop editors, each laid out as the real one is:
//!
//! - **Clipchamp** (Windows 11): project title and Export across the top, an icon
//!   toolbar and its media/text/transitions/backgrounds panel on the left, the stage
//!   and its playback bar in the middle, the property panel's icon tabs on the right,
//!   and the timeline with its toolbar along the bottom.
//! - **iMovie** (macOS): a toolbar with Import, the media/audio/titles/backgrounds/
//!   transitions tabs and Share; the browser on the left and the viewer on the right
//!   under the adjustments bar; the magnetic timeline below with no track headers.
//! - **Kdenlive** (Ubuntu): a main toolbar with Render; Project Bin, Effect Stack /
//!   Compositions and the Project Monitor side by side; the timeline with its track
//!   headers (lock, hide, mute) and tool bar at the bottom.
use super::widgets::{
    self, chip, slider, target, text_button, text_button_off, tool, tool_off, Lanes, Skin,
};
use super::{Editor, Field, Product, Prop};
use crate::desktop_scene::{shared::Align, Painter};
use crate::AppEnv;
use cw_scene::{Color, Primitive, Rect};
use cw_video::{Source, TrackKind, TransitionKind};

pub fn render(ed: &Editor, p: &mut Painter, env: &AppEnv<'_>) {
    let s = widgets::skin(ed.product, false);
    p.scene.background = s.bg;
    p.box_(Rect::new(0, 0, env.width, env.height), s.bg, 0);
    match ed.product {
        Product::Clipchamp => clipchamp(ed, p, env, &s),
        Product::Imovie => imovie(ed, p, env, &s),
        Product::Kdenlive | Product::VideoEditor => kdenlive(ed, p, env, &s),
    }
    widgets::sheet(p, ed, env, &s);
}

const BACKGROUNDS: [(&str, &str); 8] = [
    ("000000", "Black"),
    ("ffffff", "White"),
    ("1e3a8a", "Navy"),
    ("0f766e", "Teal"),
    ("b91c1c", "Red"),
    ("f59e0b", "Amber"),
    ("7c3aed", "Violet"),
    ("374151", "Slate"),
];
const TITLE_STYLES: [(&str, &str); 5] = [
    ("plain", "Plain text"),
    ("headline", "Headline"),
    ("lower", "Lower third"),
    ("top", "Caption"),
    ("credits", "Credits"),
];

/// Timeline height for a window of `h`.
fn timeline_height(h: u32) -> u32 {
    (h * 2 / 5).clamp(150, 320).min(h / 2)
}

/// Lays inspector controls out in one or more columns: sliders fill cells left to right,
/// and rows of chips take a full line.
struct Flow {
    x: i32,
    y: i32,
    w: u32,
    cols: u32,
    col: u32,
    row: i32,
}
impl Flow {
    fn cell_w(&self) -> u32 {
        (self.w.saturating_sub((self.cols - 1) * 20)) / self.cols
    }
    #[allow(clippy::too_many_arguments)]
    fn slider(
        &mut self,
        p: &mut Painter,
        ed: &Editor,
        env: &AppEnv<'_>,
        s: &Skin,
        label: &str,
        prop: Prop,
        unit: &str,
    ) {
        let cw = self.cell_w();
        let x = self.x + (self.col * (cw + 20)) as i32;
        let end = slider(p, ed, env, s, x, self.y, cw, label, prop, unit);
        self.row = self.row.max(end - self.y);
        self.col += 1;
        if self.col == self.cols {
            self.break_line();
        }
    }
    fn break_line(&mut self) {
        if self.col > 0 || self.row > 0 {
            self.y += self.row;
        }
        self.col = 0;
        self.row = 0;
    }
    /// A full-width line of `h` pixels at the current position.
    fn line(&mut self, h: i32) -> (i32, i32) {
        self.break_line();
        let at = (self.x, self.y);
        self.y += h;
        at
    }
}

/// Inspector pages: the controls for one aspect of the selected clip.
#[allow(clippy::too_many_arguments)]
fn inspector(
    p: &mut Painter,
    ed: &Editor,
    env: &AppEnv<'_>,
    s: &Skin,
    x: i32,
    y: i32,
    w: u32,
    page: &str,
    cols: u32,
) -> i32 {
    let feats = ed.features(false);
    let mut f = Flow {
        x,
        y,
        w,
        cols: cols.max(1),
        col: 0,
        row: 0,
    };
    let Some(c) = ed.selected_clip() else {
        if let Some(t) = ed
            .selected_transition
            .and_then(|id| ed.project.transitions.iter().find(|t| t.id == id))
        {
            let (lx, ly) = f.line(24);
            p.strong(lx, ly, w, ed.product.transition_name(t.kind), 13, s.text);
            f.slider(p, ed, env, s, "Duration", Prop::Transition, "");
            let (lx, ly) = f.line(34);
            text_button(p, s, Rect::new(lx, ly, 120, 26), "Remove", "delete", false);
            return f.y;
        }
        let (lx, ly) = f.line(24);
        p.left(lx, ly, w, "Select a clip on the timeline", 12, s.muted);
        return f.y;
    };
    let audio = ed.project.track(c.track).map(|t| t.kind) == Some(TrackKind::Audio);
    match page {
        "audio" | "volume" => {
            f.slider(p, ed, env, s, "Volume", Prop::Volume, "%");
            let muted = ed.value(Prop::Volume, env.clock_us) == Some(0);
            let (lx, ly) = f.line(32);
            chip(
                p,
                s,
                Rect::new(lx, ly, 90, 24),
                if muted { "Unmute" } else { "Mute" },
                "mute-clip",
                muted,
            );
        }
        "fade" => {
            f.slider(p, ed, env, s, "Fade in", Prop::FadeIn, "");
            f.slider(p, ed, env, s, "Fade out", Prop::FadeOut, "");
        }
        "color" if !audio => {
            f.slider(
                p,
                ed,
                env,
                s,
                if ed.product == Product::Clipchamp {
                    "Exposure"
                } else {
                    "Brightness"
                },
                Prop::Brightness,
                "",
            );
            f.slider(p, ed, env, s, "Contrast", Prop::Contrast, "");
            f.slider(p, ed, env, s, "Saturation", Prop::Saturation, "");
            f.slider(p, ed, env, s, "Temperature", Prop::Temperature, "");
            if feats.opacity && ed.product == Product::Clipchamp {
                f.slider(p, ed, env, s, "Transparency", Prop::Opacity, "");
            }
        }
        "speed" => {
            f.slider(p, ed, env, s, "Speed", Prop::Speed, "");
            let (mut bx, ly) = f.line(32);
            for (label, v) in [
                ("0.25x", 25),
                ("0.5x", 50),
                ("1x", 100),
                ("2x", 200),
                ("4x", 400),
            ] {
                chip(
                    p,
                    s,
                    Rect::new(bx, ly, 46, 24),
                    label,
                    &format!("set:speed:{v}"),
                    c.speed == v,
                );
                bx += 50;
            }
            if feats.reverse {
                let (lx, ly) = f.line(32);
                chip(
                    p,
                    s,
                    Rect::new(lx, ly, 90, 24),
                    "Reverse",
                    "reverse",
                    c.reverse,
                );
            }
        }
        "transform" if !audio => {
            f.slider(p, ed, env, s, "Position X", Prop::X, " px");
            f.slider(p, ed, env, s, "Position Y", Prop::Y, " px");
            f.slider(p, ed, env, s, "Scale", Prop::Scale, "");
            f.slider(p, ed, env, s, "Rotation", Prop::Rotation, "");
            if ed.product == Product::Kdenlive {
                f.slider(p, ed, env, s, "Opacity", Prop::Opacity, "");
            }
            let (mut bx, ly) = f.line(32);
            for (label, cmd) in [("Rotate 90°", "rotate"), ("Reset", "reset")] {
                let cw = p.measure(label, 11, false) + 16;
                chip(p, s, Rect::new(bx, ly, cw, 24), label, cmd, false);
                bx += cw as i32 + 6;
            }
        }
        "crop" if !audio => {
            if feats.ken_burns {
                let (mut bx, ly) = f.line(34);
                let ken = c.scale.animated();
                for (label, cmd, on) in [
                    ("Fit", "fit", c.scale.value == 1000 && !ken),
                    ("Crop to Fill", "fill", c.scale.value > 1000 && !ken),
                    ("Ken Burns", "ken-burns", ken),
                ] {
                    let cw = p.measure(label, 11, false) + 16;
                    chip(p, s, Rect::new(bx, ly, cw, 24), label, cmd, on);
                    bx += cw as i32 + 6;
                }
                chip(p, s, Rect::new(bx, ly, 34, 24), "↻", "rotate", false);
            }
            f.slider(p, ed, env, s, "Left", Prop::CropLeft, "");
            f.slider(p, ed, env, s, "Right", Prop::CropRight, "");
            f.slider(p, ed, env, s, "Top", Prop::CropTop, "");
            f.slider(p, ed, env, s, "Bottom", Prop::CropBottom, "");
        }
        "overlay" if !audio => {
            let (lx, ly) = f.line(20);
            p.left(lx, ly, w, "Picture in Picture", 12, s.muted);
            let (mut bx, ly) = f.line(32);
            for (label, corner) in [
                ("↖", "top-left"),
                ("↗", "top-right"),
                ("↙", "bottom-left"),
                ("↘", "bottom-right"),
            ] {
                chip(
                    p,
                    s,
                    Rect::new(bx, ly, 34, 24),
                    label,
                    &format!("pip:{corner}"),
                    false,
                );
                bx += 38;
            }
            chip(p, s, Rect::new(bx + 8, ly, 70, 24), "Full", "reset", false);
            f.slider(p, ed, env, s, "Opacity", Prop::Opacity, "");
        }
        "title" => {
            f.break_line();
            f.y = widgets::title_controls(p, ed, env, s, x, f.y, w);
        }
        _ => {
            let (lx, ly) = f.line(24);
            p.left(
                lx,
                ly,
                w,
                if audio {
                    "Sound has no picture to adjust"
                } else {
                    "Choose a property"
                },
                12,
                s.muted,
            );
        }
    }
    f.break_line();
    f.y
}

fn transitions_panel(p: &mut Painter, ed: &Editor, s: &Skin, r: Rect, cols: u32) {
    let site = ed.transition_site().is_ok();
    let tw = (r.width.saturating_sub(8 * (cols - 1))) / cols;
    for (i, kind) in TransitionKind::ALL.into_iter().enumerate() {
        let (c, row) = (i as u32 % cols, i as u32 / cols);
        let cell = Rect::new(r.x + (c * (tw + 8)) as i32, r.y + (row * 74) as i32, tw, 66);
        let name = ed.product.transition_name(kind);
        if site {
            p.button(
                cell,
                s.raised,
                s.radius,
                &target(&format!("add-transition:{}", kind.id())),
                name,
            );
        } else {
            p.box_(cell, s.panel, s.radius);
            p.disabled("Select a clip that another clip follows");
        }
        // A small picture of what the transition does.
        let pic = Rect::new(cell.x + 8, cell.y + 8, cell.width - 16, 32);
        let (a, b) = (Color::rgb(74, 108, 170), Color::rgb(200, 120, 60));
        match kind {
            TransitionKind::CrossDissolve => {
                p.box_(pic, a, 3);
                p.box_(pic, Color(b.0, b.1, b.2, 128), 3);
            }
            TransitionKind::DipToBlack | TransitionKind::DipToWhite => {
                let mid = if kind == TransitionKind::DipToBlack {
                    Color::BLACK
                } else {
                    Color::WHITE
                };
                let third = pic.width / 3;
                p.box_(Rect::new(pic.x, pic.y, third, pic.height), a, 0);
                p.box_(
                    Rect::new(pic.x + third as i32, pic.y, third, pic.height),
                    mid,
                    0,
                );
                p.box_(
                    Rect::new(
                        pic.x + 2 * third as i32,
                        pic.y,
                        pic.width - 2 * third,
                        pic.height,
                    ),
                    b,
                    0,
                );
            }
            TransitionKind::Wipe => {
                p.box_(pic, a, 0);
                p.box_(Rect::new(pic.x, pic.y, pic.width / 2, pic.height), b, 0);
            }
            TransitionKind::Slide => {
                p.box_(pic, a, 0);
                p.box_(
                    Rect::new(
                        pic.x + pic.width as i32 / 3,
                        pic.y,
                        pic.width - pic.width / 3,
                        pic.height,
                    ),
                    b,
                    0,
                );
            }
        }
        p.label(
            cell.x,
            cell.y + 44,
            cell.width,
            name,
            11,
            if site { s.text } else { s.muted },
            false,
            Align::Center,
        );
    }
}

fn titles_panel(p: &mut Painter, ed: &Editor, s: &Skin, r: Rect) -> i32 {
    let mut y = r.y;
    for (style, label) in TITLE_STYLES {
        let row = Rect::new(r.x, y, r.width, 44);
        p.button(
            row,
            s.raised,
            s.radius,
            &target(&format!("add-title:{style}")),
            &format!("Add {label}"),
        );
        let (size, bold) = match style {
            "headline" => (18, true),
            "credits" => (15, true),
            "lower" => (13, true),
            _ => (14, false),
        };
        p.label(
            row.x + 12,
            row.y + (44 - size as i32 - 6) / 2,
            row.width - 50,
            label,
            size,
            s.text,
            bold,
            Align::Left,
        );
        p.symbol(
            "plus",
            row.x + row.width as i32 - 28,
            row.y + 14,
            16,
            s.accent,
        );
        y += 50;
    }
    if let Some(problem) = &ed.text_problem {
        p.left(r.x, y, r.width, problem, 11, Color::rgb(255, 120, 120));
        y += 20;
    }
    y
}

fn backgrounds_panel(p: &mut Painter, s: &Skin, r: Rect) {
    let cols = (r.width / 64).max(1);
    let tw = r.width / cols - 6;
    for (i, (hex, name)) in BACKGROUNDS.iter().enumerate() {
        let (c, row) = (i as u32 % cols, i as u32 / cols);
        let cell = Rect::new(
            r.x + (c * (tw + 6)) as i32,
            r.y + (row * (tw * 9 / 16 + 24)) as i32,
            tw,
            tw * 9 / 16,
        );
        let color = cw_raster::parse_hex(hex)
            .map(widgets::rgba)
            .unwrap_or(Color::BLACK);
        p.button(
            cell,
            color,
            s.radius,
            &target(&format!("add-color:{hex}")),
            &format!("Add {name} background"),
        );
        p.border(cell, Color::TRANSPARENT, s.radius, s.line);
        p.left(
            cell.x,
            cell.y + cell.height as i32 + 2,
            tw,
            name,
            10,
            s.muted,
        );
    }
}

/// The timeline area's own toolbar: undo/redo, split, delete, snapping and zoom.
fn timeline_bar(
    p: &mut Painter,
    ed: &Editor,
    env: &AppEnv<'_>,
    s: &Skin,
    r: Rect,
    width_for_fit: u32,
) {
    p.box_(r, s.bar, 0);
    p.hline(r.x, r.y + r.height as i32 - 1, r.width, s.line);
    let b = r.height.saturating_sub(8).min(28);
    let y = r.y + (r.height as i32 - b as i32) / 2;
    let mut x = r.x + 8;
    let mut put =
        |p: &mut Painter, symbol: &str, label: &str, command: &str, off: Option<&str>, on: bool| {
            let rect = Rect::new(x, y, b, b);
            match off {
                Some(why) => tool_off(p, s, rect, symbol, why),
                None => tool(p, s, rect, symbol, label, command, on, false),
            }
            x += b as i32 + 4;
        };
    let now = ed.now(env.clock_us);
    let can_split = ed
        .project
        .clips
        .iter()
        .any(|c| c.start < now && now < c.end());
    let selected = ed.selected.is_some() || ed.selected_transition.is_some();
    put(
        p,
        "undo",
        "Undo",
        "undo",
        ed.undo.is_empty().then_some("Nothing to undo"),
        false,
    );
    put(
        p,
        "redo",
        "Redo",
        "redo",
        ed.redo.is_empty().then_some("Nothing to redo"),
        false,
    );
    put(
        p,
        "scissors",
        "Split",
        "split",
        (!can_split).then_some("No clip under the playhead"),
        false,
    );
    put(
        p,
        "trash",
        "Delete",
        "delete",
        (!selected).then_some("Nothing is selected"),
        false,
    );
    if ed.product == Product::Kdenlive {
        put(
            p,
            "arrow-left",
            "Delete and close gap",
            "ripple-delete",
            ed.selected.is_none().then_some("No clip is selected"),
            false,
        );
    }
    put(
        p,
        "link",
        if ed.snapping {
            "Snapping on"
        } else {
            "Snapping off"
        },
        "snap",
        None,
        ed.snapping,
    );
    if ed.product == Product::Kdenlive {
        let add_v = Rect::new(x, y, 40, b);
        text_button(p, s, add_v, "+V", "add-track:video", false);
        x += 44;
        let add_a = Rect::new(x, y, 40, b);
        text_button(p, s, add_a, "+A", "add-track:audio", false);
    }
    // Zoom at the right: out, a slider, in, fit.
    let right = r.x + r.width as i32 - 8;
    let fit = Rect::new(right - 44, y, 44, b);
    text_button(
        p,
        s,
        fit,
        "Fit",
        &format!("zoom-fit:{width_for_fit}"),
        false,
    );
    let zin = Rect::new(fit.x - b as i32 - 4, y, b, b);
    tool(p, s, zin, "zoom-in", "Zoom in", "zoom-in", false, false);
    let track_w = 120u32;
    let zr = Rect::new(
        zin.x - track_w as i32 - 6,
        y + (b as i32 - 16) / 2,
        track_w,
        16,
    );
    let span = i64::from(super::ZOOM_MAX - super::ZOOM_MIN);
    let t = i64::from(ed.zoom - super::ZOOM_MIN) * 1_000_000 / span;
    let pos = (fmath_isqrt(t) * i64::from(track_w) / 1000) as i32;
    p.box_(Rect::new(zr.x, zr.y + 6, zr.width, 4), s.raised, 2);
    p.circle(zr.x + pos, zr.y + 8, 7, s.text);
    p.node(
        zr,
        Primitive::Region,
        Some((&target(&format!("zoom:{track_w}")), "Timeline zoom")),
    );
    tool(
        p,
        s,
        Rect::new(zr.x - b as i32 - 6, y, b, b),
        "zoom-out",
        "Zoom out",
        "zoom-out",
        false,
        false,
    );
}
fn fmath_isqrt(v: i64) -> i64 {
    cw_raster::fmath::isqrt(v.max(0) as u64) as i64
}

fn status(p: &mut Painter, ed: &Editor, s: &Skin, x: i32, y: i32, w: u32) {
    if !ed.status.is_empty() {
        p.label(x, y, w, &ed.status, 11, s.muted, false, Align::Center);
    }
}

// ---- Clipchamp -----------------------------------------------------------------------

fn clipchamp(ed: &Editor, p: &mut Painter, env: &AppEnv<'_>, s: &Skin) {
    let (w, h) = (env.width, env.height);
    let top = 48u32;
    let tl_h = timeline_height(h);
    let body_h = h.saturating_sub(top + tl_h);
    // Top bar: the editable project name, the status, and Export.
    p.box_(Rect::new(0, 0, w, top), s.bar, 0);
    widgets::field(
        p,
        s,
        Rect::new(84, 10, 220, 28),
        &ed.project.name,
        ed.field == Field::Project,
        "project-name",
    );
    status(p, ed, s, 320, 16, w.saturating_sub(460));
    let export = Rect::new(w as i32 - 104, 9, 92, 30);
    if ed.project.duration() == 0 || ed.export.is_some() {
        text_button_off(
            p,
            s,
            export,
            "Export",
            if ed.export.is_some() {
                "An export is running"
            } else {
                "Add media to the timeline first"
            },
        );
    } else {
        text_button(p, s, export, "Export", "export", true);
    }
    let save = Rect::new(export.x - 72, 9, 64, 30);
    text_button(p, s, save, "Save", "save", false);
    let open = Rect::new(save.x - 72, 9, 64, 30);
    text_button(p, s, open, "Open", "open", false);
    // Left toolbar: icon tabs with their names under them.
    let rail = 76u32;
    p.box_(Rect::new(0, top as i32, rail, body_h), s.bar, 0);
    let tabs = [
        ("media", "film", "Your media"),
        ("text", "text-tool", "Text"),
        ("transitions", "layers", "Transitions"),
        ("backgrounds", "palette", "Backgrounds"),
    ];
    for (i, (id, symbol, label)) in tabs.iter().enumerate() {
        let r = Rect::new(4, top as i32 + 8 + i as i32 * 64, rail - 8, 58);
        let on = ed.tab == *id;
        p.button(
            r,
            if on { s.raised } else { Color::TRANSPARENT },
            s.radius,
            &target(&format!("tab:{id}")),
            label,
        );
        p.symbol(
            symbol,
            r.x + (r.width as i32 - 22) / 2,
            r.y + 8,
            22,
            if on { s.accent } else { s.text },
        );
        p.label(
            r.x,
            r.y + 36,
            r.width,
            label,
            10,
            s.text,
            false,
            Align::Center,
        );
    }
    // The panel for the chosen tab.
    let panel_w = 250u32.min(w / 4);
    let panel = Rect::new(rail as i32, top as i32, panel_w, body_h);
    p.box_(panel, s.panel, 0);
    let inner = Rect::new(
        panel.x + 12,
        panel.y + 12,
        panel_w - 24,
        body_h.saturating_sub(24),
    );
    // Timeline geometry first: bin tiles drop onto it.
    let tl = Rect::new(0, (top + body_h) as i32, w, tl_h);
    let bar_h = 38u32;
    let lanes_r = Rect::new(0, tl.y + bar_h as i32, w, tl_h - bar_h);
    let g = widgets::lanes(ed, env, lanes_r, 44, 22, 44);
    match ed.tab.as_str() {
        "text" => {
            p.strong(inner.x, inner.y, inner.width, "Text", 14, s.text);
            titles_panel(
                p,
                ed,
                s,
                Rect::new(inner.x, inner.y + 30, inner.width, inner.height),
            );
        }
        "transitions" => {
            p.strong(inner.x, inner.y, inner.width, "Transitions", 14, s.text);
            transitions_panel(
                p,
                ed,
                s,
                Rect::new(inner.x, inner.y + 30, inner.width, inner.height),
                2,
            );
        }
        "backgrounds" => {
            p.strong(inner.x, inner.y, inner.width, "Backgrounds", 14, s.text);
            backgrounds_panel(
                p,
                s,
                Rect::new(inner.x, inner.y + 30, inner.width, inner.height),
            );
        }
        _ => {
            p.strong(inner.x, inner.y, inner.width, "Your media", 14, s.text);
            text_button(
                p,
                s,
                Rect::new(inner.x, inner.y + 28, inner.width, 32),
                "Import media",
                "import",
                true,
            );
            widgets::bin(
                p,
                ed,
                env,
                Rect::new(
                    inner.x,
                    inner.y + 72,
                    inner.width,
                    inner.height.saturating_sub(72),
                ),
                s,
                g,
                (inner.width - 8) / 2,
                false,
            );
        }
    }
    // Right: property panel tabs, and the page of the chosen one.
    let prop_rail = 64u32;
    let rail_x = w as i32 - prop_rail as i32;
    p.box_(Rect::new(rail_x, top as i32, prop_rail, body_h), s.bar, 0);
    let is_title = ed
        .selected_clip()
        .is_some_and(|c| matches!(c.source, Source::Title(_)));
    let pages: Vec<(&str, &str, &str)> = if is_title {
        vec![
            ("title", "text-tool", "Text"),
            ("fade", "contrast", "Fade"),
            ("transform", "move", "Transform"),
        ]
    } else {
        vec![
            ("audio", "volume", "Audio"),
            ("fade", "contrast", "Fade"),
            ("color", "sliders", "Adjust colors"),
            ("speed", "dial", "Speed"),
            ("transform", "move", "Transform"),
            ("crop", "crop", "Crop"),
        ]
    };
    let has_selection = ed.selected.is_some() || ed.selected_transition.is_some();
    for (i, (id, symbol, label)) in pages.iter().enumerate() {
        let r = Rect::new(
            rail_x + 4,
            top as i32 + 8 + i as i32 * 58,
            prop_rail - 8,
            52,
        );
        if ed.selected.is_none() {
            p.box_(r, Color::TRANSPARENT, s.radius);
            p.disabled("Select a clip on the timeline");
            p.symbol(
                symbol,
                r.x + (r.width as i32 - 20) / 2,
                r.y + 6,
                20,
                s.muted,
            );
            p.label(
                r.x,
                r.y + 30,
                r.width,
                label,
                9,
                s.muted,
                false,
                Align::Center,
            );
            continue;
        }
        let on = ed.inspector == *id;
        p.button(
            r,
            if on { s.raised } else { Color::TRANSPARENT },
            s.radius,
            &target(&format!("inspector:{id}")),
            label,
        );
        p.symbol(
            symbol,
            r.x + (r.width as i32 - 20) / 2,
            r.y + 6,
            20,
            if on { s.accent } else { s.text },
        );
        p.label(
            r.x,
            r.y + 30,
            r.width,
            label,
            9,
            s.text,
            false,
            Align::Center,
        );
    }
    let open_page = has_selection && (!ed.inspector.is_empty() || ed.selected_transition.is_some());
    let prop_w = if open_page { 240u32 } else { 0 };
    if open_page {
        let pr = Rect::new(rail_x - prop_w as i32, top as i32, prop_w, body_h);
        p.box_(pr, s.panel, 0);
        let (page, title) = if ed.selected.is_some() {
            pages
                .iter()
                .find(|(id, _, _)| *id == ed.inspector)
                .or(pages.first())
                .map_or(("", ""), |(id, _, l)| (*id, *l))
        } else {
            ("", "Transition")
        };
        p.strong(pr.x + 12, pr.y + 12, prop_w - 24, title, 14, s.text);
        inspector(p, ed, env, s, pr.x + 12, pr.y + 40, prop_w - 24, page, 1);
    }
    // The stage.
    let stage_x = (rail + panel_w) as i32;
    let stage_w = (rail_x - prop_w as i32 - stage_x).max(40) as u32;
    let controls = 44u32;
    let mon = Rect::new(
        stage_x + 16,
        top as i32 + 12,
        stage_w.saturating_sub(32),
        body_h.saturating_sub(controls + 20),
    );
    widgets::monitor(p, ed, env, mon, s);
    let under = Rect::new(stage_x, mon.y + mon.height as i32 + 4, stage_w, controls);
    widgets::transport(
        p,
        ed,
        env,
        Rect::new(
            under.x + 150,
            under.y + 6,
            under.width.saturating_sub(150),
            32,
        ),
        s,
    );
    p.left(
        under.x + 16,
        under.y + 14,
        160,
        &widgets::clock_text(ed, env),
        12,
        s.text,
    );
    // Timeline.
    timeline_bar(p, ed, env, s, Rect::new(0, tl.y, w, bar_h), g.width);
    widgets::timeline(p, ed, env, lanes_r, s, g, 44);
    widgets::progress(p, ed, s, Rect::new(w as i32 - 330, top as i32 + 8, 310, 40));
}

// ---- iMovie ------------------------------------------------------------------------

fn imovie(ed: &Editor, p: &mut Painter, env: &AppEnv<'_>, s: &Skin) {
    let (w, h) = (env.width, env.height);
    let top = 52u32;
    let tl_h = timeline_height(h);
    let body_h = h.saturating_sub(top + tl_h);
    p.box_(Rect::new(0, 0, w, top), s.bar, 0);
    p.hline(0, top as i32 - 1, w, s.line);
    // Left of the toolbar: Projects and Import.
    tool(
        p,
        s,
        Rect::new(10, 12, 92, 28),
        "chevron-left",
        "Projects",
        "open",
        false,
        true,
    );
    tool(
        p,
        s,
        Rect::new(108, 12, 84, 28),
        "download",
        "Import",
        "import",
        false,
        true,
    );
    tool(
        p,
        s,
        Rect::new(198, 12, 64, 28),
        "document",
        "Save",
        "save",
        false,
        true,
    );
    // Centre: the browser's tabs, as a segmented control.
    let tabs = [
        ("media", "My Media"),
        ("audio", "Audio & Video"),
        ("titles", "Titles"),
        ("backgrounds", "Backgrounds"),
        ("transitions", "Transitions"),
    ];
    let seg_w: u32 = tabs.iter().map(|(_, l)| p.measure(l, 12, false) + 20).sum();
    let mut x = ((w as i32 - seg_w as i32) / 2).max(270);
    for (id, label) in tabs {
        let tw = p.measure(label, 12, false) + 20;
        let r = Rect::new(x, 13, tw, 26);
        let on = ed.tab == id;
        p.button(
            r,
            if on { s.raised } else { Color::TRANSPARENT },
            5,
            &target(&format!("tab:{id}")),
            label,
        );
        p.label(
            r.x,
            r.y + 4,
            tw,
            label,
            12,
            if on { s.text } else { s.muted },
            on,
            Align::Center,
        );
        x += tw as i32;
    }
    // Right: Share, which exports a file.
    let share = Rect::new(w as i32 - 46, 12, 34, 28);
    if ed.project.duration() == 0 || ed.export.is_some() {
        tool_off(
            p,
            s,
            share,
            "share",
            if ed.export.is_some() {
                "An export is running"
            } else {
                "The movie is empty"
            },
        );
    } else {
        tool(p, s, share, "share", "Share", "export", false, false);
    }
    let tl = Rect::new(0, (top + body_h) as i32, w, tl_h);
    let bar_h = 32u32;
    let lanes_r = Rect::new(0, tl.y + bar_h as i32, w, tl_h - bar_h);
    let g = widgets::lanes(ed, env, lanes_r, 0, 20, 50);
    // Browser.
    let bw = w * 9 / 20;
    let browser = Rect::new(0, top as i32, bw, body_h);
    p.box_(browser, s.panel, 0);
    let inner = Rect::new(12, top as i32 + 12, bw - 24, body_h.saturating_sub(24));
    match ed.tab.as_str() {
        "titles" => {
            titles_panel(p, ed, s, inner);
        }
        "backgrounds" => backgrounds_panel(p, s, inner),
        "transitions" => transitions_panel(p, ed, s, inner, 3),
        tab => {
            // My Media shows everything; Audio & Video shows the sound.
            if ed.project.media.is_empty() {
                p.center(
                    inner.x,
                    inner.y + inner.height as i32 / 2 - 30,
                    inner.width,
                    "Import Media",
                    16,
                    s.muted,
                );
                text_button(
                    p,
                    s,
                    Rect::new(
                        inner.x + inner.width as i32 / 2 - 60,
                        inner.y + inner.height as i32 / 2,
                        120,
                        28,
                    ),
                    "Import Media",
                    "import",
                    true,
                );
            } else if tab == "audio"
                && !ed
                    .project
                    .media
                    .iter()
                    .any(|m| m.kind == cw_video::MediaKind::Audio)
            {
                p.center(
                    inner.x,
                    inner.y + 30,
                    inner.width,
                    "No sound in this library",
                    12,
                    s.muted,
                );
            } else {
                widgets::bin(p, ed, env, inner, s, g, 132, false);
            }
        }
    }
    // Viewer with the adjustments bar above it.
    let vx = bw as i32;
    let vw = w - bw;
    let adj = Rect::new(vx, top as i32, vw, 34);
    p.box_(adj, s.bar, 0);
    let is_title = ed
        .selected_clip()
        .is_some_and(|c| matches!(c.source, Source::Title(_)));
    let tools: Vec<(&str, &str, &str)> = if is_title {
        vec![("title", "text-tool", "Title")]
    } else {
        vec![
            ("color", "palette", "Color correction"),
            ("crop", "crop", "Cropping"),
            ("overlay", "layers", "Video overlay settings"),
            ("audio", "volume", "Volume"),
            ("speed", "dial", "Speed"),
            ("fade", "contrast", "Fade"),
        ]
    };
    let mut tx = adj.x + 12;
    for (id, symbol, label) in &tools {
        let r = Rect::new(tx, adj.y + 4, 28, 26);
        if ed.selected.is_none() {
            tool_off(p, s, r, symbol, "Select a clip first");
        } else {
            tool(
                p,
                s,
                r,
                symbol,
                label,
                &format!("inspector:{id}"),
                ed.inspector == *id,
                false,
            );
        }
        tx += 32;
    }
    if ed.selected.is_some() {
        text_button(
            p,
            s,
            Rect::new(adj.x + vw as i32 - 84, adj.y + 5, 72, 24),
            "Reset All",
            "reset",
            false,
        );
    }
    let mut vy = adj.y + 34;
    if (ed.selected.is_some() && !ed.inspector.is_empty()) || ed.selected_transition.is_some() {
        // The chosen adjustment's controls, in a strip over the viewer.
        let page = tools
            .iter()
            .find(|(id, _, _)| *id == ed.inspector)
            .or(tools.first())
            .map_or("", |(id, _, _)| *id);
        // As tall as the controls need, measured by laying them out once aside, and
        // never so tall the viewer disappears.
        let mut aside = Painter::new(vw, body_h);
        let need = inspector(&mut aside, ed, env, s, 0, 0, vw - 24, page, 3) + 16;
        let room = body_h as i32 - 34 - 40 - 90;
        let strip_h = need.min(room).max(40) as u32;
        let strip = Rect::new(vx, vy, vw, strip_h);
        p.box_(strip, s.panel, 0);
        let _ = inspector(p, ed, env, s, vx + 12, vy + 8, vw - 24, page, 3);
        vy += strip_h as i32;
    }
    let controls = 40u32;
    let mon = Rect::new(
        vx + 10,
        vy + 6,
        vw - 20,
        (top as i32 + body_h as i32 - vy - controls as i32 - 8).max(40) as u32,
    );
    widgets::monitor(p, ed, env, mon, s);
    let under = Rect::new(vx, mon.y + mon.height as i32 + 2, vw, controls);
    widgets::transport(
        p,
        ed,
        env,
        Rect::new(
            under.x + 110,
            under.y + 4,
            under.width.saturating_sub(110),
            30,
        ),
        s,
    );
    p.left(
        under.x + 12,
        under.y + 12,
        140,
        &widgets::clock_text(ed, env),
        12,
        s.text,
    );
    // Timeline, with its own small bar: snapping, zoom and the status line.
    timeline_bar(p, ed, env, s, Rect::new(0, tl.y, w, bar_h), g.width);
    status(p, ed, s, 420, tl.y + 8, w.saturating_sub(820));
    widgets::timeline(p, ed, env, lanes_r, s, g, 0);
    widgets::progress(
        p,
        ed,
        s,
        Rect::new(w as i32 - 330, top as i32 + 40, 310, 40),
    );
}

// ---- Kdenlive ------------------------------------------------------------------------

fn kdenlive(ed: &Editor, p: &mut Painter, env: &AppEnv<'_>, s: &Skin) {
    let (w, h) = (env.width, env.height);
    let top = 38u32;
    let tl_h = timeline_height(h);
    let body_h = h.saturating_sub(top + tl_h);
    // Main toolbar.
    p.box_(Rect::new(0, 0, w, top), s.bar, 0);
    p.hline(0, top as i32 - 1, w, s.line);
    let mut x = 8;
    for (symbol, label, command) in [
        ("document", "New", "new"),
        ("folder", "Open", "open"),
        ("download", "Save", "save"),
    ] {
        tool(
            p,
            s,
            Rect::new(x, 5, 28, 28),
            symbol,
            label,
            command,
            false,
            false,
        );
        x += 32;
    }
    x += 10;
    for (symbol, label, command, empty) in [
        ("undo", "Undo", "undo", ed.undo.is_empty()),
        ("redo", "Redo", "redo", ed.redo.is_empty()),
    ] {
        let r = Rect::new(x, 5, 28, 28);
        if empty {
            tool_off(
                p,
                s,
                r,
                symbol,
                if command == "undo" {
                    "Nothing to undo"
                } else {
                    "Nothing to redo"
                },
            );
        } else {
            tool(p, s, r, symbol, label, command, false, false);
        }
        x += 32;
    }
    status(p, ed, s, x + 20, 11, w.saturating_sub(x as u32 + 140));
    let render = Rect::new(w as i32 - 92, 5, 84, 28);
    if ed.project.duration() == 0 || ed.export.is_some() {
        text_button_off(
            p,
            s,
            render,
            "Render",
            if ed.export.is_some() {
                "A render is running"
            } else {
                "The timeline is empty"
            },
        );
    } else {
        text_button(p, s, render, "Render", "export", true);
    }
    let tl = Rect::new(0, (top + body_h) as i32, w, tl_h);
    let bar_h = 34u32;
    let lanes_r = Rect::new(0, tl.y + bar_h as i32, w, tl_h - bar_h);
    let g = widgets::lanes(ed, env, lanes_r, 120, 22, 40);
    // Three docks: Project Bin | Effect Stack / Compositions | Project Monitor.
    let bin_w = w * 3 / 10;
    let fx_w = w * 3 / 10;
    let mon_w = w - bin_w - fx_w;
    let dock = |p: &mut Painter, x: i32, width: u32, title: &str| {
        p.box_(Rect::new(x, top as i32, width, body_h), s.panel, 0);
        p.box_(Rect::new(x, top as i32, width, 24), s.bar, 0);
        p.strong(x + 8, top as i32 + 4, width - 16, title, 11, s.text);
        p.vline(x + width as i32 - 1, top as i32, body_h, s.line);
    };
    dock(p, 0, bin_w, "Project Bin");
    let btop = top as i32 + 26;
    tool(
        p,
        s,
        Rect::new(6, btop, 26, 24),
        "plus",
        "Add Clip or Folder",
        "import",
        false,
        false,
    );
    tool(
        p,
        s,
        Rect::new(34, btop, 26, 24),
        "palette",
        "Add Color Clip",
        "add-color:000000",
        false,
        false,
    );
    tool(
        p,
        s,
        Rect::new(62, btop, 26, 24),
        "text-tool",
        "Add Title Clip",
        "add-title:plain",
        false,
        false,
    );
    widgets::bin(
        p,
        ed,
        env,
        Rect::new(6, btop + 30, bin_w - 12, body_h.saturating_sub(60)),
        s,
        g,
        0,
        true,
    );
    // Effect Stack and Compositions, as tabs of one dock.
    let fx_x = bin_w as i32;
    dock(p, fx_x, fx_w, "");
    let on_fx = ed.tab != "compositions";
    for (i, (id, label)) in [
        ("effects", "Effect/Composition Stack"),
        ("compositions", "Compositions"),
    ]
    .iter()
    .enumerate()
    {
        let tw = (fx_w / 2) as i32;
        let r = Rect::new(fx_x + i as i32 * tw, top as i32, tw as u32, 24);
        let on = (i == 0) == on_fx;
        p.button(
            r,
            if on { s.panel } else { s.bar },
            0,
            &target(&format!("tab:{id}")),
            label,
        );
        p.label(
            r.x,
            r.y + 4,
            r.width,
            label,
            11,
            if on { s.text } else { s.muted },
            on,
            Align::Center,
        );
    }
    let fx_inner_x = fx_x + 10;
    let fx_inner_w = fx_w - 20;
    if on_fx {
        let groups: Vec<(&str, &str)> = match ed.selected_clip() {
            Some(c) if ed.project.track(c.track).map(|t| t.kind) == Some(TrackKind::Audio) => {
                vec![("audio", "Volume"), ("fade", "Fade"), ("speed", "Speed")]
            }
            Some(c) if matches!(c.source, Source::Title(_)) => vec![
                ("title", "Title"),
                ("transform", "Transform"),
                ("fade", "Fade"),
            ],
            Some(_) => vec![
                ("transform", "Transform"),
                ("crop", "Crop"),
                ("color", "Color"),
                ("fade", "Fade"),
                ("speed", "Speed"),
            ],
            None => vec![],
        };
        let mut y = top as i32 + 30;
        if groups.is_empty() {
            let _ = inspector(p, ed, env, s, fx_inner_x, y, fx_inner_w, "", 1);
        }
        // One group open at a time, the others collapsed to their headers.
        let open = if groups.iter().any(|(id, _)| *id == ed.inspector) {
            ed.inspector.as_str()
        } else {
            groups.first().map_or("", |(id, _)| *id)
        };
        for (id, label) in &groups {
            let hr = Rect::new(fx_inner_x - 4, y, fx_inner_w + 8, 22);
            p.button(hr, s.bar, 2, &target(&format!("inspector:{id}")), label);
            p.symbol(
                if *id == open {
                    "chevron-down"
                } else {
                    "chevron-right"
                },
                hr.x + 4,
                hr.y + 3,
                16,
                s.muted,
            );
            p.left(hr.x + 24, hr.y + 3, hr.width - 30, label, 12, s.text);
            y += 26;
            if *id == open {
                y = inspector(p, ed, env, s, fx_inner_x, y + 4, fx_inner_w, id, 1);
                y += 4;
            }
        }
    } else {
        transitions_panel(
            p,
            ed,
            s,
            Rect::new(
                fx_inner_x,
                top as i32 + 32,
                fx_inner_w,
                body_h.saturating_sub(40),
            ),
            2,
        );
    }
    // Project Monitor.
    let mx = fx_x + fx_w as i32;
    dock(p, mx, mon_w, "Project Monitor");
    let controls = 40u32;
    let mon = Rect::new(
        mx + 8,
        top as i32 + 30,
        mon_w - 34,
        body_h.saturating_sub(30 + controls + 6),
    );
    widgets::monitor(p, ed, env, mon, s);
    // The audio level meter beside the monitor: the mix's peak during the frame showing.
    let meter = Rect::new(mx + mon_w as i32 - 20, mon.y, 10, mon.height);
    p.box_(meter, Color::BLACK, 2);
    let level = u32::from(ed.level(env.clock_us));
    let lit = meter.height * level / 255;
    if lit > 0 {
        let color = if level > 230 {
            Color::rgb(230, 60, 60)
        } else if level > 180 {
            Color::rgb(240, 200, 60)
        } else {
            Color::rgb(80, 200, 110)
        };
        p.box_(
            Rect::new(meter.x + 1, meter.y + (meter.height - lit) as i32, 8, lit),
            color,
            1,
        );
    }
    let under = Rect::new(mx, mon.y + mon.height as i32 + 2, mon_w, controls);
    widgets::transport(
        p,
        ed,
        env,
        Rect::new(
            under.x + 120,
            under.y + 4,
            under.width.saturating_sub(120),
            30,
        ),
        s,
    );
    p.left(
        under.x + 8,
        under.y + 12,
        110,
        &widgets::clock_text(ed, env),
        12,
        s.accent,
    );
    // Timeline.
    timeline_bar(p, ed, env, s, Rect::new(0, tl.y, w, bar_h), g.width);
    widgets::timeline(p, ed, env, lanes_r, s, g, 120);
    widgets::progress(
        p,
        ed,
        s,
        Rect::new(w as i32 - 330, top as i32 + 30, 310, 40),
    );
    let _: Lanes = g;
}
