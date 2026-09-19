//! iOS 18 presentation: Dynamic Island status bar, SpringBoard grid and glass dock,
//! full-screen applications with native navigation bars, and blurred system panels.
//! Every icon launches a real simulator application; clocks use simulation time.
use super::shared::{Align, Painter, ShellContext, WindowView};
use cw_scene::{Color, Rect};

const BLUE: Color = Color::rgb(0, 122, 255);
const INK: Color = Color::rgb(0, 0, 0);
const GRAY: Color = Color::rgb(142, 142, 147);
const RED: Color = Color::rgb(255, 59, 48);
const BAR: Color = Color(249, 249, 249, 240);
const HAIRLINE: Color = Color(60, 60, 67, 74);
const WHITE70: Color = Color(255, 255, 255, 178);
const GREEN: Color = Color::rgb(52, 199, 89);
/// Stops on every slider: 0% to 100% in tens, so a click lands on an exact level.
const STOPS: u32 = 11;
/// (application id, icon asset, name). Every application in the catalogue now has its
/// own artwork, so the whole roster is drawn on SpringBoard. The last four populate the
/// dock when installed, which is what keeps the home screen looking like a phone.
const APPS: [(&str, &str, &str); 19] = [
    ("calendar", "calendar", "Calendar"),
    ("notes", "notes", "Notes"),
    ("docs", "docs", "Pages"),
    ("spreadsheet", "spreadsheet", "Numbers"),
    ("files", "files", "Files"),
    ("photos", "photos", "Photos"),
    ("music", "music", "Music"),
    ("imovie", "imovie", "iMovie"),
    ("maps", "maps", "Maps"),
    ("weather", "weather", "Weather"),
    ("clock", "clock", "Clock"),
    ("calculator", "calculator", "Calculator"),
    ("contacts", "contacts", "Contacts"),
    ("settings", "settings", "Settings"),
    ("terminal", "terminal", "Terminal"),
    ("editor", "docs", "TextEdit"),
    ("mail", "mail", "Mail"),
    ("browser", "browser", "Safari"),
    ("chat", "chat", "Messages"),
];

/// App Library categories. Every application in `APPS` sits in exactly one, so the
/// name a plate carries is stable and `shell:group:<name>` can expand the same set
/// twice running — a category cut out of the grid by position could not promise that.
const LIBRARY_GROUPS: [(&str, &[&str]); 6] = [
    (
        "Productivity",
        &["calendar", "notes", "docs", "spreadsheet", "contacts"],
    ),
    ("Utilities", &["files", "clock", "calculator", "settings"]),
    ("Social", &["mail", "chat", "browser"]),
    ("Photo & Video", &["photos", "music", "imovie"]),
    ("Travel", &["maps", "weather"]),
    ("Developer", &["terminal", "editor"]),
];

/// The title this shell paints in a window's navigation bar. A phone leaves it
/// empty where the application's own content carries the name instead.
pub(super) fn navigation_title(w: &WindowView) -> String {
    match w.kind.as_str() {
        // Files names the folder it is showing, taken from the window's real path.
        "files" => w
            .document
            .trim_end_matches('/')
            .rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap_or("Browse")
            .to_owned(),
        "editor" => String::new(),
        kind => app_name(kind).map_or_else(|| w.title.clone(), str::to_owned),
    }
}
/// The name this window goes by, for a reader that needs one even where the bar
/// is left blank.
pub(super) fn window_title(w: &WindowView) -> String {
    let painted = navigation_title(w);
    if !painted.is_empty() {
        return painted;
    }
    if w.kind == "editor" && !w.document.is_empty() {
        return w
            .document
            .rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap_or(&w.document)
            .to_owned();
    }
    app_name(&w.kind).map_or_else(|| w.title.clone(), str::to_owned)
}
pub(super) fn app_name(kind: &str) -> Option<&'static str> {
    APPS.iter().find(|(k, _, _)| *k == kind).map(|(_, _, n)| *n)
}
fn asset(kind: &str) -> &str {
    APPS.iter()
        .find(|(k, _, _)| *k == kind)
        .map_or(kind, |(_, a, _)| *a)
}
struct Grid {
    size: u32,
    margin: i32,
    stride: i32,
}
impl Grid {
    fn new(width: u32) -> Self {
        let size = (width * 60 / 390).clamp(44, 72);
        let margin = (width * 27 / 390) as i32;
        Self {
            size,
            margin,
            stride: (width as i32 - margin * 2 - size as i32) / 3,
        }
    }
    fn x(&self, column: i32) -> i32 {
        self.margin + column * self.stride
    }
}

fn app(p: &mut Painter, x: i32, y: i32, size: u32, kind: &str, name: &str, show_label: bool) {
    p.platform_icon(
        Rect::new(x, y, size, size),
        "ios",
        asset(kind),
        &format!("shell:launch:{kind}"),
        name,
    );
    if show_label {
        home_label(p, x - 14, y + size as i32 + 6, size + 28, name);
    }
}
fn home_label(p: &mut Painter, x: i32, y: i32, width: u32, text: &str) {
    p.label(
        x,
        y + 1,
        width,
        text,
        12,
        Color(0, 0, 0, 90),
        false,
        Align::Center,
    );
    p.label(x, y, width, text, 12, Color::WHITE, false, Align::Center);
}

/// The dock's four slots: iOS 18's own defaults, with Mail standing in for Phone, which
/// this simulation has no telephony to back. Only installed ones are drawn; iOS never
/// moves another application into the dock on its own.
const DOCK: [&str; 4] = ["mail", "browser", "chat", "music"];

/// SpringBoard's geometry at one screen size: a four-column grid of up to six rows, the
/// page dots, and the floating dock. `background` paints from it and `home_pages`
/// counts from it, so the pages a swipe walks are exactly the pages that are drawn.
struct Home {
    grid: Grid,
    top: i32,
    pitch: i32,
    rows: i32,
    /// Centre line of the page dots.
    dots: i32,
    dock: Rect,
}
impl Home {
    fn new(width: u32, height: u32) -> Self {
        let grid = Grid::new(width);
        let size = grid.size as i32;
        let dock_height = (size + 32).min(96);
        let dock = Rect::new(
            12,
            height as i32 - dock_height - 12,
            width.saturating_sub(24),
            dock_height as u32,
        );
        let dots = dock.y - 24;
        // The first row starts beneath the status bar; rows then share the height down
        // to the dots evenly, which is how iOS spreads six rows over a tall phone.
        let top = if height >= 700 { 72 } else { 60 };
        let space = (dots - 16 - top).max(0);
        let rows = (space / (size + 30)).clamp(1, 6);
        let pitch = (space / rows).min(size + 44);
        Self {
            grid,
            top,
            pitch,
            rows,
            dots,
            dock,
        }
    }
    /// The 2×2 Calendar widget on the first page, when the page is tall enough for it.
    fn widget(&self) -> bool {
        self.rows >= 2
    }
    fn capacity(&self, page: usize) -> usize {
        let cells = (self.rows * 4) as usize;
        if page == 0 && self.widget() {
            cells - 4
        } else {
            cells
        }
    }
    /// Column and row of the `index`th icon on `page`, filled row by row around the
    /// widget, the way SpringBoard flows icons.
    fn cell(&self, page: usize, index: usize) -> (i32, i32) {
        let index = index as i32;
        if page == 0 && self.widget() {
            if index < 4 {
                return (2 + index % 2, index / 2);
            }
            return ((index - 4) % 4, 2 + (index - 4) / 4);
        }
        (index % 4, index / 4)
    }
    fn y(&self, row: i32) -> i32 {
        self.top + row * self.pitch
    }
    /// Split the home screen's applications into pages; there is always a first one.
    fn paginate<T: Copy>(&self, apps: &[T]) -> Vec<Vec<T>> {
        let mut pages = vec![Vec::new()];
        for app in apps {
            let page = pages.len() - 1;
            if pages[page].len() == self.capacity(page) {
                pages.push(Vec::new());
            }
            pages.last_mut().expect("at least one page").push(*app);
        }
        pages
    }
}
type App = &'static (&'static str, &'static str, &'static str);
/// Installed applications in the dock, and those on the pages, in catalogue order.
fn springboard(installed: impl Fn(&str) -> bool) -> (Vec<App>, Vec<App>) {
    let dock = DOCK
        .iter()
        .filter(|kind| installed(kind))
        .filter_map(|kind| APPS.iter().find(|(k, _, _)| k == kind))
        .collect();
    let pages = APPS
        .iter()
        .filter(|(kind, _, _)| installed(kind) && !DOCK.contains(kind))
        .collect();
    (dock, pages)
}
/// How many home screen pages a phone of this size shows for these applications. An
/// empty list means every application, as it does for `ShellContext::installed`.
pub fn home_pages(installed: &[String], width: u32, height: u32) -> u32 {
    let (_, apps) = springboard(|kind| installed.is_empty() || installed.iter().any(|a| a == kind));
    Home::new(width, height).paginate(&apps).len() as u32
}

/// The small Calendar widget, driven by the status bar's own clock.
fn calendar_widget(p: &mut Painter, ctx: &ShellContext<'_>, home: &Home) {
    let date = ctx.date();
    let size = home.grid.size as i32;
    let widget = Rect::new(
        home.grid.x(0),
        home.top,
        (home.grid.stride + size) as u32,
        (home.pitch + size) as u32,
    );
    p.drop_shadow(widget, 22, 14, 50, 6);
    p.box_(widget, Color::WHITE, 22);
    p.strong(
        widget.x + 16,
        widget.y + 14,
        widget.width - 32,
        &date.weekday_name().to_uppercase(),
        11,
        RED,
    );
    p.left(
        widget.x + 15,
        widget.y + 27,
        widget.width - 30,
        &date.day.to_string(),
        40,
        INK,
    );
    p.left(
        widget.x + 16,
        widget.y + widget.height as i32 - 34,
        widget.width - 32,
        &format!("{} {}", date.month_name(), date.year),
        12,
        GRAY,
    );
    home_label(
        p,
        widget.x,
        widget.y + widget.height as i32 + 6,
        widget.width,
        "Calendar",
    );
    p.region(
        widget,
        if ctx.installed("calendar") {
            "shell:launch:calendar"
        } else {
            "shell:panel:calendar"
        },
        "Calendar widget",
    );
}

pub fn background(p: &mut Painter, ctx: &ShellContext<'_>) {
    p.asset(Rect::new(0, 0, ctx.width, ctx.height), "wallpaper/ios");
    // A locked or sleeping display must not announce launchable icons behind it, and
    // the App Library and Today View are screens of their own beside the pages, not
    // sheets over one, so neither leaves a page of icons underneath.
    if ctx.active || !ctx.awake() || ctx.launcher_open || ctx.panel == Some("calendar") {
        return;
    }
    let width = ctx.width as i32;
    let home = Home::new(ctx.width, ctx.height);
    let size = home.grid.size;
    let (dock_apps, apps) = springboard(|kind| ctx.installed(kind));
    let pages = home.paginate(&apps);
    let page = (ctx.home_page as usize).min(pages.len() - 1);
    if page == 0 && home.widget() {
        calendar_widget(p, ctx, &home);
    }
    for (i, (kind, _, name)) in pages[page].iter().enumerate() {
        let (column, row) = home.cell(page, i);
        app(p, home.grid.x(column), home.y(row), size, kind, name, true);
    }
    // Page dots. Each one is a real page and tapping one goes there; swiping walks
    // them too, and past the last one lies the App Library.
    let count = pages.len() as i32;
    let spacing = 16;
    let first = width / 2 - (count - 1) * spacing / 2;
    for i in 0..count {
        let current = i as usize == page;
        let cx = first + i * spacing;
        p.circle(
            cx,
            home.dots,
            4,
            if current {
                Color::WHITE
            } else {
                Color(255, 255, 255, 110)
            },
        );
        p.region(
            Rect::new(cx - spacing / 2, home.dots - 14, spacing as u32, 28),
            &format!("shell:home-page:{i}"),
            &format!("Page {} of {count}", i + 1),
        );
        announce(p, if current { "Current page" } else { "Page" });
    }
    let dock = home.dock;
    p.glass(
        dock,
        34,
        22,
        Color(255, 255, 255, 70),
        Some(Color(255, 255, 255, 40)),
    );
    // A full dock uses the grid's columns; a shorter one centres what it has.
    let slots = dock_apps.len() as i32;
    for (i, (kind, _, name)) in dock_apps.iter().enumerate() {
        let x = if slots == 4 {
            home.grid.x(i as i32)
        } else {
            width / 2 - (slots * home.grid.stride - (home.grid.stride - size as i32)) / 2
                + i as i32 * home.grid.stride
        };
        app(
            p,
            x,
            dock.y + (dock.height as i32 - size as i32) / 2,
            size,
            kind,
            name,
            false,
        );
    }
}

/// The status bar, reporting the switches the device really holds: the Focus moon
/// beside the time, the aeroplane in place of the signal bars, no Wi-Fi fan with Wi-Fi
/// off, and a yellow battery in Low Power Mode. The lock screen and the cover sheet show
/// the time large beneath it, so there the bar leaves it out, as iOS does.
fn status(p: &mut Painter, ctx: &ShellContext<'_>, color: Color, show_time: bool) {
    let w = ctx.width as i32;
    let (hour, minute) = ctx.hour_minute();
    let island = (w / 2 - 63, 126);
    let time = format!(
        "{}:{minute:02}",
        if hour % 12 == 0 { 12 } else { hour % 12 }
    );
    let focus = ctx.switch("do_not_disturb");
    let text = p.measure(&time, 17, true) as i32;
    let slot = island.0 - 8;
    let x = 8 + (slot - text - if focus { 20 } else { 0 }) / 2;
    if show_time {
        p.label(x, 16, text as u32 + 4, &time, 17, color, true, Align::Left);
    }
    if focus {
        let moon = if show_time {
            x + text + 5
        } else {
            8 + (slot - 14) / 2
        };
        p.symbol("moon", moon, 20, 14, color);
    }
    p.box_(
        Rect::new(island.0, 11, island.1, 37),
        Color::rgb(2, 2, 3),
        19,
    );
    p.circle(w / 2 + 40, 29, 6, Color::rgb(14, 16, 26));
    p.circle(w / 2 + 40, 29, 2, Color::rgb(30, 38, 66));
    let right = island.0 + island.1 as i32;
    let cx = right + (w - right) / 2;
    if ctx.switch("airplane_mode") {
        p.symbol("airplane", cx - 38, 19, 17, color);
    } else {
        p.symbol("cellular", cx - 38, 19, 18, color);
    }
    if ctx.switch("wifi") {
        p.symbol("wifi", cx - 14, 19, 17, color);
    }
    let battery = if ctx.switch("battery_saver") {
        Color::rgb(255, 204, 0)
    } else {
        color
    };
    p.symbol("battery", cx + 9, 15, 27, battery);
}

