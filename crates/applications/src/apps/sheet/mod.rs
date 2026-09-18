//! Spreadsheets on the `cw-sheet` engine, each platform in its own product's clothes:
//! Microsoft Excel on Windows (and as a second application on the Mac), Numbers on
//! macOS and iOS, LibreOffice Calc on Ubuntu, and Google Sheets on Android. One model
//! of the workbook, selection and editing drives all of them; only the chrome differs.
//!
//! Files are real: workbooks are read and written as XLSX, ODS or CSV bytes on the
//! machine's filesystem, and nothing on screen is decoration. Every toolbar or ribbon
//! control either runs a command in [`Book::command`] or is painted disabled with the
//! reason it cannot.
use super::push_bounded;
use crate::desktop_scene::DesktopTheme;
use crate::AppEffect;
use cw_sheet::{Cell, ChartKind, Clip, Range, Workbook};
use serde::{Deserialize, Serialize};

mod chart;
mod chrome;
mod grid;
#[cfg(test)]
mod tests;

pub use grid::Geom;

/// Clock origin: tick 0 of the world is 2026-09-17 09:00:00 UTC.
pub const EPOCH_UNIX_US: i64 = 1_789_635_600_000_000;
/// Rows a Page Up or Page Down moves.
const PAGE: u32 = 20;
/// Longest text a cell editor takes.
const EDIT_LIMIT: usize = 32 * 1024;

/// Which product is on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flavor {
    Excel,
    Numbers,
    Calc,
    Sheets,
}
impl Flavor {
    pub fn of(theme: DesktopTheme, excel: bool) -> Self {
        if excel {
            return Self::Excel;
        }
        match theme {
            DesktopTheme::Windows => Self::Excel,
            DesktopTheme::Macos | DesktopTheme::Ios => Self::Numbers,
            DesktopTheme::Ubuntu => Self::Calc,
            DesktopTheme::Android => Self::Sheets,
        }
    }
    pub fn product(self, theme: DesktopTheme) -> &'static str {
        match (self, theme) {
            (Self::Excel, DesktopTheme::Macos) => "Microsoft Excel",
            (Self::Excel, _) => "Excel",
            (Self::Numbers, _) => "Numbers",
            (Self::Calc, _) => "LibreOffice Calc",
            (Self::Sheets, _) => "Sheets",
        }
    }
    /// The format a new document is saved in: Calc's own ODS, XLSX for the rest.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Calc => "ods",
            _ => "xlsx",
        }
    }
    /// What a new workbook is called before it is saved.
    pub fn untitled(self) -> &'static str {
        match self {
            Self::Excel => "Book1",
            Self::Numbers => "Untitled",
            Self::Calc => "Untitled 1",
            Self::Sheets => "Untitled spreadsheet",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Excel => "excel",
            Self::Numbers => "numbers",
            Self::Calc => "calc",
            Self::Sheets => "sheets",
        }
    }
    pub fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "excel" => Self::Excel,
            "numbers" => Self::Numbers,
            "calc" => Self::Calc,
            "sheets" => Self::Sheets,
            _ => return None,
        })
    }
    fn application(self) -> &'static str {
        match self {
            Self::Excel => "Microsoft Excel",
            Self::Numbers => "Numbers",
            Self::Calc => "LibreOffice/24.2",
            Self::Sheets => "Google Sheets",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Editing {
    pub text: String,
    /// Byte offset of the caret in `text`.
    pub caret: usize,
    /// Entered by typing over the cell (arrows commit) rather than F2 (arrows move
    /// the caret), as Excel's Enter and Edit modes.
    pub fresh: bool,
    /// Editing happens in the formula bar rather than in the cell.
    pub in_bar: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Drag {
    /// Extending the selection from the anchor.
    Select,
    /// Pulling the fill handle; the selection is the source.
    Fill { source: Range, to: Cell },
    /// Pointing at cells while typing a formula: the reference being inserted
    /// replaces the text from `at` on.
    Point { at: usize, from: Cell },
}
/// The folder a spreadsheet lists for Open, and what it found there.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    pub path: String,
    pub entries: Vec<String>,
    pub problem: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Book {
    /// Launched as Microsoft Excel rather than the platform's own spreadsheet.
    pub excel: bool,
    pub workbook: Workbook,
    /// The file on disk, once it has one.
    pub path: Option<String>,
    pub name: String,
    pub modified: bool,
    /// A file being read; the grid shows it once it arrives.
    pub loading: Option<String>,
    pub sheet: usize,
    pub active: Cell,
    pub anchor: Cell,
    /// First scrollable row and column shown (frozen panes are always shown).
    pub scroll: (u32, u32),
    pub zoom: u32,
    pub editing: Option<Editing>,
    /// Name box being typed into.
    pub name_box: Option<String>,
    /// Sheet tab being renamed, and the name so far.
    pub renaming: Option<(usize, String)>,
    /// What Copy or Cut took, with the text it put on the clipboard.
    pub clip: Option<(usize, Clip, bool, Range, String)>,
    pub menu: Option<String>,
    /// Excel's ribbon tab, Calc's menu, Numbers' sidebar pane.
    pub ribbon: String,
    /// The start screen / Open list is showing.
    pub browsing: bool,
    pub folder: Folder,
    pub chart: Option<usize>,
    pub message: Option<String>,
    pub drag: Option<Drag>,
    /// Numbers' Format sidebar is open.
    pub inspector: bool,
    pub gridlines: bool,
    /// The product this workbook was created or last saved in (`calc`, `numbers`, …),
    /// which decides a new file's name and format. Controls that create or save carry
    /// it, because the model itself never sees the theme.
    #[serde(default)]
    pub product: Option<String>,
}

fn ext(path: &str) -> String {
    path.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}
fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}
fn stem(name: &str) -> &str {
    name.rsplit_once('.').map_or(name, |(s, _)| s)
}
fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(p, _)| p)
}
/// Whether the spreadsheet opens a file of this name.
pub fn opens(name: &str) -> bool {
    matches!(ext(name).as_str(), "xlsx" | "xlsm" | "ods" | "csv")
}

impl Book {
    pub fn launch(
        argument: &str,
        window: u64,
        clock_us: u64,
        excel: bool,
    ) -> (Self, Vec<AppEffect>) {
        let mut wb = Workbook::new();
        wb.set_now(EPOCH_UNIX_US + clock_us as i64);
        let is_file = opens(argument);
        let folder = if is_file {
            parent(argument).to_owned()
        } else {
            argument.trim_end_matches('/').to_owned()
        };
        let mut book = Self {
            excel,
            workbook: wb,
            path: None,
            name: String::new(),
            modified: false,
            loading: None,
            sheet: 0,
            active: Cell::new(0, 0),
            anchor: Cell::new(0, 0),
            scroll: (0, 0),
            zoom: 100,
            editing: None,
            name_box: None,
            renaming: None,
            clip: None,
            menu: None,
            ribbon: "home".into(),
            browsing: !is_file,
            folder: Folder {
                path: if folder.is_empty() {
                    "Documents".into()
                } else {
                    folder
                },
                ..Folder::default()
            },
            chart: None,
            message: None,
            drag: None,
            inspector: false,
            gridlines: true,
            product: None,
        };
        let mut effects = vec![AppEffect::ListDirectory {
            window,
            tab: 0,
            path: book.folder.path.clone(),
        }];
        if is_file {
            book.loading = Some(argument.to_owned());
            book.name = file_name(argument).to_owned();
            effects.insert(
                0,
                AppEffect::ReadBytes {
                    window,
                    path: argument.to_owned(),
                },
            );
        }
        (book, effects)
    }
    pub fn flavor(&self, theme: DesktopTheme) -> Flavor {
        Flavor::of(theme, self.excel)
    }
    pub fn document(&self) -> String {
        self.path.clone().unwrap_or_default()
    }
    /// The window title each product gives an open workbook.
    pub fn window_title(&self, theme: DesktopTheme) -> String {
        let flavor = self.flavor(theme);
        let product = flavor.product(theme);
        if self.name.is_empty() || self.browsing || theme.mobile() {
            return product.into();
        }
        match (flavor, theme) {
            (Flavor::Excel, DesktopTheme::Macos) => self.name.clone(),
            (Flavor::Excel, _) => format!("{} - Excel", stem(&self.name)),
            (Flavor::Numbers, _) => stem(&self.name).to_owned(),
            _ => format!("{} - {product}", self.name),
        }
    }
    pub fn caption(&self) -> String {
        self.name.clone()
    }
    /// Whether a text field has focus: the cell editor, the name box or a sheet tab
    /// being renamed. Typing over a selected cell also starts the editor, as Excel's
    /// Enter mode does, but no field is focused until it has.
    pub fn accepts_text(&self) -> bool {
        self.editing.is_some() || self.name_box.is_some() || self.renaming.is_some()
    }
    pub fn selection(&self) -> Range {
        Range::new(self.anchor, self.active)
    }
    fn sheet_ref(&self) -> &cw_sheet::Sheet {
        &self.workbook.sheets[self.sheet.min(self.workbook.sheets.len() - 1)]
    }
    fn hidden(&self) -> std::collections::BTreeSet<u32> {
        self.sheet_ref().hidden_rows()
    }
    fn set_now(&mut self, clock_us: u64) {
        self.workbook.set_now(EPOCH_UNIX_US + clock_us as i64);
    }

