//! A YAML parser for configuration documents, resolving to the blueprint's
//! order-preserving document model.

use crate::node::{Map, Node};
use std::collections::HashMap;

/// A parse failure, with the 1-based line it was found on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for YamlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for YamlError {}

/// Parse one YAML document.
pub fn parse(text: &str) -> Result<Node, YamlError> {
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|l| l.trim_end_matches('\r').to_owned())
        .collect();
    // `split` leaves a phantom empty line after a final newline, which a `|+` block
    // scalar would otherwise keep as content.
    if text.ends_with('\n') {
        lines.pop();
    }
    Parser {
        lines,
        i: 0,
        anchors: HashMap::new(),
        started: false,
        depth: 0,
    }
    .document()
}

/// Recursion is bounded so that a pathological document is refused rather than
/// overflowing the stack.
const MAX_DEPTH: usize = 128;

struct Parser {
    lines: Vec<String>,
    i: usize,
    anchors: HashMap<String, Node>,
    /// Whether the document proper has begun: a `---` after that point starts a
    /// second document rather than this one.
    started: bool,
    depth: usize,
}

/// A mapping under construction. Merge keys are held back until the end so that
/// explicit entries, and earlier merges, win over later ones.
#[derive(Default)]
struct MapBuilder {
    map: Map,
    merged: Vec<(Node, usize)>,
}

impl MapBuilder {
    fn insert(&mut self, key: String, value: Node, line: usize) -> Result<(), YamlError> {
        if self.map.contains_key(&key) {
            return Err(YamlError {
                line: line + 1,
                message: format!("duplicate mapping key {key}"),
            });
        }
        self.map.insert(key, value);
        Ok(())
    }
    fn merge(&mut self, value: Node, line: usize) {
        self.merged.push((value, line));
    }
    fn finish(mut self) -> Result<Node, YamlError> {
        for (value, line) in self.merged {
            let sources = match value {
                Node::List(items) => items,
                other => vec![other],
            };
            for source in sources {
                let Node::Map(entries) = source else {
                    return Err(YamlError {
                        line: line + 1,
                        message: "a merge key must name a mapping".to_owned(),
                    });
                };
                for (key, value) in entries {
                    if !self.map.contains_key(&key) {
                        self.map.insert(key, value);
                    }
                }
            }
        }
        Ok(Node::Map(self.map))
    }
}

impl Parser {
    fn err(&self, line: usize, message: impl Into<String>) -> YamlError {
        YamlError {
            line: line + 1,
            message: message.into(),
        }
    }

    fn document(&mut self) -> Result<Node, YamlError> {
        self.skip_blanks()?;
        self.started = true;
        if self.i >= self.lines.len() {
            return Ok(Node::Null);
        }
        let value = self.parse_block(0)?;
        self.skip_blanks()?;
        if self.i < self.lines.len() {
            return Err(self.err(self.i, "unexpected content after the document"));
        }
        Ok(value)
    }

    /// Advance past blank and comment lines, and over the document markers. A `---`
    /// once the document has begun starts a second document, which this parser
    /// refuses rather than silently dropping.
    fn skip_blanks(&mut self) -> Result<(), YamlError> {
        while self.i < self.lines.len() {
            let text = self.lines[self.i].trim_end();
            let body = text.trim_start();
            if body.is_empty() || body.starts_with('#') {
                self.i += 1;
                continue;
            }
            if text == "..." || text.starts_with("... ") {
                self.i = self.lines.len();
                break;
            }
            if text == "---" || text.starts_with("--- ") {
                if self.started {
                    return Err(self.err(self.i, "multiple documents are not supported"));
                }
                self.started = true;
                let tail = &text[3..];
                let at = 3 + tail.len() - tail.trim_start().len();
                let node = text[at..].to_owned();
                if node.is_empty() || node.starts_with('#') {
                    self.i += 1;
                    continue;
                }
                // Keep the node's column, so its block reads the same as it would
                // without the marker.
                self.lines[self.i] = " ".repeat(at) + &node;
            }
            break;
        }
        Ok(())
    }

    fn indent(&self, line: usize) -> Result<usize, YamlError> {
        let mut n = 0;
        for c in self.lines[line].chars() {
            match c {
                ' ' => n += 1,
                '\t' => return Err(self.err(line, "tabs cannot be used to indent")),
                _ => break,
            }
        }
        Ok(n)
    }

    fn rest(&self, line: usize, pos: usize) -> &str {
        &self.lines[line][pos..]
    }

    fn at(&self, pos: usize) -> Option<char> {
        self.lines.get(self.i)?.get(pos..)?.chars().next()
    }

    /// Whether everything after `pos` on `line` is blank or a comment.
    fn tail_ok(&self, line: usize, pos: usize) -> bool {
        let tail = self.lines[line].get(pos..).unwrap_or("").trim_start();
        tail.is_empty() || tail.starts_with('#')
    }