/// Dimmed, blurred wallpaper behind every system panel.
fn scrim(p: &mut Painter, ctx: &ShellContext<'_>, tint: Color, action: &str, label: &str) {
    let full = Rect::new(0, 0, ctx.width, ctx.height);
    p.glass(full, 0, 36, tint, None);
    p.region(full, action, label);
}
fn module(p: &mut Painter, r: Rect, radius: u32) {
    p.box_(r, Color(255, 255, 255, 46), radius);
}

/// Publish the position of the switch just painted, so its state is observable.
fn announce(p: &mut Painter, value: &str) {
    if let Some(s) = p.scene.nodes.last_mut().and_then(|n| n.semantic.as_mut()) {
        s.value = Some(value.into());
    }
}
/// Round Control Centre tile bound to one real system switch, drawn in its true state.
fn switch_tile(
    p: &mut Painter,
    ctx: &ShellContext<'_>,
    r: Rect,
    symbol: &str,
    name: &str,
    label: &str,
) {
    let on = ctx.switch(name);
    p.button(
        r,
        if on {
            Color::WHITE
        } else {
            Color(255, 255, 255, 46)
        },
        r.height / 2,
        &format!("shell:toggle:{name}"),
        label,
    );
    announce(p, if on { "On" } else { "Off" });
    p.symbol(
        symbol,
        r.x + r.width as i32 / 2 - 12,
        r.y + r.height as i32 / 2 - 12,
        24,
        if on { INK } else { Color::WHITE },
    );
}
/// Eleven stops keep every click on an exact percentage; 0% sits at the bottom.
fn level_column(
    p: &mut Painter,
    ctx: &ShellContext<'_>,
    track: Rect,
    symbol: &str,
    name: &str,
    label: &str,
) {
    let level = u32::from(ctx.level(name));
    module(p, track, 24);
    let fill = track.height * level / 100;
    let crest = track.y + (track.height - fill) as i32;
    p.box_(
        Rect::new(track.x, crest, track.width, fill),
        Color::WHITE,
        24,
    );
    p.box_(
        Rect::new(track.x, crest, track.width, fill.min(24)),
        Color::WHITE,
        0,
    );
    p.symbol(
        symbol,
        track.x + track.width as i32 / 2 - 11,
        track.y + track.height as i32 - 36,
        22,
        Color::rgb(90, 90, 96),
    );
    for i in 0..STOPS {
        let (y0, y1) = (track.height * i / STOPS, track.height * (i + 1) / STOPS);
        let percent = (STOPS - 1 - i) * 100 / (STOPS - 1);
        p.region(
            Rect::new(track.x, track.y + y0 as i32, track.width, y1 - y0),
            &format!("shell:set:{name}:{percent}"),
            &format!("{label} {percent} percent"),
        );
    }
}
fn control_center(p: &mut Painter, ctx: &ShellContext<'_>) {
    scrim(
        p,
        ctx,
        Color(20, 20, 28, 150),
        "shell:dismiss",
        "Dismiss Control Center",
    );
    let unit = ((ctx.width as i32 - 56 - 3 * 16) / 4).clamp(56, 76);
    let step = unit + 16;
    let left = (ctx.width as i32 - (unit * 4 + 48)) / 2;
    let top = 96;
    let white = Color::WHITE;
    // Connectivity quadrant: four radios the system really owns. Cellular is not one
    // of them — there is no modem state to read — so Bluetooth takes that corner.
    let net = Rect::new(left, top, (unit * 2 + 16) as u32, (unit * 2 + 16) as u32);
    module(p, net, 34);
    for (i, (symbol, name, tint, label)) in [
        (
            "airplane",
            "airplane_mode",
            Color::rgb(255, 149, 0),
            "Airplane Mode",
        ),
        ("hotspot", "hotspot", GREEN, "Personal Hotspot"),
        ("wifi", "wifi", BLUE, "Wi-Fi"),
        ("bluetooth", "bluetooth", BLUE, "Bluetooth"),
    ]
    .iter()
    .enumerate()
    {
        let cx = net.x + net.width as i32 / 4 + (i % 2) as i32 * net.width as i32 / 2;
        let cy = net.y + net.height as i32 / 4 + (i / 2) as i32 * net.height as i32 / 2;
        let r = (unit * 2 / 5) as u32;
        let on = ctx.switch(name);
        p.button(
            Rect::new(cx - r as i32, cy - r as i32, r * 2, r * 2),
            if on { *tint } else { Color(255, 255, 255, 40) },
            r,
            &format!("shell:toggle:{name}"),
            label,
        );
        announce(p, if on { "On" } else { "Off" });
        p.symbol(symbol, cx - 10, cy - 10, 20, white);
    }
    // Now Playing. Nothing in the protocol moves a transport, so the card is the one
    // thing it honestly can be: the way into the Music application, which owns the real
    // now-playing record. With Music absent there is nothing behind it at all.
    let media = Rect::new(left + step * 2, top, net.width, net.height);
    let music = ctx.installed("music");
    if music {
        p.button(
            media,
            Color(255, 255, 255, 46),
            34,
            "shell:launch:music",
            "Now Playing",
        );
    } else {
        module(p, media, 34);
        p.disabled("Now Playing");
    }
    p.strong(
        media.x + 18,
        media.y + 16,
        media.width - 36,
        "Now Playing",
        13,
        Color::WHITE,
    );
    p.symbol("music", media.x + 18, media.y + 44, 24, WHITE70);
    p.left(
        media.x + 18,
        media.y + media.height as i32 - 34,
        media.width - 36,
        if music { "Open Music" } else { "Not Playing" },
        14,
        WHITE70,
    );
    let second = top + step * 2;
    switch_tile(
        p,
        ctx,
        Rect::new(left, second, unit as u32, unit as u32),
        "rotate-lock",
        "rotation_lock",
        "Rotation Lock",
    );
    switch_tile(
        p,
        ctx,
        Rect::new(left + step, second, unit as u32, unit as u32),
        "display",
        "dark_mode",
        "Dark Mode",
    );
    // Focus is Do Not Disturb: the same switch every other surface reads.
    let dnd = ctx.switch("do_not_disturb");
    let focus = Rect::new(left, second + step, (unit * 2 + 16) as u32, unit as u32);
    p.button(
        focus,
        if dnd {
            Color(255, 255, 255, 190)
        } else {
            Color(255, 255, 255, 46)
        },
        (unit / 2) as u32,
        "shell:toggle:do_not_disturb",
        "Focus",
    );
    announce(p, if dnd { "On" } else { "Off" });
    p.circle(
        focus.x + unit / 2,
        focus.y + unit / 2,
        (unit / 3) as u32,
        if dnd {
            Color::rgb(88, 86, 214)
        } else {
            Color(255, 255, 255, 50)
        },
    );
    p.symbol(
        "moon",
        focus.x + unit / 2 - 10,
        focus.y + unit / 2 - 10,
        20,
        white,
    );
    p.strong(
        focus.x + unit + 2,
        focus.y + unit / 2 - 9,
        focus.width - unit as u32 - 8,
        "Focus",
        15,
        if dnd { INK } else { white },
    );
    let column = (unit * 2 + 16) as u32;
    level_column(
        p,
        ctx,
        Rect::new(left + step * 2, second, unit as u32, column),
        "sun",
        "brightness",
        "Brightness",
    );
    level_column(
        p,
        ctx,
        Rect::new(left + step * 3, second, unit as u32, column),
        "volume",
        "volume",
        "Volume",
    );
    // Bottom row: the switches that are left, plus a door into Settings. Clock and
    // Camera are gone — no such application exists to launch.
    let third = second + step * 2;
    let fits = |y: i32| y + unit <= ctx.height as i32 - 60;
    for (i, (symbol, name, label)) in [
        ("flashlight", "flashlight", "Flashlight"),
        ("battery", "battery_saver", "Low Power Mode"),
        ("night-light", "night_light", "Night Shift"),
    ]
    .iter()
    .enumerate()
    {
        if !fits(third) {
            break;
        }
        let tile = Rect::new(left + i as i32 * step, third, unit as u32, unit as u32);
        switch_tile(p, ctx, tile, symbol, name, label);
    }
    if fits(third) {
        let gear = Rect::new(left + 3 * step, third, unit as u32, unit as u32);
        p.button(
            gear,
            Color(255, 255, 255, 46),
            (unit / 2) as u32,
            "shell:settings",
            "Settings",
        );
        p.symbol(
            "gear",
            gear.x + unit / 2 - 12,
            gear.y + unit / 2 - 12,
            24,
            white,
        );
    }
}

/// One Settings row. Every variant moves real state.
enum Row {
    /// Label, system switch name.
    Switch(&'static str, &'static str),
    /// Label, level name.
    Level(&'static str, &'static str),
    /// Label, power target, tint.
    Power(&'static str, &'static str, Color),
}
const GROUPS: &[&[Row]] = &[
    &[
        Row::Switch("Airplane Mode", "airplane_mode"),
        Row::Switch("Wi-Fi", "wifi"),
        Row::Switch("Bluetooth", "bluetooth"),
        Row::Switch("Personal Hotspot", "hotspot"),
    ],
    &[
        Row::Switch("Do Not Disturb", "do_not_disturb"),
        Row::Switch("Rotation Lock", "rotation_lock"),
    ],
    &[
        Row::Switch("Dark Mode", "dark_mode"),
        Row::Switch("Night Shift", "night_light"),
        Row::Level("Brightness", "brightness"),
        Row::Level("Volume", "volume"),
        Row::Switch("Low Power Mode", "battery_saver"),
    ],
    &[
        Row::Power("Lock Screen", "shell:power:lock", BLUE),
        Row::Power("Restart", "shell:power:restart", BLUE),
        Row::Power("Shut Down", "shell:power:off", RED),
    ],
];
fn row_height(row: &Row) -> u32 {
    if matches!(row, Row::Level(..)) {
        54
    } else {
        42
    }
}
/// The iOS switch, drawn from the switch it controls.
fn switch_knob(p: &mut Painter, r: Rect, on: bool) {
    let knob = Rect::new(
        r.x + r.width as i32 - 67,
        r.y + (r.height as i32 - 31) / 2,
        51,
        31,
    );
    p.box_(knob, if on { GREEN } else { Color::rgb(229, 229, 234) }, 15);
    p.circle(
        knob.x + if on { 36 } else { 16 },
        knob.y + 15,
        13,
        Color::WHITE,
    );
}
/// Horizontal twin of `level_column`: same stops, same ids, so the two agree.
fn level_row(p: &mut Painter, ctx: &ShellContext<'_>, r: Rect, name: &str, label: &str) {
    let level = u32::from(ctx.level(name));
    p.left(
        r.x + 16,
        r.y + 6,
        r.width.saturating_sub(120),
        label,
        16,
        INK,
    );
    p.right(
        r.x + r.width as i32 - 72,
        r.y + 6,
        56,
        &format!("{level}%"),
        16,
        GRAY,
    );
    let track = Rect::new(r.x + 16, r.y + 30, r.width.saturating_sub(32), 12);
    p.box_(track, Color::rgb(229, 229, 234), 6);
    p.box_(
        Rect::new(track.x, track.y, track.width * level / 100, track.height),
        BLUE,
        6,
    );
    let knob = track.x + (track.width * level / 100) as i32;
    p.circle(knob, track.y + 6, 11, Color(0, 0, 0, 40));
    p.circle(knob, track.y + 6, 10, Color::WHITE);
    for i in 0..STOPS {
        let (x0, x1) = (track.width * i / STOPS, track.width * (i + 1) / STOPS);
        let percent = i * 100 / (STOPS - 1);
        p.region(
            Rect::new(track.x + x0 as i32, r.y + 24, x1 - x0, 28),
            &format!("shell:set:{name}:{percent}"),
            &format!("{label} {percent} percent"),
        );
    }
}
/// Settings. Every row here moves the switch or level the Control Centre draws, so
/// the two screens can never disagree.
fn settings(p: &mut Painter, ctx: &ShellContext<'_>) {
    // Settings is a full-screen application, not a sheet: it has no close button, and
    // it is left the way every application is, by the home indicator's swipe. Its
    // backdrop absorbs taps rather than letting them reach what it covers.
    let sheet = Rect::new(0, 0, ctx.width, ctx.height);
    p.box_(sheet, Color::rgb(242, 242, 247), 0);
    p.region(sheet, "shell:noop", "Settings");
    p.label(
        20,
        52,
        ctx.width.saturating_sub(40),
        "Settings",
        30,
        INK,
        true,
        Align::Left,
    );
    let width = ctx.width.saturating_sub(32);
    let bottom = sheet.y + sheet.height as i32 - 20;
    let mut y = 96;
    for group in GROUPS {
        let height: u32 = group.iter().map(row_height).sum();
        if y + height as i32 > bottom {
            break;
        }
        p.box_(Rect::new(16, y, width, height), Color::WHITE, 12);
        for (i, row) in group.iter().enumerate() {
            let r = Rect::new(16, y, width, row_height(row));
            if i > 0 {
                p.hline(r.x + 16, r.y, r.width - 16, HAIRLINE);
            }
            match row {
                Row::Switch(label, name) => {
                    let on = ctx.switch(name);
                    p.button(
                        r,
                        Color::TRANSPARENT,
                        0,
                        &format!("shell:toggle:{name}"),
                        label,
                    );
                    announce(p, if on { "On" } else { "Off" });
                    p.left(
                        r.x + 16,
                        r.y + 11,
                        r.width.saturating_sub(90),
                        label,
                        16,
                        INK,
                    );
                    switch_knob(p, r, on);
                }
                Row::Level(label, name) => level_row(p, ctx, r, name, label),
                Row::Power(label, target, tint) => {
                    p.button(r, Color::TRANSPARENT, 0, target, label);
                    p.left(
                        r.x + 16,
                        r.y + 11,
                        r.width.saturating_sub(32),
                        label,
                        16,
                        *tint,
                    );
                }
            }
            y += r.height as i32;
        }
        y += 12;
    }
}

