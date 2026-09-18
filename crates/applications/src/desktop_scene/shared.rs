//! Small drawing API shared by independent platform shells.
use super::DesktopTheme;
use cw_scene::{metrics, Color, Node, Primitive, Rect, RoundedClip, Scene, Typeface};
#[derive(Clone, Debug, Default)]
pub struct WindowView {
    pub id: u64,
    pub title: String,
    pub kind: String,
    pub rect: Rect,
    pub focused: bool,
    pub maximized: bool,
    pub minimized: bool,
    /// Application-local coordinates, excluding the native window frame.
    pub content: Option<Scene>,
    /// Folder path, document path or URL the window presents; empty when none.
    pub document: String,
    /// Human title of the presented page or document, when the application has one.
    pub caption: String,
    /// Home folder of the machine's user, so a path bar can start at Home and a title
    /// can abbreviate it the way the platform does.
    pub home: String,
    /// Unsaved changes, shown the way each platform marks an edited document.
    pub modified: bool,
    /// The address or search field of this window currently owns keyboard input.
    pub editing: bool,
    /// Tab labels this window presents; empty when the application is not tabbed.
    pub tabs: Vec<String>,
    pub active_tab: usize,
    /// Whether this window's history really has somewhere to go, so a frame can grey
    /// out Back and Forward instead of painting controls that would be refused.
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// Virtual desktop this window lives on. A window on another desktop is not
    /// minimised, it is elsewhere, and a taskbar should say so.
    pub workspace: u32,
    /// File manager presentation, so a frame can light the mode its control switches.
    pub view_grid: bool,
    pub sort_key: String,
    pub query: String,
    /// Path of the file manager's selected item, empty when nothing is selected, so a
    /// frame's Share control greys instead of offering a share with nothing in it.
    pub selection: String,
    /// A browser window's page zoom in percent; 0 in a view that never set it.
    pub zoom: u16,
    /// The application paints a dark theme of its own, and a frame that draws that
    /// application's title bar (Visual Studio Code's) draws it to match.
    pub dark_chrome: bool,
    /// Named facts an application lends a frame that draws controls on its behalf —
    /// which of its menus is open, which of its panes are shown — so those controls
    /// light the state they switch rather than guessing it.
    pub chrome: Vec<(String, String)>,
}
impl WindowView {
    pub fn chrome(&self, name: &str) -> Option<&str> {
        self.chrome
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}
impl WindowView {
    /// `(user, host, directory)` a terminal's prompt names, with the home folder
    /// written `~` as bash and zsh write it. `None` for a PowerShell prompt, or a
    /// window that is not showing one.
    pub fn shell_identity(&self) -> Option<(String, String, String)> {
        let line = self
            .caption
            .trim_end()
            .trim_end_matches(['$', '%', '#'])
            .trim_end();
        if line.starts_with("PS ") {
            return None;
        }
        // zsh prints `user@host dir`; bash prints `user@host:dir`.
        let (who, dir) = match line.split_once(' ') {
            Some((who, dir)) if !who.contains(':') => (who, dir.trim()),
            _ => line.split_once(':')?,
        };
        let (user, host) = who.split_once('@')?;
        let home = self.home.trim_end_matches('/');
        let dir = if !home.is_empty() && (dir == home || dir.starts_with(&format!("{home}/"))) {
            format!("~{}", &dir[home.len()..])
        } else {
            dir.to_owned()
        };
        Some((user.to_owned(), host.to_owned(), dir))
    }
    /// A path with the home folder written `~`, as GNOME and macOS subtitles write it.
    pub fn tilde(&self, path: &str) -> String {
        let home = self.home.trim_end_matches('/');
        if !home.is_empty() && (path == home || path.starts_with(&format!("{home}/"))) {
            format!("~{}", &path[home.len()..])
        } else {
            path.to_owned()
        }
    }
    /// Page zoom in percent, 100 when unset.
    pub fn zoom_percent(&self) -> u16 {
        if self.zoom == 0 {
            100
        } else {
            self.zoom
        }
    }
    pub fn action(&self, name: &str) -> String {
        format!("window:{}:{name}", self.id)
    }
    /// Browser tabs live in the browser session; file manager tabs live in the window.
    pub fn tab_select(&self, index: usize) -> String {
        if self.kind == "browser" {
            format!("shell:tab:select:{index}")
        } else {
            self.action(&format!("content:files-tab:{index}"))
        }
    }
    pub fn tab_close(&self, index: usize) -> String {
        if self.kind == "browser" {
            format!("shell:tab:close:{index}")
        } else {
            self.action(&format!("content:files-closetab:{index}"))
        }
    }
    pub fn tab_new(&self) -> String {
        if self.kind == "browser" {
            "shell:tab:new".into()
        } else {
            self.action("content:files-newtab")
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct ShellOptions {
    pub installed_apps: Vec<String>,
    pub panel: Option<String>,
    pub search: String,
    pub hover: Option<(i32, i32)>,
    /// Desktop icon selected by a single click, awaiting the second one.
    pub desktop_selection: Option<String>,
    pub settings: crate::SystemSettings,
    pub screen: crate::ScreenState,
    pub panel_month: i32,
    pub text_entry: bool,
    pub keyboard: crate::KeyboardState,
    pub bookmarks: Vec<crate::Bookmark>,
    pub downloads: Vec<crate::Download>,
    pub notifications: Vec<crate::Notice>,
    pub workspaces: u32,
    pub workspace: u32,
    pub library_group: Option<String>,
    pub bookmarked: bool,
    pub panel_over_launcher: bool,
    /// The end of what the focused field holds before its caret, so a keyboard can
    /// complete the word being typed.
    pub typed: String,
    /// Home screen page a paged launcher (SpringBoard) is showing, 0 first.
    pub home_page: u32,
    /// The machine's user, for account tiles and lock screens.
    pub user: String,
    /// The user's home folder, for menus that go to its standard folders.
    pub home: String,
    /// Documents the user really opened, newest first (`DesktopState::recents`).
    pub recents: Vec<String>,
}
/// Words a phone keyboard offers to complete, most common first. A fixed list, so two
/// machines typing the same letters are offered the same words.
pub const WORDS: &[&str] = &[
    "the",
    "be",
    "to",
    "of",
    "and",
    "a",
    "in",
    "that",
    "have",
    "it",
    "for",
    "not",
    "on",
    "with",
    "he",
    "as",
    "you",
    "do",
    "at",
    "this",
    "but",
    "his",
    "by",
    "from",
    "they",
    "we",
    "say",
    "her",
    "she",
    "or",
    "an",
    "will",
    "my",
    "one",
    "all",
    "would",
    "there",
    "their",
    "what",
    "so",
    "up",
    "out",
    "if",
    "about",
    "who",
    "get",
    "which",
    "go",
    "me",
    "when",
    "make",
    "can",
    "like",
    "time",
    "no",
    "just",
    "him",
    "know",
    "take",
    "people",
    "into",
    "year",
    "your",
    "good",
    "some",
    "could",
    "them",
    "see",
    "other",
    "than",
    "then",
    "now",
    "look",
    "only",
    "come",
    "its",
    "over",
    "think",
    "also",
    "back",
    "after",
    "use",
    "two",
    "how",
    "our",
    "work",
    "first",
    "well",
    "way",
    "even",
    "new",
    "want",
    "because",
    "any",
    "these",
    "give",
    "day",
    "most",
    "us",
    "is",
    "was",
    "are",
    "were",
    "has",
    "had",
    "been",
    "did",
    "said",
    "am",
    "thanks",
    "thank",
    "please",
    "sorry",
    "hello",
    "hi",
    "hey",
    "yes",
    "okay",
    "sure",
    "great",
    "meeting",
    "calendar",
    "email",
    "message",
    "project",
    "report",
    "review",
    "schedule",
    "today",
    "tomorrow",
    "yesterday",
    "morning",
    "afternoon",
    "evening",
    "week",
    "weekend",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
    "document",
    "file",
    "folder",
    "draft",
    "send",
    "reply",
    "forward",
    "attached",
    "attachment",
    "update",
    "question",
    "answer",
    "team",
    "lunch",
    "coffee",
    "call",
    "later",
    "soon",
    "again",
    "maybe",
    "really",
    "right",
    "left",
    "next",
    "last",
    "before",
    "should",
    "need",
    "help",
    "let",
    "find",
    "open",
    "close",
    "save",
    "share",
    "search",
    "note",
    "notes",
    "number",
    "phone",
    "address",
    "home",
    "office",
    "company",
    "customer",
    "invoice",
    "payment",
    "budget",
    "plan",
    "launch",
    "release",
    "deadline",
    "agenda",
    "minutes",
    "summary",
    "follow",
    "following",
    "information",
    "important",
    "available",
    "possible",
    "problem",
    "issue",
    "ticket",
    "fix",
    "change",
    "changes",
    "reviewed",
    "approve",
    "approved",
    "confirm",
    "confirmed",
    "cancel",
    "cancelled",
    "delay",
    "delayed",
    "happy",
    "birthday",
    "love",
    "nice",
    "cool",
    "awesome",
    "done",
    "ready",
    "working",
    "wondering",
    "interested",
    "discuss",
    "discussion",
    "quick",
    "quickly",
];

pub struct ShellContext<'a> {
    pub theme: DesktopTheme,
    pub width: u32,
    pub height: u32,
    pub clock_us: u64,
    pub title: &'a str,
    pub launcher_open: bool,
    pub active: bool,
    pub windows: &'a [WindowView],
    pub installed_apps: &'a [String],
    pub panel: Option<&'a str>,
    pub search: &'a str,
    pub hover: Option<(i32, i32)>,
    pub desktop_selection: Option<&'a str>,
    pub settings: &'a crate::SystemSettings,
    pub screen: crate::ScreenState,
    /// Months away from the current one that the calendar panel is showing.
    pub panel_month: i32,
    /// The next keystroke inserts text rather than invoking a command, so a phone
    /// should be showing its keyboard.
    pub text_entry: bool,
    pub keyboard: crate::KeyboardState,
    /// Pages the user saved, files the browser wrote, and things the machine wants to
    /// say — all real records, never invented by the shell that draws them.
    pub bookmarks: &'a [crate::Bookmark],
    pub downloads: &'a [crate::Download],
    pub notifications: &'a [crate::Notice],
    /// Virtual desktops: how many there are, and which one is on screen.
    pub workspaces: u32,
    pub workspace: u32,
    /// Launcher category the user expanded.
    pub library_group: Option<&'a str>,
    /// The page the focused browser window shows is already bookmarked.
    pub bookmarked: bool,
    pub panel_over_launcher: bool,
    /// The end of what the focused field holds before its caret.
    pub typed: &'a str,
    /// Home screen page the user swiped to. A shell clamps it to the pages it has.
    pub home_page: u32,
    pub user: &'a str,
    pub home: &'a str,
    pub recents: &'a [String],
}
impl ShellContext<'_> {
    pub fn selected(&self, id: &str) -> bool {
        self.desktop_selection == Some(id)
    }
    /// Current position of a system switch; unknown names read as off.
    pub fn switch(&self, name: &str) -> bool {
        self.settings.flag(name).unwrap_or(false)
    }
    pub fn level(&self, name: &str) -> u8 {
        self.settings.level(name).unwrap_or(0)
    }
    pub fn awake(&self) -> bool {
        self.screen == crate::ScreenState::Active
    }
    /// Date the calendar panel is showing: the world's own date, moved by whole months.
    pub fn panel_date(&self) -> CalendarDate {
        CalendarDate::shifted_months(self.clock_us, self.panel_month)
    }
    /// Letters type upper case right now.
    pub fn shifted(&self) -> bool {
        self.keyboard.upper()
    }
    /// Interaction that opens Calendar on `day` of the month the panel is showing. `None`
    /// when Calendar is not installed, or the date is before the world began and so has
    /// no day to open — a cell that cannot do anything must not look as if it can.
    /// Up to `limit` completions of the word being typed, each with the
    /// `shell:insert:` target that types the rest of it and a space. Nothing is offered
    /// before a letter is typed, or once the word is already complete.
    pub fn suggestions(&self, limit: usize) -> Vec<(String, String)> {
        if !self.text_entry {
            return vec![];
        }
        let start = self
            .typed
            .char_indices()
            .rev()
            .take_while(|(_, c)| c.is_ascii_alphabetic() || *c == '\'')
            .last()
            .map(|(i, _)| i)
            .unwrap_or(self.typed.len());
        let prefix = &self.typed[start..];
        if prefix.is_empty() {
            return vec![];
        }
        let lower = prefix.to_ascii_lowercase();
        let capital = prefix.starts_with(|c: char| c.is_ascii_uppercase());
        WORDS
            .iter()
            .filter(|w| w.len() > lower.len() && w.starts_with(lower.as_str()))
            .take(limit)
            .map(|w| {
                let rest = &w[prefix.len()..];
                let mut shown = w.to_string();
                if capital {
                    shown[..1].make_ascii_uppercase();
                }
                (shown, format!("shell:insert:{rest} "))
            })
            .collect()
    }
    pub fn open_day(&self, day: u64) -> Option<String> {
        if !self.installed("calendar") {
            return None;
        }
        let shown = self.panel_date();
        if day == 0 || day > shown.days_in_month {
            return None;
        }
        let first = CalendarDate::from_clock(0);
        if (shown.year, shown.month, day) < (first.year, first.month, first.day) {
            return None;
        }
        Some(format!(
            "shell:launch:calendar/{:04}-{:02}-{:02}",
            shown.year, shown.month, day
        ))
    }
    pub fn unseen_notices(&self) -> usize {
        self.notifications.iter().filter(|n| !n.seen).count()
    }
    pub fn hovered(&self, rect: Rect) -> bool {
        self.hover.is_some_and(|(x, y)| rect.contains(x, y))
    }
    pub fn installed(&self, id: &str) -> bool {
        self.installed_apps.is_empty() || self.installed_apps.iter().any(|a| a == id)
    }
    /// World clock: episodes begin at 09:00 on Thursday 17 September 2026.
    pub fn hour_minute(&self) -> (u64, u64) {
        let minutes = 9 * 60 + self.clock_us / 60_000_000;
        ((minutes / 60) % 24, minutes % 60)
    }
    /// 24-hour `09:00`.
    pub fn time(&self) -> String {
        let (h, m) = self.hour_minute();
        format!("{h:02}:{m:02}")
    }
    /// 12-hour `9:00 AM`.
    pub fn time12(&self) -> String {
        let (h, m) = self.hour_minute();
        let hour = if h % 12 == 0 { 12 } else { h % 12 };
        format!("{hour}:{m:02} {}", if h < 12 { "AM" } else { "PM" })
    }
    pub fn date(&self) -> CalendarDate {
        CalendarDate::from_clock(self.clock_us)
    }
}
pub const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
pub const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];
/// Gregorian date derived only from simulation time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalendarDate {
    pub year: u64,
    /// 1-12.
    pub month: u64,
    pub day: u64,
    /// 0 is Sunday.
    pub weekday: u64,
    /// Weekday of the first day of this month; 0 is Sunday.
    pub first_weekday: u64,
    pub days_in_month: u64,
}
impl CalendarDate {
    /// The world's date moved by whole months, for a panel that pages a month grid.
    /// The day is clamped into the target month, so paging from the 31st is stable.
    pub fn shifted_months(clock_us: u64, months: i32) -> Self {
        let mut date = Self::from_clock(clock_us);
        if months == 0 {
            return date;
        }
        let total = (date.year as i64) * 12 + (date.month as i64 - 1) + months as i64;
        let (year, month) = (
            total.div_euclid(12).max(0) as u64,
            total.rem_euclid(12) as u64 + 1,
        );
        let leap =
            |y: u64| y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
        let days = match month {
            4 | 6 | 9 | 11 => 30,
            2 if leap(year) => 29,
            2 => 28,
            _ => 31,
        };
        // Weekday of the 1st: step the known first weekday by the months crossed.
        let days_between = |from: (u64, u64), to: (u64, u64)| -> i64 {
            let mut total = 0i64;
            let (mut y, mut m) = from;
            while (y, m) != to {
                let length = match m {
                    4 | 6 | 9 | 11 => 30,
                    2 if leap(y) => 29,
                    2 => 28,
                    _ => 31,
                } as i64;
                total += length;
                m += 1;
                if m > 12 {
                    m = 1;
                    y += 1;
                }
            }
            total
        };
        let base = (date.year, date.month);
        let target = (year, month);
        let delta = if months > 0 {
            days_between(base, target)
        } else {
            -days_between(target, base)
        };
        let first = (date.first_weekday as i64 + delta).rem_euclid(7) as u64;
        date.year = year;
        date.month = month;
        date.days_in_month = days;
        date.first_weekday = first;
        date.day = date.day.min(days);
        date.weekday = (first + (date.day - 1)) % 7;
        date
    }
    pub fn from_clock(clock_us: u64) -> Self {
        let elapsed = (clock_us / 60_000_000 + 9 * 60) / (24 * 60);
        let leap =
            |y: u64| y.is_multiple_of(4) && (!y.is_multiple_of(100) || y.is_multiple_of(400));
        let length = |y: u64, m: u64| match m {
            4 | 6 | 9 | 11 => 30,
            2 if leap(y) => 29,
            2 => 28,
            _ => 31,
        };
        // Whole 400-year cycles are exactly 146097 days and preserve the weekday phase.
        let (mut year, mut month, mut day) = (2026 + elapsed / 146_097 * 400, 9, 17);
        let mut remaining = elapsed % 146_097;
        while remaining > 0 {
            let left = length(year, month) - day;
            if remaining <= left {
                day += remaining;
                break;
            }
            remaining -= left + 1;
            day = 1;
            month += 1;
            if month > 12 {
                month = 1;
                year += 1;
            }
        }
        let weekday = (4 + elapsed) % 7;
        Self {
            year,
            month,
            day,
            weekday,
            first_weekday: (weekday + 35 - (day - 1) % 7) % 7,
            days_in_month: length(year, month),
        }
    }
    pub fn month_name(&self) -> &'static str {
        MONTHS[(self.month - 1) as usize]
    }
    pub fn weekday_name(&self) -> &'static str {
        WEEKDAYS[self.weekday as usize]
    }
}
pub struct Painter {
    pub scene: Scene,
    pub next: u64,
    pub z: i32,
}
/// Horizontal placement of a single-line label inside its box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}
/// Sine in 1/1024 units for whole degrees (Bhaskara I), identical on every target.
pub fn sin1024(degrees: i32) -> i32 {
    let d = degrees.rem_euclid(360);
    let (d, sign) = if d > 180 { (d - 180, -1) } else { (d, 1) };
    let k = i64::from(d) * i64::from(180 - d);
    sign * (4096 * k / (40500 - k)) as i32
}
pub fn cos1024(degrees: i32) -> i32 {
    sin1024(degrees + 90)
}
/// Points along a circular arc, clockwise from 12 o'clock, for stroked paths.
pub fn arc_points(cx: i32, cy: i32, radius: i32, from: i32, to: i32, step: i32) -> Vec<(i32, i32)> {
    let mut points = Vec::new();
    let mut a = from;
    while a < to {
        points.push(a);
        a += step.max(1);
    }
    points.push(to);
    points
        .into_iter()
        .map(|a| {
            (
                cx + (radius * sin1024(a) + 512).div_euclid(1024),
                cy - (radius * cos1024(a) + 512).div_euclid(1024),
            )
        })
        .collect()
}
impl Painter {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            scene: Scene::new(width, height),
            next: 1 << 60,
            z: 0,
        }
    }
    pub fn themed(theme: DesktopTheme, width: u32, height: u32, first_id: u64) -> Self {
        let mut p = Self::new(width, height);
        p.next = first_id;
        p.scene.typeface = theme.typeface();
        p
    }
    pub fn typeface(&self) -> Typeface {
        self.scene.typeface
    }
    /// Exact single-line pixel width in this painter's bundled UI font.
    pub fn measure(&self, text: &str, size: u16, bold: bool) -> u32 {
        metrics::text_width(self.scene.typeface, bold, text, size)
    }
    /// One ellipsized line, aligned within `width`. Returns the painted text width.
    #[allow(clippy::too_many_arguments)]
    pub fn label(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        text: &str,
        size: u16,
        color: Color,
        bold: bool,
        align: Align,
    ) -> u32 {
        let text = metrics::ellipsize(self.scene.typeface, bold, text, size, width);
        let measured = self.measure(&text, size, bold).min(width);
        let x = x + match align {
            Align::Left => 0,
            Align::Center => (width - measured) as i32 / 2,
            Align::Right => (width - measured) as i32,
        };
        // One spare pixel keeps the final glyph's antialiased edge inside the bounds.
        let bounds = Rect::new(
            x,
            y,
            measured + 2,
            u32::from(size) + u32::from(size) / 2 + 2,
        );
        let primitive = if bold {
            Primitive::UiTextBold { text, color, size }
        } else {
            Primitive::UiText { text, color, size }
        };
        self.node(bounds, primitive, None);
        measured
    }
    pub fn left(&mut self, x: i32, y: i32, width: u32, text: &str, size: u16, color: Color) -> u32 {
        self.label(x, y, width, text, size, color, false, Align::Left)
    }
    pub fn center(&mut self, x: i32, y: i32, width: u32, text: &str, size: u16, color: Color) {
        self.label(x, y, width, text, size, color, false, Align::Center);
    }
    pub fn right(&mut self, x: i32, y: i32, width: u32, text: &str, size: u16, color: Color) {
        self.label(x, y, width, text, size, color, false, Align::Right);
    }
    pub fn strong(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        text: &str,
        size: u16,
        color: Color,
    ) -> u32 {
        self.label(x, y, width, text, size, color, true, Align::Left)
    }
    pub fn strong_center(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        text: &str,
        size: u16,
        color: Color,
    ) {
        self.label(x, y, width, text, size, color, true, Align::Center);
    }
    /// Word-wrapped paragraph; returns its height so callers can flow content below.
    pub fn paragraph(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        text: &str,
        size: u16,
        color: Color,
    ) -> u32 {
        let lines = metrics::wrap(self.scene.typeface, false, text, size, width).len() as u32;
        let line_height = u32::from(size) + u32::from(size).div_ceil(4);
        let height = lines * line_height + u32::from(size) / 2;
        self.node(
            Rect::new(x, y, width, height),
            Primitive::UiText {
                text: text.into(),
                color,
                size,
            },
            None,
        );
        lines * line_height
    }
    /// Tinted bundled glyph, e.g. `symbol("wifi", ...)`.
    pub fn symbol(&mut self, name: &str, x: i32, y: i32, size: u32, color: Color) {
        self.node(
            Rect::new(x, y, size, size),
            Primitive::Symbol {
                asset: format!("symbol/{name}"),
                color,
            },
            None,
        );
    }
    /// Frosted material: blurred backdrop, translucent tint and an optional hairline.
    pub fn glass(&mut self, r: Rect, radius: u32, blur: u32, tint: Color, edge: Option<Color>) {
        self.node(r, Primitive::Backdrop { radius, blur }, None);
        self.node(
            r,
            Primitive::RoundedBox {
                fill: tint,
                border: edge,
                border_width: u32::from(edge.is_some()),
                radius,
            },
            None,
        );
    }
    /// Soft shadow with explicit softness, strength and downward offset.
    pub fn drop_shadow(&mut self, r: Rect, radius: u32, blur: u32, alpha: u8, dy: i32) {
        let pad = blur as i32;
        self.node(
            Rect::new(
                r.x - pad,
                r.y - pad + dy,
                r.width + blur * 2,
                r.height + blur * 2,
            ),
            Primitive::Shadow {
                color: Color(0, 0, 0, alpha),
                radius: radius + blur / 3,
                blur,
            },
            None,
        );
    }
    pub fn circle(&mut self, cx: i32, cy: i32, radius: u32, color: Color) {
        self.box_(
            Rect::new(
                cx - radius as i32,
                cy - radius as i32,
                radius * 2,
                radius * 2,
            ),
            color,
            radius,
        );
    }
    pub fn ring(&mut self, cx: i32, cy: i32, radius: u32, width: u32, color: Color) {
        self.node(
            Rect::new(
                cx - radius as i32,
                cy - radius as i32,
                radius * 2,
                radius * 2,
            ),
            Primitive::RoundedBox {
                fill: Color::TRANSPARENT,
                border: Some(color),
                border_width: width,
                radius,
            },
            None,
        );
    }
    pub fn hline(&mut self, x: i32, y: i32, width: u32, color: Color) {
        self.box_(Rect::new(x, y, width, 1), color, 0);
    }
    pub fn vline(&mut self, x: i32, y: i32, height: u32, color: Color) {
        self.box_(Rect::new(x, y, 1, height), color, 0);
    }
    /// Vertical gradient approximated by one-pixel rows between two colours.
    pub fn gradient(&mut self, r: Rect, top: Color, bottom: Color, steps: u32) {
        let steps = steps.clamp(1, r.height.max(1));
        for i in 0..steps {
            let y0 = r.height * i / steps;
            let y1 = r.height * (i + 1) / steps;
            let mix = |a: u8, b: u8| {
                ((u32::from(a) * (steps - 1 - i) + u32::from(b) * i) / (steps - 1).max(1)) as u8
            };
            self.box_(
                Rect::new(r.x, r.y + y0 as i32, r.width, y1 - y0),
                Color(
                    mix(top.0, bottom.0),
                    mix(top.1, bottom.1),
                    mix(top.2, bottom.2),
                    mix(top.3, bottom.3),
                ),
                0,
            );
        }
    }
    /// Live miniature of an application scene, scaled to `bounds` and made inert.
    pub fn thumbnail(&mut self, content: &Scene, bounds: Rect, radius: u32) {
        let scale = (i64::from(bounds.width) * 1024 / i64::from(content.width.max(1))) as i32;
        let mark = self.scene.nodes.len();
        self.box_(bounds, content.background, 0);
        let mut nodes: Vec<_> = content.nodes.iter().collect();
        nodes.sort_by_key(|n| (n.z, n.id));
        for source in nodes {
            let mut n = source.clone();
            n.id = self.next;
            self.next += 1;
            n.z = self.z;
            n.transform.a = n.transform.a * scale / 1024;
            n.transform.b = n.transform.b * scale / 1024;
            n.transform.c = n.transform.c * scale / 1024;
            n.transform.d = n.transform.d * scale / 1024;
            n.transform.tx = bounds.x + n.transform.tx * scale / 1024;
            n.transform.ty = bounds.y + n.transform.ty * scale / 1024;
            n.clip = source
                .clip
                .map(|clip| {
                    Rect::new(
                        bounds.x + clip.x * scale / 1024,
                        bounds.y + clip.y * scale / 1024,
                        (u64::from(clip.width) * scale as u64 / 1024) as u32,
                        (u64::from(clip.height) * scale as u64 / 1024) as u32,
                    )
                })
                .map_or(Some(bounds), |clip| clip.intersection(bounds));
            if n.clip.is_none() {
                continue;
            }
            n.interaction = None;
            n.semantic = None;
            self.scene.nodes.push(n);
        }
        self.round_clip_since(mark, bounds, radius);
    }
    /// Clip every node added since `mark` to a rounded rectangle (window corners).
    pub fn round_clip_since(&mut self, mark: usize, rect: Rect, radius: u32) {
        if radius == 0 {
            return;
        }
        for n in &mut self.scene.nodes[mark..] {
            n.rounded_clip = Some(RoundedClip { rect, radius });
        }
    }
    pub fn node(&mut self, r: Rect, p: Primitive, interaction: Option<(&str, &str)>) {
        let mut n = Node::new(self.next, r, p);
        self.next += 1;
        n.z = self.z;
        if let Some((action, label)) = interaction {
            n = n.interactive(action, "button", label);
        }
        self.scene.nodes.push(n);
    }
    /// Mark the last node an announced-disabled control: no interaction, not focusable.
    /// Every shell needs this, so it lives beside the drawing it annotates.
    pub fn disabled(&mut self, label: &str) {
        if let Some(n) = self.scene.nodes.last_mut() {
            n.interaction = None;
            n.semantic = Some(cw_scene::Semantic {
                role: "button".into(),
                label: label.into(),
                value: None,
                disabled: true,
                focusable: false,
            });
        }
    }
    /// A click target one layer above whatever the surrounding panel registered, so a
    /// specific control inside a larger clickable widget wins its own hit test.
    pub fn region_above(&mut self, r: Rect, action: &str, label: &str) {
        self.z += 1;
        self.region(r, action, label);
        self.z -= 1;
    }
    pub fn region(&mut self, r: Rect, action: &str, label: &str) {
        self.node(r, Primitive::Region, Some((action, label)));
    }
    pub fn box_(&mut self, r: Rect, c: Color, radius: u32) {
        self.node(
            r,
            Primitive::RoundedBox {
                fill: c,
                border: None,
                border_width: 0,
                radius,
            },
            None,
        );
    }
    pub fn border(&mut self, r: Rect, c: Color, radius: u32, border: Color) {
        self.node(
            r,
            Primitive::RoundedBox {
                fill: c,
                border: Some(border),
                border_width: 1,
                radius,
            },
            None,
        );
    }
    pub fn text(&mut self, x: i32, y: i32, w: u32, text: &str, size: u16, c: Color) {
        self.node(
            Rect::new(x, y, w, u32::from(size) + 9),
            Primitive::UiText {
                text: text.into(),
                color: c,
                size,
            },
            None,
        );
    }
    pub fn button(&mut self, r: Rect, c: Color, radius: u32, action: &str, label: &str) {
        self.node(
            r,
            Primitive::RoundedBox {
                fill: c,
                border: None,
                border_width: 0,
                radius,
            },
            Some((action, label)),
        );
    }
    pub fn path(&mut self, points: Vec<(i32, i32)>, fill: Color) {
        self.node(
            Rect::new(0, 0, self.scene.width, self.scene.height),
            Primitive::Path {
                points,
                fill: Some(fill),
                stroke: None,
                stroke_width: 0,
                closed: true,
            },
            None,
        );
    }
    pub fn line(&mut self, points: Vec<(i32, i32)>, color: Color, thickness: u16) {
        self.node(
            Rect::new(0, 0, self.scene.width, self.scene.height),
            Primitive::Path {
                points,
                fill: None,
                stroke: Some(color),
                stroke_width: thickness,
                closed: false,
            },
            None,
        );
    }
    pub fn shadow(&mut self, r: Rect, radius: u32) {
        self.node(
            Rect::new(r.x - 18, r.y - 12, r.width + 36, r.height + 36),
            Primitive::Shadow {
                color: Color(0, 0, 0, 85),
                radius,
                blur: 18,
            },
            None,
        );
    }
    pub fn bold(&mut self, x: i32, y: i32, w: u32, text: &str, size: u16, c: Color) {
        self.node(
            Rect::new(x, y, w, u32::from(size) + 9),
            Primitive::UiTextBold {
                text: text.into(),
                color: c,
                size,
            },
            None,
        );
    }
    /// Asset ids are stable serialized identifiers, resolved by the renderer.
    pub fn asset(&mut self, r: Rect, id: &str) {
        self.node(r, Primitive::AssetImage { asset: id.into() }, None);
    }
    pub fn icon(&mut self, x: i32, y: i32, size: u32, kind: &str, label: bool) {
        self.asset(Rect::new(x, y, size, size), &format!("icon/common/{kind}"));
        self.region(
            Rect::new(x, y, size, size),
            &format!("shell:launch:{kind}"),
            &format!("Open {kind}"),
        );
        if label {
            self.text(
                x - 16,
                y + size as i32 + 5,
                size + 32,
                kind,
                12,
                Color::WHITE,
            );
        }
    }
    pub fn platform_icon(
        &mut self,
        r: Rect,
        platform: &str,
        kind: &str,
        action: &str,
        label: &str,
    ) {
        self.asset(r, &format!("icon/{platform}/{kind}"));
        self.region(r, action, label);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paging_a_month_grid_lands_on_real_gregorian_months() {
        // The world starts Thursday 17 September 2026.
        let now = CalendarDate::from_clock(0);
        assert_eq!(
            (now.year, now.month, now.day, now.weekday),
            (2026, 9, 17, 4)
        );
        assert_eq!(CalendarDate::shifted_months(0, 0), now);
        let next = CalendarDate::shifted_months(0, 1);
        assert_eq!((next.year, next.month, next.days_in_month), (2026, 10, 31));
        // 1 Oct 2026 is a Thursday, so the grid starts four columns in.
        assert_eq!(next.first_weekday, 4);
        let back = CalendarDate::shifted_months(0, -9);
        assert_eq!((back.year, back.month, back.days_in_month), (2025, 12, 31));
        assert_eq!(back.first_weekday, 1);
        // February of a leap year, reached forwards and backwards, must agree.
        let leap = CalendarDate::shifted_months(0, 5);
        assert_eq!((leap.year, leap.month, leap.days_in_month), (2027, 2, 28));
        let far = CalendarDate::shifted_months(0, 29);
        assert_eq!((far.year, far.month, far.days_in_month), (2029, 2, 28));
        let leap_year = CalendarDate::shifted_months(0, 17);
        assert_eq!(
            (leap_year.year, leap_year.month, leap_year.days_in_month),
            (2028, 2, 29)
        );
        // A day beyond the target month's length clamps instead of overflowing.
        assert_eq!(CalendarDate::shifted_months(0, 5).day, 17);
        // Paging out and back returns exactly where it started.
        for months in [1, 7, 13, -1, -7, -13] {
            let there = CalendarDate::shifted_months(0, months);
            assert_eq!(
                (there.year, there.month),
                {
                    let t = (2026i64 * 12 + 8) + months as i64;
                    ((t.div_euclid(12)) as u64, (t.rem_euclid(12)) as u64 + 1)
                },
                "{months}"
            );
        }
    }
    #[test]
    fn calendar_tracks_world_time_across_months_years_and_leap_days() {
        let day = 86_400_000_000;
        let d = CalendarDate::from_clock(0);
        assert_eq!(
            (
                d.year,
                d.month,
                d.day,
                d.weekday,
                d.first_weekday,
                d.days_in_month
            ),
            (2026, 9, 17, 4, 2, 30)
        );
        // The episode starts at 09:00, so the date rolls over fifteen hours in.
        assert_eq!(CalendarDate::from_clock(day * 15 / 24 - 1).day, 17);
        assert_eq!(CalendarDate::from_clock(day * 15 / 24).day, 18);
        let d = CalendarDate::from_clock(14 * day);
        assert_eq!((d.month, d.day, d.weekday), (10, 1, 4));
        let d = CalendarDate::from_clock(106 * day);
        assert_eq!((d.year, d.month, d.day, d.weekday), (2027, 1, 1, 5));
        let d = CalendarDate::from_clock((106 + 365 + 59) * day);
        assert_eq!((d.year, d.month, d.day), (2028, 2, 29));
        assert_eq!(sin1024(90), 1024);
        assert_eq!(cos1024(180), -1024);
        assert!((sin1024(30) - 512).abs() < 4);
    }
    #[test]
    fn labels_are_measured_aligned_and_ellipsized() {
        let mut p = Painter::themed(DesktopTheme::Macos, 400, 100, 1);
        let width = p.label(10, 10, 200, "Finder", 13, Color::BLACK, true, Align::Center);
        let node = p.scene.nodes.last().unwrap();
        assert_eq!(node.bounds.x, 10 + (200 - width as i32) / 2);
        p.label(
            10,
            40,
            60,
            "A very long window title",
            13,
            Color::BLACK,
            false,
            Align::Left,
        );
        let Primitive::UiText { text, .. } = &p.scene.nodes.last().unwrap().primitive else {
            panic!("label is UI text");
        };
        assert!(text.ends_with('…') && p.measure(text, 13, false) <= 60);
    }
}
