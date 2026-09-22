/// Where a pointer is in a press on an application's drag surface.
/// `delta` pixels of wheel turn as whole steps of `unit` pixels, never zero for a turn
/// that moved: a small trackpad nudge still moves a row.
pub fn wheel_steps(delta: i32, unit: i32) -> i32 {
    if delta == 0 {
        return 0;
    }
    let steps = delta / unit.max(1);
    if steps == 0 {
        delta.signum()
    } else {
        steps
    }
}
/// Height of one terminal line, so a wheel notch walks the scrollback by whole lines.
pub(super) const TERMINAL_LINE: i32 = 19;
/// One turn of a pointer wheel (or a trackpad scroll), in pixels as a browser reports
/// them: positive `dy` rolls towards the user and moves content up, 120 per notch.
/// `shift` and `ctrl` are the modifiers held, which applications give meanings of
/// their own (Shift scrolls sideways, Ctrl zooms).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Wheel {
    pub dx: i32,
    pub dy: i32,
    pub shift: bool,
    pub ctrl: bool,
}
impl Wheel {
    pub fn vertical(dy: i32) -> Self {
        Self {
            dy,
            ..Self::default()
        }
    }
    /// The vertical turn as whole steps of `unit` pixels (see `wheel_steps`).
    pub fn lines(&self, unit: i32) -> i32 {
        wheel_steps(self.dy, unit)
    }
    /// The turn along the axis Shift selects: a vertical wheel scrolls sideways with
    /// Shift held, as on every desktop.
    pub fn horizontal(&self) -> i32 {
        if self.dx != 0 {
            self.dx
        } else if self.shift {
            self.dy
        } else {
            0
        }
    }
}
/// Byte offset in `text` nearest the point `(dx, dy)` of a monospace text view whose
/// topmost visible line is `first`. The 8x18 cell is the editor's painted grid.
pub fn caret_for_point(text: &str, first: usize, dx: i32, dy: i32) -> usize {
    caret_for_point_wrapped(text, first, 0, dx, dy)
}
/// The rows an editor paints: byte ranges into `text`, one per visual row. With
/// `columns` > 0 a line wider than that many cells is soft-wrapped, after the
/// last space that fits or, in a word longer than the row, at the edge. Cells are
/// `Primitive::Text`'s: a wide character (CJK, emoji) takes two, a combining mark
/// none. A row that ends where the next begins is a soft wrap; a newline sits
/// between the others.
pub fn editor_rows(text: &str, columns: usize) -> Vec<(usize, usize)> {
    let mut rows = Vec::new();
    let mut start = 0;
    for line in text.split('\n') {
        let end = start + line.len();
        let mut from = start;
        loop {
            let rest = &text[from..end];
            // The first row of `rest` in cells; a cluster is never split.
            let cut = cw_scene::text::terminal::wrap(rest, columns)
                .first()
                .map(|row| row.end)
                .filter(|&cut| columns > 0 && cut < rest.len());
            match cut {
                None => {
                    rows.push((from, end));
                    break;
                }
                Some(cut) => {
                    let brk = rest[..cut].rfind(' ').map_or(cut, |space| space + 1);
                    rows.push((from, from + brk));
                    from += brk;
                }
            }
        }
        start = end + 1;
    }
    rows
}
/// Visual (row, column) of the caret at byte `cursor` among `editor_rows`. At a soft
/// wrap the caret belongs to the start of the next row, as it does in a real editor.
pub fn editor_caret_cell(text: &str, cursor: usize, columns: usize) -> (usize, usize) {
    let mut cursor = cursor.min(text.len());
    while !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    let rows = editor_rows(text, columns);
    for (i, (start, end)) in rows.iter().enumerate() {
        let soft = rows.get(i + 1).is_some_and(|(next, _)| next == end);
        if cursor >= *start && (cursor < *end || (cursor == *end && !soft)) {
            return (i, cw_scene::text::terminal::columns(&text[*start..cursor]));
        }
    }
    let (start, end) = rows[rows.len() - 1];
    (
        rows.len() - 1,
        cw_scene::text::terminal::columns(&text[start..end]),
    )
}
/// `caret_for_point` on soft-wrapped rows `columns` characters wide (0: no wrap).
pub fn caret_for_point_wrapped(
    text: &str,
    first: usize,
    columns: usize,
    dx: i32,
    dy: i32,
) -> usize {
    const CELL_W: i32 = 8;
    const LINE_H: i32 = 18;
    let row = first + (dy.max(0) / LINE_H) as usize;
    let column = ((dx.max(0) + CELL_W / 2) / CELL_W) as usize;
    let rows = editor_rows(text, columns);
    let Some(&(start, end)) = rows.get(row) else {
        return text.len();
    };
    let content = &text[start..end];
    match cw_scene::text::terminal::byte_at_column(content, column) {
        Some(i) => start + i,
        // Past the end of a soft-wrapped row: the caret stays on this row, before its
        // last character, rather than jumping to the start of the next one.
        None if rows.get(row + 1).is_some_and(|(next, _)| *next == end) => content
            .char_indices()
            .next_back()
            .map_or(start, |(i, _)| start + i),
        None => end,
    }
}
/// Byte offset in a one-line monospace field for a click `dx` pixels from its first
/// character, on the same 8px cell the terminal paints.
pub fn caret_for_column(text: &str, dx: i32) -> usize {
    const CELL_W: i32 = 8;
    let column = ((dx.max(0) + CELL_W / 2) / CELL_W) as usize;
    // Cells, not characters: a wide character spans two, a combining mark none.
    cw_scene::text::terminal::byte_at_column(text, column).unwrap_or(text.len())
}
pub(super) fn parent_folder(path: &str) -> &str {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) | None => "/",
        Some((parent, _)) => parent,
    }
}