/// Locked or sleeping display. It covers every window and the only way back is a
/// real power action; there is no authentication model to prompt against.
fn power_screen(p: &mut Painter, ctx: &ShellContext<'_>) {
    let full = Rect::new(0, 0, ctx.width, ctx.height);
    if ctx.screen == crate::ScreenState::Off {
        p.box_(full, Color::rgb(0, 0, 0), 0);
        p.region(full, "shell:power:wake", "Wake the display");
        p.symbol(
            "power",
            ctx.width as i32 / 2 - 14,
            ctx.height as i32 / 2 - 14,
            28,
            Color(255, 255, 255, 26),
        );
        return;
    }
    // The lock screen is the wallpaper itself, barely dimmed, with the time over it;
    // it hides every application behind it.
    p.asset(full, "wallpaper/ios");
    p.box_(full, Color(0, 0, 0, 40), 0);
    p.region(full, "shell:power:wake", "Unlock");
    status(p, ctx, Color::WHITE, false);
    p.symbol(
        "lock",
        ctx.width as i32 / 2 - 10,
        58,
        20,
        Color(255, 255, 255, 210),
    );
    lock_clock(p, ctx, 92);
    // The two lock screen quick actions. The torch is the real switch Control Center
    // flips; there is no camera in the simulation, so that button says so.
    let h = ctx.height as i32;
    let torch = Rect::new(46, h - 104, 50, 50);
    let lit = ctx.switch("flashlight");
    p.button(
        torch,
        if lit {
            Color::WHITE
        } else {
            Color(20, 20, 26, 110)
        },
        25,
        "shell:toggle:flashlight",
        "Flashlight",
    );
    announce(p, if lit { "On" } else { "Off" });
    p.symbol(
        "flashlight",
        torch.x + 14,
        torch.y + 14,
        22,
        if lit { INK } else { Color::WHITE },
    );
    let camera = Rect::new(ctx.width as i32 - 96, h - 104, 50, 50);
    p.box_(camera, Color(20, 20, 26, 110), 25);
    p.disabled("Camera, no camera in this simulation");
    p.symbol("camera", camera.x + 14, camera.y + 14, 22, WHITE70);
    p.center(0, h - 40, ctx.width, "Swipe up to open", 15, WHITE70);
    // The home indicator: a swipe up from it opens the phone, which is the pointer
    // gesture the router recognises; a tap here, as anywhere on the glass, also wakes.
    p.box_(
        Rect::new(ctx.width as i32 / 2 - 67, h - 13, 134, 5),
        Color::WHITE,
        3,
    );
}

/// The lock screen's date and large clock, which Notification Center repeats. Returns
/// the y beneath them.
fn lock_clock(p: &mut Painter, ctx: &ShellContext<'_>, top: i32) -> i32 {
    let date = ctx.date();
    let (hour, minute) = ctx.hour_minute();
    p.strong_center(
        0,
        top,
        ctx.width,
        &format!(
            "{}, {} {}",
            date.weekday_name(),
            date.month_name(),
            date.day
        ),
        19,
        Color(255, 255, 255, 225),
    );
    let size = (ctx.width / 4).min(96);
    p.label(
        0,
        top + 18,
        ctx.width,
        &format!(
            "{}:{minute:02}",
            if hour % 12 == 0 { 12 } else { hour % 12 }
        ),
        size as u16,
        Color::WHITE,
        true,
        Align::Center,
    );
    top + 18 + size as i32 * 3 / 2
}

/// Notification Center: the cover sheet pulled down from the top of the screen, the
/// lock screen's date and time above the notices the machine really posted. Tapping
/// its empty glass does nothing, as on the device; a swipe up puts it away.
fn notification_center(p: &mut Painter, ctx: &ShellContext<'_>) {
    scrim(
        p,
        ctx,
        Color(10, 12, 24, 120),
        "shell:noop",
        "Notification Center",
    );
    let mut y = lock_clock(p, ctx, 70) + 24;
    if ctx.notifications.is_empty() {
        p.center(0, y + 12, ctx.width, "No Older Notifications", 15, WHITE70);
        return;
    }
    p.strong(
        24,
        y,
        ctx.width - 200,
        "Notification Center",
        17,
        Color::WHITE,
    );
    if ctx.unseen_notices() > 0 {
        let clear = Rect::new(ctx.width as i32 - 156, y - 6, 140, 30);
        p.button(
            clear,
            Color(255, 255, 255, 56),
            15,
            "shell:notifications:seen",
            "Mark all as read",
        );
        p.center(
            clear.x,
            clear.y + 8,
            140,
            "Mark all as read",
            12,
            Color::WHITE,
        );
    }
    y += 40;
    // Every row is a notice an application really posted, and opening one dispatches
    // the action that notice carries rather than a guess about where it came from.
    for (i, notice) in ctx.notifications.iter().enumerate() {
        let row = Rect::new(16, y + i as i32 * 72, ctx.width - 32, 64);
        if row.y + 64 > ctx.height as i32 - 40 {
            break;
        }
        p.button(
            row,
            Color(255, 255, 255, if notice.seen { 40 } else { 72 }),
            20,
            &format!("shell:notice:{i}"),
            &notice.title,
        );
        p.symbol(
            notice_symbol(&notice.app),
            row.x + 16,
            row.y + 22,
            20,
            Color::WHITE,
        );
        p.strong(
            row.x + 50,
            row.y + 12,
            row.width - 68,
            &notice.title,
            14,
            Color::WHITE,
        );
        p.left(
            row.x + 50,
            row.y + 32,
            row.width - 68,
            &notice.body,
            13,
            WHITE70,
        );
        if !notice.seen {
            p.circle(row.x + row.width as i32 - 16, row.y + 16, 4, BLUE);
        }
    }
}

/// Today View: the screen left of the first home screen page. A search field and a
/// stack of widgets; the month widget pages for real and opens Calendar on a day.
fn today_view(p: &mut Painter, ctx: &ShellContext<'_>) {
    scrim(p, ctx, Color(10, 12, 24, 90), "shell:noop", "Today View");
    let field = Rect::new(16, 60, ctx.width - 32, 40);
    p.button(
        field,
        Color(255, 255, 255, 56),
        12,
        "shell:search",
        "Search",
    );
    p.symbol("search", field.x + 12, field.y + 12, 16, WHITE70);
    p.left(
        field.x + 36,
        field.y + 10,
        field.width - 80,
        "Search",
        17,
        WHITE70,
    );
    month_grid(p, ctx, 116);
}

/// Glyph for whatever posted a notice. The set is small on purpose: a notice names an
/// application, not an icon, and inventing artwork for one would be worse than a bell.
fn notice_symbol(app: &str) -> &'static str {
    match app {
        "chat" => "chat",
        "mail" => "mail",
        "browser" => "globe",
        "calendar" => "calendar",
        "files" => "folder",
        "screenshot" | "photos" => "image",
        _ => "bell",
    }
}

/// The Today view's month. It draws the month the panel is paging — `ctx.panel_date()` —
/// rather than pinning itself to the world's own, and both chevrons move it for real.
/// Returns the height it used so the notification below it can flow.
fn month_grid(p: &mut Painter, ctx: &ShellContext<'_>, top: i32) -> i32 {
    let shown = ctx.panel_date();
    let today = ctx.date();
    let rows = (shown.first_weekday + shown.days_in_month)
        .div_ceil(7)
        .max(1) as i32;
    let height = 76 + rows * 30;
    let card = Rect::new(16, top, ctx.width - 32, height as u32);
    p.box_(card, Color(255, 255, 255, 46), 22);
    let right = card.x + card.width as i32;
    // The heading doubles as the way home, and only when the grid has left today.
    let head = Rect::new(card.x + 8, top + 6, card.width.saturating_sub(112), 32);
    p.strong(
        head.x + 8,
        top + 14,
        head.width.saturating_sub(16),
        &format!("{} {}", shown.month_name(), shown.year),
        17,
        Color::WHITE,
    );
    if ctx.panel_month != 0 {
        p.region(
            head,
            "shell:month:today",
            &format!("Back to {} {}", today.month_name(), today.year),
        );
    }
    for (i, (symbol, action, label)) in [
        ("chevron-left", "shell:month:prev", "Previous month"),
        ("chevron-right", "shell:month:next", "Next month"),
    ]
    .iter()
    .enumerate()
    {
        let hit = Rect::new(right - 88 + i as i32 * 44, top + 4, 40, 36);
        p.symbol(symbol, hit.x + 13, hit.y + 10, 16, Color::WHITE);
        p.region(hit, action, label);
    }
    let cell = ((card.width - 24) / 7) as i32;
    for (i, day) in ["S", "M", "T", "W", "T", "F", "S"].iter().enumerate() {
        p.center(
            card.x + 12 + i as i32 * cell,
            top + 46,
            cell as u32,
            day,
            11,
            WHITE70,
        );
    }
    // Each day opens Calendar on that day. Without Calendar installed, or before the
    // world began, a day is shown and not offered: nothing could honour the tap.
    if !ctx.installed("calendar") {
        p.box_(
            Rect::new(card.x + 12, top + 62, card.width - 24, (rows * 30) as u32),
            Color::TRANSPARENT,
            0,
        );
        p.disabled("Calendar days");
    }
    for day in 1..=shown.days_in_month {
        let slot = (day - 1 + shown.first_weekday) as i32;
        let x = card.x + 12 + (slot % 7) * cell;
        let y = top + 66 + (slot / 7) * 30;
        if let Some(open) = ctx.open_day(day) {
            p.region_above(
                Rect::new(x, y - 5, cell as u32, 28),
                &open,
                &format!("{} {day}", shown.month_name()),
            );
        }
        let is_today = ctx.panel_month == 0 && day == today.day;
        if is_today {
            p.circle(x + cell / 2, y + 9, 13, RED);
        }
        p.label(
            x,
            y,
            cell as u32,
            &day.to_string(),
            13,
            if is_today {
                Color::WHITE
            } else {
                Color(255, 255, 255, 225)
            },
            false,
            Align::Center,
        );
    }
    height
}

fn search(p: &mut Painter, ctx: &ShellContext<'_>) {
    scrim(
        p,
        ctx,
        Color(16, 16, 26, 130),
        "shell:dismiss",
        "Dismiss Search",
    );
    let query = ctx.search.to_lowercase();
    let results: Vec<_> = APPS
        .iter()
        .filter(|(kind, _, label)| {
            ctx.installed(kind) && (kind.contains(&query) || label.to_lowercase().contains(&query))
        })
        .collect();
    let grid = Grid::new(ctx.width);
    // The field sits just above the keyboard, and the grid stops where the field does.
    let field = Rect::new(
        16,
        ctx.height as i32 - 92 - keyboard_height(ctx),
        ctx.width - 32,
        44,
    );
    p.strong(
        24,
        78,
        ctx.width - 48,
        if query.is_empty() {
            "Siri Suggestions"
        } else {
            "Applications"
        },
        13,
        WHITE70,
    );
    if results.is_empty() {
        p.center(0, 150, ctx.width, "No Results", 17, WHITE70);
    } else {
        let fits = ((field.y - 122) / (grid.size as i32 + 40)).max(1) as usize;
        let results = &results[..results.len().min(fits * 4)];
        let rows = results.len().div_ceil(4) as u32;
        let card = Rect::new(12, 104, ctx.width - 24, rows * (grid.size + 40) + 14);
        p.box_(card, Color(255, 255, 255, 40), 26);
        for (i, (kind, _, name)) in results.iter().enumerate() {
            app(
                p,
                grid.x((i % 4) as i32),
                card.y + 18 + (i / 4) as i32 * (grid.size as i32 + 40),
                grid.size,
                kind,
                name,
                true,
            );
        }
    }
    p.button(
        field,
        Color(255, 255, 255, 60),
        22,
        "shell:search",
        "Search applications",
    );
    p.symbol("search", field.x + 16, field.y + 14, 16, WHITE70);
    if ctx.search.is_empty() {
        p.left(
            field.x + 42,
            field.y + 12,
            field.width - 90,
            "Search",
            17,
            WHITE70,
        );
    } else {
        let end = p.left(
            field.x + 42,
            field.y + 12,
            field.width - 90,
            ctx.search,
            17,
            Color::WHITE,
        );
        p.box_(
            Rect::new(field.x + 44 + end as i32, field.y + 11, 2, 22),
            BLUE,
            1,
        );
    }
    // Dictation: there is no speech input in the simulation, so it is announced off.
    p.symbol(
        "mic",
        field.x + field.width as i32 - 34,
        field.y + 13,
        18,
        WHITE70,
    );
    p.disabled("Dictation");
}

fn app_switcher(p: &mut Painter, ctx: &ShellContext<'_>) {
    scrim(p, ctx, Color(10, 10, 18, 110), "shell:home", "Home Screen");
    let w = ctx.width as i32;
    if ctx.windows.is_empty() {
        p.center(
            0,
            ctx.height as i32 / 2 - 12,
            ctx.width,
            "No Recently Used Apps",
            17,
            WHITE70,
        );
        return;
    }
    let card_width = ctx.width * 68 / 100;
    let card_height = ctx.height * 64 / 100;
    let top = (ctx.height as i32 - card_height as i32) / 2 + 10;
    let count = ctx.windows.len() as i32;
    // Oldest cards peek from the left; the most recent sits forward on the right.
    for (i, window) in ctx.windows.iter().enumerate() {
        let depth = count - 1 - i as i32;
        let x = (w - card_width as i32) / 2 + 26 - depth * (card_width as i32 * 2 / 5);
        if x + (card_width as i32) < 0 {
            continue;
        }
        let card = Rect::new(x, top, card_width, card_height);
        p.drop_shadow(card, 26, 22, 120, 8);
        if let Some(content) = &window.content {
            p.thumbnail(content, card, 26);
        } else {
            p.box_(card, Color::rgb(250, 250, 252), 26);
        }
        // A tap switches to the application; a swipe up on its card closes it, which
        // is the only close the App Switcher has. The router recognises that swipe.
        p.region(
            card,
            &window.action("focus"),
            &format!("Switch to {}, swipe up to close", window.title),
        );
        p.asset(
            Rect::new(x + 4, top - 42, 32, 32),
            &format!("icon/ios/{}", asset(&window.kind)),
        );
        p.strong(
            x + 44,
            top - 34,
            card_width - 80,
            app_name(&window.kind).unwrap_or(&window.title),
            15,
            Color::WHITE,
        );
    }
}

