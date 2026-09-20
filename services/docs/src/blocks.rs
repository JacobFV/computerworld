//! A document body read as blocks. Bodies are plain text; the pages draw them the way the
//! products would: a short unpunctuated line ahead of other lines is a heading, `- ` lines are
//! bullets, `- [ ]` / `- [x]` lines are to-dos, `1. ` lines are numbered, an indented line that
//! is none of those continues the block above it, and everything else is a paragraph.
//!
//! Every word keeps its position in the whole body (`split_whitespace` order, markers
//! included), because that position is the id of a link written in the prose:
//! `body-link-<n>`, the same numbering `cw_service_common::links` gave the `Page` version.
use cw_service_common::html::{el, link, Html};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Heading,
    Para,
    Bullet,
    Todo(bool),
    Numbered(String),
    /// A blank line: vertical space, nothing else.
    Gap,
}
#[derive(Clone, Debug)]
pub(crate) struct Block {
    /// Index of the body line the block starts on; `body-line-<n>` on the page.
    pub line: usize,
    pub kind: Kind,
    /// Nesting from the line's indentation: 0 at the margin, 1 for an indented item.
    pub depth: usize,
    /// Words with their position in the whole body.
    pub words: Vec<(usize, String)>,
}
fn ordinal(word: &str) -> Option<&str> {
    let digits = word.strip_suffix('.')?;
    (!digits.is_empty() && digits.len() <= 3 && digits.bytes().all(|b| b.is_ascii_digit())).then_some(digits)
}
pub(crate) fn parse(body: &str) -> Vec<Block> {
    let lines: Vec<&str> = body.lines().collect();
    let mut out: Vec<Block> = Vec::new();
    let mut position = 0usize;
    for (n, raw) in lines.iter().enumerate() {
        let mut words: Vec<(usize, String)> = raw
            .split_whitespace()
            .map(|w| {
                position += 1;
                (position - 1, w.to_owned())
            })
            .collect();
        if words.is_empty() {
            if out.last().is_some_and(|b| b.kind != Kind::Gap) {
                out.push(Block { line: n, kind: Kind::Gap, depth: 0, words });
            }
            continue;
        }
        let indent = raw.len() - raw.trim_start().len();
        let depth = usize::from(indent >= 2);
        let first = words[0].1.clone();
        let kind = if first == "-" || first == "*" || first == "•" {
            words.remove(0);
            match words.first().map(|w| w.1.as_str()) {
                Some("[x]") | Some("[X]") => {
                    words.remove(0);
                    Kind::Todo(true)
                }
                Some("[") if words.get(1).is_some_and(|w| w.1 == "]") => {
                    words.drain(0..2);
                    Kind::Todo(false)
                }
                _ => Kind::Bullet,
            }
        } else if let Some(digits) = ordinal(&first) {
            let digits = digits.to_owned();
            words.remove(0);
            Kind::Numbered(digits)
        } else if indent >= 2 && out.last().is_some_and(|b| b.kind != Kind::Gap) {
            // A wrapped line: it belongs to the block above.
            out.last_mut().unwrap().words.extend(words);
            continue;
        } else {
            let text = raw.trim();
            let after_break = n == 0 || lines[n - 1].trim().is_empty();
            let leads = lines.get(n + 1).is_some_and(|l| !l.trim().is_empty());
            let plain_end = !text.ends_with(['.', ':', ';', ',', ')', '?', '!']);
            if after_break && leads && plain_end && text.chars().count() <= 64 && !text.contains(": ") && !text.contains("://") {
                Kind::Heading
            } else {
                Kind::Para
            }
        };
        out.push(Block { line: n, kind, depth, words });
    }
    while out.last().is_some_and(|b| b.kind == Kind::Gap) {
        out.pop();
    }
    out
}
/// The words of a block as inline content: text, with every `http(s)://` word a real link.
pub(crate) fn inline(words: &[(usize, String)]) -> Vec<Html> {
    let mut out = Vec::new();
    let mut run = String::new();
    for (i, (position, word)) in words.iter().enumerate() {
        if i > 0 {
            run.push(' ');
        }
        let url = word.trim_end_matches(['.', ',', ';', ')', ']']);
        if url.starts_with("http://") || url.starts_with("https://") {
            if !run.is_empty() {
                out.push(Html::Text(std::mem::take(&mut run)));
            }
            out.push(link(&format!("body-link-{position}"), url, url));
            run.push_str(&word[url.len()..]);
        } else {
            run.push_str(word);
        }
    }
    if !run.is_empty() {
        out.push(Html::Text(run));
    }
    out
}
/// The first few lines of a body as tiny paragraphs: what a file thumbnail shows.
pub(crate) fn thumbnail(body: &str, lines: usize) -> Html {
    el("div").class("mini").each(parse(body).into_iter().filter(|b| b.kind != Kind::Gap).take(lines), |b| {
        let text = b.words.iter().map(|w| w.1.as_str()).collect::<Vec<_>>().join(" ");
        el("p").class(if b.kind == Kind::Heading { "h" } else { "" }).text(text)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bodies_read_as_headings_lists_todos_and_wrapped_lines_with_stable_word_positions() {
        let body = "Onboarding\nFor your first week.\n\nDay 1\n- [ ] Clone http://github.com/northstar/atlas and run it.\n- [x] Read the wiki\n1. Freeze the branch at\n   http://youtube.com/watch?v=1\n\nRelease code: ATLAS-2026\n";
        let blocks = parse(body);
        let kinds: Vec<&Kind> = blocks.iter().map(|b| &b.kind).collect();
        assert_eq!(
            kinds,
            [&Kind::Heading, &Kind::Para, &Kind::Gap, &Kind::Heading, &Kind::Todo(false), &Kind::Todo(true), &Kind::Numbered("1".into()), &Kind::Gap, &Kind::Para]
        );
        // The link keeps the index `split_whitespace` gives it over the whole body.
        let expected: Vec<usize> = body
            .split_whitespace()
            .enumerate()
            .filter(|(_, w)| w.starts_with("http://"))
            .map(|(i, _)| i)
            .collect();
        let found: Vec<usize> = blocks.iter().flat_map(|b| &b.words).filter(|w| w.1.starts_with("http://")).map(|w| w.0).collect();
        assert_eq!(found, expected);
        let html: String = inline(&blocks[4].words).iter().map(Html::render).collect();
        assert_eq!(html, format!("Clone <a id=\"body-link-{0}\" href=\"http://github.com/northstar/atlas\">http://github.com/northstar/atlas</a> and run it.", expected[0]));
        assert_eq!(blocks[6].words.last().unwrap().1, "http://youtube.com/watch?v=1", "the wrapped line joined its item");
    }
}