    // ----- files -----

    /// The folder listing an Open asked for.
    pub fn listed(&mut self, entries: Vec<String>) {
        self.folder.entries = entries
            .into_iter()
            .filter(|e| e.ends_with('/') || opens(e))
            .collect();
        self.folder.problem = None;
    }
    pub fn listing_failed(&mut self, reason: &str) {
        self.folder.problem = Some(reason.to_owned());
    }
    /// A file this application asked for arrived (or could not be read).
    pub fn bytes(&mut self, path: &str, result: Result<Vec<u8>, String>, clock_us: u64) {
        if self.loading.as_deref() != Some(path) {
            return;
        }
        self.loading = None;
        let parsed = result.and_then(|bytes| match ext(path).as_str() {
            "xlsx" | "xlsm" => cw_sheet::xlsx::read(&bytes),
            "ods" => cw_sheet::ods::read(&bytes),
            "csv" => Ok(cw_sheet::csv::read(
                &String::from_utf8_lossy(&bytes),
                stem(file_name(path)),
            )),
            other => Err(format!("{other} files are not spreadsheets")),
        });
        match parsed {
            Ok(mut wb) => {
                wb.set_now(EPOCH_UNIX_US + clock_us as i64);
                self.workbook = wb;
                self.path = Some(path.to_owned());
                self.name = file_name(path).to_owned();
                self.modified = false;
                self.sheet = 0;
                self.active = Cell::new(0, 0);
                self.anchor = self.active;
                self.scroll = (0, 0);
                self.editing = None;
                self.chart = None;
                self.browsing = false;
                self.message = None;
            }
            Err(e) => {
                self.message = Some(format!("{} could not be opened: {e}", file_name(path)));
                self.browsing = self.path.is_none() && self.name.is_empty();
            }
        }
    }
    /// A save finished.
    pub fn saved(&mut self, path: &str, result: Result<(), String>) {
        match result {
            Ok(()) => {
                if Some(path) == self.path.as_deref() {
                    self.modified = false;
                }
                if !self.folder.entries.iter().any(|e| e == file_name(path))
                    && parent(path) == self.folder.path
                {
                    self.folder.entries.push(file_name(path).to_owned());
                    self.folder.entries.sort();
                }
            }
            Err(e) => self.message = Some(format!("{} could not be saved: {e}", file_name(path))),
        }
    }
    fn save_to(&mut self, window: u64, path: String, flavor: Flavor, csv: bool) -> Vec<AppEffect> {
        let bytes = if csv {
            cw_sheet::csv::write(&self.workbook, self.sheet).into_bytes()
        } else if ext(&path) == "ods" {
            cw_sheet::ods::write(&self.workbook, flavor.application())
        } else {
            cw_sheet::xlsx::write(&self.workbook, flavor.application())
        };
        let mut effects = Vec::new();
        let folder = parent(&path);
        if !folder.is_empty() {
            effects.push(AppEffect::CreateDirectory {
                window,
                path: folder.to_owned(),
            });
        }
        effects.push(AppEffect::WriteBytes {
            window,
            path,
            bytes,
        });
        effects
    }
    fn save(&mut self, window: u64, flavor: Flavor) -> Vec<AppEffect> {
        self.commit_edit();
        let path = match &self.path {
            // CSV keeps one sheet and no formulas; saving over it keeps it CSV, as the
            // desktop programs do once the user has been warned.
            Some(p) => p.clone(),
            None => {
                let name = if self.name.is_empty() {
                    flavor.untitled().to_owned()
                } else {
                    stem(&self.name).to_owned()
                };
                let path = format!("{}/{}.{}", self.folder.path, name, flavor.extension());
                self.path = Some(path.clone());
                self.name = file_name(&path).to_owned();
                path
            }
        };
        let csv = ext(&path) == "csv";
        self.save_to(window, path, flavor, csv)
    }

    // ----- editing -----