fn app_library(p: &mut Painter, ctx: &ShellContext<'_>) {
    scrim(
        p,
        ctx,
        Color(18, 18, 30, 120),
        // The App Library is a screen, not a sheet: its empty glass does nothing, and
        // it is left by swiping back to the pages or by the home gesture.
        "shell:noop",
        "App Library",
    );
    let field = Rect::new(20, 66, ctx.width - 40, 42);
    p.button(
        field,
        Color(255, 255, 255, 56),
        14,
        "shell:search",
        "Search App Library",
    );
    // Typing here really goes into the query, so the field has to show it rather than
    // keep announcing itself while the keyboard fills it.
    if ctx.search.is_empty() {
        let hint = p.measure("App Library", 17, false) as i32;
        p.symbol(
            "search",
            ctx.width as i32 / 2 - hint / 2 - 24,
            field.y + 13,
            16,
            WHITE70,
        );
        p.left(
            ctx.width as i32 / 2 - hint / 2,
            field.y + 11,
            hint as u32 + 4,
            "App Library",
            17,
            WHITE70,
        );
    } else {
        p.symbol("search", field.x + 14, field.y + 13, 16, WHITE70);
        let end = p.left(
            field.x + 38,
            field.y + 11,
            field.width - 60,
            ctx.search,
            17,
            Color::WHITE,
        );
        p.box_(
            Rect::new(field.x + 40 + end as i32, field.y + 10, 2, 22),
            BLUE,
            1,
        );
    }
    let groups = library_groups(ctx);
    // One category expanded fills the screen with everything in it, which is the only
    // reason a plate is worth tapping; the chevron collapses it again.
    if let Some((title, members)) = ctx
        .library_group
        .and_then(|open| groups.iter().find(|(title, _)| *title == open))
    {
        p.symbol("chevron-left", 18, 124, 22, Color::WHITE);
        let text = p.strong(44, 124, ctx.width - 80, title, 20, Color::WHITE);
        p.region(
            Rect::new(12, 116, text + 44, 38),
            "shell:group:",
            &format!("Close {title}"),
        );
        let grid = Grid::new(ctx.width);
        for (i, (kind, _, name)) in members.iter().enumerate() {
            let y = 172 + (i / 4) as i32 * (grid.size as i32 + 44);
            if y + grid.size as i32 > ctx.height as i32 - 60 {
                break;
            }
            app(p, grid.x((i % 4) as i32), y, grid.size, kind, name, true);
        }
        return;
    }
    let pod = (ctx.width - 40 - 20) / 2;
    let icon = (pod - 48) / 2;
    for (n, (title, members)) in groups.iter().enumerate() {
        let px = 20 + (n % 2) as i32 * (pod as i32 + 20);
        let py = 132 + (n / 2) as i32 * (pod as i32 + 40);
        if py + pod as i32 > ctx.height as i32 - 60 {
            break;
        }
        let plate = Rect::new(px, py, pod, pod);
        // The plate expands the category; the icons drawn over it still launch, so a
        // tap lands on whichever of the two it was actually aimed at.
        p.button(
            plate,
            Color(255, 255, 255, 50),
            30,
            &format!("shell:group:{title}"),
            &format!("{title} group"),
        );
        for (i, (kind, _, name)) in members.iter().take(4).enumerate() {
            app(
                p,
                px + 16 + (i % 2) as i32 * (icon as i32 + 16),
                py + 16 + (i / 2) as i32 * (icon as i32 + 16),
                icon,
                kind,
                name,
                false,
            );
        }
        p.label(
            px,
            py + pod as i32 + 8,
            pod,
            title,
            13,
            Color::WHITE,
            false,
            Align::Center,
        );
    }
}

/// One category and the entries of `APPS` it holds.
type LibraryGroup = (
    &'static str,
    Vec<&'static (&'static str, &'static str, &'static str)>,
);
/// The App Library's categories, holding only applications this machine really has.
/// An empty category is dropped rather than drawn as a plate with nothing inside.
fn library_groups(ctx: &ShellContext<'_>) -> Vec<LibraryGroup> {
    LIBRARY_GROUPS
        .iter()
        .map(|(title, kinds)| {
            let members = kinds
                .iter()
                .filter(|kind| ctx.installed(kind))
                .filter_map(|kind| APPS.iter().find(|(k, _, _)| k == kind))
                .collect::<Vec<_>>();
            (*title, members)
        })
        .filter(|(_, members)| !members.is_empty())
        .collect()
}

/// Topmost visible window: the one a keystroke would reach.
fn front<'a>(ctx: &'a ShellContext<'_>) -> Option<&'a WindowView> {
    ctx.windows
        .iter()
        .rev()
        .find(|w| w.focused && !w.minimized)
        .or_else(|| ctx.windows.iter().rev().find(|w| !w.minimized))
}

/// Topmost visible window, when it is Safari.
fn focused_browser<'a>(ctx: &'a ShellContext<'_>) -> Option<&'a WindowView> {
    front(ctx).filter(|w| w.kind == "browser")
}

/// The keyboard's letter rows. Letters type lowercase and there is no `123` plane:/// The three planes of the keyboard, top row first. Between them they carry every
/// printable ASCII character, so nothing needs a modifier or a long press that does not
/// exist here. One deviation: where iOS puts a bullet last on the symbol plane this puts
/// a backtick, because a terminal needs one and no other plane would have it.
fn plane_rows(plane: crate::Plane) -> [&'static str; 3] {
    match plane {
        crate::Plane::Letters => ["qwertyuiop", "asdfghjkl", "zxcvbnm"],
        crate::Plane::Numbers => ["1234567890", "-/:;()$&@\"", ".,?!'"],
        crate::Plane::Symbols => ["[]{}#%^*+=", "_\\|~<>\u{20ac}\u{a3}\u{a5}`", ".,?!'"],
    }
}

/// Height of the QuickType bar over the keys.
const QUICKTYPE: i32 = 40;

/// Key grid at this width: gap, key width, key height, and the keyboard's own height with
/// its QuickType bar. Every plane is four rows tall, so switching one never moves the
/// field above it.
fn key_grid(width: u32) -> (i32, i32, i32, i32) {
    let gap = (width as i32 / 65).clamp(4, 8);
    let key_w = (width as i32 - 6 - gap * 9) / 10;
    let key_h = (key_w * 5 / 4).clamp(32, 46);
    (gap, key_w, key_h, key_h * 4 + gap * 3 + 24 + QUICKTYPE)
}

/// QuickType: up to three completions of the word being typed, from the same fixed word
/// list the Android keyboard uses, each typing the rest of the word and a space through
/// `shell:insert:`. With nothing to complete the bar stays, empty, as it does on iOS.
fn quicktype(p: &mut Painter, ctx: &ShellContext<'_>, top: i32) {
    let w = ctx.width as i32;
    let slot = w / 3;
    for (i, (word, action)) in ctx.suggestions(3).iter().enumerate() {
        let r = Rect::new(
            i as i32 * slot,
            top + 2,
            slot as u32,
            (QUICKTYPE - 4) as u32,
        );
        p.button(r, Color::TRANSPARENT, 8, action, word);
        p.center(r.x, r.y + 8, r.width, word, 16, INK);
        if i > 0 {
            p.vline(r.x, r.y + 8, 20, Color(60, 60, 67, 60));
        }
    }
}

/// Pixels the keyboard takes from the bottom of the screen, 0 when it is down, including
/// the home indicator strip it leaves clear beneath itself. `ctx.text_entry` is the
/// keystroke router's own answer, hoisted before the scene exists, so a painted keyboard
/// and a real keystroke cannot disagree; a dark display is the one case it does not cover.
fn keyboard_height(ctx: &ShellContext<'_>) -> i32 {
    let total = key_grid(ctx.width).3 + 34;
    if !ctx.text_entry || !ctx.awake() || total * 2 > ctx.height as i32 {
        return 0;
    }
    total
}