    fn parse_block(&mut self, min: usize) -> Result<Node, YamlError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err(self.i, "nesting is too deep"));
        }
        let value = self.parse_block_inner(min);
        self.depth -= 1;
        value
    }

    /// Parse a block node whose first content line is indented at least `min`.
    fn parse_block_inner(&mut self, min: usize) -> Result<Node, YamlError> {
        self.skip_blanks()?;
        if self.i >= self.lines.len() {
            return Ok(Node::Null);
        }
        let ind = self.indent(self.i)?;
        if ind < min {
            return Ok(Node::Null);
        }
        let content = self.rest(self.i, ind);
        if is_dash(content) {
            return self.parse_seq(ind);
        }
        if key_split(content).is_some() {
            return self.parse_map(ind);
        }
        self.parse_node(ind, ind, true)
    }

    fn parse_map(&mut self, indent: usize) -> Result<Node, YamlError> {
        let mut builder = MapBuilder::default();
        loop {
            self.skip_blanks()?;
            if self.i >= self.lines.len() {
                break;
            }
            let line = self.i;
            let ind = self.indent(line)?;
            if ind < indent {
                break;
            }
            if ind > indent {
                return Err(self.err(line, "unexpected indentation inside a mapping"));
            }
            if is_dash(self.rest(line, ind)) {
                return Err(self.err(
                    line,
                    "a sequence item cannot start where a mapping is open at the same indentation",
                ));
            }
            let Some(key) = key_split(self.rest(line, ind)) else {
                return Err(self.err(line, "expected a mapping key"));
            };
            let value = self.parse_node(ind + key.after, ind, true)?;
            if key.text == "<<" && !key.quoted {
                builder.merge(value, line);
            } else {
                let name = scalar_key(&key.text, key.quoted).map_err(|m| self.err(line, m))?;
                builder.insert(name, value, line)?;
            }
        }
        builder.finish()
    }

    fn parse_seq(&mut self, indent: usize) -> Result<Node, YamlError> {
        let mut items = Vec::new();
        loop {
            self.skip_blanks()?;
            if self.i >= self.lines.len() {
                break;
            }
            let line = self.i;
            let ind = self.indent(line)?;
            if ind < indent {
                break;
            }
            if ind > indent {
                return Err(self.err(line, "unexpected indentation inside a sequence"));
            }
            if !is_dash(self.rest(line, ind)) {
                break;
            }
            let tail = self.rest(line, ind + 1);
            let at = ind + 1 + tail.len() - tail.trim_start().len();
            let body = self.lines[line][at..].to_owned();
            if body.is_empty() || body.starts_with('#') {
                self.i += 1;
                items.push(self.after_dash(ind)?);
            } else if is_dash(&body) || key_split(&body).is_some() {
                // Blank the dash so the nested collection parses at its own column.
                self.lines[line] = " ".repeat(at) + &body;
                items.push(self.parse_block(at)?);
            } else {
                // A scalar item folds against the dash's indentation, not its column.
                items.push(self.parse_node(at, ind, false)?);
            }
        }
        Ok(Node::List(items))
    }

    /// The value of a `-` that carried nothing else on its line.
    fn after_dash(&mut self, indent: usize) -> Result<Node, YamlError> {
        self.skip_blanks()?;
        if self.i >= self.lines.len() {
            return Ok(Node::Null);
        }
        if self.indent(self.i)? > indent {
            return self.parse_block(indent + 1);
        }
        Ok(Node::Null)
    }

    /// The value of a `key:` that carried nothing else on its line. A block sequence
    /// may sit at the key's own indentation; a block mapping must be deeper.
    fn after_key(&mut self, owner: usize, sibling_seq: bool) -> Result<Node, YamlError> {
        self.skip_blanks()?;
        if self.i >= self.lines.len() {
            return Ok(Node::Null);
        }
        let ind = self.indent(self.i)?;
        if ind > owner {
            return self.parse_block(owner + 1);
        }
        if ind == owner && sibling_seq && is_dash(self.rest(self.i, ind)) {
            return self.parse_seq(owner);
        }
        Ok(Node::Null)
    }

    /// Parse the node starting at `pos` on the current line. `owner` is the
    /// indentation the node's continuation lines must exceed; `sibling_seq` allows a
    /// block sequence at `owner` itself, which is legal under a mapping key.
    fn parse_node(
        &mut self,
        pos: usize,
        owner: usize,
        sibling_seq: bool,
    ) -> Result<Node, YamlError> {
        let line = self.i;
        let lead = self.rest(line, pos);
        let pos = pos + lead.len() - lead.trim_start().len();
        let head = self.rest(line, pos).to_owned();
        let first = head.chars().next().unwrap_or('#');
        if first == '#' {
            self.i = line + 1;
            return self.after_key(owner, sibling_seq);
        }
        match first {
            '&' => {
                let name = token(&head);
                if name.len() == 1 {
                    return Err(self.err(line, "an anchor needs a name"));
                }
                let value = self.parse_node(pos + name.len(), owner, sibling_seq)?;
                self.anchors.insert(name[1..].to_owned(), value.clone());
                Ok(value)
            }
            '*' => {
                let name = token(&head);
                if !self.tail_ok(line, pos + name.len()) {
                    return Err(self.err(line, "unexpected content after an alias"));
                }
                self.i = line + 1;
                self.alias(&name[1..], line)
            }
            '|' | '>' => self.parse_block_scalar(pos, owner),
            '[' | '{' => {
                let mut end = pos;
                let value = self.parse_flow(&mut end)?;
                if !self.tail_ok(self.i, end) {
                    return Err(self.err(self.i, "unexpected content after a flow collection"));
                }
                self.i += 1;
                Ok(value)
            }
            _ => self.parse_scalar_node(pos, owner),
        }
    }

    fn alias(&self, name: &str, line: usize) -> Result<Node, YamlError> {
        self.anchors
            .get(name)
            .cloned()
            .ok_or_else(|| self.err(line, format!("unknown alias *{name}")))
    }

    /// A quoted scalar, or a plain one that may fold over the lines beneath it.
    fn parse_scalar_node(&mut self, pos: usize, owner: usize) -> Result<Node, YamlError> {
        let line = self.i;
        if matches!(self.at(pos), Some('\'') | Some('"')) {
            let mut end = pos;
            let text = self.scan_quoted(&mut end)?;
            if !self.tail_ok(self.i, end) {
                return Err(self.err(self.i, "unexpected content after a quoted scalar"));
            }
            self.i += 1;
            return Ok(Node::String(text));
        }
        let (mut text, end) = scan_plain(self.rest(line, pos), false);
        if !self.tail_ok(line, pos + end) {
            return Err(self.err(line, "a plain scalar cannot contain ': '; quote the value"));
        }
        self.i = line + 1;
        let mut breaks = 0;
        while self.i < self.lines.len() {
            if self.lines[self.i].trim().is_empty() {
                breaks += 1;
                self.i += 1;
                continue;
            }
            let ind = self.indent(self.i)?;
            if ind <= owner {
                break;
            }
            let content = self.rest(self.i, ind);
            if content.starts_with('#') || is_dash(content) || key_split(content).is_some() {
                break;
            }
            let (more, end) = scan_plain(content, false);
            if !self.tail_ok(self.i, ind + end) {
                return Err(self.err(
                    self.i,
                    "a plain scalar cannot contain ': '; quote the value",
                ));
            }
            // One line break folds to a space; further breaks survive as newlines.
            if breaks == 0 {
                text.push(' ');
            } else {
                text.push_str(&"\n".repeat(breaks));
            }
            text.push_str(&more);
            breaks = 0;
            self.i += 1;
        }
        typed_scalar(&text).map_err(|m| self.err(line, m))
    }

    fn parse_block_scalar(&mut self, pos: usize, owner: usize) -> Result<Node, YamlError> {
        let line = self.i;
        let header = self.rest(line, pos).to_owned();
        let folded = header.starts_with('>');
        let indicators = token(&header[1..]);
        let mut chomp = Chomp::Clip;
        let mut explicit = None;
        for c in indicators.chars() {
            match c {
                '-' => chomp = Chomp::Strip,
                '+' => chomp = Chomp::Keep,
                '1'..='9' => explicit = Some(c as usize - '0' as usize),
                _ => return Err(self.err(line, format!("invalid block scalar header {header}"))),
            }
        }
        if !self.tail_ok(line, pos + 1 + indicators.len()) {
            return Err(self.err(line, "unexpected content after a block scalar header"));
        }
        self.i = line + 1;
        let base = match explicit {
            Some(n) => owner + n,
            None => {
                let mut found = None;
                for j in self.i..self.lines.len() {
                    if self.lines[j].trim().is_empty() {
                        continue;
                    }
                    found = Some(self.indent(j)?);
                    break;
                }
                match found {
                    Some(n) if n > owner => n,
                    // Nothing is indented under the header, so the scalar is empty.
                    _ => return Ok(Node::String(String::new())),
                }
            }
        };
        let mut body: Vec<String> = Vec::new();
        while self.i < self.lines.len() {
            let text = &self.lines[self.i];
            if text.trim().is_empty() {
                body.push(String::new());
                self.i += 1;
                continue;
            }
            // Counted rather than measured by `indent`, because a tab past the
            // block's own indentation is content and not an indentation error.
            let ind = text.len() - text.trim_start_matches(' ').len();
            if ind < base {
                break;
            }
            body.push(text[base..].to_owned());
            self.i += 1;
        }
        let kept = if body.iter().all(|l| l.trim().is_empty()) {
            String::new()
        } else if folded {
            fold_body(&body)
        } else {
            body.join("\n") + "\n"
        };
        Ok(Node::String(match chomp {
            Chomp::Keep => kept,
            Chomp::Strip => kept.trim_end_matches('\n').to_owned(),
            Chomp::Clip => {
                let trimmed = kept.trim_end_matches('\n');
                if trimmed.is_empty() {
                    String::new()
                } else {
                    format!("{trimmed}\n")
                }
            }
        }))
    }

    /// A quoted scalar, which may fold over the lines beneath it.
    fn scan_quoted(&mut self, pos: &mut usize) -> Result<String, YamlError> {
        let start = self.i;
        let quote = self.at(*pos).unwrap_or('"');
        let mut from = *pos + 1;
        let mut out = String::new();
        loop {
            let piece =
                quoted_body(&self.lines[self.i], from, quote).map_err(|m| self.err(self.i, m))?;
            out.push_str(&piece.text);
            if let Some(end) = piece.end {
                *pos = end;
                return Ok(out);
            }
            let keep = out.trim_end_matches([' ', '\t']).len();
            out.truncate(keep);
            let mut breaks = 1;
            self.i += 1;
            while self.i < self.lines.len() && self.lines[self.i].trim().is_empty() {
                breaks += 1;
                self.i += 1;
            }
            if self.i >= self.lines.len() {
                return Err(self.err(start, "unterminated quoted scalar"));
            }
            if !piece.escaped_break {
                if breaks == 1 {
                    out.push(' ');
                } else {
                    out.push_str(&"\n".repeat(breaks - 1));
                }
            }
            let line = &self.lines[self.i];
            from = line.len() - line.trim_start().len();
        }
    }

    fn parse_flow(&mut self, pos: &mut usize) -> Result<Node, YamlError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(self.err(self.i, "nesting is too deep"));
        }
        let value = self.parse_flow_inner(pos);
        self.depth -= 1;
        value
    }

    fn parse_flow_inner(&mut self, pos: &mut usize) -> Result<Node, YamlError> {
        self.flow_space(pos, self.i)?;
        let line = self.i;
        match self.at(*pos) {
            Some('[') => {
                *pos += 1;
                self.flow_seq(pos, line)
            }
            Some('{') => {
                *pos += 1;
                self.flow_map(pos, line)
            }
            Some('\'') | Some('"') => Ok(Node::String(self.scan_quoted(pos)?)),
            Some('&') => {
                let name = token(self.rest(line, *pos));
                if name.len() == 1 {
                    return Err(self.err(line, "an anchor needs a name"));
                }
                *pos += name.len();
                let value = self.parse_flow(pos)?;
                self.anchors.insert(name[1..].to_owned(), value.clone());
                Ok(value)
            }
            Some('*') => {
                let name = token(self.rest(line, *pos));
                *pos += name.len();
                self.alias(&name[1..], line)
            }
            _ => {
                let (text, end) = scan_plain(self.rest(line, *pos), true);
                if text.is_empty() {
                    return Err(self.err(line, "expected a value in a flow collection"));
                }
                *pos += end;
                typed_scalar(&text).map_err(|m| self.err(line, m))
            }
        }
    }

    fn flow_seq(&mut self, pos: &mut usize, start: usize) -> Result<Node, YamlError> {
        let mut items = Vec::new();
        loop {
            self.flow_space(pos, start)?;
            if self.at(*pos) == Some(']') {
                *pos += 1;
                return Ok(Node::List(items));
            }
            items.push(self.parse_flow(pos)?);
            self.flow_space(pos, start)?;
            match self.at(*pos) {
                Some(',') => *pos += 1,
                Some(']') => {
                    *pos += 1;
                    return Ok(Node::List(items));
                }
                _ => return Err(self.err(self.i, "expected ',' or ']' in a flow sequence")),
            }
        }
    }

    fn flow_map(&mut self, pos: &mut usize, start: usize) -> Result<Node, YamlError> {
        let mut builder = MapBuilder::default();
        loop {
            self.flow_space(pos, start)?;
            if self.at(*pos) == Some('}') {
                *pos += 1;
                return builder.finish();
            }
            let line = self.i;
            let (text, quoted) = match self.at(*pos) {
                Some('\'') | Some('"') => (self.scan_quoted(pos)?, true),
                _ => {
                    let (text, end) = scan_plain(self.rest(line, *pos), true);
                    if text.is_empty() {
                        return Err(self.err(line, "expected a mapping key in a flow mapping"));
                    }
                    *pos += end;
                    (text, false)
                }
            };
            self.flow_space(pos, start)?;
            let value = if self.at(*pos) == Some(':') {
                *pos += 1;
                self.parse_flow(pos)?
            } else {
                Node::Null
            };
            if text == "<<" && !quoted {
                builder.merge(value, line);
            } else {
                let name = scalar_key(&text, quoted).map_err(|m| self.err(line, m))?;
                builder.insert(name, value, line)?;
            }
            self.flow_space(pos, start)?;
            match self.at(*pos) {
                Some(',') => *pos += 1,
                Some('}') => {
                    *pos += 1;
                    return builder.finish();
                }
                _ => return Err(self.err(self.i, "expected ',' or '}' in a flow mapping")),
            }
        }
    }

    /// Skip separation space, line breaks and comments inside a flow collection.
    /// `start` is the line the collection opened on, which is where an unterminated
    /// one is reported.
    fn flow_space(&mut self, pos: &mut usize, start: usize) -> Result<(), YamlError> {
        loop {
            if self.i >= self.lines.len() {
                return Err(self.err(start, "unterminated flow collection"));
            }
            let line = &self.lines[self.i];
            if *pos >= line.len() {
                self.i += 1;
                *pos = 0;
                continue;
            }
            let c = line[*pos..].chars().next().unwrap_or(' ');
            if c == ' ' || c == '\t' {
                *pos += 1;
                continue;
            }
            if c == '#' && (*pos == 0 || matches!(line.as_bytes()[*pos - 1], b' ' | b'\t')) {
                self.i += 1;
                *pos = 0;
                continue;
            }
            return Ok(());
        }
    }
}