    fn start_edit(&mut self, text: String, fresh: bool, in_bar: bool) {
        let caret = text.len();
        self.editing = Some(Editing {
            text,
            caret,
            fresh,
            in_bar,
        });
        self.menu = None;
        self.chart = None;
    }
    /// Commit the cell editor into the active cell. A formula that does not parse is
    /// kept open with Excel's message rather than stored.
    fn commit_edit(&mut self) -> bool {
        let Some(e) = self.editing.take() else {
            return true;
        };
        // Excel closes the parentheses a formula left open, as its autocorrect does.
        let mut text = e.text.clone();
        if text.starts_with('=') && Workbook::parse_entry(&text).is_err() {
            let open = unclosed_parens(&text);
            if open > 0 {
                let closed = format!("{text}{}", ")".repeat(open));
                if Workbook::parse_entry(&closed).is_ok() {
                    text = closed;
                }
            }
        }
        match self.workbook.set_input(self.sheet, self.active, &text) {
            Ok(()) => {
                self.modified = true;
                true
            }
            Err(problem) => {
                self.message = Some(format!("There's a problem with this formula: {problem}"));
                self.editing = Some(e);
                false
            }
        }
    }
    fn insert_text(&mut self, text: &str) {
        if let Some(e) = &mut self.editing {
            let mut s = String::new();
            push_bounded(&mut s, text, EDIT_LIMIT.saturating_sub(e.text.len()));
            e.text.insert_str(e.caret, &s);
            e.caret += s.len();
        }
    }
    /// Text typed with no editor open starts one, replacing the cell (Excel's Enter mode).
    pub fn text(&mut self, text: &str) -> Result<(), String> {
        if self.loading.is_some() {
            return Err("the workbook is still opening".into());
        }
        if let Some(name) = &mut self.name_box {
            push_bounded(name, text, 64);
            return Ok(());
        }
        if let Some((_, name)) = &mut self.renaming {
            push_bounded(name, text, 31);
            return Ok(());
        }
        if self.browsing {
            return Err("choose a workbook first".into());
        }
        if self.editing.is_none() {
            self.start_edit(String::new(), true, false);
        }
        self.insert_text(text);
        Ok(())
    }
    fn move_active(&mut self, dr: i64, dc: i64, extend: bool) {
        let hidden = self.hidden();
        let mut row = i64::from(self.active.row);
        let step = dr.signum();
        let mut left = dr.abs();
        while left > 0 {
            let next = row + step;
            if !(0..i64::from(cw_sheet::MAX_ROWS)).contains(&next) {
                break;
            }
            row = next;
            if !hidden.contains(&(row as u32)) {
                left -= 1;
            }
        }
        let col = (i64::from(self.active.col) + dc).clamp(0, i64::from(cw_sheet::MAX_COLS) - 1);
        self.active = Cell::new(row as u32, col as u32);
        if !extend {
            self.anchor = self.active;
        }
        self.chart = None;
        self.reveal();
    }
    /// Scroll so the active cell is on screen (assuming a typical window's worth).
    fn reveal(&mut self) {
        let (fr, fc) = self.sheet_ref().freeze;
        if self.active.row >= fr {
            if self.active.row < self.scroll.0.max(fr) {
                self.scroll.0 = self.active.row;
            } else if self.active.row >= self.scroll.0.max(fr) + 24 {
                self.scroll.0 = self.active.row - 23;
            }
        }
        if self.active.col >= fc {
            if self.active.col < self.scroll.1.max(fc) {
                self.scroll.1 = self.active.col;
            } else if self.active.col >= self.scroll.1.max(fc) + 10 {
                self.scroll.1 = self.active.col - 9;
            }
        }
    }
    /// Ctrl+Arrow: to the edge of the current block of data, or the next one.
    fn jump(&mut self, dr: i64, dc: i64, extend: bool) {
        let filled = |c: Cell| !self.workbook.value(self.sheet, c).is_empty();
        let step = |c: Cell| -> Option<Cell> {
            let r = i64::from(c.row) + dr;
            let k = i64::from(c.col) + dc;
            (r >= 0
                && k >= 0
                && r < i64::from(cw_sheet::MAX_ROWS)
                && k < i64::from(cw_sheet::MAX_COLS))
            .then(|| Cell::new(r as u32, k as u32))
        };
        let used = self.workbook.used_range(self.sheet);
        let limit =
            |c: Cell| used.is_some_and(|u| c.row <= u.end.row + 1 && c.col <= u.end.col + 1);
        let mut c = self.active;
        let Some(next) = step(c) else {
            return;
        };
        if filled(c) && filled(next) {
            while let Some(n) = step(c) {
                if !filled(n) {
                    break;
                }
                c = n;
            }
        } else {
            c = next;
            while !filled(c) && limit(c) {
                match step(c) {
                    Some(n) => c = n,
                    None => break,
                }
            }
            if !filled(c) {
                // No more data that way: the sheet's edge, as Excel goes.
                c = Cell::new(
                    if dr > 0 {
                        cw_sheet::MAX_ROWS - 1
                    } else if dr < 0 {
                        0
                    } else {
                        c.row
                    },
                    if dc > 0 {
                        cw_sheet::MAX_COLS - 1
                    } else if dc < 0 {
                        0
                    } else {
                        c.col
                    },
                );
            }
        }
        self.active = c;
        if !extend {
            self.anchor = c;
        }
        self.reveal();
    }
    /// The block of data around the active cell (Ctrl+A's first step, a chart's
    /// default range).
    pub fn current_region(&self) -> Range {
        let filled = |r: u32, c: u32| !self.workbook.value(self.sheet, Cell::new(r, c)).is_empty();
        let mut rg = Range::single(self.active);
        loop {
            let mut grown = rg;
            let (r0, r1, c0, c1) = (rg.start.row, rg.end.row, rg.start.col, rg.end.col);
            let col_has = |c: u32, a: u32, b: u32| (a..=b).any(|r| filled(r, c));
            let row_has = |r: u32, a: u32, b: u32| (a..=b).any(|c| filled(r, c));
            if r0 > 0 && row_has(r0 - 1, c0.saturating_sub(1), c1 + 1) {
                grown.start.row -= 1;
            }
            if row_has(r1 + 1, c0.saturating_sub(1), c1 + 1) {
                grown.end.row += 1;
            }
            if c0 > 0 && col_has(c0 - 1, r0.saturating_sub(1), r1 + 1) {
                grown.start.col -= 1;
            }
            if col_has(c1 + 1, r0.saturating_sub(1), r1 + 1) {
                grown.end.col += 1;
            }
            if grown == rg || grown.rows() > 10_000 || grown.cols() > 500 {
                return rg;
            }
            rg = grown;
        }
    }
    fn copy(&mut self, window: u64, cut: bool) -> Vec<AppEffect> {
        let range = self.selection();
        if u64::from(range.rows()) * u64::from(range.cols()) > 200_000 {
            self.message = Some("That selection is too large to copy.".into());
            return vec![];
        }
        let clip = self.workbook.copy(self.sheet, range);
        // The clipboard gets tab-separated text, as every spreadsheet puts there.
        let mut text = String::new();
        for r in range.start.row..=range.end.row {
            let row: Vec<String> = (range.start.col..=range.end.col)
                .map(|c| self.workbook.display(self.sheet, Cell::new(r, c)))
                .collect();
            text.push_str(&row.join("\t"));
            text.push('\n');
        }
        self.clip = Some((self.sheet, clip, cut, range, text.clone()));
        vec![AppEffect::CopyText { window, text }]
    }
    /// Paste from the machine's clipboard. What this workbook copied pastes with its
    /// formulas and formats; other text is split on tabs and lines and typed in.
    pub fn paste(&mut self, text: &str) -> Result<(), String> {
        if self.editing.is_some() {
            self.insert_text(text.lines().next().unwrap_or(""));
            return Ok(());
        }
        let target = self.selection();
        if let Some((sheet, clip, cut, from, copied)) = self.clip.clone() {
            if copied == text {
                if cut && sheet == self.sheet {
                    let dest = self.workbook.move_range(self.sheet, from, target.start)?;
                    self.clip = None;
                    self.select_range(dest);
                } else {
                    let dest =
                        self.workbook
                            .paste(&clip, self.sheet, target.start, Some(target))?;
                    self.select_range(dest);
                }
                self.modified = true;
                return Ok(());
            }
        }
        let rows: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
        if rows.len() > 10_000 {
            return Err("the clipboard holds too many rows to paste".into());
        }
        for (i, line) in rows.iter().enumerate() {
            for (j, field) in line.trim_end_matches('\r').split('\t').enumerate() {
                let c = Cell::new(target.start.row + i as u32, target.start.col + j as u32);
                self.workbook.set_input(self.sheet, c, field).or_else(|_| {
                    // Text that looks like a broken formula arrives as text.
                    self.workbook.set_input(self.sheet, c, &format!("'{field}"))
                })?;
            }
        }
        self.modified = true;
        Ok(())
    }
    fn select_range(&mut self, r: Range) {
        self.anchor = r.start;
        self.active = r.end;
    }
    /// AutoSum: a SUM of the numbers above (or to the left) of the active cell, left in
    /// the editor to confirm; over a selected block, sums go under each column.
    fn autosum(&mut self) {
        let sel = self.selection();
        let is_num = |c: Cell| {
            matches!(
                self.workbook.value(self.sheet, c),
                cw_sheet::Value::Number(_)
            )
        };
        if !sel.is_single() {
            for col in sel.start.col..=sel.end.col {
                let target = Cell::new(sel.end.row + 1, col);
                let formula = format!(
                    "=SUM({})",
                    Range::new(Cell::new(sel.start.row, col), Cell::new(sel.end.row, col)).a1()
                );
                if self
                    .workbook
                    .set_input(self.sheet, target, &formula)
                    .is_ok()
                {
                    self.modified = true;
                }
            }
            return;
        }
        let at = self.active;
        let mut top = at.row;
        while top > 0 && is_num(Cell::new(top - 1, at.col)) {
            top -= 1;
        }
        let formula = if top < at.row {
            format!(
                "=SUM({})",
                Range::new(Cell::new(top, at.col), Cell::new(at.row - 1, at.col)).a1()
            )
        } else {
            let mut left = at.col;
            while left > 0 && is_num(Cell::new(at.row, left - 1)) {
                left -= 1;
            }
            if left < at.col {
                format!(
                    "=SUM({})",
                    Range::new(Cell::new(at.row, left), Cell::new(at.row, at.col - 1)).a1()
                )
            } else {
                "=SUM()".to_string()
            }
        };
        self.start_edit(formula, false, false);
        if let Some(e) = &mut self.editing {
            if e.text.ends_with("()") {
                e.caret -= 1;
            }
        }
    }
    fn go_to(&mut self, text: &str) -> Result<(), String> {
        let t = text.trim();
        // The active cell is a range's top-left; the anchor its far corner.
        if let Some(r) = Range::parse(t) {
            self.active = r.start;
            self.anchor = r.end;
            self.reveal();
            return Ok(());
        }
        match self.workbook.name(t) {
            Some((s, r)) => {
                self.sheet = s;
                self.active = r.start;
                self.anchor = r.end;
                self.reveal();
                Ok(())
            }
            None => {
                // A name box entry that is neither a reference nor a name defines one.
                self.workbook
                    .define_name(t, self.sheet, self.selection())
                    .map(|_| self.modified = true)
            }
        }
    }

    // ----- keys -----