/// What the return key says. The keystroke is `Enter` whichever cap it wears.
fn return_cap(ctx: &ShellContext<'_>) -> &'static str {
    if ctx.panel == Some("search") || ctx.launcher_open {
        "search"
    } else if front(ctx).is_some_and(|w| w.kind == "browser" && w.editing) {
        "go"
    } else {
        "return"
    }
}

/// One key. The whole cell takes the tap, so the gaps between keys are not dead, and the
/// transparent hit target is painted last so it is the topmost thing over its own face.
#[allow(clippy::too_many_arguments)]
fn key(
    p: &mut Painter,
    face: Rect,
    gap: i32,
    cap: &str,
    size: u16,
    fill: Color,
    action: &str,
    label: &str,
) {
    p.box_(
        Rect::new(face.x, face.y + 1, face.width, face.height),
        Color(0, 0, 0, 45),
        5,
    );
    p.box_(face, fill, 5);
    if !cap.is_empty() {
        p.label(
            face.x,
            face.y + (face.height as i32 - i32::from(size) * 3 / 2) / 2,
            face.width,
            cap,
            size,
            INK,
            false,
            Align::Center,
        );
    }
    p.button(
        Rect::new(
            face.x - gap / 2,
            face.y - gap / 2,
            face.width + gap as u32,
            face.height + gap as u32,
        ),
        Color::TRANSPARENT,
        0,
        action,
        label,
    );
}

/// The shift key, drawn in the state it really holds: grey when off, light for the next
/// character only, and light with a bar beneath it when locked. One tap moves it on, as
/// it does on the device, because `shell:key:Shift` cycles the same three positions.
fn shift_key(p: &mut Painter, ctx: &ShellContext<'_>, r: Rect, gap: i32, off: Color) {
    let shift = ctx.keyboard.shift;
    let lit = shift != crate::Shift::Off;
    let fill = if lit { Color::WHITE } else { off };
    key(p, r, gap, "", 16, fill, "shell:key:Shift", "shift");
    announce(
        p,
        match shift {
            crate::Shift::Off => "Off",
            crate::Shift::Once => "Next character",
            crate::Shift::Lock => "Locked",
        },
    );
    let (cx, cy) = (r.x + r.width as i32 / 2, r.y + r.height as i32 / 2);
    p.symbol("arrow-up", cx - 11, cy - 13, 22, INK);
    if shift == crate::Shift::Lock {
        p.box_(Rect::new(cx - 8, cy + 10, 16, 2), INK, 1);
    }
}

/// The system keyboard. Every key dispatches into the same `keyboard.v1` pipeline an
/// actor's own keyboard action uses, so a painted key and a scripted keystroke agree.
/// A letter key always sends its lower-case character: the shift is applied by the
/// handler, which is what spends a one-shot shift exactly once.
fn keyboard(p: &mut Painter, ctx: &ShellContext<'_>) {
    if keyboard_height(ctx) == 0 {
        return;
    }
    let (gap, kw, kh, total) = key_grid(ctx.width);
    let w = ctx.width as i32;
    let full = kw * 10 + gap * 9;
    let left = (w - full) / 2;
    let top = ctx.height as i32 - 34 - total;
    p.glass(
        Rect::new(0, top, ctx.width, (total + 34) as u32),
        0,
        20,
        Color(209, 212, 217, 246),
        None,
    );
    quicktype(p, ctx, top);
    let heavy = Color::rgb(174, 179, 189);
    let plane = ctx.keyboard.plane;
    let upper = ctx.shifted();
    let wide = kw * 3 / 2;
    let mut y = top + QUICKTYPE + 6;
    for (row, keys) in plane_rows(plane).iter().enumerate() {
        let n = keys.chars().count() as i32;
        let x0 = left + (full - (n * kw + (n - 1) * gap)) / 2;
        for (i, ch) in keys.chars().enumerate() {
            let cap = if upper {
                ch.to_uppercase().to_string()
            } else {
                ch.to_string()
            };
            key(
                p,
                Rect::new(x0 + i as i32 * (kw + gap), y, kw as u32, kh as u32),
                gap,
                &cap,
                22,
                Color::WHITE,
                &format!("shell:type:{ch}"),
                &cap,
            );
        }
        if row == 2 {
            // Left of the bottom row: shift on the letters, the other symbol plane
            // elsewhere, which is where iOS puts each of them.
            let slot = Rect::new(left, y, wide as u32, kh as u32);
            match plane {
                crate::Plane::Letters => shift_key(p, ctx, slot, gap, heavy),
                crate::Plane::Numbers => {
                    key(p, slot, gap, "#+=", 16, heavy, "shell:plane:symbols", "#+=");
                }
                crate::Plane::Symbols => {
                    key(p, slot, gap, "123", 16, heavy, "shell:plane:numbers", "123");
                }
            }
            let back = Rect::new(left + full - wide, y, wide as u32, kh as u32);
            key(p, back, gap, "", 16, heavy, "shell:key:Backspace", "delete");
            p.symbol(
                "backspace",
                back.x + back.width as i32 / 2 - 11,
                y + kh / 2 - 11,
                22,
                INK,
            );
        }
        y += kh + gap;
    }
    let (plane_cap, plane_action) = match plane {
        crate::Plane::Letters => ("123", "shell:plane:numbers"),
        _ => ("ABC", "shell:plane:letters"),
    };
    let ret = return_cap(ctx);
    let mut x = left;
    for (cap, action, width, fill) in [
        (plane_cap, plane_action, kw * 2 + gap, heavy),
        ("space", "shell:type: ", kw * 5 + gap * 4, Color::WHITE),
        (ret, "shell:key:Enter", kw * 3 + gap * 2, heavy),
    ] {
        key(
            p,
            Rect::new(x, y, width as u32, kh as u32),
            gap,
            cap,
            16,
            fill,
            action,
            cap,
        );
        x += width + gap;
    }
    // The dictation key beneath the keys, beside the home indicator. There is no speech
    // input in the simulation, so it is announced off rather than offered.
    p.symbol(
        "mic",
        w - 44,
        ctx.height as i32 - 31,
        20,
        Color(0, 0, 0, 90),
    );
    p.disabled("Dictation, no speech input in this simulation");
}

pub fn chrome(p: &mut Painter, ctx: &ShellContext<'_>) {
    let w = ctx.width as i32;
    // A locked or sleeping display hides every window; only a power action returns.
    if !ctx.awake() {
        power_screen(p, ctx);
        return;
    }
    if ctx.launcher_open {
        app_library(p, ctx);
    }
    match ctx.panel {
        Some("overview") => app_switcher(p, ctx),
        Some("search") => search(p, ctx),
        Some("calendar") => today_view(p, ctx),
        Some("notifications") => notification_center(p, ctx),
        Some("settings") => settings(p, ctx),
        // Safari's own sheets, each opened by one button on its toolbar.
        Some("view") if focused_browser(ctx).is_some() => {
            safari_tabs(p, ctx, focused_browser(ctx).unwrap());
        }
        Some("context") if focused_browser(ctx).is_some() => {
            share_sheet(p, ctx, focused_browser(ctx).unwrap());
        }
        Some("file") if focused_browser(ctx).is_some() => {
            bookmarks_sheet(p, ctx, focused_browser(ctx).unwrap());
        }
        Some("page") if focused_browser(ctx).is_some() => {
            page_menu(p, ctx, focused_browser(ctx).unwrap());
        }
        Some(_) => control_center(p, ctx),
        None => {}
    }
    let light_content = ctx.active && !ctx.launcher_open && ctx.panel.is_none();
    let dark_app = light_content
        && ctx
            .windows
            .iter()
            .rev()
            .find(|w| !w.minimized)
            .is_some_and(|w| w.kind == "terminal");
    // Settings is drawn as the light full-screen application it is on the device.
    let fg = if (light_content && !dark_app) || ctx.panel == Some("settings") {
        INK
    } else {
        Color::WHITE
    };
    status(p, ctx, fg, ctx.panel != Some("notifications"));
    // The status bar is where the two pull-down gestures start: Notification Center
    // from the left and middle, Control Center from the right of the Dynamic Island. A
    // tap there opens neither, exactly as on the glass; see `shell:gesture:*`.
    gesture(
        p,
        Rect::new(0, 0, (w / 2 + 66).max(1) as u32, 50),
        "shell:gesture:notifications",
        "Status bar, swipe down for Notification Center",
    );
    gesture(
        p,
        Rect::new(w / 2 + 66, 0, (w / 2 - 66).max(1) as u32, 50),
        "shell:gesture:control-center",
        "Status bar, swipe down for Control Center",
    );
    keyboard(p, ctx);
    home_indicator(p, ctx, fg);
}

/// A gesture affordance: an interaction target a pointer reaches by dragging from it,
/// not by tapping it. The router turns a swipe that starts here into the gesture and
/// treats a tap as the device does, as nothing; `application.v1 shell` performs it.
fn gesture(p: &mut Painter, r: Rect, action: &str, label: &str) {
    p.region(r, action, label);
    if let Some(s) = p.scene.nodes.last_mut().and_then(|n| n.semantic.as_mut()) {
        s.role = "gesture".into();
    }
}

/// Whether the home indicator is on screen. It sits over every application and
/// application-like surface, and over the Notification Center cover sheet, but not on
/// the home screen, the App Library, Today View, Control Center or the App Switcher.
fn shows_home_indicator(ctx: &ShellContext<'_>) -> bool {
    match ctx.panel {
        Some("notifications" | "settings") => true,
        Some("view" | "context" | "file" | "page") => focused_browser(ctx).is_some(),
        Some(_) => false,
        None => ctx.active && !ctx.launcher_open,
    }
}

/// The home indicator, the thin bar along the bottom edge. It is not a button: a swipe
/// up from it goes home, a longer one opens the App Switcher, and a tap does nothing.
fn home_indicator(p: &mut Painter, ctx: &ShellContext<'_>, fg: Color) {
    if !shows_home_indicator(ctx) {
        return;
    }
    let (w, h) = (ctx.width as i32, ctx.height as i32);
    // Over the keyboard's light plate the indicator has to darken to stay visible.
    let bar = if keyboard_height(ctx) > 0 { INK } else { fg };
    p.box_(Rect::new(w / 2 - 67, h - 13, 134, 5), bar, 3);
    gesture(
        p,
        Rect::new(w / 2 - 100, h - 28, 200, 28),
        "shell:gesture:home",
        "Home indicator, swipe up to go home",
    );
}

pub fn window_frame(p: &mut Painter, ctx: &ShellContext<'_>, window: &WindowView) {
    let dark = window.kind == "terminal";
    p.box_(
        Rect::new(0, 0, ctx.width, ctx.height),
        if dark {
            Color::rgb(0, 0, 0)
        } else if window.kind == "files" {
            Color::WHITE
        } else {
            Color::rgb(249, 249, 249)
        },
        0,
    );
    if window.kind == "browser" || window.kind == "music" {
        return;
    }
    let ink = if dark { Color::WHITE } else { INK };
    let folder = window.document.trim_end_matches('/');
    let title = navigation_title(window);
    let title = title.as_str();
    // An application with a large title shows it in its content, not twice: the bar
    // stays clear until the large title has scrolled under it, and then carries the
    // title inline over a hairline, as UINavigationBar does.
    let large = window
        .content
        .as_ref()
        .and_then(|c| c.scrolls.iter().find(|a| a.title.is_some()));
    let inline = large.is_none_or(|a| a.title_collapsed());
    if inline {
        let title = large.and_then(|a| a.title.as_deref()).unwrap_or(title);
        p.strong_center(90, 62, ctx.width.saturating_sub(180), title, 17, ink);
    }
    if !dark && window.kind != "files" && inline {
        p.hline(0, 95, ctx.width, HAIRLINE);
    }
    // In Files the chevron pops one folder, exactly like the crumb the content draws;
    // at the root there is nowhere to pop to, so no chevron is painted at all. No other
    // application gets a way "Home" in its navigation bar: an iPhone has none there, and
    // leaving an application is the home indicator's swipe.
    // An application's own way back to its parent screen (Mail's Mailboxes, a
    // conversation's list) is the bar's leading chevron, named for where it goes.
    if let Some((target, label)) = window.chrome("nav").and_then(|nav| {
        let mut parts = nav.splitn(3, '\t');
        (parts.next()? == "back").then_some(())?;
        Some((parts.next()?, parts.next().unwrap_or("")))
    }) {
        p.symbol("chevron-left", 8, 61, 22, BLUE);
        let text = if label.is_empty() {
            0
        } else {
            p.left(29, 62, 120, label, 17, BLUE)
        };
        p.region(
            Rect::new(4, 51, text + 34, 42),
            &window.action(&format!("content:{target}")),
            if label.is_empty() { "Back" } else { label },
        );
    }
    if window.kind == "files" && !folder.is_empty() {
        let label = folder
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .and_then(|parent| parent.rsplit('/').find(|s| !s.is_empty()))
            .unwrap_or("Browse");
        p.symbol("chevron-left", 8, 61, 22, BLUE);
        let text = p.left(29, 62, 80, label, 17, BLUE);
        p.region(
            Rect::new(4, 51, text + 34, 42),
            &window.action("content:files-up"),
            label,
        );
    }
}

/// Safari: address bar beneath the status bar and the standard bottom toolbar.
pub fn browser_chrome(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    let site = super::window_content_rect_for_kind(ctx.theme, w.rect, "browser");
    let width = ctx.width;
    p.box_(Rect::new(0, 0, width, site.y as u32), BAR, 0);
    p.hline(0, site.y - 1, width, HAIRLINE);
    let field = Rect::new(12, 56, width - 24, 40);
    p.button(
        field,
        if w.editing {
            Color::WHITE
        } else {
            Color(118, 118, 128, 40)
        },
        12,
        &w.action("content:shell:address"),
        "Address and search",
    );
    let typed = w
        .title
        .split_once(" — ")
        .map_or(w.title.as_str(), |(_, a)| a);
    let reload = Rect::new(field.x + field.width as i32 - 38, field.y + 4, 32, 32);
    if typed.is_empty() {
        p.symbol("search", field.x + 12, field.y + 12, 16, GRAY);
        p.left(
            field.x + 36,
            field.y + 10,
            field.width - 80,
            "Search or enter website name",
            16,
            GRAY,
        );
    } else if w.editing {
        let end = p.left(field.x + 14, field.y + 10, field.width - 60, typed, 16, INK);
        p.box_(
            Rect::new(field.x + 15 + end as i32, field.y + 9, 2, 22),
            BLUE,
            1,
        );
    } else {
        let host = typed
            .split_once("://")
            .map_or(typed, |(_, r)| r)
            .split('/')
            .next()
            .unwrap_or(typed);
        let text = p.measure(host, 16, false).min(field.width - 100) as i32;
        let x = field.x + (field.width as i32 - text) / 2;
        p.symbol("lock", x - 17, field.y + 13, 13, GRAY);
        p.left(x, field.y + 10, field.width - 100, host, 16, INK);
        // "AA" opens the page menu, whose text size really re-lays the page out.
        let aa = Rect::new(field.x + 4, field.y + 4, 40, 32);
        p.button(
            aa,
            Color::TRANSPARENT,
            8,
            "shell:panel:page",
            "Page Settings",
        );
        p.left(field.x + 12, field.y + 11, 30, "AA", 14, INK);
    }
    p.symbol("reload", reload.x + 8, reload.y + 8, 16, INK);
    p.region(reload, &w.action("content:shell:reload"), "Reload");
    let bar_y = site.y + site.height as i32;
    p.box_(
        Rect::new(0, bar_y, width, ctx.height.saturating_sub(bar_y as u32)),
        BAR,
        0,
    );
    p.hline(0, bar_y, width, HAIRLINE);
    let slot = width as i32 / 5;
    for (i, (symbol, label)) in [
        ("chevron-left", "Back"),
        ("chevron-right", "Forward"),
        ("share", "Share"),
        ("book", "Bookmarks"),
        ("tabs", "Tabs"),
    ]
    .iter()
    .enumerate()
    {
        // Share opens the sheet, Bookmarks the saved pages, Tabs the tab overview. A
        // blank tab has nothing to hand over and no application may be able to receive
        // it, so Share greys out rather than dispatching what would be refused; Back and
        // Forward follow the tab's real history for the same reason.
        let shareable = !w.document.is_empty() && (ctx.installed("chat") || ctx.installed("mail"));
        let action = match i {
            0 => w.can_go_back.then(|| w.action("content:shell:back")),
            1 => w.can_go_forward.then(|| w.action("content:shell:forward")),
            2 => shareable.then(|| "shell:panel:context".to_owned()),
            3 => Some("shell:panel:file".to_owned()),
            _ => Some("shell:panel:view".to_owned()),
        };
        let cx = slot * i as i32 + slot / 2;
        p.symbol(
            symbol,
            cx - 11,
            bar_y + 11,
            22,
            if action.is_some() {
                BLUE
            } else {
                Color(0, 122, 255, 90)
            },
        );
        match action {
            Some(action) => p.region(Rect::new(cx - 24, bar_y + 2, 48, 42), &action, label),
            None => p.disabled(label),
        }
        if i == 4 && !w.tabs.is_empty() {
            p.label(
                cx - 11,
                bar_y + 16,
                22,
                &w.tabs.len().to_string(),
                11,
                BLUE,
                true,
                Align::Center,
            );
        }
    }
}

/// Safari's tab overview, drawn from the browser session's real tabs. Selecting,
/// closing and adding all reach the session through `WindowView`.
fn safari_tabs(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    scrim(
        p,
        ctx,
        Color(20, 20, 26, 170),
        "shell:dismiss",
        "Close tab overview",
    );
    let gap = 16;
    let card_w = (ctx.width as i32 - gap * 3) / 2;
    let card_h = card_w * 4 / 3;
    let bar_y = ctx.height as i32 - 64;
    for (i, title) in w.tabs.iter().enumerate() {
        let x = gap + (i as i32 % 2) * (card_w + gap);
        let y = 104 + (i as i32 / 2) * (card_h + 34);
        if y + card_h > bar_y - 8 {
            break;
        }
        let card = Rect::new(x, y, card_w as u32, card_h as u32);
        if i == w.active_tab {
            p.border(
                Rect::new(x - 3, y - 3, card.width + 6, card.height + 6),
                Color::TRANSPARENT,
                17,
                BLUE,
            );
        }
        p.button(card, Color::WHITE, 14, &w.tab_select(i), title);
        p.left(x + 10, y + 9, card.width.saturating_sub(44), title, 13, INK);
        p.hline(x, y + 33, card.width, HAIRLINE);
        p.center(
            x,
            y + card_h / 2 - 8,
            card.width,
            if i == w.active_tab { &w.document } else { "" },
            12,
            GRAY,
        );
        let close = Rect::new(x + card_w - 30, y + 5, 24, 24);
        p.button(
            close,
            Color(118, 118, 128, 40),
            12,
            &w.tab_close(i),
            &format!("Close {title}"),
        );
        p.symbol("close", close.x + 6, close.y + 6, 12, GRAY);
    }
    p.box_(
        Rect::new(0, bar_y, ctx.width, ctx.height.saturating_sub(bar_y as u32)),
        BAR,
        0,
    );
    p.hline(0, bar_y, ctx.width, HAIRLINE);
    let plus = Rect::new(12, bar_y + 8, 44, 44);
    p.button(plus, Color::TRANSPARENT, 22, &w.tab_new(), "New Tab");
    p.symbol("plus", plus.x + 12, plus.y + 12, 20, BLUE);
    p.strong_center(
        0,
        bar_y + 19,
        ctx.width,
        &if w.tabs.len() == 1 {
            "1 Tab".to_owned()
        } else {
            format!("{} Tabs", w.tabs.len())
        },
        15,
        INK,
    );
    let done = Rect::new(ctx.width as i32 - 88, bar_y + 8, 76, 44);
    p.button(done, Color::TRANSPARENT, 12, "shell:dismiss", "Done");
    p.right(done.x, bar_y + 19, 64, "Done", 17, BLUE);
}

/// Safari's Share sheet. Every row hands the page on screen to something that really
/// takes it: a messaging application, or the bookmark store every browser window shares.
/// An application this machine does not have is left off the sheet rather than greyed.
/// Safari's "AA" menu: the page's text size, stepped through the browser's zoom
/// levels and remembered for the site. The percentage between the two A's resets it.
fn page_menu(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    // The page stays visible behind the menu, so the new text size can be seen.
    p.region(
        Rect::new(0, 0, ctx.width, ctx.height),
        "shell:dismiss",
        "Close Page Menu",
    );
    let card = Rect::new(12, 108, ctx.width.saturating_sub(24).min(320), 64);
    p.drop_shadow(card, 16, 24, 70, 8);
    p.box_(card, Color(242, 242, 247, 250), 16);
    p.region(card, "shell:noop", "Page Menu");
    let zoom = w.zoom_percent();
    let third = card.width as i32 / 3;
    let smaller = Rect::new(card.x, card.y, third as u32, card.height);
    let reset = Rect::new(card.x + third, card.y, third as u32, card.height);
    let larger = Rect::new(card.x + third * 2, card.y, third as u32, card.height);
    p.button(
        smaller,
        Color::TRANSPARENT,
        16,
        "shell:zoom:out",
        "Smaller Text",
    );
    p.center(
        smaller.x,
        card.y + 22,
        smaller.width,
        "A",
        14,
        if zoom > 50 { INK } else { GRAY },
    );
    p.button(
        reset,
        Color::TRANSPARENT,
        0,
        "shell:zoom:reset",
        "Actual Size",
    );
    p.center(
        reset.x,
        card.y + 22,
        reset.width,
        &format!("{zoom}%"),
        16,
        INK,
    );
    p.button(
        larger,
        Color::TRANSPARENT,
        16,
        "shell:zoom:in",
        "Larger Text",
    );
    p.center(
        larger.x,
        card.y + 17,
        larger.width,
        "A",
        22,
        if zoom < 300 { INK } else { GRAY },
    );
    p.vline(reset.x, card.y + 14, 36, HAIRLINE);
    p.vline(larger.x, card.y + 14, 36, HAIRLINE);
}

fn share_sheet(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    scrim(
        p,
        ctx,
        Color(20, 20, 26, 150),
        "shell:dismiss",
        "Close Share Sheet",
    );
    // A blank tab has no address, so every row would be refused; the sheet says that
    // rather than filling itself with hand-offs nothing could take.
    let mut rows: Vec<(&str, &str, &str)> = Vec::new();
    if !w.document.is_empty() {
        if ctx.installed("chat") {
            rows.push(("chat", "shell:share:chat", "Messages"));
        }
        if ctx.installed("mail") {
            rows.push(("mail", "shell:share:mail", "Mail"));
        }
        rows.push(if ctx.bookmarked {
            ("star", "shell:bookmark", "Remove Bookmark")
        } else {
            ("star-outline", "shell:bookmark", "Add Bookmark")
        });
    }
    let height = 78 + rows.len().max(1) as i32 * 56;
    let top = ctx.height as i32 - height - 30;
    p.box_(
        Rect::new(8, top, ctx.width - 16, (height + 30) as u32),
        Color(242, 242, 247, 250),
        30,
    );
    let subject = if w.caption.is_empty() {
        w.document.as_str()
    } else {
        w.caption.as_str()
    };
    p.strong(
        28,
        top + 18,
        ctx.width.saturating_sub(120),
        subject,
        16,
        INK,
    );
    p.left(
        28,
        top + 40,
        ctx.width.saturating_sub(120),
        &w.document,
        13,
        GRAY,
    );
    let done = Rect::new(ctx.width as i32 - 84, top + 12, 68, 40);
    p.button(done, Color(118, 118, 128, 30), 20, "shell:dismiss", "Done");
    p.center(done.x, top + 22, 68, "Done", 15, INK);
    if rows.is_empty() {
        p.center(0, top + 88, ctx.width, "Nothing to Share", 15, GRAY);
        return;
    }
    for (i, (symbol, action, label)) in rows.iter().enumerate() {
        let row = Rect::new(16, top + 70 + i as i32 * 56, ctx.width - 32, 48);
        p.button(row, Color::WHITE, 14, action, label);
        p.symbol(symbol, row.x + 16, row.y + 14, 20, BLUE);
        p.left(row.x + 50, row.y + 14, row.width - 70, label, 16, INK);
    }
}

/// Safari's Bookmarks list, drawn from the machine's real bookmark store. Opening one
/// navigates the tab; the toggle at the top saves or unsaves the page on screen.
fn bookmarks_sheet(p: &mut Painter, ctx: &ShellContext<'_>, w: &WindowView) {
    scrim(
        p,
        ctx,
        Color(20, 20, 26, 150),
        "shell:dismiss",
        "Close Bookmarks",
    );
    let card = Rect::new(8, 88, ctx.width - 16, ctx.height.saturating_sub(140));
    p.box_(card, Color(242, 242, 247, 250), 30);
    p.strong_center(card.x, card.y + 20, card.width, "Bookmarks", 17, INK);
    let done = Rect::new(card.x + card.width as i32 - 84, card.y + 12, 68, 40);
    p.button(done, Color(118, 118, 128, 30), 20, "shell:dismiss", "Done");
    p.center(done.x, card.y + 22, 68, "Done", 15, INK);
    let mut y = card.y + 62;
    // An empty tab has no address to save, so the toggle only appears with a page.
    if !w.document.is_empty() {
        let row = Rect::new(card.x + 8, y, card.width - 16, 48);
        let label = if ctx.bookmarked {
            "Remove Bookmark"
        } else {
            "Add Bookmark"
        };
        p.button(row, Color::WHITE, 14, "shell:bookmark", label);
        p.symbol(
            if ctx.bookmarked {
                "star"
            } else {
                "star-outline"
            },
            row.x + 16,
            row.y + 14,
            20,
            BLUE,
        );
        p.left(row.x + 50, row.y + 14, row.width - 70, label, 16, INK);
        y += 60;
    }
    if ctx.bookmarks.is_empty() {
        p.center(card.x, y + 24, card.width, "No Bookmarks", 15, GRAY);
        return;
    }
    for (i, bookmark) in ctx.bookmarks.iter().enumerate() {
        let row = Rect::new(card.x + 8, y + i as i32 * 56, card.width - 16, 52);
        if row.y + 52 > card.y + card.height as i32 - 8 {
            break;
        }
        p.button(
            row,
            Color::WHITE,
            14,
            &format!("shell:bookmark:open:{i}"),
            &bookmark.title,
        );
        p.symbol("book", row.x + 16, row.y + 15, 18, BLUE);
        p.strong(
            row.x + 50,
            row.y + 8,
            row.width - 70,
            &bookmark.title,
            15,
            INK,
        );
        p.left(
            row.x + 50,
            row.y + 28,
            row.width - 70,
            &bookmark.url,
            12,
            GRAY,
        );
    }
}

#[cfg(test)]
mod library_invariant_tests {
    use super::*;
    /// The App Library promises every application sits in exactly one category. A new
    /// app added to `APPS` but not here would silently vanish from the library.
    #[test]
    fn every_application_is_in_exactly_one_library_category() {
        for (kind, _, _) in APPS {
            let homes: Vec<_> = LIBRARY_GROUPS
                .iter()
                .filter(|(_, members)| members.contains(&kind))
                .map(|(title, _)| *title)
                .collect();
            assert_eq!(homes.len(), 1, "{kind} is in {homes:?}");
        }
        for (title, members) in LIBRARY_GROUPS {
            for member in members {
                assert!(
                    APPS.iter().any(|(k, _, _)| k == member),
                    "{title} lists {member}, which is not an application"
                );
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::desktop_scene::DesktopTheme;

    #[test]
    fn home_only_exposes_installed_apps_and_shows_no_home_indicator() {
        let installed = vec!["files".to_owned(), "browser".to_owned()];
        let ctx = ShellContext {
            theme: DesktopTheme::Ios,
            width: 390,
            height: 844,
            clock_us: 0,
            title: "",
            launcher_open: false,
            active: false,
            windows: &[],
            installed_apps: &installed,
            panel: None,
            search: "",
            hover: None,
            desktop_selection: None,
            settings: &crate::SystemSettings::DEFAULT,
            screen: crate::ScreenState::Active,
            panel_month: 0,
            text_entry: false,
            keyboard: crate::KeyboardState::default(),
            bookmarks: &[],
            downloads: &[],
            notifications: &[],
            workspaces: 1,
            workspace: 0,
            library_group: None,
            bookmarked: false,
            panel_over_launcher: false,
            home_page: 0,
            typed: "",
            user: "alice",
            home: "/Users/alice",
            recents: &[],
            battery: true,
            anchor: None,
            overview: Default::default(),
        };
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        background(&mut p, &ctx);
        chrome(&mut p, &ctx);
        let mut launches = Vec::new();
        for node in &p.scene.nodes {
            if let Some(id) = node
                .interaction
                .as_deref()
                .and_then(|a| a.strip_prefix("shell:launch:"))
            {
                assert!(installed.iter().any(|app| app == id));
                launches.push(id.to_owned());
            }
        }
        assert!(launches.contains(&"files".to_owned()) && launches.contains(&"browser".to_owned()));
        // The home screen has no home indicator, and nothing on it is a "Home" button.
        assert!(!p.scene.nodes.iter().any(|n| matches!(
            n.interaction.as_deref(),
            Some("shell:home" | "shell:gesture:home")
        )));
        assert!(p.scene.nodes.iter().any(|node| matches!(&node.primitive, cw_scene::Primitive::UiTextBold {text,..} if text == "9:00")));
    }

    /// Shared phone context; every test below varies only what it is about.
    fn phone<'a>(
        windows: &'a [WindowView],
        panel: Option<&'a str>,
        settings: &'a crate::SystemSettings,
        screen: crate::ScreenState,
    ) -> ShellContext<'a> {
        ShellContext {
            theme: DesktopTheme::Ios,
            width: 390,
            height: 844,
            clock_us: 0,
            title: "",
            launcher_open: false,
            active: !windows.is_empty(),
            windows,
            installed_apps: &[],
            panel,
            search: "",
            hover: None,
            desktop_selection: None,
            settings,
            screen,
            panel_month: 0,
            text_entry: false,
            keyboard: crate::KeyboardState::default(),
            bookmarks: &[],
            downloads: &[],
            notifications: &[],
            workspaces: 1,
            workspace: 0,
            library_group: None,
            bookmarked: false,
            panel_over_launcher: false,
            home_page: 0,
            typed: "",
            user: "alice",
            home: "/Users/alice",
            recents: &[],
            battery: true,
            anchor: None,
            overview: Default::default(),
        }
    }
    fn ids(p: &Painter) -> Vec<String> {
        p.scene
            .nodes
            .iter()
            .filter_map(|n| n.interaction.clone())
            .collect()
    }
    /// Whether some node paints exactly this line of text.
    fn shown(p: &Painter, needle: &str) -> bool {
        p.scene.nodes.iter().any(|n| {
            matches!(
                &n.primitive,
                cw_scene::Primitive::UiText { text, .. }
                    | cw_scene::Primitive::UiTextBold { text, .. } if text == needle
            )
        })
    }
    /// Announced disabled: named, unreachable and out of the focus order.
    fn greyed(p: &Painter, label: &str) -> bool {
        p.scene.nodes.iter().any(|n| {
            n.interaction.is_none()
                && n.semantic
                    .as_ref()
                    .is_some_and(|s| s.label == label && s.disabled && !s.focusable)
        })
    }
    fn phone_window(id: u64, kind: &str) -> WindowView {
        WindowView {
            id,
            title: kind.into(),
            kind: kind.into(),
            rect: Rect::new(0, 0, 390, 844),
            focused: true,
            ..Default::default()
        }
    }
    fn value(p: &Painter, action: &str) -> Option<String> {
        p.scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some(action))
            .and_then(|n| n.semantic.as_ref())
            .and_then(|s| s.value.clone())
    }

    #[test]
    fn control_center_moves_real_switches_and_levels_and_reads_them_back() {
        let settings = crate::SystemSettings {
            wifi: false,
            flashlight: true,
            brightness: 40,
            volume: 0,
            ..crate::SystemSettings::DEFAULT
        };
        let ctx = phone(&[], Some("quick"), &settings, crate::ScreenState::Active);
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let painted = ids(&p);
        for expected in [
            "shell:toggle:airplane_mode",
            "shell:toggle:hotspot",
            "shell:toggle:wifi",
            "shell:toggle:bluetooth",
            "shell:toggle:rotation_lock",
            "shell:toggle:dark_mode",
            "shell:toggle:do_not_disturb",
            "shell:toggle:flashlight",
            "shell:toggle:battery_saver",
            "shell:toggle:night_light",
            "shell:settings",
            "shell:set:brightness:100",
            "shell:set:brightness:0",
            "shell:set:volume:50",
        ] {
            assert!(
                painted.iter().any(|id| id == expected),
                "missing {expected}"
            );
        }
        // Nothing in this shell may resolve to a do-nothing handler.
        assert!(!painted.iter().any(|id| id == "shell:noop"));
        assert_eq!(value(&p, "shell:toggle:wifi").as_deref(), Some("Off"));
        assert_eq!(value(&p, "shell:toggle:flashlight").as_deref(), Some("On"));
        // The brightness column runs 100% at the top to 0% at the bottom.
        assert_eq!(
            p.scene.hit_test(238, 274).unwrap().interaction.as_deref(),
            Some("shell:set:brightness:100")
        );
        assert_eq!(
            p.scene.hit_test(238, 424).unwrap().interaction.as_deref(),
            Some("shell:set:brightness:0")
        );
        // Now Playing is the way into Music, which owns the real now-playing record;
        // there is still no transport id, so no transport button is painted.
        assert!(painted.iter().any(|id| id == "shell:launch:music"));
        assert!(!painted.iter().any(|id| id.starts_with("shell:media")));
        // Without Music there is nothing behind the card, and it says so.
        let installed = ["browser".to_owned()];
        let mut ctx = phone(&[], Some("quick"), &settings, crate::ScreenState::Active);
        ctx.installed_apps = &installed;
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        assert!(greyed(&p, "Now Playing"));
        assert!(!ids(&p).iter().any(|id| id == "shell:launch:music"));
        assert!(shown(&p, "Not Playing"));
    }

    #[test]
    fn settings_screen_shows_and_flips_the_same_state_as_control_center() {
        let settings = crate::SystemSettings {
            wifi: false,
            do_not_disturb: true,
            brightness: 40,
            ..crate::SystemSettings::DEFAULT
        };
        let ctx = phone(&[], Some("settings"), &settings, crate::ScreenState::Active);
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let painted = ids(&p);
        for expected in [
            "shell:toggle:airplane_mode",
            "shell:toggle:wifi",
            "shell:toggle:bluetooth",
            "shell:toggle:hotspot",
            "shell:toggle:do_not_disturb",
            "shell:toggle:rotation_lock",
            "shell:toggle:dark_mode",
            "shell:toggle:night_light",
            "shell:toggle:battery_saver",
            "shell:set:brightness:0",
            "shell:set:brightness:100",
            "shell:set:volume:100",
            "shell:power:lock",
            "shell:power:restart",
            "shell:power:off",
            "shell:gesture:home",
        ] {
            assert!(
                painted.iter().any(|id| id == expected),
                "missing {expected}"
            );
        }
        // Settings is a full-screen application: it has no close button, and it is left
        // by the home indicator like every other application.
        assert!(!painted.iter().any(|id| id == "shell:dismiss"));
        assert_eq!(value(&p, "shell:toggle:wifi").as_deref(), Some("Off"));
        assert_eq!(
            value(&p, "shell:toggle:do_not_disturb").as_deref(),
            Some("On")
        );
        assert!(p.scene.nodes.iter().any(
            |n| matches!(&n.primitive, cw_scene::Primitive::UiText { text, .. } if text == "40%")
        ));
        // The shortest phone the gallery renders must still reach the power actions.
        let short = ShellContext {
            height: 780,
            ..phone(&[], Some("settings"), &settings, crate::ScreenState::Active)
        };
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 780, 1);
        chrome(&mut p, &short);
        assert!(ids(&p).iter().any(|id| id == "shell:power:off"));
    }

    #[test]
    fn locked_and_sleeping_screens_cover_everything_and_offer_a_real_wake() {
        for screen in [crate::ScreenState::Locked, crate::ScreenState::Off] {
            let settings = crate::SystemSettings::DEFAULT;
            let ctx = phone(&[], None, &settings, screen);
            let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
            background(&mut p, &ctx);
            chrome(&mut p, &ctx);
            assert_eq!(
                p.scene.hit_test(195, 500).unwrap().interaction.as_deref(),
                Some("shell:power:wake"),
                "{screen:?}"
            );
            assert!(
                !ids(&p).iter().any(|id| id.starts_with("shell:launch:")
                    || id == "shell:home"
                    || id.starts_with("shell:panel:")),
                "{screen:?} still exposes the running system"
            );
        }
    }

    #[test]
    fn safari_toolbar_and_tab_overview_reach_the_real_browser_session() {
        let window = WindowView {
            id: 7,
            title: "Browser — https://intranet.internal/".into(),
            kind: "browser".into(),
            rect: Rect::new(0, 0, 390, 844),
            focused: true,
            document: "https://intranet.internal/".into(),
            tabs: vec!["Intranet".into(), "News".into()],
            active_tab: 1,
            can_go_back: true,
            can_go_forward: true,
            ..Default::default()
        };
        let windows = [window.clone()];
        let settings = crate::SystemSettings::DEFAULT;
        let ctx = phone(&windows, None, &settings, crate::ScreenState::Active);
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        browser_chrome(&mut p, &ctx, &window);
        let painted = ids(&p);
        for expected in [
            "window:7:content:shell:back",
            "window:7:content:shell:forward",
            "window:7:content:shell:reload",
            "window:7:content:shell:address",
            "shell:panel:view",
        ] {
            assert!(
                painted.iter().any(|id| id == expected),
                "missing {expected}"
            );
        }
        // Share, Bookmarks and the "AA" page menu each open their own sheet.
        for expected in ["shell:panel:context", "shell:panel:file"] {
            assert!(
                painted.iter().any(|id| id == expected),
                "missing {expected}"
            );
        }
        assert!(painted.iter().any(|id| id == "shell:panel:page"));
        let ctx = phone(
            &windows,
            Some("view"),
            &settings,
            crate::ScreenState::Active,
        );
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let painted = ids(&p);
        for expected in [
            "shell:tab:select:0",
            "shell:tab:select:1",
            "shell:tab:close:0",
            "shell:tab:close:1",
            "shell:tab:new",
            "shell:dismiss",
        ] {
            assert!(
                painted.iter().any(|id| id == expected),
                "missing {expected}"
            );
        }
    }

    #[test]
    fn the_keyboard_follows_the_routers_own_answer_about_text_entry() {
        let settings = crate::SystemSettings::DEFAULT;
        let base = |panel| phone(&[], panel, &settings, crate::ScreenState::Active);
        // `ctx.text_entry` is the keystroke router's decision, so the shell asks it
        // rather than guessing from the window in front.
        for (text_entry, screen, up) in [
            (false, crate::ScreenState::Active, false),
            (true, crate::ScreenState::Active, true),
            // A dark or locked display is the one case the router does not cover.
            (true, crate::ScreenState::Locked, false),
            (true, crate::ScreenState::Off, false),
        ] {
            let ctx = ShellContext {
                text_entry,
                screen,
                ..base(None)
            };
            let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
            background(&mut p, &ctx);
            chrome(&mut p, &ctx);
            assert_eq!(
                ids(&p).iter().any(|id| id.starts_with("shell:type:")),
                up,
                "text_entry={text_entry} screen={screen:?}"
            );
        }
        // Spotlight's query field takes text, so the keys come up under it.
        let ctx = ShellContext {
            text_entry: true,
            ..base(Some("search"))
        };
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let painted = ids(&p);
        for ch in "abcdefghijklmnopqrstuvwxyz ".chars() {
            assert!(
                painted.iter().any(|id| id == &format!("shell:type:{ch}")),
                "no key types {ch:?}"
            );
        }
        for expected in [
            "shell:key:Backspace",
            "shell:key:Enter",
            "shell:key:Shift",
            "shell:plane:numbers",
        ] {
            assert!(
                painted.iter().any(|id| id == expected),
                "missing {expected}"
            );
        }
        // The query field is lifted clear of the keys instead of hiding behind them.
        let field = p
            .scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some("shell:search"))
            .unwrap()
            .bounds;
        let keys = p
            .scene
            .nodes
            .iter()
            .filter(|n| n.interaction.as_deref() == Some("shell:type:q"))
            .map(|n| n.bounds.y)
            .min()
            .unwrap();
        assert!(
            field.y + field.height as i32 <= keys,
            "the field is covered"
        );
        // Every key lands on screen, clear of the status bar and the home indicator.
        for n in p.scene.nodes.iter().filter(|n| {
            n.interaction.as_deref().is_some_and(|a| {
                a.starts_with("shell:type:")
                    || a.starts_with("shell:key:")
                    || a.starts_with("shell:plane:")
            })
        }) {
            let b = n.bounds;
            assert!(
                b.x >= 0 && b.x + b.width as i32 <= 390,
                "{:?} runs off the edge",
                n.interaction
            );
            assert!(
                b.y > 50 && b.y + b.height as i32 <= 810,
                "{:?} escapes the keyboard",
                n.interaction
            );
        }
    }

    #[test]
    fn the_three_planes_between_them_type_every_printable_character() {
        let settings = crate::SystemSettings::DEFAULT;
        let mut reachable = std::collections::BTreeSet::new();
        for (plane, switches) in [
            (
                crate::Plane::Letters,
                ["shell:plane:numbers", "shell:key:Shift"],
            ),
            (
                crate::Plane::Numbers,
                ["shell:plane:letters", "shell:plane:symbols"],
            ),
            (
                crate::Plane::Symbols,
                ["shell:plane:letters", "shell:plane:numbers"],
            ),
        ] {
            let ctx = ShellContext {
                text_entry: true,
                keyboard: crate::KeyboardState {
                    plane,
                    ..Default::default()
                },
                ..phone(&[], Some("search"), &settings, crate::ScreenState::Active)
            };
            let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
            chrome(&mut p, &ctx);
            let painted = ids(&p);
            // Every plane can be left again, and keeps delete and return.
            for id in switches
                .iter()
                .chain(["shell:key:Backspace", "shell:key:Enter"].iter())
            {
                assert!(painted.iter().any(|a| a == id), "{plane:?} lacks {id}");
            }
            for typed in painted
                .iter()
                .filter_map(|id| id.strip_prefix("shell:type:"))
            {
                assert_eq!(typed.chars().count(), 1, "{typed:?} is not one character");
                assert!(!typed.chars().any(char::is_control));
                reachable.insert(typed.chars().next().unwrap());
            }
            // Every cap is a glyph the bundled font really has: a missing one would
            // paint a key that looks blank and says nothing about what it types.
            for row in plane_rows(plane) {
                for ch in row.chars() {
                    assert!(
                        p.measure(&ch.to_string(), 22, false) > 0,
                        "{ch:?} has no glyph"
                    );
                }
            }
        }
        // Shift supplies the capitals, so lower case is all the keys have to carry.
        for ch in (0x20u8..0x7f)
            .map(char::from)
            .filter(|c| !c.is_ascii_uppercase())
        {
            assert!(reachable.contains(&ch), "no plane types {ch:?}");
        }
    }

    #[test]
    fn the_shift_key_changes_the_cap_but_never_the_character_it_sends() {
        let settings = crate::SystemSettings::DEFAULT;
        for (shift, cap, state) in [
            (crate::Shift::Off, "q", "Off"),
            (crate::Shift::Once, "Q", "Next character"),
            (crate::Shift::Lock, "Q", "Locked"),
        ] {
            let ctx = ShellContext {
                text_entry: true,
                keyboard: crate::KeyboardState {
                    shift,
                    ..Default::default()
                },
                ..phone(&[], Some("search"), &settings, crate::ScreenState::Active)
            };
            let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
            chrome(&mut p, &ctx);
            // The id stays lower case whatever the cap reads: the handler applies the
            // modifier, which is what spends a one-shot shift exactly once.
            assert!(ids(&p).iter().any(|id| id == "shell:type:q"), "{shift:?}");
            assert!(!ids(&p).iter().any(|id| id == "shell:type:Q"), "{shift:?}");
            assert!(shown(&p, cap), "the q key should read {cap}");
            assert_eq!(value(&p, "shell:key:Shift").as_deref(), Some(state));
        }
    }

    #[test]
    fn a_tapped_key_types_the_character_it_shows_into_the_focused_field() {
        let settings = crate::SystemSettings::DEFAULT;
        let windows = [phone_window(5, "terminal")];
        // The same desktop the shell action reaches, driven only by what the keys say.
        let mut desktop = crate::DesktopState::default();
        let (id, _) = desktop.launch("terminal", "").unwrap();
        desktop.focused = Some(id);
        for (plane, keys) in [
            (
                crate::Plane::Letters,
                ["shell:type:l", "shell:type:s", "shell:type: "].as_slice(),
            ),
            (
                crate::Plane::Numbers,
                ["shell:type:.", "shell:key:Backspace"].as_slice(),
            ),
        ] {
            let ctx = ShellContext {
                text_entry: true,
                keyboard: crate::KeyboardState {
                    plane,
                    ..Default::default()
                },
                ..phone(&windows, None, &settings, crate::ScreenState::Active)
            };
            let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
            chrome(&mut p, &ctx);
            for want in keys {
                let node = p
                    .scene
                    .nodes
                    .iter()
                    .find(|n| n.interaction.as_deref() == Some(*want))
                    .unwrap_or_else(|| panic!("no key emits {want}"));
                let (cx, cy) = (
                    node.bounds.x + node.bounds.width as i32 / 2,
                    node.bounds.y + node.bounds.height as i32 / 2,
                );
                assert_eq!(
                    p.scene.hit_test(cx, cy).unwrap().interaction.as_deref(),
                    Some(*want),
                    "{want} is not the topmost control at its own centre"
                );
                match want.strip_prefix("shell:type:") {
                    Some(text) => desktop.text(text).unwrap(),
                    None => {
                        desktop.key(want.trim_start_matches("shell:key:")).unwrap();
                    }
                }
            }
        }
        // Return runs what the keys typed, backspace and space included.
        let effects = desktop.key("Enter").unwrap();
        assert!(
            matches!(effects.as_slice(), [crate::AppEffect::Execute { command, .. }] if command == "ls "),
            "{effects:?}"
        );
    }

    #[test]
    fn safari_greys_back_and_forward_until_the_tab_has_history() {
        let settings = crate::SystemSettings::DEFAULT;
        let fresh = WindowView {
            tabs: vec!["New Tab".into()],
            ..phone_window(9, "browser")
        };
        let windows = [fresh.clone()];
        let ctx = phone(&windows, None, &settings, crate::ScreenState::Active);
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        browser_chrome(&mut p, &ctx, &fresh);
        for label in ["Back", "Forward"] {
            assert!(greyed(&p, label), "{label} must be greyed, not refused");
        }
        assert!(!ids(&p)
            .iter()
            .any(|id| id.ends_with("shell:back") || id.ends_with("shell:forward")));
        let visited = WindowView {
            can_go_back: true,
            can_go_forward: true,
            ..fresh
        };
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        browser_chrome(&mut p, &ctx, &visited);
        let painted = ids(&p);
        for expected in [
            "window:9:content:shell:back",
            "window:9:content:shell:forward",
        ] {
            assert!(
                painted.iter().any(|id| id == expected),
                "missing {expected}"
            );
        }
    }

    #[test]
    fn the_today_view_pages_its_month_and_draws_the_one_it_is_showing() {
        let settings = crate::SystemSettings::DEFAULT;
        let ctx = phone(&[], Some("calendar"), &settings, crate::ScreenState::Active);
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let painted = ids(&p);
        for expected in ["shell:month:prev", "shell:month:next"] {
            assert!(
                painted.iter().any(|id| id == expected),
                "missing {expected}"
            );
        }
        // The world's own month is already today, so there is nowhere for it to return.
        assert!(!painted.iter().any(|id| id == "shell:month:today"));
        assert!(shown(&p, "September 2026"));
        assert!(shown(&p, "30") && !shown(&p, "31"));
        // Four months on really is January 2027, and the way back appears with it.
        let ctx = ShellContext {
            panel_month: 4,
            ..phone(&[], Some("calendar"), &settings, crate::ScreenState::Active)
        };
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        assert!(shown(&p, "January 2027"));
        assert!(shown(&p, "31"));
        assert!(ids(&p).iter().any(|id| id == "shell:month:today"));
        // Each day opens Calendar on that day.
        assert!(ids(&p)
            .iter()
            .any(|id| id == "shell:launch:calendar/2027-01-31"));
        assert!(!greyed(&p, "Calendar days"));
        // Without Calendar installed, days are announced as display, not as controls.
        let installed = vec!["mail".to_owned()];
        let ctx = ShellContext {
            panel_month: 4,
            installed_apps: &installed,
            ..phone(&[], Some("calendar"), &settings, crate::ScreenState::Active)
        };
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        assert!(greyed(&p, "Calendar days"));
        assert!(!ids(&p)
            .iter()
            .any(|id| id.starts_with("shell:launch:calendar/")));
    }

    fn notice(app: &str, title: &str, action: Option<&str>, seen: bool) -> crate::Notice {
        crate::Notice {
            app: app.into(),
            title: title.into(),
            body: "body".into(),
            time_us: 0,
            action: action.map(str::to_owned),
            seen,
        }
    }

    #[test]
    fn the_app_library_expands_a_real_category_and_collapses_it_again() {
        let settings = crate::SystemSettings::DEFAULT;
        let mut ctx = phone(&[], None, &settings, crate::ScreenState::Active);
        ctx.launcher_open = true;
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let painted = ids(&p);
        // Every plate the library draws expands the category whose name it carries.
        for (title, _) in LIBRARY_GROUPS {
            assert!(
                painted
                    .iter()
                    .any(|id| id == &format!("shell:group:{title}")),
                "no plate expands {title}"
            );
        }
        assert!(!painted.iter().any(|id| id == "shell:group:"));
        // Nothing is announced as a dead group any more.
        assert!(!p.scene.nodes.iter().any(|n| n
            .semantic
            .as_ref()
            .is_some_and(|sem| sem.disabled && sem.label.ends_with(" group"))));
        // Expanded, the category lists everything in it and offers the way back.
        let mut ctx = phone(&[], None, &settings, crate::ScreenState::Active);
        ctx.launcher_open = true;
        ctx.library_group = Some("Utilities");
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let painted = ids(&p);
        assert!(painted.iter().any(|id| id == "shell:group:"));
        for kind in ["files", "clock", "calculator", "settings"] {
            assert!(
                painted
                    .iter()
                    .any(|id| id == &format!("shell:launch:{kind}")),
                "expanded Utilities omits {kind}"
            );
        }
        assert!(!painted.iter().any(|id| id == "shell:launch:chat"));
        assert!(!painted.iter().any(|id| id.starts_with("shell:group:U")));
        // A category holding nothing installed is never drawn at all.
        let installed = ["files".to_owned()];
        let mut ctx = phone(&[], None, &settings, crate::ScreenState::Active);
        ctx.launcher_open = true;
        ctx.installed_apps = &installed;
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let painted = ids(&p);
        assert!(painted.iter().any(|id| id == "shell:group:Utilities"));
        assert!(!painted.iter().any(|id| id == "shell:group:Social"));
    }

    #[test]
    fn safari_share_and_bookmarks_reach_the_stores_behind_them() {
        let window = WindowView {
            id: 7,
            title: "Safari — https://intranet.internal/".into(),
            kind: "browser".into(),
            rect: Rect::new(0, 0, 390, 844),
            focused: true,
            document: "https://intranet.internal/".into(),
            caption: "Intranet".into(),
            tabs: vec!["Intranet".into()],
            ..Default::default()
        };
        let windows = [window];
        let settings = crate::SystemSettings::DEFAULT;
        // The Share sheet hands the page to applications that can really receive it.
        let ctx = phone(
            &windows,
            Some("context"),
            &settings,
            crate::ScreenState::Active,
        );
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let painted = ids(&p);
        for expected in ["shell:share:chat", "shell:share:mail", "shell:bookmark"] {
            assert!(
                painted.iter().any(|id| id == expected),
                "share sheet lacks {expected}"
            );
        }
        assert!(shown(&p, "Add Bookmark"));
        // A machine without a messaging application is not offered one.
        let installed = ["browser".to_owned()];
        let mut ctx = phone(
            &windows,
            Some("context"),
            &settings,
            crate::ScreenState::Active,
        );
        ctx.installed_apps = &installed;
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let painted = ids(&p);
        assert!(!painted.iter().any(|id| id.starts_with("shell:share")));
        assert!(painted.iter().any(|id| id == "shell:bookmark"));
        // Bookmarks lists the machine's real store, and each row navigates to one.
        let saved = [
            crate::Bookmark {
                title: "Intranet".into(),
                url: "https://intranet.internal/".into(),
            },
            crate::Bookmark {
                title: "News".into(),
                url: "https://news.internal/".into(),
            },
        ];
        let mut ctx = phone(
            &windows,
            Some("file"),
            &settings,
            crate::ScreenState::Active,
        );
        ctx.bookmarks = &saved;
        ctx.bookmarked = true;
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let painted = ids(&p);
        for expected in [
            "shell:bookmark",
            "shell:bookmark:open:0",
            "shell:bookmark:open:1",
        ] {
            assert!(
                painted.iter().any(|id| id == expected),
                "bookmarks sheet lacks {expected}"
            );
        }
        // The toggle reads the store rather than guessing which way it goes.
        assert!(shown(&p, "Remove Bookmark") && !shown(&p, "Add Bookmark"));
        // An empty store says so instead of painting rows that open nothing.
        let ctx = phone(
            &windows,
            Some("file"),
            &settings,
            crate::ScreenState::Active,
        );
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        assert!(shown(&p, "No Bookmarks"));
        assert!(!ids(&p)
            .iter()
            .any(|id| id.starts_with("shell:bookmark:open")));
    }

    #[test]
    fn the_today_view_lists_real_notices_and_dictation_stays_off() {
        let settings = crate::SystemSettings::DEFAULT;
        let notices = [
            notice("chat", "Ready to share", Some("shell:launch:chat"), false),
            notice("screenshot", "Screenshot saved", None, true),
        ];
        // Today View holds widgets and search, never the notices.
        let mut ctx = phone(&[], Some("calendar"), &settings, crate::ScreenState::Active);
        ctx.notifications = &notices;
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        assert!(ids(&p).iter().any(|id| id == "shell:search"));
        assert!(ids(&p).iter().any(|id| id == "shell:month:next"));
        assert!(!ids(&p).iter().any(|id| id.starts_with("shell:notice")));
        // Notification Center, pulled down from the status bar, lists them.
        let mut ctx = phone(
            &[],
            Some("notifications"),
            &settings,
            crate::ScreenState::Active,
        );
        ctx.notifications = &notices;
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let painted = ids(&p);
        for expected in [
            "shell:notice:0",
            "shell:notice:1",
            "shell:notifications:seen",
        ] {
            assert!(
                painted.iter().any(|id| id == expected),
                "notification centre lacks {expected}"
            );
        }
        assert!(shown(&p, "Ready to share") && shown(&p, "Screenshot saved"));
        // Nothing invents a notice: an empty feed says it is empty.
        let ctx = phone(
            &[],
            Some("notifications"),
            &settings,
            crate::ScreenState::Active,
        );
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        assert!(shown(&p, "No Older Notifications"));
        assert!(!ids(&p).iter().any(|id| id.starts_with("shell:notice")));
        // Once every notice is seen there is nothing left to mark.
        let seen = [notice("mail", "Ready to share", None, true)];
        let mut ctx = phone(
            &[],
            Some("notifications"),
            &settings,
            crate::ScreenState::Active,
        );
        ctx.notifications = &seen;
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        assert!(!ids(&p).iter().any(|id| id == "shell:notifications:seen"));
        // Search still has no microphone to reach, and announces that rather than paint it.
        let mut ctx = phone(&[], Some("search"), &settings, crate::ScreenState::Active);
        ctx.text_entry = true;
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        assert!(greyed(&p, "Dictation"));
        assert!(!ids(&p).iter().any(|id| id.contains("dictat")));
    }

    #[test]
    fn files_navigation_bar_pops_one_folder_and_stops_at_the_root() {
        let settings = crate::SystemSettings::DEFAULT;
        let ctx = phone(&[], None, &settings, crate::ScreenState::Active);
        let files = |document: &str| WindowView {
            id: 3,
            title: "files".into(),
            kind: "files".into(),
            rect: Rect::new(0, 0, 390, 844),
            focused: true,
            document: document.into(),
            ..Default::default()
        };
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        window_frame(&mut p, &ctx, &files("/home/alice/Documents"));
        assert_eq!(
            p.scene.hit_test(20, 70).unwrap().interaction.as_deref(),
            Some("window:3:content:files-up")
        );
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        window_frame(&mut p, &ctx, &files("/"));
        assert!(p.scene.hit_test(20, 70).is_none());
        // No other application paints a way "Home" or to the App Library in its
        // navigation bar: an iPhone has neither there.
        for kind in ["editor", "calendar", "mail", "notes", "terminal"] {
            let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
            window_frame(
                &mut p,
                &ctx,
                &WindowView {
                    id: 4,
                    title: kind.into(),
                    kind: kind.into(),
                    rect: Rect::new(0, 0, 390, 844),
                    focused: true,
                    ..Default::default()
                },
            );
            assert!(ids(&p).is_empty(), "{kind} frame paints {:?}", ids(&p));
        }
    }

    /// Paint a whole phone: SpringBoard underneath, then the system chrome.
    fn springboard_at<'a>(ctx: &ShellContext<'a>) -> Painter {
        let mut p = Painter::themed(DesktopTheme::Ios, ctx.width, ctx.height, 1);
        background(&mut p, ctx);
        chrome(&mut p, ctx);
        p
    }
    /// Applications launched by icons, leaving out the Calendar widget.
    fn launches(p: &Painter) -> Vec<String> {
        p.scene
            .nodes
            .iter()
            .filter(|n| {
                !n.semantic
                    .as_ref()
                    .is_some_and(|s| s.label == "Calendar widget")
            })
            .filter_map(|n| n.interaction.as_deref()?.strip_prefix("shell:launch:"))
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn springboard_is_a_four_by_six_grid_over_a_separate_dock() {
        let settings = crate::SystemSettings::DEFAULT;
        let home = Home::new(390, 844);
        assert_eq!(home.rows, 6);
        // The whole roster fits one page of a tall phone: the widget takes four cells.
        assert_eq!(home_pages(&[], 390, 844), 1);
        let ctx = phone(&[], None, &settings, crate::ScreenState::Active);
        let p = springboard_at(&ctx);
        let on_screen = launches(&p);
        for kind in DOCK {
            assert_eq!(
                on_screen.iter().filter(|k| *k == kind).count(),
                1,
                "{kind} is drawn once, in the dock"
            );
        }
        // Every application is on screen exactly once, dock and page together.
        let mut all: Vec<_> = on_screen.clone();
        all.sort();
        all.dedup();
        assert_eq!(all.len(), on_screen.len());
        assert_eq!(on_screen.len(), APPS.len());
        // Four columns: icons sit on exactly four x positions.
        let mut columns: Vec<i32> = p
            .scene
            .nodes
            .iter()
            .filter(|n| {
                n.interaction
                    .as_deref()
                    .is_some_and(|a| a.starts_with("shell:launch:"))
            })
            .map(|n| n.bounds.x)
            .collect();
        columns.sort();
        columns.dedup();
        assert_eq!(columns.len(), 4, "{columns:?}");
        // Dock icons wear no names; page icons do.
        assert!(!shown(&p, "Mail") && !shown(&p, "Safari"));
        assert!(shown(&p, "Notes") && shown(&p, "Calendar"));
        // One page means one dot, announced as the current page.
        assert_eq!(
            value(&p, "shell:home-page:0").as_deref(),
            Some("Current page")
        );
        assert!(!ids(&p).iter().any(|id| id == "shell:home-page:1"));
    }

    #[test]
    fn a_shorter_phone_pages_its_icons_and_the_dots_reach_every_page() {
        let settings = crate::SystemSettings::DEFAULT;
        let (width, height) = (390, 600);
        assert_eq!(home_pages(&[], width, height), 2);
        let mut ctx = phone(&[], None, &settings, crate::ScreenState::Active);
        ctx.height = height;
        let first = springboard_at(&ctx);
        ctx.home_page = 1;
        let second = springboard_at(&ctx);
        // The two pages share the dock and nothing else, and between them hold it all.
        let (a, b) = (launches(&first), launches(&second));
        let dock: Vec<String> = DOCK.iter().map(|k| k.to_string()).collect();
        let page_a: Vec<_> = a.iter().filter(|k| !dock.contains(k)).collect();
        let page_b: Vec<_> = b.iter().filter(|k| !dock.contains(k)).collect();
        assert!(!page_a.is_empty() && !page_b.is_empty());
        assert!(page_a.iter().all(|k| !page_b.contains(k)));
        assert_eq!(page_a.len() + page_b.len() + dock.len(), APPS.len());
        // The calendar widget lives on the first page only.
        assert!(first.scene.nodes.iter().any(|n| n
            .semantic
            .as_ref()
            .is_some_and(|s| s.label == "Calendar widget")));
        assert!(!second.scene.nodes.iter().any(|n| n
            .semantic
            .as_ref()
            .is_some_and(|s| s.label == "Calendar widget")));
        // Two dots, the current one announced, each a target that sits above the dock.
        for (p, current) in [(&first, 0), (&second, 1)] {
            for page in 0..2 {
                let id = format!("shell:home-page:{page}");
                let dot = p
                    .scene
                    .nodes
                    .iter()
                    .find(|n| n.interaction.as_deref() == Some(id.as_str()))
                    .unwrap();
                assert!(dot.bounds.y + dot.bounds.height as i32 <= Home::new(width, height).dock.y);
                assert_eq!(
                    value(p, &id).as_deref(),
                    Some(if page == current {
                        "Current page"
                    } else {
                        "Page"
                    })
                );
            }
        }
        // A page past the last one shows the last one.
        ctx.home_page = 9;
        assert_eq!(launches(&springboard_at(&ctx)), b);
        // The App Library and Today View are screens of their own, not over a page.
        ctx.home_page = 0;
        ctx.launcher_open = true;
        let library = springboard_at(&ctx);
        assert!(!ids(&library)
            .iter()
            .any(|id| id.starts_with("shell:home-page")));
        ctx.launcher_open = false;
        ctx.panel = Some("calendar");
        let today = springboard_at(&ctx);
        assert!(!ids(&today)
            .iter()
            .any(|id| id.starts_with("shell:home-page")));
    }

    #[test]
    fn the_home_indicator_is_a_gesture_affordance_not_a_home_button() {
        let settings = crate::SystemSettings::DEFAULT;
        let windows = [phone_window(2, "notes")];
        let ctx = phone(&windows, None, &settings, crate::ScreenState::Active);
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let bar = p.scene.hit_test(195, 836).unwrap();
        assert_eq!(bar.interaction.as_deref(), Some("shell:gesture:home"));
        assert_eq!(bar.semantic.as_ref().unwrap().role, "gesture");
        // Nothing an application screen paints is a tap-to-go-home control.
        assert!(!ids(&p)
            .iter()
            .any(|id| id == "shell:home" || id == "shell:launcher"));
        // The status bar is where the two pull-downs start, and it is a gesture too.
        assert_eq!(
            p.scene.hit_test(40, 20).unwrap().interaction.as_deref(),
            Some("shell:gesture:notifications")
        );
        assert_eq!(
            p.scene.hit_test(350, 20).unwrap().interaction.as_deref(),
            Some("shell:gesture:control-center")
        );
        // The indicator is over applications and the cover sheet, but not over
        // Control Center, the App Switcher or the App Library.
        for (panel, launcher, shows) in [
            (Some("notifications"), false, true),
            (Some("settings"), false, true),
            (Some("quick"), false, false),
            (Some("overview"), false, false),
            (None, true, false),
        ] {
            let mut ctx = phone(&windows, panel, &settings, crate::ScreenState::Active);
            ctx.launcher_open = launcher;
            let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
            chrome(&mut p, &ctx);
            assert_eq!(
                ids(&p).iter().any(|id| id == "shell:gesture:home"),
                shows,
                "{panel:?} launcher {launcher}"
            );
        }
        // The App Switcher has no close buttons: a card is swiped up to close it, and
        // the space around the cards goes home, as on the device.
        let ctx = phone(
            &windows,
            Some("overview"),
            &settings,
            crate::ScreenState::Active,
        );
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        assert!(!ids(&p).iter().any(|id| id.ends_with(":close")));
        assert!(ids(&p).iter().any(|id| id == "window:2:focus"));
        assert_eq!(
            p.scene.hit_test(195, 820).unwrap().interaction.as_deref(),
            Some("shell:home")
        );
    }

    #[test]
    fn quicktype_completes_the_word_being_typed_and_dictation_is_announced_off() {
        let settings = crate::SystemSettings::DEFAULT;
        let windows = [phone_window(2, "terminal")];
        let mut ctx = phone(&windows, None, &settings, crate::ScreenState::Active);
        ctx.text_entry = true;
        ctx.typed = "echo mee";
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        assert!(ids(&p).iter().any(|id| id == "shell:insert:ting "));
        assert!(shown(&p, "meeting"));
        assert!(greyed(&p, "Dictation, no speech input in this simulation"));
        // The bar sits above the keys, which still start below it.
        let bar = p
            .scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some("shell:insert:ting "))
            .unwrap()
            .bounds;
        let q = p
            .scene
            .nodes
            .iter()
            .find(|n| n.interaction.as_deref() == Some("shell:type:q"))
            .unwrap()
            .bounds;
        assert!(bar.y + bar.height as i32 <= q.y);
    }

    #[test]
    fn the_status_bar_and_lock_screen_report_the_real_switches() {
        let settings = crate::SystemSettings {
            airplane_mode: true,
            wifi: false,
            do_not_disturb: true,
            battery_saver: true,
            flashlight: true,
            ..crate::SystemSettings::DEFAULT
        };
        let symbols = |p: &Painter| -> Vec<(String, cw_scene::Color)> {
            p.scene
                .nodes
                .iter()
                .filter_map(|n| match &n.primitive {
                    cw_scene::Primitive::Symbol { asset, color } => {
                        Some((asset.trim_start_matches("symbol/").to_owned(), *color))
                    }
                    _ => None,
                })
                .collect()
        };
        let ctx = phone(&[], None, &settings, crate::ScreenState::Active);
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        let drawn = symbols(&p);
        let has = |name: &str| drawn.iter().any(|(s, _)| s == name);
        assert!(has("airplane") && has("moon"));
        assert!(!has("cellular") && !has("wifi"));
        assert!(drawn
            .iter()
            .any(|(s, c)| s == "battery" && *c == Color::rgb(255, 204, 0)));
        // The lock screen hides the applications, keeps the torch real and says the
        // camera is not there.
        let windows = [phone_window(2, "notes")];
        let ctx = phone(&windows, None, &settings, crate::ScreenState::Locked);
        let mut p = Painter::themed(DesktopTheme::Ios, 390, 844, 1);
        chrome(&mut p, &ctx);
        assert_eq!(value(&p, "shell:toggle:flashlight").as_deref(), Some("On"));
        assert!(greyed(&p, "Camera, no camera in this simulation"));
        assert!(!ids(&p).iter().any(|id| id.starts_with("window:")));
        assert!(p
            .scene
            .nodes
            .iter()
            .any(|n| matches!(&n.primitive, cw_scene::Primitive::AssetImage { asset } if asset == "wallpaper/ios")));
    }
}