enum Chomp {
    Clip,
    Strip,
    Keep,
}

/// A `key:` at the head of a line.
struct Key {
    text: String,
    quoted: bool,
    /// Offset just past the colon.
    after: usize,
}

fn is_dash(content: &str) -> bool {
    content == "-" || content.starts_with("- ") || content.starts_with("-\t")
}

/// An anchor or alias name, which ends at whitespace or a flow delimiter.
fn token(text: &str) -> String {
    let mut out = String::new();
    for (i, c) in text.char_indices() {
        if i > 0 && (c.is_whitespace() || matches!(c, ',' | '[' | ']' | '{' | '}')) {
            break;
        }
        out.push(c);
    }
    out
}

/// Split `key:` off the head of `content`, if there is one there.
fn key_split(content: &str) -> Option<Key> {
    let first = content.chars().next()?;
    if matches!(first, '[' | '{' | '#' | ',' | ']' | '}') {
        return None;
    }
    if first == '\'' || first == '"' {
        let piece = quoted_body(content, 1, first).ok()?;
        let end = piece.end?;
        let tail = &content[end..];
        let spaces = tail.len() - tail.trim_start().len();
        if !tail.trim_start().starts_with(':') {
            return None;
        }
        let after = end + spaces + 1;
        if !matches!(
            content.as_bytes().get(after),
            None | Some(b' ') | Some(b'\t')
        ) {
            return None;
        }
        return Some(Key {
            text: piece.text,
            quoted: true,
            after,
        });
    }
    let bytes = content.as_bytes();
    for i in 0..bytes.len() {
        if bytes[i] == b'#' && i > 0 && matches!(bytes[i - 1], b' ' | b'\t') {
            return None;
        }
        if bytes[i] == b':' && matches!(bytes.get(i + 1), None | Some(b' ') | Some(b'\t')) {
            let text = content[..i].trim_end();
            if text.is_empty() {
                return None;
            }
            return Some(Key {
                text: text.to_owned(),
                quoted: false,
                after: i + 1,
            });
        }
    }
    None
}