    pub fn key(
        &mut self,
        window: u64,
        key: &str,
        clock_us: u64,
        flavor_hint: Option<Flavor>,
    ) -> Result<Vec<AppEffect>, String> {
        self.set_now(clock_us);
        if self.loading.is_some() {
            return Err("the workbook is still opening".into());
        }
        let flavor = flavor_hint.unwrap_or_else(|| self.product_flavor());
        let key = key.replace("Meta+", "Ctrl+");
        if self.message.is_some() && matches!(key.as_str(), "Enter" | "Escape") {
            self.message = None;
            return Ok(vec![]);
        }
        if let Some(name) = self.name_box.clone() {
            match key.as_str() {
                "Enter" => {
                    self.name_box = None;
                    self.go_to(&name)?;
                }
                "Escape" => self.name_box = None,
                "Backspace" => {
                    if let Some(n) = &mut self.name_box {
                        n.pop();
                    }
                }
                other => return Err(format!("unsupported name box key {other}")),
            }
            return Ok(vec![]);
        }
        if let Some((i, name)) = self.renaming.clone() {
            match key.as_str() {
                "Enter" => {
                    self.renaming = None;
                    self.workbook.rename_sheet(i, &name)?;
                    self.modified = true;
                }
                "Escape" => self.renaming = None,
                "Backspace" => {
                    if let Some((_, n)) = &mut self.renaming {
                        n.pop();
                    }
                }
                other => return Err(format!("unsupported rename key {other}")),
            }
            return Ok(vec![]);
        }
        if self.editing.is_some() {
            return self.edit_key(window, &key);
        }
        if self.browsing {
            return match key.as_str() {
                "Escape" if !self.name.is_empty() || self.path.is_some() => {
                    self.browsing = false;
                    Ok(vec![])
                }
                other => Err(format!("unsupported key {other} on the file list")),
            };
        }
        match key.as_str() {
            "ArrowUp" => self.move_active(-1, 0, false),
            "ArrowDown" | "Enter" => self.move_active(1, 0, false),
            "ArrowLeft" | "Shift+Tab" => self.move_active(0, -1, false),
            "ArrowRight" | "Tab" => self.move_active(0, 1, false),
            "Shift+Enter" => self.move_active(-1, 0, false),
            "Shift+ArrowUp" => self.move_active(-1, 0, true),
            "Shift+ArrowDown" => self.move_active(1, 0, true),
            "Shift+ArrowLeft" => self.move_active(0, -1, true),
            "Shift+ArrowRight" => self.move_active(0, 1, true),
            "Ctrl+ArrowUp" => self.jump(-1, 0, false),
            "Ctrl+ArrowDown" => self.jump(1, 0, false),
            "Ctrl+ArrowLeft" => self.jump(0, -1, false),
            "Ctrl+ArrowRight" => self.jump(0, 1, false),
            "Ctrl+Shift+ArrowUp" => self.jump(-1, 0, true),
            "Ctrl+Shift+ArrowDown" => self.jump(1, 0, true),
            "Ctrl+Shift+ArrowLeft" => self.jump(0, -1, true),
            "Ctrl+Shift+ArrowRight" => self.jump(0, 1, true),
            "PageDown" => self.move_active(i64::from(PAGE), 0, false),
            "PageUp" => self.move_active(-i64::from(PAGE), 0, false),
            "Home" => {
                self.active.col = 0;
                self.anchor = self.active;
                self.reveal();
            }
            "Ctrl+Home" => {
                self.active = Cell::new(self.sheet_ref().freeze.0, self.sheet_ref().freeze.1);
                self.anchor = self.active;
                self.reveal();
            }
            "Ctrl+End" => {
                if let Some(u) = self.workbook.used_range(self.sheet) {
                    self.active = u.end;
                    self.anchor = u.end;
                    self.reveal();
                }
            }
            "Ctrl+a" => {
                let region = self.current_region();
                if self.selection() == region || region.is_single() {
                    self.anchor = Cell::new(0, 0);
                    self.active = Cell::new(cw_sheet::MAX_ROWS - 1, cw_sheet::MAX_COLS - 1);
                } else {
                    self.anchor = region.start;
                    self.active = region.end;
                }
            }
            "F2" => {
                let input = self.workbook.input(self.sheet, self.active);
                self.start_edit(input, false, false);
            }
            "Backspace" => {
                self.start_edit(String::new(), true, false);
            }
            "Delete" => {
                if let Some(i) = self.chart.take() {
                    self.workbook.remove_chart(self.sheet, i)?;
                } else {
                    self.workbook.clear(self.sheet, self.selection())?;
                }
                self.modified = true;
            }
            "Escape" => {
                self.menu = None;
                self.clip = None;
                self.chart = None;
            }
            "Ctrl+c" => return Ok(self.copy(window, false)),
            "Ctrl+x" => return Ok(self.copy(window, true)),
            "Ctrl+v" => return Ok(vec![AppEffect::Paste { window }]),
            "Ctrl+s" => return Ok(self.save(window, flavor)),
            other => {
                let command = match other {
                    "Ctrl+z" => "undo",
                    "Ctrl+y" | "Ctrl+Shift+z" => "redo",
                    "Ctrl+b" => "bold",
                    "Ctrl+i" => "italic",
                    "Ctrl+u" => "underline",
                    "Ctrl+d" => "filldown",
                    "Ctrl+r" => "fillright",
                    "Alt+=" => "autosum",
                    _ => return Err(format!("unsupported spreadsheet key {other}")),
                };
                return self.command(window, command, clock_us, flavor);
            }
        }
        Ok(vec![])
    }
    fn edit_key(&mut self, window: u64, key: &str) -> Result<Vec<AppEffect>, String> {
        let e = self.editing.as_mut().expect("editing");
        let text = &mut e.text;
        match key {
            "Enter" | "Tab" | "Shift+Enter" | "Shift+Tab" => {
                if self.commit_edit() {
                    match key {
                        "Enter" => self.move_active(1, 0, false),
                        "Tab" => self.move_active(0, 1, false),
                        "Shift+Enter" => self.move_active(-1, 0, false),
                        _ => self.move_active(0, -1, false),
                    }
                }
            }
            "Escape" => {
                self.editing = None;
                self.drag = None;
            }
            "Backspace" => {
                if e.caret > 0 {
                    let at = text[..e.caret]
                        .char_indices()
                        .next_back()
                        .map_or(0, |(i, _)| i);
                    text.drain(at..e.caret);
                    e.caret = at;
                }
            }
            "Delete" => {
                if let Some(ch) = text[e.caret..].chars().next() {
                    text.drain(e.caret..e.caret + ch.len_utf8());
                }
            }
            "Home" => e.caret = 0,
            "End" => e.caret = text.len(),
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown"
                if e.fresh && !text.starts_with('=') =>
            {
                // Enter mode: an arrow commits and moves, as in Excel.
                if self.commit_edit() {
                    match key {
                        "ArrowLeft" => self.move_active(0, -1, false),
                        "ArrowRight" => self.move_active(0, 1, false),
                        "ArrowUp" => self.move_active(-1, 0, false),
                        _ => self.move_active(1, 0, false),
                    }
                }
            }
            "ArrowLeft" => {
                e.caret = text[..e.caret]
                    .char_indices()
                    .next_back()
                    .map_or(0, |(i, _)| i);
            }
            "ArrowRight" => {
                if let Some(ch) = text[e.caret..].chars().next() {
                    e.caret += ch.len_utf8();
                }
            }
            "Ctrl+v" => return Ok(vec![AppEffect::Paste { window }]),
            other => return Err(format!("unsupported editing key {other}")),
        }
        Ok(vec![])
    }

    // ----- pointer -----

