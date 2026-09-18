//! The editor's text model: a document with a caret and an anchor (a selection is the
//! span between them), and an undo history of reversible edits rather than snapshots,
//! so a long session's history is bounded by what was typed, not by file size.
use super::syntax::Language;
use serde::{Deserialize, Serialize};

/// Undo depth. VS Code keeps more, but a snapshot must not grow without bound.
const UNDO_LIMIT: usize = 200;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edit {
    pub at: usize,
    pub removed: String,
    pub inserted: String,
    /// (cursor, anchor) before and after the edit.
    pub before: (usize, usize),
    pub after: (usize, usize),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Doc {
    pub text: String,
    pub cursor: usize,
    pub anchor: usize,
    /// Column Up and Down aim for, kept across shorter lines.
    #[serde(default)]
    pub goal: Option<usize>,
    #[serde(default)]
    pub undo: Vec<Edit>,
    #[serde(default)]
    pub redo: Vec<Edit>,
    /// The last edit was a typed character that later ones may join, as VS Code groups
    /// a run of typing into one undo step.
    #[serde(default)]
    pub typing: bool,
}

/// Search options shared by the find widget and the Search view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindOptions {
    pub case: bool,
    pub word: bool,
    pub regex: bool,
}

pub fn is_word(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// Every match of `query` in `text` as byte ranges. An invalid regex is an error the
/// widget shows, never a silent empty result.
pub fn find_all(text: &str, query: &str, o: FindOptions) -> Result<Vec<(usize, usize)>, String> {
    if query.is_empty() {
        return Ok(vec![]);
    }
    let pattern = if o.regex {
        query.to_owned()
    } else {
        regex::escape(query)
    };
    let pattern = if o.word {
        format!(r"\b(?:{pattern})\b")
    } else {
        pattern
    };
    let re = regex::RegexBuilder::new(&pattern)
        .case_insensitive(!o.case)
        .multi_line(true)
        .size_limit(1 << 20)
        .build()
        .map_err(|e| {
            e.to_string()
                .lines()
                .last()
                .unwrap_or("invalid regular expression")
                .trim()
                .to_owned()
        })?;
    Ok(re
        .find_iter(text)
        .filter(|m| m.end() > m.start())
        .take(10_000)
        .map(|m| (m.start(), m.end()))
        .collect())
}

impl Doc {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
    pub fn selection(&self) -> (usize, usize) {
        (self.cursor.min(self.anchor), self.cursor.max(self.anchor))
    }
    pub fn has_selection(&self) -> bool {
        self.cursor != self.anchor
    }
    pub fn selected_text(&self) -> &str {
        let (a, b) = self.selection();
        &self.text[a..b]
    }
    /// Clamp positions onto character boundaries, so a restored or hand-built state
    /// can never slice through a UTF-8 sequence.
    pub fn sanitize(&mut self) {
        let fix = |text: &str, mut p: usize| {
            p = p.min(text.len());
            while !text.is_char_boundary(p) {
                p -= 1;
            }
            p
        };
        self.cursor = fix(&self.text, self.cursor);
        self.anchor = fix(&self.text, self.anchor);
    }
    pub fn line_start(&self, pos: usize) -> usize {
        self.text[..pos].rfind('\n').map_or(0, |i| i + 1)
    }
    pub fn line_end(&self, pos: usize) -> usize {
        self.text[pos..]
            .find('\n')
            .map_or(self.text.len(), |i| pos + i)
    }
    pub fn line_of(&self, pos: usize) -> usize {
        self.text[..pos].bytes().filter(|b| *b == b'\n').count()
    }
    pub fn line_count(&self) -> usize {
        self.text.bytes().filter(|b| *b == b'\n').count() + 1
    }
    /// Byte range of line `n` (0-based), without its newline; clamped to the last line.
    pub fn line_range(&self, n: usize) -> (usize, usize) {
        let mut start = 0;
        for _ in 0..n {
            match self.text[start..].find('\n') {
                Some(i) => start += i + 1,
                None => break,
            }
        }
        (start, self.line_end(start))
    }
    /// Column of `pos` in characters.
    pub fn col_of(&self, pos: usize) -> usize {
        self.text[self.line_start(pos)..pos].chars().count()
    }
    /// 0-based (line, column) of the caret.
    pub fn position(&self) -> (usize, usize) {
        (self.line_of(self.cursor), self.col_of(self.cursor))
    }
    fn at_col(&self, line_start: usize, col: usize) -> usize {
        let end = self.line_end(line_start);
        self.text[line_start..end]
            .char_indices()
            .nth(col)
            .map_or(end, |(i, _)| line_start + i)
    }
    fn prev(&self, pos: usize) -> usize {
        self.text[..pos]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    }
    fn next(&self, pos: usize) -> usize {
        self.text[pos..]
            .chars()
            .next()
            .map_or(pos, |c| pos + c.len_utf8())
    }
    fn char_before(&self, pos: usize) -> Option<char> {
        self.text[..pos].chars().next_back()
    }
    fn char_at(&self, pos: usize) -> Option<char> {
        self.text[pos..].chars().next()
    }

    /// Replace `start..end` with `inserted`, leaving the caret at `(cursor, anchor)`.
    fn apply(
        &mut self,
        start: usize,
        end: usize,
        inserted: &str,
        after: (usize, usize),
        typing: bool,
    ) {
        let before = (self.cursor, self.anchor);
        let removed = self.text[start..end].to_owned();
        self.text.replace_range(start..end, inserted);
        self.cursor = after.0;
        self.anchor = after.1;
        self.goal = None;
        self.redo.clear();
        if typing && self.typing {
            if let Some(last) = self.undo.last_mut() {
                if last.removed.is_empty()
                    && removed.is_empty()
                    && last.at + last.inserted.len() == start
                    && !last.inserted.ends_with('\n')
                {
                    last.inserted.push_str(inserted);
                    last.after = after;
                    return;
                }
            }
        }
        self.undo.push(Edit {
            at: start,
            removed,
            inserted: inserted.to_owned(),
            before,
            after,
        });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.typing = typing;
    }
    /// Replace the selection (or insert at the caret) with `s`, caret after it.
    pub fn insert(&mut self, s: &str) {
        let (a, b) = self.selection();
        let end = a + s.len();
        self.apply(a, b, s, (end, end), false);
    }
    /// A character typed at the keyboard: closes brackets and quotes, types over a
    /// closing character that is already there, and surrounds a selection.
    pub fn type_char(&mut self, c: char, lang: Language) {
        let closing = match c {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            '"' | '\'' | '`' => Some(c),
            _ => None,
        };
        let quote = matches!(c, '"' | '\'' | '`');
        let auto = lang != Language::PlainText
            && !(quote && lang == Language::Markdown)
            && !(c == '\'' && lang == Language::Rust)
            && !(c == '`'
                && !matches!(
                    lang,
                    Language::JavaScript
                        | Language::TypeScript
                        | Language::Markdown
                        | Language::Shell
                ));
        let (a, b) = self.selection();
        if let (Some(close), true, true) = (closing, auto, a != b) {
            let inner = self.text[a..b].to_owned();
            let wrapped = format!("{c}{inner}{close}");
            let start = a + c.len_utf8();
            self.apply(a, b, &wrapped, (start + inner.len(), start), false);
            return;
        }
        if a == b
            && matches!(c, ')' | ']' | '}' | '"' | '\'' | '`')
            && self.char_at(a) == Some(c)
            && auto
        {
            let next = self.next(a);
            self.cursor = next;
            self.anchor = next;
            self.typing = false;
            return;
        }
        if let (Some(close), true) = (closing, auto) {
            let next_ok = self.char_at(b).is_none_or(|n| {
                n.is_whitespace() || matches!(n, ')' | ']' | '}' | ',' | ';' | ':')
            });
            let prev_ok = !quote || self.char_before(a).is_none_or(|p| !is_word(p) && p != c);
            if next_ok && prev_ok {
                let pair = format!("{c}{close}");
                let caret = a + c.len_utf8();
                self.apply(a, b, &pair, (caret, caret), false);
                return;
            }
        }
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        let end = a + s.len();
        self.apply(a, b, s, (end, end), a == b);
    }
    /// Enter: keep the line's indentation, indent once more after an opener, and put a
    /// closing bracket that was right after the caret on its own line.
    pub fn newline(&mut self, lang: Language, tab: usize) {
        let (a, b) = self.selection();
        let start = self.line_start(a);
        let indent: String = self.text[start..a]
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let before = self.text[start..a].trim_end();
        let opener = before.chars().next_back();
        let deeper = matches!(opener, Some('{' | '[' | '('))
            || (lang == Language::Python && opener == Some(':'))
            || (lang == Language::Shell
                && (before.ends_with(" then")
                    || before.ends_with(" do")
                    || before == "do"
                    || before == "then"));
        let unit = " ".repeat(tab.max(1));
        let between = matches!(
            (opener, self.char_at(b)),
            (Some('{'), Some('}')) | (Some('['), Some(']')) | (Some('('), Some(')'))
        );
        if between {
            let inserted = format!("\n{indent}{unit}\n{indent}");
            let caret = a + 1 + indent.len() + unit.len();
            self.apply(a, b, &inserted, (caret, caret), false);
            return;
        }
        let inserted = format!("\n{indent}{}", if deeper { unit.as_str() } else { "" });
        let caret = a + inserted.len();
        self.apply(a, b, &inserted, (caret, caret), false);
    }
    /// Backspace: a selection goes; an empty pair goes together; inside indentation it
    /// removes back to the previous tab stop, as `editor.useTabStops` does.
    pub fn backspace(&mut self, tab: usize) {
        let (a, b) = self.selection();
        if a != b {
            self.apply(a, b, "", (a, a), false);
            return;
        }
        if a == 0 {
            return;
        }
        let prev = self.prev(a);
        let pair = matches!(
            (self.char_before(a), self.char_at(a)),
            (Some('('), Some(')'))
                | (Some('['), Some(']'))
                | (Some('{'), Some('}'))
                | (Some('"'), Some('"'))
                | (Some('\''), Some('\''))
        );
        if pair {
            let next = self.next(a);
            self.apply(prev, next, "", (prev, prev), false);
            return;
        }
        let start = self.line_start(a);
        let lead = &self.text[start..a];
        if !lead.is_empty() && lead.bytes().all(|c| c == b' ') && tab > 0 {
            let col = lead.len();
            let back = match col % tab {
                0 => tab,
                r => r,
            };
            let from = a - back.min(col);
            self.apply(from, a, "", (from, from), false);
            return;
        }
        self.apply(prev, a, "", (prev, prev), false);
    }
    pub fn delete(&mut self) {
        let (a, b) = self.selection();
        if a != b {
            self.apply(a, b, "", (a, a), false);
        } else if a < self.text.len() {
            let next = self.next(a);
            self.apply(a, next, "", (a, a), false);
        }
    }
    pub fn delete_word_left(&mut self) {
        if self.has_selection() {
            return self.delete();
        }
        let to = self.word_left(self.cursor);
        let at = self.cursor;
        self.apply(to, at, "", (to, to), false);
    }
    pub fn delete_word_right(&mut self) {
        if self.has_selection() {
            return self.delete();
        }
        let to = self.word_right(self.cursor);
        let at = self.cursor;
        self.apply(at, to, "", (at, at), false);
    }
    /// Lines the selection touches, as the byte range from the first line's start to the
    /// last line's end. A selection ending at column 0 does not take that line.
    fn line_block(&self) -> (usize, usize) {
        let (a, mut b) = self.selection();
        if b > a && self.line_start(b) == b {
            b -= 1;
        }
        (self.line_start(a), self.line_end(b))
    }
    /// Rewrite every touched line with `f`, keeping caret and anchor on the same text.
    fn map_lines(&mut self, f: impl Fn(usize, &str) -> String) {
        let (start, end) = self.line_block();
        let old = self.text[start..end].to_owned();
        let new: Vec<String> = old.split('\n').enumerate().map(|(i, l)| f(i, l)).collect();
        let new = new.join("\n");
        // Map a position through the per-line change, clamped to its line.
        let remap = |pos: usize| -> usize {
            if pos < start {
                return pos;
            }
            let rel = pos - start;
            let mut old_off = 0;
            let mut new_off = 0;
            let old_lines: Vec<&str> = old.split('\n').collect();
            let new_lines: Vec<&str> = new.split('\n').collect();
            for (i, line) in old_lines.iter().enumerate() {
                let len = line.len();
                if rel <= old_off + len {
                    let col = rel - old_off;
                    let delta = new_lines[i].len() as isize - len as isize;
                    let lead = line.len() - line.trim_start().len();
                    let moved = if col >= lead {
                        (col as isize + delta).max(0) as usize
                    } else {
                        col
                    };
                    let mut p = new_off + moved.min(new_lines[i].len());
                    while !new.is_char_boundary(p) {
                        p -= 1;
                    }
                    return start + p;
                }
                old_off += len + 1;
                new_off += new_lines[i].len() + 1;
            }
            start + new.len()
        };
        let after = (remap(self.cursor), remap(self.anchor));
        if new != old {
            self.apply(start, end, &new, after, false);
        }
    }
    /// Tab: indent every selected line, or insert spaces to the next tab stop.
    pub fn indent(&mut self, tab: usize) {
        let tab = tab.max(1);
        let (a, b) = self.selection();
        if a != b && self.line_of(a) != self.line_of(b) {
            let unit = " ".repeat(tab);
            self.map_lines(|_, l| {
                if l.is_empty() {
                    String::new()
                } else {
                    format!("{unit}{l}")
                }
            });
            return;
        }
        let col = self.col_of(a);
        let spaces = " ".repeat(tab - col % tab);
        let end = a + spaces.len();
        self.apply(a, b, &spaces, (end, end), false);
    }
    /// Shift+Tab: remove one level of indentation from every touched line.
    pub fn outdent(&mut self, tab: usize) {
        let tab = tab.max(1);
        self.map_lines(|_, l| {
            if let Some(rest) = l.strip_prefix('\t') {
                return rest.to_owned();
            }
            let n = l.bytes().take(tab).take_while(|c| *c == b' ').count();
            l[n..].to_owned()
        });
    }
    /// Ctrl+/: comment or uncomment the touched lines with the language's line token,
    /// or wrap them in its block comment when it has none.
    pub fn toggle_comment(&mut self, lang: Language) {
        if let Some(token) = lang.line_comment() {
            let (start, end) = self.line_block();
            let lines: Vec<&str> = self.text[start..end].split('\n').collect();
            let body: Vec<&&str> = lines.iter().filter(|l| !l.trim().is_empty()).collect();
            let commented =
                !body.is_empty() && body.iter().all(|l| l.trim_start().starts_with(token));
            let indent = body
                .iter()
                .map(|l| l.len() - l.trim_start().len())
                .min()
                .unwrap_or(0);
            let token = token.to_owned();
            self.map_lines(move |_, l| {
                if l.trim().is_empty() {
                    return l.to_owned();
                }
                if commented {
                    let lead = l.len() - l.trim_start().len();
                    let rest = &l[lead + token.len()..];
                    let rest = rest.strip_prefix(' ').unwrap_or(rest);
                    format!("{}{rest}", &l[..lead])
                } else {
                    let cut = indent.min(l.len());
                    format!("{}{token} {}", &l[..cut], &l[cut..])
                }
            });
        } else if let Some((open, close)) = lang.block_comment() {
            let (start, end) = self.line_block();
            let block = self.text[start..end].to_owned();
            let lead = block.len() - block.trim_start().len();
            let inner = block.trim();
            let new =
                if let Some(body) = inner.strip_prefix(open).and_then(|b| b.strip_suffix(close)) {
                    format!("{}{}", &block[..lead], body.trim())
                } else {
                    format!("{}{open} {inner} {close}", &block[..lead])
                };
            let caret = start + new.len();
            self.apply(start, end, &new, (caret, caret), false);
        }
    }
    /// Alt+Up / Alt+Down: move the touched lines past their neighbour.
    pub fn move_lines(&mut self, up: bool) {
        let (start, end) = self.line_block();
        let (cur, anc) = (self.cursor - start, self.anchor - start);
        if up {
            if start == 0 {
                return;
            }
            let prev_start = self.line_start(start - 1);
            let above = self.text[prev_start..start - 1].to_owned();
            let block = self.text[start..end].to_owned();
            let new = format!("{block}\n{above}");
            self.apply(
                prev_start,
                end,
                &new,
                (prev_start + cur, prev_start + anc),
                false,
            );
        } else {
            if end >= self.text.len() {
                return;
            }
            let next_end = self.line_end(end + 1);
            let below = self.text[end + 1..next_end].to_owned();
            let block = self.text[start..end].to_owned();
            let new = format!("{below}\n{block}");
            let shift = below.len() + 1;
            self.apply(
                start,
                next_end,
                &new,
                (start + shift + cur, start + shift + anc),
                false,
            );
        }
    }
    /// Shift+Alt+Up / Down: duplicate the touched lines above or below.
    pub fn copy_lines(&mut self, up: bool) {
        let (start, end) = self.line_block();
        let block = self.text[start..end].to_owned();
        let (cur, anc) = (self.cursor, self.anchor);
        let inserted = format!("{block}\n");
        if up {
            self.apply(start, start, &inserted, (cur, anc), false);
        } else {
            let shift = inserted.len();
            self.apply(start, start, &inserted, (cur + shift, anc + shift), false);
        }
    }
    /// Ctrl+Shift+K.
    pub fn delete_lines(&mut self) {
        let (start, end) = self.line_block();
        let (from, to) = if end < self.text.len() {
            (start, end + 1)
        } else if start > 0 {
            (start - 1, end)
        } else {
            (start, end)
        };
        let caret = from.min(self.text.len() - (to - from));
        self.apply(from, to, "", (caret, caret), false);
        let line_start = self.line_start(self.cursor.min(self.text.len()));
        self.cursor = line_start;
        self.anchor = line_start;
    }
    /// Ctrl+Enter: open a new line below without breaking this one.
    pub fn insert_line_below(&mut self, tab: usize, lang: Language) {
        let end = self.line_end(self.cursor);
        self.cursor = end;
        self.anchor = end;
        self.newline(lang, tab);
    }
    pub fn select_all(&mut self) {
        self.anchor = 0;
        self.cursor = self.text.len();
        self.typing = false;
    }
    /// Place the caret; `select` extends the selection from the anchor instead.
    pub fn set(&mut self, pos: usize, select: bool) {
        let mut pos = pos.min(self.text.len());
        while !self.text.is_char_boundary(pos) {
            pos -= 1;
        }
        self.cursor = pos;
        if !select {
            self.anchor = pos;
        }
        self.typing = false;
    }
    pub fn left(&mut self, select: bool) {
        self.goal = None;
        if self.has_selection() && !select {
            let (a, _) = self.selection();
            return self.set(a, false);
        }
        let p = self.prev(self.cursor);
        self.set(p, select);
    }
    pub fn right(&mut self, select: bool) {
        self.goal = None;
        if self.has_selection() && !select {
            let (_, b) = self.selection();
            return self.set(b, false);
        }
        let p = self.next(self.cursor);
        self.set(p, select);
    }
    pub fn vertical(&mut self, lines: isize, select: bool) {
        let goal = self.goal.unwrap_or_else(|| self.col_of(self.cursor));
        let line = self.line_of(self.cursor) as isize;
        let target = line + lines;
        let pos = if target < 0 {
            0
        } else if target as usize >= self.line_count() {
            self.text.len()
        } else {
            let (start, _) = self.line_range(target as usize);
            self.at_col(start, goal)
        };
        self.set(pos, select);
        self.goal = Some(goal);
    }
    /// Home goes to the first non-blank character, then to column 0.
    pub fn home(&mut self, select: bool) {
        let start = self.line_start(self.cursor);
        let end = self.line_end(self.cursor);
        let first = start
            + self.text[start..end]
                .char_indices()
                .find(|(_, c)| !c.is_whitespace())
                .map_or(end - start, |(i, _)| i);
        let pos = if self.cursor == first { start } else { first };
        self.set(pos, select);
    }
    pub fn end(&mut self, select: bool) {
        let pos = self.line_end(self.cursor);
        self.set(pos, select);
    }
    pub fn word_left(&self, pos: usize) -> usize {
        let mut p = pos;
        while p > 0 && self.char_before(p).is_some_and(char::is_whitespace) {
            p = self.prev(p);
        }
        let word = self.char_before(p).is_some_and(is_word);
        while p > 0 {
            let Some(c) = self.char_before(p) else { break };
            if c.is_whitespace() || is_word(c) != word {
                break;
            }
            p = self.prev(p);
        }
        p
    }
    pub fn word_right(&self, pos: usize) -> usize {
        let mut p = pos;
        while p < self.text.len() && self.char_at(p).is_some_and(char::is_whitespace) {
            p = self.next(p);
        }
        let word = self.char_at(p).is_some_and(is_word);
        while p < self.text.len() {
            let Some(c) = self.char_at(p) else { break };
            if c.is_whitespace() || is_word(c) != word {
                break;
            }
            p = self.next(p);
        }
        p
    }
    /// The word under the caret, for seeding a search.
    pub fn word_at_cursor(&self) -> String {
        let start = self.word_left(self.cursor.min(self.text.len()));
        let end = self.word_right(start);
        self.text[start..end].trim().to_owned()
    }
    /// Go to a 1-based line and column, as Ctrl+G takes them.
    pub fn goto(&mut self, line: usize, col: usize) {
        let (start, _) = self.line_range(line.saturating_sub(1));
        let pos = self.at_col(start, col.saturating_sub(1));
        self.set(pos, false);
    }
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop() else {
            return false;
        };
        self.text
            .replace_range(edit.at..edit.at + edit.inserted.len(), &edit.removed);
        (self.cursor, self.anchor) = edit.before;
        self.redo.push(edit);
        self.typing = false;
        self.goal = None;
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        self.text
            .replace_range(edit.at..edit.at + edit.removed.len(), &edit.inserted);
        (self.cursor, self.anchor) = edit.after;
        self.undo.push(edit);
        self.typing = false;
        self.goal = None;
        true
    }
    /// Replace the whole text as one undoable edit (a file reloaded, a replace-all).
    pub fn replace_all_text(&mut self, text: &str) {
        let len = self.text.len();
        let caret = self.cursor.min(text.len());
        self.apply(0, len, text, (caret, caret), false);
        self.sanitize();
    }
    /// Replace `range` with `with`, selecting nothing, caret after it.
    pub fn replace_range(&mut self, range: (usize, usize), with: &str) {
        let end = range.0 + with.len();
        self.apply(range.0, range.1, with, (end, end), false);
    }
    /// The bracket matching the one beside the caret, as a pair of byte offsets.
    pub fn matching_bracket(&self) -> Option<(usize, usize)> {
        let pos = self.cursor;
        let bytes = self.text.as_bytes();
        let candidates = [pos.checked_sub(1), Some(pos)];
        for at in candidates.into_iter().flatten() {
            let Some(&c) = bytes.get(at) else { continue };
            let (open, close, forward) = match c {
                b'(' => (b'(', b')', true),
                b'[' => (b'[', b']', true),
                b'{' => (b'{', b'}', true),
                b')' => (b'(', b')', false),
                b']' => (b'[', b']', false),
                b'}' => (b'{', b'}', false),
                _ => continue,
            };
            let mut depth = 0i32;
            if forward {
                for (i, &b) in bytes.iter().enumerate().skip(at) {
                    if b == open {
                        depth += 1;
                    } else if b == close {
                        depth -= 1;
                        if depth == 0 {
                            return Some((at, i));
                        }
                    }
                }
            } else {
                for i in (0..=at).rev() {
                    let b = bytes[i];
                    if b == close {
                        depth += 1;
                    } else if b == open {
                        depth -= 1;
                        if depth == 0 {
                            return Some((i, at));
                        }
                    }
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const PY: Language = Language::Python;
    fn typed(doc: &mut Doc, s: &str) {
        for c in s.chars() {
            doc.type_char(c, PY);
        }
    }
    #[test]
    fn typing_groups_into_one_undo_step_and_redo_restores_it() {
        let mut d = Doc::new("");
        typed(&mut d, "hello");
        d.newline(PY, 4);
        typed(&mut d, "world");
        assert_eq!(d.text, "hello\nworld");
        assert!(d.undo());
        assert_eq!(d.text, "hello\n");
        assert!(d.undo());
        assert_eq!(d.text, "hello");
        assert!(d.undo());
        assert_eq!(d.text, "");
        assert!(!d.undo());
        assert!(d.redo());
        assert!(d.redo());
        assert!(d.redo());
        assert_eq!(d.text, "hello\nworld");
        assert_eq!(d.cursor, d.text.len());
    }
    #[test]
    fn brackets_close_and_are_typed_over_and_selections_are_surrounded() {
        let mut d = Doc::new("");
        typed(&mut d, "print(x)");
        assert_eq!(d.text, "print(x)");
        let mut d = Doc::new("name");
        d.select_all();
        d.type_char('"', PY);
        assert_eq!(d.text, "\"name\"");
        // Backspace between an empty pair takes both.
        let mut d = Doc::new("");
        d.type_char('[', PY);
        assert_eq!(d.text, "[]");
        d.backspace(4);
        assert_eq!(d.text, "");
    }
    #[test]
    fn enter_keeps_indentation_and_indents_after_openers() {
        let mut d = Doc::new("def f():");
        d.set(d.text.len(), false);
        d.newline(PY, 4);
        typed(&mut d, "return 1");
        assert_eq!(d.text, "def f():\n    return 1");
        d.newline(PY, 4);
        assert!(d.text.ends_with("\n    "));
        let mut d = Doc::new("fn a() {}");
        d.set(8, false);
        d.newline(Language::Rust, 4);
        assert_eq!(d.text, "fn a() {\n    \n}");
        assert_eq!(d.position(), (1, 4));
    }
    #[test]
    fn tab_indents_selected_lines_and_shift_tab_and_backspace_use_tab_stops() {
        let mut d = Doc::new("a\nb\nc");
        d.set(0, false);
        d.set(3, true);
        d.indent(4);
        assert_eq!(d.text, "    a\n    b\nc");
        d.outdent(4);
        assert_eq!(d.text, "a\nb\nc");
        let mut d = Doc::new("ab");
        d.set(1, false);
        d.indent(4);
        assert_eq!(d.text, "a   b");
        let mut d = Doc::new("        x");
        d.set(8, false);
        d.backspace(4);
        assert_eq!(d.text, "    x");
    }
    #[test]
    fn toggle_comment_round_trips_per_language() {
        let mut d = Doc::new("x = 1\n    y = 2");
        d.select_all();
        d.toggle_comment(PY);
        assert_eq!(d.text, "# x = 1\n#     y = 2");
        d.select_all();
        d.toggle_comment(PY);
        assert_eq!(d.text, "x = 1\n    y = 2");
        let mut d = Doc::new("let a = 1;");
        d.toggle_comment(Language::JavaScript);
        assert_eq!(d.text, "// let a = 1;");
        let mut d = Doc::new("  <p>hi</p>");
        d.toggle_comment(Language::Html);
        assert_eq!(d.text, "  <!-- <p>hi</p> -->");
        d.toggle_comment(Language::Html);
        assert_eq!(d.text, "  <p>hi</p>");
    }
    #[test]
    fn lines_move_copy_and_delete() {
        let mut d = Doc::new("one\ntwo\nthree");
        d.set(5, false);
        d.move_lines(true);
        assert_eq!(d.text, "two\none\nthree");
        assert_eq!(d.position(), (0, 1));
        d.move_lines(false);
        assert_eq!(d.text, "one\ntwo\nthree");
        d.copy_lines(false);
        assert_eq!(d.text, "one\ntwo\ntwo\nthree");
        assert_eq!(d.position().0, 2);
        d.delete_lines();
        assert_eq!(d.text, "one\ntwo\nthree");
    }
    #[test]
    fn caret_motion_selection_words_and_goto() {
        let mut d = Doc::new("alpha beta\n  gamma");
        d.set(0, false);
        d.set(d.word_right(0), true);
        assert_eq!(d.selected_text(), "alpha");
        d.vertical(1, false);
        assert_eq!(d.position(), (1, 5));
        d.home(false);
        assert_eq!(d.position(), (1, 2));
        d.home(false);
        assert_eq!(d.position(), (1, 0));
        d.goto(1, 7);
        assert_eq!(d.position(), (0, 6));
        d.end(true);
        assert_eq!(d.selected_text(), "beta");
        d.delete_word_left();
        assert_eq!(d.text, "alpha \n  gamma");
        // Up and down keep the goal column across a short line.
        let mut d = Doc::new("abcdef\nxy\nlonger line");
        d.set(d.text.len(), false);
        d.vertical(-1, false);
        d.vertical(-1, false);
        assert_eq!(d.position(), (0, 6));
    }
    #[test]
    fn find_is_literal_regex_case_and_word_aware() {
        let text = "Foo foo food FOO";
        let lit = FindOptions::default();
        assert_eq!(find_all(text, "foo", lit).unwrap().len(), 4);
        let case = FindOptions { case: true, ..lit };
        assert_eq!(find_all(text, "foo", case).unwrap(), vec![(4, 7), (8, 11)]);
        let word = FindOptions { word: true, ..lit };
        assert_eq!(find_all(text, "foo", word).unwrap().len(), 3);
        let re = FindOptions { regex: true, ..lit };
        assert_eq!(find_all(text, "fo+d", re).unwrap(), vec![(8, 12)]);
        assert_eq!(find_all("a.b", ".", lit).unwrap(), vec![(1, 2)]);
        assert!(find_all(text, "(", re).is_err());
    }
    #[test]
    fn bracket_matching_finds_the_partner() {
        let mut d = Doc::new("f(a[1], {b})");
        d.set(1, false);
        assert_eq!(d.matching_bracket(), Some((1, 11)));
        d.set(12, false);
        assert_eq!(d.matching_bracket(), Some((1, 11)));
        d.set(4, false);
        assert_eq!(d.matching_bracket(), Some((3, 5)));
    }
}