/// A plain scalar, up to the end of the line, a comment, or — inside a flow
/// collection — the delimiters that close it. YAML ends a plain scalar at `: `, so
/// a value that needs one has to be quoted.
fn scan_plain(content: &str, flow: bool) -> (String, usize) {
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'#' && i > 0 && matches!(bytes[i - 1], b' ' | b'\t') {
            break;
        }
        if b == b':' {
            let stop = match bytes.get(i + 1) {
                None | Some(b' ') | Some(b'\t') => true,
                Some(b',') | Some(b']') | Some(b'}') => flow,
                _ => false,
            };
            if stop {
                break;
            }
        }
        if flow && matches!(b, b',' | b'[' | b']' | b'{' | b'}') {
            break;
        }
        i += 1;
    }
    (content[..i].trim_end().to_owned(), i)
}

struct QuotedLine {
    text: String,
    /// Offset just past the closing quote, or `None` when the line ended first.
    end: Option<usize>,
    /// The line ended on a `\`, which swallows the break instead of folding it.
    escaped_break: bool,
}

/// Scan a quoted scalar from `pos` (just inside the opening quote) to the end of the
/// line or to its closing quote.
fn quoted_body(line: &str, pos: usize, quote: char) -> Result<QuotedLine, String> {
    let mut out = String::new();
    let mut i = pos;
    while let Some(c) = line[i..].chars().next() {
        i += c.len_utf8();
        if c == quote {
            if quote == '\'' && line[i..].starts_with('\'') {
                out.push('\'');
                i += 1;
                continue;
            }
            return Ok(QuotedLine {
                text: out,
                end: Some(i),
                escaped_break: false,
            });
        }
        if quote == '"' && c == '\\' {
            let Some(e) = line[i..].chars().next() else {
                return Ok(QuotedLine {
                    text: out,
                    end: None,
                    escaped_break: true,
                });
            };
            i += e.len_utf8();
            match e {
                '\\' => out.push('\\'),
                '"' => out.push('"'),
                '/' => out.push('/'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'b' => out.push('\u{8}'),
                'f' => out.push('\u{c}'),
                '0' => out.push('\0'),
                'x' => {
                    let v = hex_digits(line, &mut i, 2)?;
                    out.push(char::from_u32(v).ok_or("invalid \\x escape")?);
                }
                'u' => {
                    let v = hex_digits(line, &mut i, 4)?;
                    out.push(unicode_escape(line, &mut i, v)?);
                }
                _ => return Err(format!("unknown escape \\{e}")),
            }
            continue;
        }
        out.push(c);
    }
    Ok(QuotedLine {
        text: out,
        end: None,
        escaped_break: false,
    })
}

fn hex_digits(line: &str, i: &mut usize, n: usize) -> Result<u32, String> {
    let digits = line
        .get(*i..*i + n)
        .filter(|d| d.chars().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| format!("expected {n} hexadecimal digits in an escape"))?;
    *i += n;
    u32::from_str_radix(digits, 16).map_err(|e| e.to_string())
}

/// A `\u` escape, joining a surrogate pair into the character it encodes.
fn unicode_escape(line: &str, i: &mut usize, value: u32) -> Result<char, String> {
    let value = if (0xd800..0xdc00).contains(&value) {
        if !line[*i..].starts_with("\\u") {
            return Err("unpaired surrogate escape".to_owned());
        }
        *i += 2;
        let low = hex_digits(line, i, 4)?;
        if !(0xdc00..0xe000).contains(&low) {
            return Err("unpaired surrogate escape".to_owned());
        }
        0x10000 + ((value - 0xd800) << 10) + (low - 0xdc00)
    } else {
        value
    };
    char::from_u32(value).ok_or_else(|| format!("invalid \\u escape {value:04X}"))
}

/// Fold a block scalar body: one break between two equally indented lines becomes a
/// space, a break beside a more indented line is kept, and blank lines survive.
fn fold_body(body: &[String]) -> String {
    let mut out = String::new();
    let mut blanks = 0;
    let mut started = false;
    let mut previous_indented = false;
    for line in body {
        if line.trim().is_empty() {
            blanks += 1;
            continue;
        }
        let indented = line.starts_with([' ', '\t']);
        if started {
            if blanks > 0 {
                out.push_str(&"\n".repeat(blanks));
            } else if indented || previous_indented {
                out.push('\n');
            } else {
                out.push(' ');
            }
        }
        out.push_str(line);
        started = true;
        previous_indented = indented;
        blanks = 0;
    }
    if started {
        out.push('\n');
        out.push_str(&"\n".repeat(blanks));
    }
    out
}

/// A mapping key as the document model sees it: a quoted key is its own text, and a
/// plain one is resolved and then written back out, so `3:` and `true:` become the
/// keys `"3"` and `"true"`.
fn scalar_key(text: &str, quoted: bool) -> Result<String, String> {
    if quoted {
        return Ok(text.to_owned());
    }
    Ok(match typed_scalar(text)? {
        Node::String(s) => s,
        Node::Number(n) => n,
        Node::Bool(b) => b.to_string(),
        _ => "null".to_owned(),
    })
}

/// Resolve a plain scalar against the YAML 1.2 core schema. `yes`/`no`/`on`/`off`
/// are strings here, as 1.2 says, and so is anything a JSON number cannot hold:
/// `.inf`, `.nan`, and an integer past 64 bits.
fn typed_scalar(text: &str) -> Result<Node, String> {
    match text {
        "" | "null" | "Null" | "NULL" | "~" => return Ok(Node::Null),
        "true" | "True" | "TRUE" => return Ok(Node::Bool(true)),
        "false" | "False" | "FALSE" => return Ok(Node::Bool(false)),
        _ => {}
    }
    match number_literal(text) {
        Some(number) => Ok(Node::Number(number?)),
        None => Ok(Node::String(text.to_owned())),
    }
}

/// Recognise a number and render it the way JavaScript would. The world file this
/// feeds is compared byte for byte against one `JSON.stringify` wrote, so `0x10`
/// has to come out `16` and `1.50` has to come out `1.5`. A literal that is a
/// number but cannot survive the round trip is refused rather than quietly changed.
fn number_literal(text: &str) -> Option<Result<String, String>> {
    if let Some(value) = integer_value(text) {
        // `f64` is the only number JavaScript has, so an integer past its exact
        // range would be written back out as a different number.
        if value.unsigned_abs() > 1 << 53 {
            return Some(Err(format!(
                "{text} is too large to be written back out exactly"
            )));
        }
        return Some(Ok(value.to_string()));
    }
    let value = float_value(text)?;
    let canonical = javascript_number(value);
    if canonical.parse::<f64>() != Ok(value) {
        return Some(Err(format!("{text} cannot be written back out exactly")));
    }
    Some(Ok(canonical))
}

/// An integer literal's value: decimal, or `0x`/`0o`/`0b`. A decimal with a leading
/// zero is a padded field (`0755` is a file mode), and one past `u64` is not a
/// number this parser claims to understand, so both stay strings.
fn integer_value(text: &str) -> Option<i128> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (radix, digits) = if let Some(d) = rest.strip_prefix("0x").or(rest.strip_prefix("0X")) {
        (16, d)
    } else if let Some(d) = rest.strip_prefix("0o").or(rest.strip_prefix("0O")) {
        (8, d)
    } else if let Some(d) = rest.strip_prefix("0b").or(rest.strip_prefix("0B")) {
        (2, d)
    } else {
        (10, rest)
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    if radix == 10 && digits.len() > 1 && digits.starts_with('0') {
        return None;
    }
    let magnitude = i128::from(u64::from_str_radix(digits, radix).ok()?);
    Some(if negative { -magnitude } else { magnitude })
}

/// A float literal's value. The shape is checked before parsing, because Rust would
/// accept `inf` and `NaN`, which YAML spells differently and JSON cannot hold.
fn float_value(text: &str) -> Option<f64> {
    let body = text.strip_prefix(['-', '+']).unwrap_or(text);
    let (mantissa, exponent) = match body.split_once(['e', 'E']) {
        Some((m, e)) => (m, Some(e)),
        None => (body, None),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((w, f)) => (w, Some(f)),
        None => (mantissa, None),
    };
    if fraction.is_none() && exponent.is_none() {
        return None;
    }
    let digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    if !digits(whole) || !fraction.is_none_or(digits) {
        return None;
    }
    if whole.is_empty() && fraction.unwrap_or("").is_empty() {
        return None;
    }
    if whole.len() > 1 && whole.starts_with('0') {
        return None;
    }
    if let Some(e) = exponent {
        let e = e.strip_prefix(['-', '+']).unwrap_or(e);
        if e.is_empty() || !digits(e) {
            return None;
        }
    }
    let value: f64 = text.parse().ok()?;
    value.is_finite().then_some(value)
}

/// `String(Number(x))`: the shortest decimal that round-trips, in fixed notation
/// while the decimal exponent is within `(-6, 21]` and in exponent form outside it.
/// Rust's own `{}` never switches to exponent form, so the switch is done here.
fn javascript_number(value: f64) -> String {
    if value == 0.0 {
        return "0".to_owned();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    let shortest = format!("{:e}", value.abs());
    let (mantissa, exponent) = shortest.split_once('e').unwrap_or((shortest.as_str(), "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    // `n` is where the decimal point sits: the value is `0.<digits> * 10^n`.
    let n = exponent.parse::<i32>().unwrap_or(0) + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat(-n as usize))
    } else {
        let e = n - 1;
        let mantissa = if k == 1 {
            digits
        } else {
            format!("{}.{}", &digits[..1], &digits[1..])
        };
        format!("{mantissa}e{}{}", if e < 0 { "-" } else { "+" }, e.abs())
    };
    format!("{sign}{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: Vec<(&str, Node)>) -> Node {
        Node::Map(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }
    fn list(items: Vec<Node>) -> Node {
        Node::List(items)
    }
    fn num(text: &str) -> Node {
        Node::Number(text.to_owned())
    }
    fn text(value: &str) -> Node {
        Node::String(value.to_owned())
    }
    fn ok(yaml: &str) -> Node {
        parse(yaml).unwrap_or_else(|e| panic!("{e}"))
    }
    fn bad(yaml: &str) -> String {
        parse(yaml).unwrap_err().message
    }
    /// The value of `a:`, for the scalar cases.
    fn scalar(literal: &str) -> Node {
        ok(&format!("a: {literal}")).get("a").unwrap().clone()
    }

    #[test]
    fn an_empty_or_commentless_document_is_null() {
        assert_eq!(ok(""), Node::Null);
        assert_eq!(ok("\n\n"), Node::Null);
        assert_eq!(ok("# only a comment\n"), Node::Null);
        assert_eq!(ok("---\n# nothing\n...\n"), Node::Null);
    }

    #[test]
    fn plain_scalars_resolve_by_the_core_schema() {
        for spelling in ["null", "Null", "NULL", "~", ""] {
            assert_eq!(scalar(spelling), Node::Null, "{spelling}");
        }
        for spelling in ["true", "True", "TRUE"] {
            assert_eq!(scalar(spelling), Node::Bool(true), "{spelling}");
        }
        for spelling in ["false", "False", "FALSE"] {
            assert_eq!(scalar(spelling), Node::Bool(false), "{spelling}");
        }
        // YAML 1.2 dropped these, and a country code of `no` is not a boolean.
        for word in ["yes", "no", "on", "off", "y", "n"] {
            assert_eq!(scalar(word), text(word), "{word}");
        }
        assert_eq!(scalar("hello world"), text("hello world"));
        assert_eq!(scalar("hello   "), text("hello"));
    }

    #[test]
    fn numbers_are_kept_in_javascript_canonical_form() {
        for (literal, canonical) in [
            ("+5", "5"),
            ("1.0", "1"),
            ("1.50", "1.5"),
            ("0x1f", "31"),
            ("0o17", "15"),
            ("0b101", "5"),
            ("1e3", "1000"),
            ("1.5e-3", "0.0015"),
            ("-0", "0"),
            ("2.5", "2.5"),
            ("1e21", "1e+21"),
            ("1e-7", "1e-7"),
            ("1e-6", "0.000001"),
            ("1e20", "100000000000000000000"),
            ("1E2", "100"),
            ("-1.5", "-1.5"),
            ("0.1", "0.1"),
            ("0", "0"),
            ("-7", "-7"),
            ("9007199254740992", "9007199254740992"),
        ] {
            assert_eq!(scalar(literal), num(canonical), "{literal}");
        }
    }

    #[test]
    fn what_is_not_a_number_stays_a_string() {
        for literal in [
            "0755",
            "1.2.3",
            ".inf",
            "-.inf",
            ".nan",
            "1_000",
            "12-34",
            "1e",
            "0x",
            "0xzz",
            "99999999999999999999999999",
            "v1.2",
            "-",
        ] {
            assert_eq!(scalar(literal), text(literal), "{literal}");
        }
    }

    #[test]
    fn a_number_that_would_not_survive_the_round_trip_is_refused() {
        let error = parse("a: 9007199254740993").unwrap_err();
        assert_eq!(error.line, 1);
        assert!(error.message.contains("exactly"), "{}", error.message);
    }

    #[test]
    fn block_mappings_nest_by_indentation() {
        assert_eq!(
            ok("a: 1\nb:\n  c: two\n  d:\n    e: true\nf: 3\n"),
            map(vec![
                ("a", num("1")),
                (
                    "b",
                    map(vec![
                        ("c", text("two")),
                        ("d", map(vec![("e", Node::Bool(true))])),
                    ])
                ),
                ("f", num("3")),
            ])
        );
        assert_eq!(ok("a:\n"), map(vec![("a", Node::Null)]));
        assert_eq!(ok("a: 1\n"), map(vec![("a", num("1"))]));
    }

    #[test]
    fn mapping_keys_are_stringified_and_may_be_quoted() {
        assert_eq!(ok("3: x\n"), map(vec![("3", text("x"))]));
        assert_eq!(ok("true: x\n"), map(vec![("true", text("x"))]));
        assert_eq!(ok("null:\n"), map(vec![("null", Node::Null)]));
        assert_eq!(ok("~: x\n"), map(vec![("null", text("x"))]));
        assert_eq!(ok("\"a b\": x\n"), map(vec![("a b", text("x"))]));
        assert_eq!(ok("'3': x\n"), map(vec![("3", text("x"))]));
        assert_eq!(ok("a#b: c#d\n"), map(vec![("a#b", text("c#d"))]));
        assert_eq!(
            ok("url: http://x/y\n"),
            map(vec![("url", text("http://x/y"))])
        );
    }

    #[test]
    fn block_sequences_take_every_shape() {
        assert_eq!(ok("- 1\n- two\n"), list(vec![num("1"), text("two")]));
        assert_eq!(
            ok("items:\n- 1\n- 2\nafter: x\n"),
            map(vec![
                ("items", list(vec![num("1"), num("2")])),
                ("after", text("x")),
            ])
        );
        assert_eq!(
            ok("items:\n  - 1\n  - 2\n"),
            map(vec![("items", list(vec![num("1"), num("2")]))])
        );
        assert_eq!(
            ok("- a: 1\n  b: 2\n- a: 3\n"),
            list(vec![
                map(vec![("a", num("1")), ("b", num("2"))]),
                map(vec![("a", num("3"))]),
            ])
        );
        assert_eq!(
            ok("- - a\n  - b\n- c\n"),
            list(vec![list(vec![text("a"), text("b")]), text("c")])
        );
        assert_eq!(ok("-\n- x\n"), list(vec![Node::Null, text("x")]));
        assert_eq!(ok("-\n  a: 1\n"), list(vec![map(vec![("a", num("1"))])]));
        assert_eq!(
            ok("- foo\n  bar\n"),
            list(vec![text("foo bar")]),
            "a scalar item folds against the dash's own indentation"
        );
    }

    #[test]
    fn flow_collections_nest_and_may_span_lines() {
        assert_eq!(
            ok("a: {}\nb: []\n"),
            map(vec![("a", map(vec![])), ("b", list(vec![]))])
        );
        assert_eq!(
            ok("a: {b: 1, c: [2, 3]}\n"),
            map(vec![(
                "a",
                map(vec![("b", num("1")), ("c", list(vec![num("2"), num("3")]))])
            )])
        );
        assert_eq!(
            ok("a: [1, 2, {c: d}]\n"),
            map(vec![(
                "a",
                list(vec![num("1"), num("2"), map(vec![("c", text("d"))])])
            )])
        );
        assert_eq!(
            ok("a: [1,\n    2,   # a comment\n    3]\n"),
            map(vec![("a", list(vec![num("1"), num("2"), num("3")]))])
        );
        assert_eq!(
            ok("{a: 1, b: two}\n"),
            map(vec![("a", num("1")), ("b", text("two"))])
        );
        assert_eq!(
            ok("a: ['x, y', \"z\"]\n"),
            map(vec![("a", list(vec![text("x, y"), text("z")]))])
        );
        assert_eq!(
            ok("a: [1, 2,]\n"),
            map(vec![("a", list(vec![num("1"), num("2")]))])
        );
    }

    #[test]
    fn quoted_scalars_are_always_strings() {
        assert_eq!(scalar("'1'"), text("1"));
        assert_eq!(scalar("\"true\""), text("true"));
        assert_eq!(scalar("'it''s'"), text("it's"));
        assert_eq!(scalar("' padded '"), text(" padded "));
        assert_eq!(scalar("'a # b'"), text("a # b"));
        assert_eq!(scalar("\"a: b\""), text("a: b"));
    }

    #[test]
    fn double_quoted_scalars_take_escapes() {
        assert_eq!(scalar(r#""a\nb""#), text("a\nb"));
        assert_eq!(
            scalar(r#""\\ \" \/ \t \r \b \f""#),
            text("\\ \" / \t \r \u{8} \u{c}")
        );
        assert_eq!(scalar(r#""\0""#), text("\0"));
        assert_eq!(scalar(r#""\x41\x7e""#), text("A~"));
        assert_eq!(scalar(r#""\u00e9\u20ac""#), text("é€"));
        assert_eq!(scalar(r#""\ud83d\ude00""#), text("😀"));
        assert_eq!(bad(r#"a: "\ud83d""#), "unpaired surrogate escape");
        assert_eq!(bad(r#"a: "\q""#), "unknown escape \\q");
        assert!(bad(r#"a: "\u12""#).contains("hexadecimal"));
    }

    #[test]
    fn scalars_fold_over_the_lines_beneath_them() {
        assert_eq!(
            ok("note: one\n  two\n"),
            map(vec![("note", text("one two"))])
        );
        assert_eq!(
            ok("note: one\n\n  two\n"),
            map(vec![("note", text("one\ntwo"))])
        );
        assert_eq!(
            ok("note: \"one\n  two\"\nb: 1\n"),
            map(vec![("note", text("one two")), ("b", num("1"))])
        );
        assert_eq!(
            ok("a: one\nb: two\n"),
            map(vec![("a", text("one")), ("b", text("two"))]),
            "a sibling key is not a continuation"
        );
    }

    #[test]
    fn block_scalars_are_literal_or_folded_and_chomp_three_ways() {
        assert_eq!(
            ok("a: |\n  one\n  two\n"),
            map(vec![("a", text("one\ntwo\n"))])
        );
        assert_eq!(
            ok("a: |-\n  one\n  two\n"),
            map(vec![("a", text("one\ntwo"))])
        );
        assert_eq!(
            ok("a: |+\n  one\n\nb: 2\n"),
            map(vec![("a", text("one\n\n")), ("b", num("2"))])
        );
        assert_eq!(
            ok("a: >\n  one\n  two\n"),
            map(vec![("a", text("one two\n"))])
        );
        assert_eq!(
            ok("a: >-\n  one\n  two\n"),
            map(vec![("a", text("one two"))])
        );
        assert_eq!(
            ok("a: >\n  one\n\n  two\n"),
            map(vec![("a", text("one\ntwo\n"))])
        );
        assert_eq!(
            ok("a: >\n  one\n    deep\n  two\n"),
            map(vec![("a", text("one\n  deep\ntwo\n"))]),
            "a more indented line keeps its breaks"
        );
        assert_eq!(ok("a: |2\n    text\n"), map(vec![("a", text("  text\n"))]));
        assert_eq!(
            ok("a: |\n  # not a comment\nb: 1\n"),
            map(vec![("a", text("# not a comment\n")), ("b", num("1"))])
        );
        assert_eq!(
            ok("a: |\nb: 1\n"),
            map(vec![("a", text("")), ("b", num("1"))])
        );
        assert!(bad("a: |x\n  t\n").contains("invalid block scalar header"));
    }

    #[test]
    fn comments_end_at_the_line_and_never_start_inside_a_scalar() {
        assert_eq!(
            ok("# leading\na: 1 # trailing\n\n# between\nb: two#three\n"),
            map(vec![("a", num("1")), ("b", text("two#three"))])
        );
        assert_eq!(
            ok("a: # nothing here\n  b: 1\n"),
            map(vec![("a", map(vec![("b", num("1"))]))])
        );
    }

    #[test]
    fn aliases_deep_copy_the_anchor_they_name() {
        assert_eq!(
            ok("base: &base\n  x: 1\ncopy: *base\n"),
            map(vec![
                ("base", map(vec![("x", num("1"))])),
                ("copy", map(vec![("x", num("1"))])),
            ])
        );
        assert_eq!(
            ok("a: &v 1\nb: *v\nc: &v 2\nd: *v\n"),
            map(vec![
                ("a", num("1")),
                ("b", num("1")),
                ("c", num("2")),
                ("d", num("2")),
            ]),
            "a later anchor of the same name wins for later aliases"
        );
        assert_eq!(
            ok("a: &s [1, 2]\nb: [*s, 3]\n"),
            map(vec![
                ("a", list(vec![num("1"), num("2")])),
                ("b", list(vec![list(vec![num("1"), num("2")]), num("3")])),
            ])
        );
        assert_eq!(bad("a: *missing\n"), "unknown alias *missing");
    }

    #[test]
    fn merge_keys_fill_in_what_the_mapping_does_not_say() {
        assert_eq!(
            ok("base: &base\n  a: 1\n  b: 2\none:\n  <<: *base\n  b: 3\n"),
            map(vec![
                ("base", map(vec![("a", num("1")), ("b", num("2"))])),
                // The explicit key keeps its place; merged keys follow it.
                ("one", map(vec![("b", num("3")), ("a", num("1"))])),
            ])
        );
        assert_eq!(
            ok("x: &x {a: 1, c: 1}\ny: &y {a: 2, b: 2}\nz:\n  <<: [*x, *y]\n"),
            map(vec![
                ("x", map(vec![("a", num("1")), ("c", num("1"))])),
                ("y", map(vec![("a", num("2")), ("b", num("2"))])),
                (
                    "z",
                    map(vec![("a", num("1")), ("c", num("1")), ("b", num("2"))])
                ),
            ]),
            "earlier entries of a merge list win over later ones"
        );
        assert_eq!(
            ok("x: &x {a: 1}\ny: {<<: *x, b: 2}\n"),
            map(vec![
                ("x", map(vec![("a", num("1"))])),
                ("y", map(vec![("b", num("2")), ("a", num("1"))])),
            ])
        );
        assert_eq!(
            bad("a: &a 1\nb:\n  <<: *a\n"),
            "a merge key must name a mapping"
        );
    }

    #[test]
    fn document_markers_bound_the_one_document() {
        assert_eq!(ok("---\na: 1\n"), map(vec![("a", num("1"))]));
        assert_eq!(ok("--- \na: 1\n...\n"), map(vec![("a", num("1"))]));
        assert_eq!(ok("--- 42\n"), num("42"));
        assert_eq!(
            ok("a: 1\n...\nnot read: true\n"),
            map(vec![("a", num("1"))])
        );
        let error = parse("a: 1\n---\nb: 2\n").unwrap_err();
        assert_eq!(error.line, 2);
        assert_eq!(error.message, "multiple documents are not supported");
        assert_eq!(
            bad("---\na: 1\n---\nb: 2\n"),
            "multiple documents are not supported"
        );
    }

    #[test]
    fn malformed_documents_name_the_line_and_the_problem() {
        let error = parse("a: 1\na: 2\n").unwrap_err();
        assert_eq!(error.line, 2);
        assert_eq!(error.to_string(), "line 2: duplicate mapping key a");
        assert_eq!(bad("a: {b: 1, b: 2}\n"), "duplicate mapping key b");

        let tabbed = parse("a:\n\tb: 1\n").unwrap_err();
        assert_eq!(tabbed.line, 2);
        assert_eq!(tabbed.message, "tabs cannot be used to indent");

        let indent = parse("a: 1\n  b: 2\n").unwrap_err();
        assert_eq!(indent.line, 2);
        assert_eq!(indent.message, "unexpected indentation inside a mapping");
        assert!(bad("- a\n  - b\n").contains("unexpected indentation inside a sequence"));

        let dash = parse("a: 1\n- b\n").unwrap_err();
        assert_eq!(dash.line, 2);
        assert!(
            dash.message.starts_with("a sequence item cannot start"),
            "{}",
            dash.message
        );

        let quote = parse("a: \"unterminated\nb: 1\n").unwrap_err();
        assert_eq!(quote.line, 1);
        assert_eq!(quote.message, "unterminated quoted scalar");
        assert_eq!(bad("a: 'still going\n"), "unterminated quoted scalar");

        let flow = parse("a: [1,\n  2\n").unwrap_err();
        assert_eq!(flow.line, 1);
        assert_eq!(flow.message, "unterminated flow collection");
        // A plain scalar in a flow collection may hold spaces, so what ends a value
        // there is a delimiter, not a gap.
        assert_eq!(ok("a: [1 2]\n"), map(vec![("a", list(vec![text("1 2")]))]));
        assert!(bad("a: [[1] 2]\n").contains("expected ',' or ']'"));
        assert!(bad("a: {b: 1 c: 2}\n").contains("expected ',' or '}'"));
        assert!(bad("a: b: c\n").contains("plain scalar cannot contain"));
        assert_eq!(bad("a: 1\nnot a key\n"), "expected a mapping key");
        assert_eq!(
            bad("scalar\nand another\n"),
            "unexpected content after the document"
        );
        assert_eq!(
            bad(&format!("a: {}\n", "[".repeat(200))),
            "nesting is too deep"
        );
    }

    #[test]
    fn a_realistic_configuration_parses_to_the_document_it_describes() {
        let document = r#"
# Deployment blueprint for the west cluster.
---
name: west-cluster
schema_version: 3
enabled: true
retries: null

defaults: &defaults
  image: registry/base:1.4
  restart: on-failure
  limits:
    cpu: 2
    memory: 512

services:
  - name: api
    <<: *defaults
    port: 8080
    tags: [http, public]
    limits:
      cpu: 4
      memory: 2048
  - name: worker      # background jobs
    <<: *defaults
    port: 0
    tags: []
    queues:
      - name: email
        rate: 12.5
      - name: images
        rate: 0.5

motd: |
  west cluster
    second line

summary: >-
  the cluster runs
  two services

owners: {primary: ada, backup: grace}
paths:
  - /var/log
  - "/etc/app.conf"
version_label: 1.2.3
mask: 0755
...
"#;
        let base = || {
            vec![
                ("image", text("registry/base:1.4")),
                ("restart", text("on-failure")),
            ]
        };
        let mut api = base();
        api.insert(0, ("name", text("api")));
        api.insert(1, ("port", num("8080")));
        api.insert(2, ("tags", list(vec![text("http"), text("public")])));
        api.insert(
            3,
            (
                "limits",
                map(vec![("cpu", num("4")), ("memory", num("2048"))]),
            ),
        );
        let mut worker = base();
        worker.insert(0, ("name", text("worker")));
        worker.insert(1, ("port", num("0")));
        worker.insert(2, ("tags", list(vec![])));
        worker.insert(
            3,
            (
                "queues",
                list(vec![
                    map(vec![("name", text("email")), ("rate", num("12.5"))]),
                    map(vec![("name", text("images")), ("rate", num("0.5"))]),
                ]),
            ),
        );
        worker.push((
            "limits",
            map(vec![("cpu", num("2")), ("memory", num("512"))]),
        ));
        assert_eq!(
            ok(document),
            map(vec![
                ("name", text("west-cluster")),
                ("schema_version", num("3")),
                ("enabled", Node::Bool(true)),
                ("retries", Node::Null),
                (
                    "defaults",
                    map(vec![
                        ("image", text("registry/base:1.4")),
                        ("restart", text("on-failure")),
                        (
                            "limits",
                            map(vec![("cpu", num("2")), ("memory", num("512"))])
                        ),
                    ])
                ),
                ("services", list(vec![map(api), map(worker)])),
                ("motd", text("west cluster\n  second line\n")),
                ("summary", text("the cluster runs two services")),
                (
                    "owners",
                    map(vec![("primary", text("ada")), ("backup", text("grace"))])
                ),
                ("paths", list(vec![text("/var/log"), text("/etc/app.conf")])),
                ("version_label", text("1.2.3")),
                ("mask", text("0755")),
            ])
        );
    }
}