    /// Whether `target` is a drag surface: the cell grid and the fill handle.
    pub fn drags(target: &str) -> bool {
        target.starts_with("sheet:grid:") || target.starts_with("sheet:fill:")
    }
    /// A wheel turn over the grid or its headers: three rows a notch, as Excel, Calc
    /// and Numbers scroll; with Shift (or a sideways turn) columns instead; with Ctrl
    /// it zooms, ten percent a notch. Frozen rows and columns never scroll away.
    pub fn wheel(&mut self, target: &str, wheel: crate::Wheel) -> Result<bool, String> {
        if !["sheet:grid:", "sheet:fill:", "sheet:col:", "sheet:row:"]
            .iter()
            .any(|p| target.starts_with(p))
        {
            return Ok(false);
        }
        let before = (self.scroll, self.zoom);
        let (fr, fc) = self.sheet_ref().freeze;
        if wheel.ctrl {
            let notches = crate::wheel_steps(-wheel.dy, 120);
            self.zoom = (self.zoom as i64 + i64::from(notches) * 10).clamp(10, 400) as u32;
        } else if wheel.horizontal() != 0 {
            let cols = crate::wheel_steps(wheel.horizontal(), 120);
            self.scroll.1 = (i64::from(self.scroll.1.max(fc)) + i64::from(cols))
                .clamp(i64::from(fc), i64::from(cw_sheet::MAX_COLS - 1))
                as u32;
        } else {
            let rows = crate::wheel_steps(wheel.dy, 40);
            self.scroll.0 = (i64::from(self.scroll.0.max(fr)) + i64::from(rows))
                .clamp(i64::from(fr), i64::from(cw_sheet::MAX_ROWS - 1))
                as u32;
        }
        Ok(before != (self.scroll, self.zoom))
    }
    /// Map a point inside the cell area (not counting headers) to a cell.
    fn cell_at(&self, geom: Geom, x: i32, y: i32) -> Cell {
        let sheet = self.sheet_ref();
        let (fr, fc) = sheet.freeze;
        let hidden = self.hidden();
        let row = {
            let mut acc = 0i32;
            let mut found = None;
            let mut r = 0u32;
            let rows = (0..fr).chain(self.scroll.0.max(fr)..cw_sheet::MAX_ROWS);
            for row in rows {
                if hidden.contains(&row) {
                    continue;
                }
                r = row;
                acc += geom.row_h as i32;
                if y < acc {
                    found = Some(row);
                    break;
                }
                if acc > 200_000 {
                    break;
                }
            }
            if y < 0 {
                if fr > 0 {
                    0
                } else {
                    self.scroll.0
                }
            } else {
                found.unwrap_or(r)
            }
        };
        let col = {
            let mut acc = 0i32;
            let mut found = None;
            let mut k = 0u32;
            for col in (0..fc).chain(self.scroll.1.max(fc)..cw_sheet::MAX_COLS) {
                k = col;
                acc += geom.col_px(sheet, col) as i32;
                if x < acc {
                    found = Some(col);
                    break;
                }
                if acc > 200_000 {
                    break;
                }
            }
            if x < 0 {
                if fc > 0 {
                    0
                } else {
                    self.scroll.1
                }
            } else {
                found.unwrap_or(k)
            }
        };
        Cell::new(row, col)
    }
    /// Pixel position of a cell's top-left inside the cell area, when on screen.
    pub fn cell_origin(&self, geom: Geom, c: Cell) -> Option<(i32, i32)> {
        let sheet = self.sheet_ref();
        let (fr, fc) = sheet.freeze;
        let hidden = self.hidden();
        let mut y = 0i32;
        let mut found_y = None;
        for row in (0..fr).chain(self.scroll.0.max(fr)..=c.row.max(self.scroll.0)) {
            if row == c.row {
                found_y = Some(y);
                break;
            }
            if !hidden.contains(&row) {
                y += geom.row_h as i32;
            }
            if y > 100_000 {
                break;
            }
        }
        let mut x = 0i32;
        let mut found_x = None;
        for col in (0..fc).chain(self.scroll.1.max(fc)..=c.col.max(self.scroll.1)) {
            if col == c.col {
                found_x = Some(x);
                break;
            }
            x += geom.col_px(sheet, col) as i32;
            if x > 100_000 {
                break;
            }
        }
        Some((found_x?, found_y?))
    }
    fn pointing(&self) -> bool {
        self.editing.as_ref().is_some_and(|e| {
            e.text.starts_with('=')
                && e.caret == e.text.len()
                && e.text
                    .trim_end()
                    .ends_with(['=', '(', ',', '+', '-', '*', '/', '^', '&', '<', '>', ':'])
        })
    }
    pub fn pointer(
        &mut self,
        target: &str,
        phase: crate::PointerPhase,
        x: i32,
        y: i32,
    ) -> Result<Vec<AppEffect>, String> {
        use crate::PointerPhase::*;
        let geom = Geom::from_target(target).ok_or("malformed grid surface")?;
        if target.starts_with("sheet:fill:") {
            // Coordinates are relative to the handle; it sits on the selection's corner.
            let sel = self.selection();
            let (hx, hy) = self
                .cell_origin(geom, Cell::new(sel.end.row + 1, sel.end.col + 1))
                .unwrap_or((0, 0));
            let at = self.cell_at(geom, hx - 3 + x, hy - 3 + y);
            match phase {
                Down => {
                    self.commit_edit();
                    self.drag = Some(Drag::Fill {
                        source: sel,
                        to: sel.end,
                    });
                }
                Move => {
                    if let Some(Drag::Fill { to, .. }) = &mut self.drag {
                        *to = at;
                    }
                }
                Up => {
                    if let Some(Drag::Fill { source, .. }) = self.drag.take() {
                        let target = fill_target(source, at);
                        if target != source {
                            self.workbook.fill(self.sheet, source, target)?;
                            self.select_range(Range::new(target.start, target.end));
                            self.anchor = target.start;
                            self.active = target.end;
                            self.modified = true;
                        }
                    }
                }
                Cancel => self.drag = None,
            }
            return Ok(vec![]);
        }
        let at = self.cell_at(geom, x, y);
        match phase {
            Down => {
                self.menu = None;
                self.chart = None;
                if self.pointing() {
                    let e = self.editing.as_mut().expect("pointing");
                    let pos = e.text.len();
                    e.text.push_str(&at.a1());
                    e.caret = e.text.len();
                    self.drag = Some(Drag::Point { at: pos, from: at });
                    return Ok(vec![]);
                }
                if !self.commit_edit() {
                    return Ok(vec![]);
                }
                self.active = at;
                self.anchor = at;
                self.drag = Some(Drag::Select);
            }
            Move | Up => {
                match self.drag.clone() {
                    Some(Drag::Select) => {
                        // The anchor stays where the press was; the active corner follows.
                        self.active = at;
                    }
                    Some(Drag::Point { at: pos, from }) => {
                        let r = Range::new(from, at);
                        if let Some(e) = &mut self.editing {
                            e.text.truncate(pos);
                            e.text.push_str(&r.a1());
                            e.caret = e.text.len();
                        }
                    }
                    _ => {}
                }
                if phase == Up {
                    // The press point is the active cell of a dragged range, as Excel keeps it.
                    if matches!(self.drag, Some(Drag::Select)) {
                        std::mem::swap(&mut self.active, &mut self.anchor);
                    }
                    self.drag = None;
                }
            }
            Cancel => self.drag = None,
        }
        Ok(vec![])
    }

    // ----- commands -----

    fn format_selection(&mut self, f: impl Fn(&mut cw_sheet::Style)) -> Result<(), String> {
        self.commit_edit();
        let sel = self.selection();
        // Whole columns or rows format the used part; the rest is empty anyway.
        let bounded = match self.workbook.used_range(self.sheet) {
            Some(u) if u64::from(sel.rows()) * u64::from(sel.cols()) > 100_000 => {
                sel.intersect(&u).unwrap_or(Range::single(sel.start))
            }
            _ => sel,
        };
        self.workbook.update_style(self.sheet, bounded, f)?;
        self.modified = true;
        Ok(())
    }
    fn toggle_style(&mut self, which: &str) -> Result<(), String> {
        let current = self.workbook.style(self.sheet, self.active);
        let on = !match which {
            "bold" => current.bold,
            "italic" => current.italic,
            _ => current.underline,
        };
        self.format_selection(|s| match which {
            "bold" => s.bold = on,
            "italic" => s.italic = on,
            _ => s.underline = on,
        })
    }
    fn adjust_decimals(&mut self, more: bool) -> Result<(), String> {
        let current = self.workbook.style(self.sheet, self.active).format;
        let base = if current == "General" {
            // Start from the decimals the value shows.
            let shown = self.workbook.display(self.sheet, self.active);
            let d = shown.split_once('.').map_or(0, |(_, f)| {
                f.chars().take_while(char::is_ascii_digit).count()
            });
            if d == 0 {
                "0".to_string()
            } else {
                format!("0.{}", "0".repeat(d))
            }
        } else {
            current
        };
        let next = change_decimals(&base, more);
        self.format_selection(|s| s.format = next.clone())
    }
    /// Run a command by name. Every control on screen reaches one of these.
    pub fn command(
        &mut self,
        window: u64,
        command: &str,
        clock_us: u64,
        flavor: Flavor,
    ) -> Result<Vec<AppEffect>, String> {
        self.set_now(clock_us);
        if self.loading.is_some() && command != "dismiss" {
            return Err("the workbook is still opening".into());
        }
        let (verb, arg) = command.split_once(':').unwrap_or((command, ""));
        // Menus and ribbon tabs only change what is shown.
        match verb {
            "menu" => {
                self.menu = if self.menu.as_deref() == Some(arg) {
                    None
                } else {
                    Some(arg.to_owned())
                };
                return Ok(vec![]);
            }
            "ribbon" => {
                self.ribbon = arg.to_owned();
                self.menu = None;
                return Ok(vec![]);
            }
            "dismiss" => {
                self.message = None;
                self.menu = None;
                return Ok(vec![]);
            }
            "inspector" => {
                self.inspector = !self.inspector;
                if !arg.is_empty() {
                    self.inspector = true;
                    self.ribbon = arg.to_owned();
                }
                return Ok(vec![]);
            }
            _ => {}
        }
        if verb != "noop" {
            self.menu = None;
        }
        match verb {
            "noop" => {}
            "select" => {
                self.commit_edit();
                let r = Range::parse(arg).ok_or("that is not a cell reference")?;
                self.anchor = r.start;
                self.active = r.end;
                std::mem::swap(&mut self.anchor, &mut self.active);
                self.chart = None;
                self.reveal();
            }
            "col" => {
                self.commit_edit();
                let c = cw_sheet::column_index(arg).ok_or("that is not a column")?;
                self.anchor = Cell::new(0, c);
                self.active = Cell::new(cw_sheet::MAX_ROWS - 1, c);
                std::mem::swap(&mut self.anchor, &mut self.active);
            }
            "row" => {
                self.commit_edit();
                let r: u32 = arg.parse().map_err(|_| "that is not a row")?;
                let r = r.checked_sub(1).ok_or("rows start at 1")?;
                self.anchor = Cell::new(r, cw_sheet::MAX_COLS - 1);
                self.active = Cell::new(r, 0);
            }
            "all" => {
                self.commit_edit();
                self.anchor = Cell::new(cw_sheet::MAX_ROWS - 1, cw_sheet::MAX_COLS - 1);
                self.active = Cell::new(0, 0);
            }
            "edit" => {
                let input = self.workbook.input(self.sheet, self.active);
                self.start_edit(input, false, arg == "bar");
            }
            "enter" => {
                if self.commit_edit() {
                    self.move_active(1, 0, false);
                }
            }
            "cancel" => self.editing = None,
            "namebox" => {
                self.commit_edit();
                self.name_box = Some(String::new());
            }
            "insertfn" => {
                // Insert Function: start (or continue) a formula with the function.
                let f = arg.to_ascii_uppercase();
                if !cw_sheet::functions::FUNCTIONS.contains(&f.as_str()) {
                    return Err(format!("{f} is not a function"));
                }
                if self.editing.is_none() {
                    self.start_edit("=".into(), false, false);
                }
                self.insert_text(&format!("{f}("));
            }
            "cut" => return Ok(self.copy(window, true)),
            "copy" => return Ok(self.copy(window, false)),
            "paste" => return Ok(vec![AppEffect::Paste { window }]),
            "bold" | "italic" | "underline" => self.toggle_style(verb)?,
            "align" => {
                let a = match arg {
                    "left" => cw_sheet::Align::Left,
                    "center" => cw_sheet::Align::Center,
                    "right" => cw_sheet::Align::Right,
                    _ => cw_sheet::Align::General,
                };
                self.format_selection(|s| s.align = a)?;
            }
            "fmt" => {
                let code = match arg {
                    "general" => "General",
                    "number" => "#,##0.00",
                    "currency" => "$#,##0.00",
                    "accounting" => "_($* #,##0.00_);_($* (#,##0.00);_($* \"-\"??_);_(@_)",
                    "percent" => "0%",
                    "date" => "m/d/yyyy",
                    "longdate" => "dddd, mmmm d, yyyy",
                    "time" => "h:mm:ss AM/PM",
                    "comma" => "#,##0.00",
                    "text" => "@",
                    "scientific" => "0.00E+00",
                    _ => return Err(format!("unknown number format {arg}")),
                };
                self.format_selection(|s| s.format = code.to_owned())?;
            }
            "dec" => self.adjust_decimals(arg == "more")?,
            "fill" | "color" => {
                let rgb = if arg == "none" {
                    None
                } else {
                    Some(parse_hex(arg).ok_or("that is not a colour")?)
                };
                if verb == "fill" {
                    self.format_selection(|s| s.fill = rgb)?;
                } else {
                    self.format_selection(|s| s.color = rgb)?;
                }
            }
            "insert" => {
                self.commit_edit();
                let sel = self.selection();
                match arg {
                    "rows" => self.workbook.insert_rows(
                        self.sheet,
                        sel.start.row,
                        sel.rows().min(1000),
                    )?,
                    "cols" => {
                        self.workbook
                            .insert_cols(self.sheet, sel.start.col, sel.cols().min(100))?
                    }
                    "sheet" => {
                        self.sheet = self.workbook.add_sheet(None)?;
                        self.active = Cell::new(0, 0);
                        self.anchor = self.active;
                        self.scroll = (0, 0);
                    }
                    _ => return Err(format!("cannot insert {arg}")),
                }
                self.modified = true;
            }
            "delete" => {
                self.commit_edit();
                let sel = self.selection();
                match arg {
                    "rows" => self.workbook.delete_rows(
                        self.sheet,
                        sel.start.row,
                        sel.rows().min(cw_sheet::MAX_ROWS - sel.start.row),
                    )?,
                    "cols" => self.workbook.delete_cols(
                        self.sheet,
                        sel.start.col,
                        sel.cols().min(cw_sheet::MAX_COLS - sel.start.col),
                    )?,
                    "sheet" => {
                        self.workbook.remove_sheet(self.sheet)?;
                        self.sheet = self.sheet.min(self.workbook.sheets.len() - 1);
                    }
                    "chart" => {
                        let i = self.chart.take().ok_or("no chart is selected")?;
                        self.workbook.remove_chart(self.sheet, i)?;
                    }
                    _ => return Err(format!("cannot delete {arg}")),
                }
                self.modified = true;
            }
            "tab" => {
                self.commit_edit();
                let i: usize = arg.parse().map_err(|_| "no such sheet")?;
                if i >= self.workbook.sheets.len() {
                    return Err("no such sheet".into());
                }
                if self.sheet != i {
                    self.sheet = i;
                    self.active = Cell::new(0, 0);
                    self.anchor = self.active;
                    self.scroll = (0, 0);
                    self.chart = None;
                }
            }
            "rename" => {
                let i = if arg.is_empty() {
                    self.sheet
                } else {
                    arg.parse().map_err(|_| "no such sheet")?
                };
                let name = self
                    .workbook
                    .sheets
                    .get(i)
                    .ok_or("no such sheet")?
                    .name
                    .clone();
                self.renaming = Some((i, name));
            }
            "autosum" => self.autosum(),
            "filldown" | "fillright" => {
                self.commit_edit();
                let sel = self.selection();
                let (source, target) = if verb == "filldown" {
                    if sel.rows() < 2 {
                        let src = Range::new(
                            Cell::new(sel.start.row.saturating_sub(1), sel.start.col),
                            Cell::new(sel.start.row.saturating_sub(1), sel.end.col),
                        );
                        if sel.start.row == 0 {
                            // Nothing above the first row: Excel does nothing.
                            return Ok(vec![]);
                        }
                        (src, Range::new(src.start, sel.end))
                    } else {
                        (
                            Range::new(sel.start, Cell::new(sel.start.row, sel.end.col)),
                            sel,
                        )
                    }
                } else if sel.cols() < 2 {
                    if sel.start.col == 0 {
                        return Ok(vec![]);
                    }
                    let src = Range::new(
                        Cell::new(sel.start.row, sel.start.col - 1),
                        Cell::new(sel.end.row, sel.start.col - 1),
                    );
                    (src, Range::new(src.start, sel.end))
                } else {
                    (
                        Range::new(sel.start, Cell::new(sel.end.row, sel.start.col)),
                        sel,
                    )
                };
                // Ctrl+D and Ctrl+R copy; they never continue a series.
                let clip = self.workbook.copy(self.sheet, source);
                let at = if verb == "filldown" {
                    Cell::new(source.end.row + 1, source.start.col)
                } else {
                    Cell::new(source.start.row, source.end.col + 1)
                };
                let rest = Range::new(at, target.end);
                if rest.intersect(&target) == Some(rest)
                    && (target.rows() > source.rows() || target.cols() > source.cols())
                {
                    self.workbook.paste(&clip, self.sheet, at, Some(rest))?;
                    self.modified = true;
                }
            }
            "clear" => {
                self.workbook.clear(self.sheet, self.selection())?;
                self.modified = true;
            }
            "clearall" => {
                self.workbook.clear_all(self.sheet, self.selection())?;
                self.modified = true;
            }
            "clearformats" => {
                self.format_selection(|s| *s = cw_sheet::Style::default())?;
            }
            "undo" => {
                self.editing = None;
                if !self.workbook.undo() {
                    return Err("there is nothing to undo".into());
                }
                self.sheet = self.sheet.min(self.workbook.sheets.len() - 1);
                self.modified = true;
            }
            "redo" => {
                if !self.workbook.redo() {
                    return Err("there is nothing to redo".into());
                }
                self.sheet = self.sheet.min(self.workbook.sheets.len() - 1);
                self.modified = true;
            }
            "sort" => {
                self.commit_edit();
                let sel = self.selection();
                let range = if sel.is_single() {
                    self.current_region()
                } else {
                    sel
                };
                // A first row of text over data is a header and stays put.
                let header = range.rows() > 1
                    && (range.start.col..=range.end.col).all(|c| {
                        matches!(
                            self.workbook
                                .value(self.sheet, Cell::new(range.start.row, c)),
                            cw_sheet::Value::Text(_) | cw_sheet::Value::Empty
                        )
                    })
                    && (range.start.col..=range.end.col).any(|c| {
                        !matches!(
                            self.workbook
                                .value(self.sheet, Cell::new(range.start.row + 1, c)),
                            cw_sheet::Value::Text(_) | cw_sheet::Value::Empty
                        )
                    });
                let by = self.active.col.clamp(range.start.col, range.end.col);
                self.workbook
                    .sort(self.sheet, range, by, arg != "desc", header)?;
                self.modified = true;
            }
            "filter" => {
                self.commit_edit();
                if self.sheet_ref().filter.is_some() {
                    self.workbook.set_filter(self.sheet, None)?;
                } else {
                    let sel = self.selection();
                    let range = if sel.is_single() {
                        self.current_region()
                    } else {
                        sel
                    };
                    self.workbook.set_filter(self.sheet, Some(range))?;
                }
                self.modified = true;
            }
            "filterpick" => {
                self.menu = Some(format!("filter:{arg}"));
            }
            "filtertoggle" => {
                let (col, value) = arg.split_once(':').ok_or("which value?")?;
                let col = cw_sheet::column_index(col).ok_or("that is not a column")?;
                self.workbook.filter_toggle(self.sheet, col, value)?;
                self.menu = Some(format!("filter:{}", cw_sheet::column_name(col)));
                self.modified = true;
            }
            "chart" => {
                self.commit_edit();
                let kind = ChartKind::parse(arg).ok_or("unknown chart type")?;
                let sel = self.selection();
                let range = if sel.is_single() {
                    self.current_region()
                } else {
                    sel
                };
                if range.is_single() && self.workbook.value(self.sheet, range.start).is_empty() {
                    self.message = Some(
                        "Select the cells with the data to chart, then insert the chart.".into(),
                    );
                    return Ok(vec![]);
                }
                let title = self.chart_title(range);
                let i = self.workbook.add_chart(self.sheet, kind, range, &title)?;
                self.chart = Some(i);
                self.modified = true;
            }
            "chartsel" => {
                let i: usize = arg.parse().map_err(|_| "no such chart")?;
                if i >= self.sheet_ref().charts.len() {
                    return Err("no such chart".into());
                }
                self.commit_edit();
                self.chart = Some(i);
            }
            "charttype" => {
                let i = self.chart.ok_or("no chart is selected")?;
                let kind = ChartKind::parse(arg).ok_or("unknown chart type")?;
                self.workbook.set_chart_kind(self.sheet, i, kind)?;
                self.modified = true;
            }
            "freeze" => {
                let (r, c) = match arg {
                    "row" => (1, 0),
                    "col" => (0, 1),
                    "panes" => (self.active.row, self.active.col),
                    _ => (0, 0),
                };
                self.workbook.set_freeze(self.sheet, r, c)?;
                self.scroll = (self.scroll.0.max(r), self.scroll.1.max(c));
                self.modified = true;
            }
            "zoom" => {
                self.zoom = match arg {
                    "in" => (self.zoom + 10).min(400),
                    "out" => self.zoom.saturating_sub(10).max(10),
                    "reset" => 100,
                    n => n.parse::<u32>().map_err(|_| "unknown zoom")?.clamp(10, 400),
                };
            }
            "gridlines" => self.gridlines = !self.gridlines,
            "scroll" => {
                let (fr, fc) = self.sheet_ref().freeze;
                match arg {
                    "up" => self.scroll.0 = self.scroll.0.saturating_sub(3).max(fr),
                    "down" => self.scroll.0 = (self.scroll.0 + 3).min(cw_sheet::MAX_ROWS - 1),
                    "pageup" => self.scroll.0 = self.scroll.0.saturating_sub(PAGE).max(fr),
                    "pagedown" => {
                        self.scroll.0 = (self.scroll.0 + PAGE).min(cw_sheet::MAX_ROWS - 1)
                    }
                    "left" => self.scroll.1 = self.scroll.1.saturating_sub(1).max(fc),
                    "right" => self.scroll.1 = (self.scroll.1 + 1).min(cw_sheet::MAX_COLS - 1),
                    _ => return Err("unknown scroll".into()),
                }
            }
            "colwidth" => {
                let (col, px) = arg.split_once(':').ok_or("which column?")?;
                let col = cw_sheet::column_index(col).ok_or("that is not a column")?;
                let px: u32 = px.parse().map_err(|_| "that is not a width")?;
                self.workbook.set_col_width(self.sheet, col, px)?;
                self.modified = true;
            }
            "autofit" => {
                let sel = self.selection();
                for col in sel.start.col..=sel.end.col.min(sel.start.col + 200) {
                    let widest = self
                        .workbook
                        .used_range(self.sheet)
                        .map(|u| {
                            (u.start.row..=u.end.row)
                                .map(|r| {
                                    self.workbook
                                        .display(self.sheet, Cell::new(r, col))
                                        .chars()
                                        .count()
                                })
                                .max()
                                .unwrap_or(0)
                        })
                        .unwrap_or(0);
                    let px = (widest as u32 * 7 + 10).max(cw_sheet::workbook::DEFAULT_COL_WIDTH);
                    self.workbook.set_col_width(self.sheet, col, px)?;
                }
                self.modified = true;
            }
            "new" => {
                self.commit_edit();
                if let Some(f) = Flavor::parse(arg) {
                    self.product = Some(f.name().into());
                }
                let flavor = self.product_flavor();
                let mut wb = Workbook::new();
                wb.set_now(EPOCH_UNIX_US + clock_us as i64);
                self.workbook = wb;
                self.path = None;
                self.name = flavor.untitled().to_owned();
                self.modified = false;
                self.browsing = false;
                self.sheet = 0;
                self.active = Cell::new(0, 0);
                self.anchor = self.active;
                self.scroll = (0, 0);
                self.chart = None;
            }
            "open" => {
                self.commit_edit();
                self.browsing = true;
                return Ok(vec![AppEffect::ListDirectory {
                    window,
                    tab: 0,
                    path: self.folder.path.clone(),
                }]);
            }
            "openfile" => {
                if !self.folder.entries.iter().any(|e| e == arg) || !opens(arg) {
                    return Err("that file is not in the list".into());
                }
                let path = format!("{}/{arg}", self.folder.path);
                self.loading = Some(path.clone());
                return Ok(vec![AppEffect::ReadBytes { window, path }]);
            }
            "folder" => {
                // Into a subfolder of the list, or up with "..".
                let next = if arg == ".." {
                    parent(&self.folder.path).to_owned()
                } else {
                    let entry = format!("{arg}/");
                    if !self.folder.entries.contains(&entry) {
                        return Err("that folder is not in the list".into());
                    }
                    format!("{}/{arg}", self.folder.path)
                };
                if next.is_empty() {
                    return Err("that is the top of the filesystem".into());
                }
                self.folder = Folder {
                    path: next.clone(),
                    ..Folder::default()
                };
                return Ok(vec![AppEffect::ListDirectory {
                    window,
                    tab: 0,
                    path: next,
                }]);
            }
            "closelist" => {
                if self.name.is_empty() && self.path.is_none() {
                    return Err("there is no workbook open to return to".into());
                }
                self.browsing = false;
            }
            "save" => {
                if let Some(f) = Flavor::parse(arg) {
                    self.product = Some(f.name().into());
                }
                let flavor = self.product_flavor();
                return Ok(self.save(window, flavor));
            }
            "savecsv" => {
                self.commit_edit();
                let base = self.path.as_deref().map_or_else(
                    || stem(&self.name).to_owned(),
                    |p| stem(file_name(p)).to_owned(),
                );
                let base = if base.is_empty() {
                    flavor.untitled().to_owned()
                } else {
                    base
                };
                let sheet_suffix = if self.workbook.sheets.len() > 1 {
                    format!(" - {}", self.sheet_ref().name)
                } else {
                    String::new()
                };
                let path = format!("{}/{base}{sheet_suffix}.csv", self.folder.path);
                return Ok(self.save_to(window, path, flavor, true));
            }
            "savexlsx" => {
                self.commit_edit();
                let base = self.path.as_deref().map_or_else(
                    || stem(&self.name).to_owned(),
                    |p| stem(file_name(p)).to_owned(),
                );
                let base = if base.is_empty() {
                    flavor.untitled().to_owned()
                } else {
                    base
                };
                let path = format!("{}/{base}.xlsx", self.folder.path);
                return Ok(self.save_to(window, path, flavor, false));
            }
            other => return Err(format!("unknown spreadsheet command {other}")),
        }
        Ok(vec![])
    }
    fn chart_title(&self, range: Range) -> String {
        // A single series is titled after its header, as Excel does.
        let header = self
            .workbook
            .value(self.sheet, Cell::new(range.start.row, range.end.col));
        match header {
            cw_sheet::Value::Text(t) if range.cols() <= 2 => t,
            _ => "Chart Title".into(),
        }
    }
    pub fn click(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
        flavor: Flavor,
    ) -> Result<Vec<AppEffect>, String> {
        let command = target
            .strip_prefix("sheet:")
            .ok_or("interaction does not belong to the spreadsheet")?;
        if Self::drags(target) {
            // A plain click on a drag surface is a press and release in one place; the
            // shell delivers those through `pointer`, so a click here is only focus.
            return Ok(vec![]);
        }
        self.command(window, command, clock_us, flavor)
    }
    /// Double-click: edit the cell (or rename the sheet tab).
    pub fn activate(
        &mut self,
        window: u64,
        target: &str,
        clock_us: u64,
        flavor: Flavor,
    ) -> Result<Vec<AppEffect>, String> {
        if target.starts_with("sheet:grid:") {
            let input = self.workbook.input(self.sheet, self.active);
            self.start_edit(input, false, false);
            return Ok(vec![]);
        }
        if let Some(i) = target.strip_prefix("sheet:tab:") {
            return self.command(window, &format!("rename:{i}"), clock_us, flavor);
        }
        if let Some(name) = target.strip_prefix("sheet:openfile:") {
            return self.command(window, &format!("openfile:{name}"), clock_us, flavor);
        }
        self.click(window, target, clock_us, flavor)
    }
}
/// Parentheses a formula opened and never closed, outside string literals.
fn unclosed_parens(text: &str) -> usize {
    let mut depth = 0usize;
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if quoted {
        0
    } else {
        depth
    }
}
/// The range a fill handle dragged to `to` covers: the source extended in the one
/// direction the drag went furthest.
fn fill_target(source: Range, to: Cell) -> Range {
    let down = i64::from(to.row) - i64::from(source.end.row);
    let up = i64::from(source.start.row) - i64::from(to.row);
    let right = i64::from(to.col) - i64::from(source.end.col);
    let left = i64::from(source.start.col) - i64::from(to.col);
    let best = down.max(up).max(right).max(left);
    if best <= 0 {
        return source;
    }
    if best == down {
        Range::new(source.start, Cell::new(to.row, source.end.col))
    } else if best == up {
        Range::new(Cell::new(to.row, source.start.col), source.end)
    } else if best == right {
        Range::new(source.start, Cell::new(source.end.row, to.col))
    } else {
        Range::new(Cell::new(source.start.row, to.col), source.end)
    }
}
fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}
/// One more or one fewer decimal place in a number format code.
fn change_decimals(code: &str, more: bool) -> String {
    let sections: Vec<String> = code
        .split(';')
        .map(|sec| {
            // The last run of digit placeholders is the one that changes.
            let chars: Vec<char> = sec.chars().collect();
            let Some(last) = chars.iter().rposition(|c| matches!(c, '0' | '#')) else {
                return sec.to_owned();
            };
            let point = chars[..=last].iter().rposition(|c| *c == '.');
            let mut out: String;
            if more {
                out = chars[..=last].iter().collect();
                if point.is_none() {
                    out.push('.');
                }
                out.push('0');
            } else {
                match point {
                    Some(p) if last > p + 1 => out = chars[..last].iter().collect(),
                    Some(p) => out = chars[..p].iter().collect(),
                    None => out = chars[..=last].iter().collect(),
                }
            }
            out.extend(&chars[last + 1..]);
            out
        })
        .collect();
    sections.join(";")
}

macro_rules! spreadsheet_app {
    ($name:ident, $kind:literal, $excel:expr) => {
        #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Box<Book>);
        impl $name {
            pub const KIND: &'static str = $kind;
            pub fn launch(argument: &str, window: u64, clock_us: u64) -> (Self, Vec<AppEffect>) {
                let (book, effects) = Book::launch(argument, window, clock_us, $excel);
                (Self(Box::new(book)), effects)
            }
            pub fn kind(&self) -> &'static str {
                Self::KIND
            }
            pub fn title(&self, theme: DesktopTheme) -> String {
                self.0.window_title(theme)
            }
            pub fn document(&self) -> String {
                self.0.document()
            }
            pub fn caption(&self) -> String {
                self.0.caption()
            }
            pub fn modified(&self) -> bool {
                self.0.modified
            }
            pub fn text(&mut self, text: &str) -> Result<(), String> {
                self.0.text(text)
            }
            pub fn key(
                &mut self,
                window: u64,
                key: &str,
                clock_us: u64,
            ) -> Result<Vec<AppEffect>, String> {
                let flavor = self.0.product_flavor();
                self.0.key(window, key, clock_us, Some(flavor))
            }
            pub fn click(
                &mut self,
                window: u64,
                target: &str,
                clock_us: u64,
            ) -> Result<Vec<AppEffect>, String> {
                let flavor = self.0.product_flavor();
                self.0.click(window, target, clock_us, flavor)
            }
            pub fn activate(
                &mut self,
                window: u64,
                target: &str,
                clock_us: u64,
            ) -> Result<Vec<AppEffect>, String> {
                let flavor = self.0.product_flavor();
                self.0.activate(window, target, clock_us, flavor)
            }
            pub fn http(
                &mut self,
                _window: u64,
                _tag: &str,
                _status: u16,
                _body: &str,
            ) -> Result<Vec<AppEffect>, String> {
                Err("spreadsheets are local files and make no requests".into())
            }
            pub fn offline(&mut self, tag: &str, reason: &str) {
                if tag == "listing" {
                    self.0.listing_failed(reason);
                } else {
                    self.0.message = Some(reason.to_owned());
                }
            }
            pub fn page(&self, page: &mut cw_protocol::Page) {
                self.0.page(page)
            }
            pub fn render(&self, p: &mut crate::desktop_scene::Painter, env: &crate::AppEnv<'_>) {
                chrome::render(&self.0, p, env)
            }
        }
        impl std::ops::Deref for $name {
            type Target = Book;
            fn deref(&self) -> &Book {
                &self.0
            }
        }
        impl std::ops::DerefMut for $name {
            fn deref_mut(&mut self) -> &mut Book {
                &mut self.0
            }
        }
    };
}
spreadsheet_app!(Spreadsheet, "spreadsheet", false);
spreadsheet_app!(Excel, "excel", true);

impl Book {
    /// The product that decides a new file's name and format.
    pub fn product_flavor(&self) -> Flavor {
        match self.product.as_deref().and_then(Flavor::parse) {
            Some(f) => f,
            None => Flavor::Excel,
        }
    }
    /// Semantic projection: the sheet as text, and the commands an agent can call.
    pub fn page(&self, page: &mut cw_protocol::Page) {
        use cw_protocol::PageElement as E;
        let act = |url: &str| cw_protocol::PageAction {
            method: "APP".into(),
            url: url.into(),
            fields: Default::default(),
        };
        page.elements.push(E::Heading {
            id: "sheet-title".into(),
            text: if self.name.is_empty() {
                "Spreadsheet".into()
            } else {
                self.name.clone()
            },
            level: 2,
        });
        if let Some(m) = &self.message {
            page.elements.push(E::Text {
                id: "sheet-message".into(),
                text: m.clone(),
            });
        }
        if self.browsing {
            page.elements.push(E::Button {
                id: "sheet:new".into(),
                text: "Blank workbook".into(),
                action: act("sheet:new"),
            });
            for e in &self.folder.entries {
                if !e.ends_with('/') {
                    let id = format!("sheet:openfile:{e}");
                    page.elements.push(E::Button {
                        id: id.clone(),
                        text: e.clone(),
                        action: act(&id),
                    });
                }
            }
            return;
        }
        let tabs: Vec<String> = self
            .workbook
            .sheets
            .iter()
            .map(|s| s.name.clone())
            .collect();
        for (i, t) in tabs.iter().enumerate() {
            let id = format!("sheet:tab:{i}");
            page.elements.push(E::Button {
                id: id.clone(),
                text: t.clone(),
                action: act(&id),
            });
        }
        page.elements.push(E::Input {
            id: "sheet-formula".into(),
            label: self.active.a1(),
            value: match &self.editing {
                Some(e) => e.text.clone(),
                None => self.workbook.input(self.sheet, self.active),
            },
            placeholder: String::new(),
        });
        page.elements.push(E::Text {
            id: "sheet-selection".into(),
            text: self.selection().a1(),
        });
        if let Some(u) = self.workbook.used_range(self.sheet) {
            let mut lines = Vec::new();
            for r in 0..=u.end.row.min(199) {
                let row: Vec<String> = (0..=u.end.col.min(25))
                    .map(|c| self.workbook.display(self.sheet, Cell::new(r, c)))
                    .collect();
                lines.push(format!("{}\t{}", r + 1, row.join("\t")));
            }
            page.elements.push(E::Text {
                id: "sheet-cells".into(),
                text: lines.join("\n"),
            });
        }
        for (id, label) in [
            ("sheet:save", "Save"),
            ("sheet:undo", "Undo"),
            ("sheet:redo", "Redo"),
            ("sheet:autosum", "AutoSum"),
            ("sheet:sort:asc", "Sort A to Z"),
            ("sheet:sort:desc", "Sort Z to A"),
            ("sheet:filter", "Filter"),
            ("sheet:chart:column", "Insert column chart"),
            ("sheet:insert:sheet", "New sheet"),
        ] {
            page.elements.push(E::Button {
                id: id.into(),
                text: label.into(),
                action: act(id),
            });
        }
    }
}
