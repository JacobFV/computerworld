//! The HTML tokenizer: the state machine of WHATWG HTML §13.2.5, pulled one token at
//! a time by the tree builder. Character tokens are batched into runs (`Token::Chars`);
//! every other token is emitted on its own. The tree builder switches `state` after a
//! start tag (RAWTEXT for `<style>`, script data for `<script>`, ...) and sets
//! `allow_cdata` when the adjusted current node is a foreign element.
//!
//! The input has already been preprocessed (BOM stripped, CR and CRLF normalised to
//! LF), so no state here mentions U+000D. Parse errors are not reported; the recovery
//! the spec attaches to each is implemented.

use super::entities;
use crate::dom::Attribute;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    pub attrs: Vec<Attribute>,
    pub self_closing: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Doctype {
    pub name: Option<String>,
    pub public_id: Option<String>,
    pub system_id: Option<String>,
    pub force_quirks: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Doctype(Doctype),
    StartTag(Tag),
    EndTag(Tag),
    Comment(String),
    /// A run of character tokens; may contain U+0000, which the tree builder handles.
    Chars(String),
    Eof,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Data,
    Rcdata,
    Rawtext,
    ScriptData,
    Plaintext,
    TagOpen,
    EndTagOpen,
    TagName,
    RcdataLessThan,
    RcdataEndTagOpen,
    RcdataEndTagName,
    RawtextLessThan,
    RawtextEndTagOpen,
    RawtextEndTagName,
    ScriptDataLessThan,
    ScriptDataEndTagOpen,
    ScriptDataEndTagName,
    ScriptDataEscapeStart,
    ScriptDataEscapeStartDash,
    ScriptDataEscaped,
    ScriptDataEscapedDash,
    ScriptDataEscapedDashDash,
    ScriptDataEscapedLessThan,
    ScriptDataEscapedEndTagOpen,
    ScriptDataEscapedEndTagName,
    ScriptDataDoubleEscapeStart,
    ScriptDataDoubleEscaped,
    ScriptDataDoubleEscapedDash,
    ScriptDataDoubleEscapedDashDash,
    ScriptDataDoubleEscapedLessThan,
    ScriptDataDoubleEscapeEnd,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentLessThan,
    CommentLessThanBang,
    CommentLessThanBangDash,
    CommentLessThanBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    DoctypeStart,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    AfterDoctypePublicKeyword,
    BeforeDoctypePublicIdentifier,
    DoctypePublicIdentifierDoubleQuoted,
    DoctypePublicIdentifierSingleQuoted,
    AfterDoctypePublicIdentifier,
    BetweenDoctypePublicAndSystemIdentifiers,
    AfterDoctypeSystemKeyword,
    BeforeDoctypeSystemIdentifier,
    DoctypeSystemIdentifierDoubleQuoted,
    DoctypeSystemIdentifierSingleQuoted,
    AfterDoctypeSystemIdentifier,
    BogusDoctype,
    CdataSection,
    CdataSectionBracket,
    CdataSectionEnd,
    CharacterReference,
    NamedCharacterReference,
    AmbiguousAmpersand,
    NumericCharacterReference,
    HexadecimalCharacterReferenceStart,
    HexadecimalCharacterReference,
    DecimalCharacterReference,
    NumericCharacterReferenceEnd,
}

const REPLACEMENT: char = '\u{FFFD}';

/// Replacements for numeric references in the C1 range (§13.2.5.84).
fn c1_replacement(code: u32) -> Option<u32> {
    Some(match code {
        0x80 => 0x20AC,
        0x82 => 0x201A,
        0x83 => 0x0192,
        0x84 => 0x201E,
        0x85 => 0x2026,
        0x86 => 0x2020,
        0x87 => 0x2021,
        0x88 => 0x02C6,
        0x89 => 0x2030,
        0x8A => 0x0160,
        0x8B => 0x2039,
        0x8C => 0x0152,
        0x8E => 0x017D,
        0x91 => 0x2018,
        0x92 => 0x2019,
        0x93 => 0x201C,
        0x94 => 0x201D,
        0x95 => 0x2022,
        0x96 => 0x2013,
        0x97 => 0x2014,
        0x98 => 0x02DC,
        0x99 => 0x2122,
        0x9A => 0x0161,
        0x9B => 0x203A,
        0x9C => 0x0153,
        0x9E => 0x017E,
        0x9F => 0x0178,
        _ => return None,
    })
}

/// Resolves a numeric character reference code to the character it produces.
pub fn numeric_reference_char(code: u32) -> char {
    if code == 0 || code > 0x10FFFF || (0xD800..=0xDFFF).contains(&code) {
        return REPLACEMENT;
    }
    let code = c1_replacement(code).unwrap_or(code);
    char::from_u32(code).unwrap_or(REPLACEMENT)
}

/// Looks up the longest named character reference at the start of `s` (without the
/// leading `&`). Returns the matched name length and the replacement.
pub fn longest_named_reference(s: &str) -> Option<(usize, &'static str)> {
    let bytes = s.as_bytes();
    let mut alnum = 0;
    while alnum < bytes.len()
        && alnum < entities::LONGEST_NAME
        && bytes[alnum].is_ascii_alphanumeric()
    {
        alnum += 1;
    }
    if alnum == 0 {
        return None;
    }
    // Names are alphanumerics optionally followed by ';'. Try the longest candidate first.
    if alnum < bytes.len() && bytes[alnum] == b';' {
        if let Some(v) = entities::lookup(&s[..alnum + 1]) {
            return Some((alnum + 1, v));
        }
    }
    let mut len = alnum;
    while len > 0 {
        if let Some(v) = entities::lookup(&s[..len]) {
            return Some((len, v));
        }
        len -= 1;
    }
    None
}

pub struct Tokenizer {
    input: String,
    pos: usize,
    pub state: State,
    return_state: State,
    /// Set by the tree builder before each `next`: true when the adjusted current node
    /// is not in the HTML namespace, so `<![CDATA[` opens a CDATA section.
    pub allow_cdata: bool,
    /// Pending character run, flushed before any other token.
    text: String,
    /// A token that must be returned after the pending character run.
    stashed: Option<Token>,
    tag: Tag,
    tag_is_end: bool,
    attr_name: String,
    attr_value: String,
    attr_names: Option<HashSet<String>>,
    comment: String,
    doctype: Doctype,
    temp: String,
    char_ref_code: u32,
    last_start_tag: String,
    eof_emitted: bool,
}

fn is_whitespace(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\x0C' | ' ')
}

impl Tokenizer {
    /// `input` must already be preprocessed (see the module documentation).
    pub fn new(input: String) -> Tokenizer {
        Tokenizer {
            input,
            pos: 0,
            state: State::Data,
            return_state: State::Data,
            allow_cdata: false,
            text: String::new(),
            stashed: None,
            tag: Tag {
                name: String::new(),
                attrs: Vec::new(),
                self_closing: false,
            },
            tag_is_end: false,
            attr_name: String::new(),
            attr_value: String::new(),
            attr_names: None,
            comment: String::new(),
            doctype: Doctype::default(),
            temp: String::new(),
            char_ref_code: 0,
            last_start_tag: String::new(),
            eof_emitted: false,
        }
    }

    /// Inserts `s` into the input at the current position (`document.write`
    /// during parsing). `s` must be preprocessed like the original input.
    pub fn insert(&mut self, s: &str) {
        self.input.insert_str(self.pos, s);
    }

    #[inline]
    fn peek(&self) -> Option<char> {
        let b = *self.input.as_bytes().get(self.pos)?;
        if b < 0x80 {
            Some(b as char)
        } else {
            self.input[self.pos..].chars().next()
        }
    }

    #[inline]
    fn consume(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    #[inline]
    fn reconsume(&mut self, c: char) {
        self.pos -= c.len_utf8();
    }

    fn starts_with_ci(&self, s: &str) -> bool {
        let rest = &self.input.as_bytes()[self.pos..];
        rest.len() >= s.len() && rest[..s.len()].eq_ignore_ascii_case(s.as_bytes())
    }

    fn starts_with(&self, s: &str) -> bool {
        self.input.as_bytes()[self.pos..].starts_with(s.as_bytes())
    }

    fn emit_char(&mut self, c: char) {
        self.text.push(c);
    }

    fn emit_str(&mut self, s: &str) {
        self.text.push_str(s);
    }

    /// Emits the pending text run (if any) before `t`.
    fn emit(&mut self, t: Token) -> Option<Token> {
        if self.text.is_empty() {
            Some(t)
        } else {
            self.stashed = Some(t);
            Some(Token::Chars(std::mem::take(&mut self.text)))
        }
    }

    fn emit_eof(&mut self) -> Option<Token> {
        self.eof_emitted = true;
        self.emit(Token::Eof)
    }

    fn start_tag(&mut self, is_end: bool) {
        self.tag = Tag {
            name: String::new(),
            attrs: Vec::new(),
            self_closing: false,
        };
        self.tag_is_end = is_end;
        self.attr_names = None;
    }

    fn start_attribute(&mut self) {
        self.attr_name.clear();
        self.attr_value.clear();
    }

    /// Called when leaving the attribute name state: drops duplicates.
    fn finish_attribute_name(&mut self) {
        let dup = if self.tag.attrs.len() < 32 {
            self.tag.attrs.iter().any(|a| a.name == self.attr_name)
        } else {
            let set = self.attr_names.get_or_insert_with(HashSet::new);
            if set.is_empty() {
                for a in &self.tag.attrs {
                    set.insert(a.name.clone());
                }
            }
            set.contains(&self.attr_name)
        };
        if dup {
            // Keep parsing the value into a throwaway attribute.
            self.attr_name.clear();
            self.attr_name.push('\0');
        }
    }

    fn finish_attribute(&mut self) {
        if self.attr_name.as_bytes().first() == Some(&0) && self.attr_name.len() == 1 {
            // A dropped duplicate.
            self.attr_value.clear();
            return;
        }
        if self.attr_name.is_empty() {
            return;
        }
        if let Some(set) = &mut self.attr_names {
            set.insert(self.attr_name.clone());
        }
        self.tag.attrs.push(Attribute {
            name: std::mem::take(&mut self.attr_name),
            value: std::mem::take(&mut self.attr_value),
        });
    }

    fn emit_tag(&mut self) -> Option<Token> {
        let tag = std::mem::replace(
            &mut self.tag,
            Tag {
                name: String::new(),
                attrs: Vec::new(),
                self_closing: false,
            },
        );
        if self.tag_is_end {
            self.emit(Token::EndTag(tag))
        } else {
            self.last_start_tag.clear();
            self.last_start_tag.push_str(&tag.name);
            self.emit(Token::StartTag(tag))
        }
    }

    fn emit_comment(&mut self) -> Option<Token> {
        let c = std::mem::take(&mut self.comment);
        self.emit(Token::Comment(c))
    }

    fn emit_doctype(&mut self) -> Option<Token> {
        let d = std::mem::take(&mut self.doctype);
        self.emit(Token::Doctype(d))
    }

    fn is_appropriate_end_tag(&self) -> bool {
        self.tag_is_end && self.tag.name == self.last_start_tag
    }

    fn in_attribute(&self) -> bool {
        matches!(
            self.return_state,
            State::AttributeValueDoubleQuoted
                | State::AttributeValueSingleQuoted
                | State::AttributeValueUnquoted
        )
    }

    /// Flush code points consumed as a character reference.
    fn flush_char_ref(&mut self) {
        if self.in_attribute() {
            self.attr_value.push_str(&self.temp);
        } else {
            let t = std::mem::take(&mut self.temp);
            self.text.push_str(&t);
            self.temp = t;
        }
        self.temp.clear();
    }

    /// Appends `c` to the current attribute value or emits it, for the ambiguous
    /// ampersand state.
    fn char_ref_char(&mut self, c: char) {
        if self.in_attribute() {
            self.attr_value.push(c);
        } else {
            self.text.push(c);
        }
    }

    /// Consumes bytes in the data-like states up to (not including) the next byte in
    /// `stops`, appending them to the text run. Returns whether anything was consumed.
    fn take_until(&mut self, stops: &[u8]) -> bool {
        let bytes = self.input.as_bytes();
        let start = self.pos;
        let mut i = start;
        while i < bytes.len() && !stops.contains(&bytes[i]) {
            i += 1;
        }
        if i > start {
            self.text.push_str(&self.input[start..i]);
            self.pos = i;
            true
        } else {
            false
        }
    }

    /// The next token. After `Token::Eof` has been returned, keeps returning it.
    pub fn next(&mut self) -> Token {
        if let Some(t) = self.stashed.take() {
            return t;
        }
        if self.eof_emitted {
            return Token::Eof;
        }
        loop {
            if let Some(t) = self.step() {
                return t;
            }
        }
    }

    fn step(&mut self) -> Option<Token> {
        use State::*;
        match self.state {
            Data => {
                self.take_until(&[b'&', b'<', 0]);
                match self.consume() {
                    Some('&') => {
                        self.return_state = Data;
                        self.state = CharacterReference;
                    }
                    Some('<') => self.state = TagOpen,
                    Some('\0') => self.emit_char('\0'),
                    Some(c) => self.emit_char(c),
                    None => return self.emit_eof(),
                }
                None
            }
            Rcdata => {
                self.take_until(&[b'&', b'<', 0]);
                match self.consume() {
                    Some('&') => {
                        self.return_state = Rcdata;
                        self.state = CharacterReference;
                    }
                    Some('<') => self.state = RcdataLessThan,
                    Some('\0') => self.emit_char(REPLACEMENT),
                    Some(c) => self.emit_char(c),
                    None => return self.emit_eof(),
                }
                None
            }
            Rawtext => {
                self.take_until(&[b'<', 0]);
                match self.consume() {
                    Some('<') => self.state = RawtextLessThan,
                    Some('\0') => self.emit_char(REPLACEMENT),
                    Some(c) => self.emit_char(c),
                    None => return self.emit_eof(),
                }
                None
            }
            ScriptData => {
                self.take_until(&[b'<', 0]);
                match self.consume() {
                    Some('<') => self.state = ScriptDataLessThan,
                    Some('\0') => self.emit_char(REPLACEMENT),
                    Some(c) => self.emit_char(c),
                    None => return self.emit_eof(),
                }
                None
            }
            Plaintext => {
                self.take_until(&[0]);
                match self.consume() {
                    Some('\0') => self.emit_char(REPLACEMENT),
                    Some(c) => self.emit_char(c),
                    None => return self.emit_eof(),
                }
                None
            }
            TagOpen => {
                match self.consume() {
                    Some('!') => self.state = MarkupDeclarationOpen,
                    Some('/') => self.state = EndTagOpen,
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.start_tag(false);
                        self.reconsume(c);
                        self.state = TagName;
                    }
                    Some('?') => {
                        // The pre-2026 behaviour: a bogus comment. The DOM has no
                        // processing instruction node.
                        self.comment.clear();
                        self.reconsume('?');
                        self.state = BogusComment;
                    }
                    Some(c) => {
                        self.emit_char('<');
                        self.reconsume(c);
                        self.state = Data;
                    }
                    None => {
                        self.emit_char('<');
                        return self.emit_eof();
                    }
                }
                None
            }
            EndTagOpen => {
                match self.consume() {
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.start_tag(true);
                        self.reconsume(c);
                        self.state = TagName;
                    }
                    Some('>') => self.state = Data,
                    Some(c) => {
                        self.comment.clear();
                        self.reconsume(c);
                        self.state = BogusComment;
                    }
                    None => {
                        self.emit_str("</");
                        return self.emit_eof();
                    }
                }
                None
            }
            TagName => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => self.state = BeforeAttributeName,
                    Some('/') => self.state = SelfClosingStartTag,
                    Some('>') => {
                        self.state = Data;
                        return self.emit_tag();
                    }
                    Some('\0') => self.tag.name.push(REPLACEMENT),
                    Some(c) => self.tag.name.push(c.to_ascii_lowercase()),
                    None => return self.emit_eof(),
                }
                None
            }
            RcdataLessThan => {
                match self.consume() {
                    Some('/') => {
                        self.temp.clear();
                        self.state = RcdataEndTagOpen;
                    }
                    Some(c) => {
                        self.emit_char('<');
                        self.reconsume(c);
                        self.state = Rcdata;
                    }
                    None => {
                        self.emit_char('<');
                        self.state = Rcdata;
                    }
                }
                None
            }
            RcdataEndTagOpen => {
                match self.consume() {
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.start_tag(true);
                        self.reconsume(c);
                        self.state = RcdataEndTagName;
                    }
                    Some(c) => {
                        self.emit_str("</");
                        self.reconsume(c);
                        self.state = Rcdata;
                    }
                    None => {
                        self.emit_str("</");
                        self.state = Rcdata;
                    }
                }
                None
            }
            RcdataEndTagName
            | RawtextEndTagName
            | ScriptDataEndTagName
            | ScriptDataEscapedEndTagName => {
                let back = match self.state {
                    RcdataEndTagName => Rcdata,
                    RawtextEndTagName => Rawtext,
                    ScriptDataEndTagName => ScriptData,
                    _ => ScriptDataEscaped,
                };
                match self.consume() {
                    Some(c) if is_whitespace(c) && self.is_appropriate_end_tag() => {
                        self.state = BeforeAttributeName
                    }
                    Some('/') if self.is_appropriate_end_tag() => self.state = SelfClosingStartTag,
                    Some('>') if self.is_appropriate_end_tag() => {
                        self.state = Data;
                        return self.emit_tag();
                    }
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.tag.name.push(c.to_ascii_lowercase());
                        self.temp.push(c);
                    }
                    other => {
                        self.emit_str("</");
                        let t = std::mem::take(&mut self.temp);
                        self.emit_str(&t);
                        if let Some(c) = other {
                            self.reconsume(c);
                        }
                        self.state = back;
                    }
                }
                None
            }
            RawtextLessThan => {
                match self.consume() {
                    Some('/') => {
                        self.temp.clear();
                        self.state = RawtextEndTagOpen;
                    }
                    Some(c) => {
                        self.emit_char('<');
                        self.reconsume(c);
                        self.state = Rawtext;
                    }
                    None => {
                        self.emit_char('<');
                        self.state = Rawtext;
                    }
                }
                None
            }
            RawtextEndTagOpen => {
                match self.consume() {
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.start_tag(true);
                        self.reconsume(c);
                        self.state = RawtextEndTagName;
                    }
                    Some(c) => {
                        self.emit_str("</");
                        self.reconsume(c);
                        self.state = Rawtext;
                    }
                    None => {
                        self.emit_str("</");
                        self.state = Rawtext;
                    }
                }
                None
            }
            ScriptDataLessThan => {
                match self.consume() {
                    Some('/') => {
                        self.temp.clear();
                        self.state = ScriptDataEndTagOpen;
                    }
                    Some('!') => {
                        self.emit_str("<!");
                        self.state = ScriptDataEscapeStart;
                    }
                    Some(c) => {
                        self.emit_char('<');
                        self.reconsume(c);
                        self.state = ScriptData;
                    }
                    None => {
                        self.emit_char('<');
                        self.state = ScriptData;
                    }
                }
                None
            }
            ScriptDataEndTagOpen => {
                match self.consume() {
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.start_tag(true);
                        self.reconsume(c);
                        self.state = ScriptDataEndTagName;
                    }
                    Some(c) => {
                        self.emit_str("</");
                        self.reconsume(c);
                        self.state = ScriptData;
                    }
                    None => {
                        self.emit_str("</");
                        self.state = ScriptData;
                    }
                }
                None
            }
            ScriptDataEscapeStart => {
                match self.consume() {
                    Some('-') => {
                        self.emit_char('-');
                        self.state = ScriptDataEscapeStartDash;
                    }
                    Some(c) => {
                        self.reconsume(c);
                        self.state = ScriptData;
                    }
                    None => self.state = ScriptData,
                }
                None
            }
            ScriptDataEscapeStartDash => {
                match self.consume() {
                    Some('-') => {
                        self.emit_char('-');
                        self.state = ScriptDataEscapedDashDash;
                    }
                    Some(c) => {
                        self.reconsume(c);
                        self.state = ScriptData;
                    }
                    None => self.state = ScriptData,
                }
                None
            }
            ScriptDataEscaped => {
                self.take_until(&[b'-', b'<', 0]);
                match self.consume() {
                    Some('-') => {
                        self.emit_char('-');
                        self.state = ScriptDataEscapedDash;
                    }
                    Some('<') => self.state = ScriptDataEscapedLessThan,
                    Some('\0') => self.emit_char(REPLACEMENT),
                    Some(c) => self.emit_char(c),
                    None => return self.emit_eof(),
                }
                None
            }
            ScriptDataEscapedDash => {
                match self.consume() {
                    Some('-') => {
                        self.emit_char('-');
                        self.state = ScriptDataEscapedDashDash;
                    }
                    Some('<') => self.state = ScriptDataEscapedLessThan,
                    Some('\0') => {
                        self.emit_char(REPLACEMENT);
                        self.state = ScriptDataEscaped;
                    }
                    Some(c) => {
                        self.emit_char(c);
                        self.state = ScriptDataEscaped;
                    }
                    None => return self.emit_eof(),
                }
                None
            }
            ScriptDataEscapedDashDash => {
                match self.consume() {
                    Some('-') => self.emit_char('-'),
                    Some('<') => self.state = ScriptDataEscapedLessThan,
                    Some('>') => {
                        self.emit_char('>');
                        self.state = ScriptData;
                    }
                    Some('\0') => {
                        self.emit_char(REPLACEMENT);
                        self.state = ScriptDataEscaped;
                    }
                    Some(c) => {
                        self.emit_char(c);
                        self.state = ScriptDataEscaped;
                    }
                    None => return self.emit_eof(),
                }
                None
            }
            ScriptDataEscapedLessThan => {
                match self.consume() {
                    Some('/') => {
                        self.temp.clear();
                        self.state = ScriptDataEscapedEndTagOpen;
                    }
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.temp.clear();
                        self.emit_char('<');
                        self.reconsume(c);
                        self.state = ScriptDataDoubleEscapeStart;
                    }
                    Some(c) => {
                        self.emit_char('<');
                        self.reconsume(c);
                        self.state = ScriptDataEscaped;
                    }
                    None => {
                        self.emit_char('<');
                        self.state = ScriptDataEscaped;
                    }
                }
                None
            }
            ScriptDataEscapedEndTagOpen => {
                match self.consume() {
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.start_tag(true);
                        self.reconsume(c);
                        self.state = ScriptDataEscapedEndTagName;
                    }
                    Some(c) => {
                        self.emit_str("</");
                        self.reconsume(c);
                        self.state = ScriptDataEscaped;
                    }
                    None => {
                        self.emit_str("</");
                        self.state = ScriptDataEscaped;
                    }
                }
                None
            }
            ScriptDataDoubleEscapeStart | ScriptDataDoubleEscapeEnd => {
                let starting = self.state == ScriptDataDoubleEscapeStart;
                match self.consume() {
                    Some(c) if is_whitespace(c) || c == '/' || c == '>' => {
                        let is_script = self.temp == "script";
                        self.state = if is_script == starting {
                            ScriptDataDoubleEscaped
                        } else {
                            ScriptDataEscaped
                        };
                        self.emit_char(c);
                    }
                    Some(c) if c.is_ascii_alphabetic() => {
                        self.temp.push(c.to_ascii_lowercase());
                        self.emit_char(c);
                    }
                    Some(c) => {
                        self.reconsume(c);
                        self.state = if starting {
                            ScriptDataEscaped
                        } else {
                            ScriptDataDoubleEscaped
                        };
                    }
                    None => {
                        self.state = if starting {
                            ScriptDataEscaped
                        } else {
                            ScriptDataDoubleEscaped
                        };
                    }
                }
                None
            }
            ScriptDataDoubleEscaped => {
                self.take_until(&[b'-', b'<', 0]);
                match self.consume() {
                    Some('-') => {
                        self.emit_char('-');
                        self.state = ScriptDataDoubleEscapedDash;
                    }
                    Some('<') => {
                        self.emit_char('<');
                        self.state = ScriptDataDoubleEscapedLessThan;
                    }
                    Some('\0') => self.emit_char(REPLACEMENT),
                    Some(c) => self.emit_char(c),
                    None => return self.emit_eof(),
                }
                None
            }
            ScriptDataDoubleEscapedDash => {
                match self.consume() {
                    Some('-') => {
                        self.emit_char('-');
                        self.state = ScriptDataDoubleEscapedDashDash;
                    }
                    Some('<') => {
                        self.emit_char('<');
                        self.state = ScriptDataDoubleEscapedLessThan;
                    }
                    Some('\0') => {
                        self.emit_char(REPLACEMENT);
                        self.state = ScriptDataDoubleEscaped;
                    }
                    Some(c) => {
                        self.emit_char(c);
                        self.state = ScriptDataDoubleEscaped;
                    }
                    None => return self.emit_eof(),
                }
                None
            }
            ScriptDataDoubleEscapedDashDash => {
                match self.consume() {
                    Some('-') => self.emit_char('-'),
                    Some('<') => {
                        self.emit_char('<');
                        self.state = ScriptDataDoubleEscapedLessThan;
                    }
                    Some('>') => {
                        self.emit_char('>');
                        self.state = ScriptData;
                    }
                    Some('\0') => {
                        self.emit_char(REPLACEMENT);
                        self.state = ScriptDataDoubleEscaped;
                    }
                    Some(c) => {
                        self.emit_char(c);
                        self.state = ScriptDataDoubleEscaped;
                    }
                    None => return self.emit_eof(),
                }
                None
            }
            ScriptDataDoubleEscapedLessThan => {
                match self.consume() {
                    Some('/') => {
                        self.temp.clear();
                        self.emit_char('/');
                        self.state = ScriptDataDoubleEscapeEnd;
                    }
                    Some(c) => {
                        self.reconsume(c);
                        self.state = ScriptDataDoubleEscaped;
                    }
                    None => self.state = ScriptDataDoubleEscaped,
                }
                None
            }
            BeforeAttributeName => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => {}
                    Some(c @ ('/' | '>')) => {
                        self.reconsume(c);
                        self.state = AfterAttributeName;
                    }
                    Some('=') => {
                        self.start_attribute();
                        self.attr_name.push('=');
                        self.state = AttributeName;
                    }
                    Some(c) => {
                        self.start_attribute();
                        self.reconsume(c);
                        self.state = AttributeName;
                    }
                    None => self.state = AfterAttributeName,
                }
                None
            }
            AttributeName => {
                match self.consume() {
                    Some(c) if is_whitespace(c) || c == '/' || c == '>' => {
                        self.finish_attribute_name();
                        self.reconsume(c);
                        self.state = AfterAttributeName;
                    }
                    Some('=') => {
                        self.finish_attribute_name();
                        self.state = BeforeAttributeValue;
                    }
                    Some('\0') => self.attr_name.push(REPLACEMENT),
                    Some(c) => self.attr_name.push(c.to_ascii_lowercase()),
                    None => {
                        self.finish_attribute_name();
                        self.state = AfterAttributeName;
                    }
                }
                None
            }
            AfterAttributeName => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => {}
                    Some('/') => {
                        self.finish_attribute();
                        self.state = SelfClosingStartTag;
                    }
                    Some('=') => self.state = BeforeAttributeValue,
                    Some('>') => {
                        self.finish_attribute();
                        self.state = Data;
                        return self.emit_tag();
                    }
                    Some(c) => {
                        self.finish_attribute();
                        self.start_attribute();
                        self.reconsume(c);
                        self.state = AttributeName;
                    }
                    None => {
                        self.finish_attribute();
                        return self.emit_eof();
                    }
                }
                None
            }
            BeforeAttributeValue => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => {}
                    Some('"') => self.state = AttributeValueDoubleQuoted,
                    Some('\'') => self.state = AttributeValueSingleQuoted,
                    Some('>') => {
                        self.finish_attribute();
                        self.state = Data;
                        return self.emit_tag();
                    }
                    Some(c) => {
                        self.reconsume(c);
                        self.state = AttributeValueUnquoted;
                    }
                    None => self.state = AttributeValueUnquoted,
                }
                None
            }
            AttributeValueDoubleQuoted | AttributeValueSingleQuoted => {
                let quote = if self.state == AttributeValueDoubleQuoted {
                    '"'
                } else {
                    '\''
                };
                // Fast path over plain bytes.
                {
                    let bytes = self.input.as_bytes();
                    let start = self.pos;
                    let mut i = start;
                    while i < bytes.len()
                        && bytes[i] != quote as u8
                        && bytes[i] != b'&'
                        && bytes[i] != 0
                    {
                        i += 1;
                    }
                    if i > start {
                        self.attr_value.push_str(&self.input[start..i]);
                        self.pos = i;
                    }
                }
                match self.consume() {
                    Some(c) if c == quote => self.state = AfterAttributeValueQuoted,
                    Some('&') => {
                        self.return_state = self.state;
                        self.state = CharacterReference;
                    }
                    Some('\0') => self.attr_value.push(REPLACEMENT),
                    Some(c) => self.attr_value.push(c),
                    None => {
                        self.finish_attribute();
                        return self.emit_eof();
                    }
                }
                None
            }
            AttributeValueUnquoted => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => {
                        self.finish_attribute();
                        self.state = BeforeAttributeName;
                    }
                    Some('&') => {
                        self.return_state = AttributeValueUnquoted;
                        self.state = CharacterReference;
                    }
                    Some('>') => {
                        self.finish_attribute();
                        self.state = Data;
                        return self.emit_tag();
                    }
                    Some('\0') => self.attr_value.push(REPLACEMENT),
                    Some(c) => self.attr_value.push(c),
                    None => {
                        self.finish_attribute();
                        return self.emit_eof();
                    }
                }
                None
            }
            AfterAttributeValueQuoted => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => {
                        self.finish_attribute();
                        self.state = BeforeAttributeName;
                    }
                    Some('/') => {
                        self.finish_attribute();
                        self.state = SelfClosingStartTag;
                    }
                    Some('>') => {
                        self.finish_attribute();
                        self.state = Data;
                        return self.emit_tag();
                    }
                    Some(c) => {
                        self.finish_attribute();
                        self.reconsume(c);
                        self.state = BeforeAttributeName;
                    }
                    None => {
                        self.finish_attribute();
                        return self.emit_eof();
                    }
                }
                None
            }
            SelfClosingStartTag => {
                match self.consume() {
                    Some('>') => {
                        self.tag.self_closing = true;
                        self.state = Data;
                        return self.emit_tag();
                    }
                    Some(c) => {
                        self.reconsume(c);
                        self.state = BeforeAttributeName;
                    }
                    None => return self.emit_eof(),
                }
                None
            }
            BogusComment => {
                match self.consume() {
                    Some('>') => {
                        self.state = Data;
                        return self.emit_comment();
                    }
                    Some('\0') => self.comment.push(REPLACEMENT),
                    Some(c) => self.comment.push(c),
                    None => return self.comment_eof(),
                }
                None
            }
            MarkupDeclarationOpen => {
                if self.starts_with("--") {
                    self.pos += 2;
                    self.comment.clear();
                    self.state = CommentStart;
                } else if self.starts_with_ci("DOCTYPE") {
                    self.pos += 7;
                    self.state = DoctypeStart;
                } else if self.starts_with("[CDATA[") {
                    self.pos += 7;
                    if self.allow_cdata {
                        self.state = CdataSection;
                    } else {
                        self.comment.clear();
                        self.comment.push_str("[CDATA[");
                        self.state = BogusComment;
                    }
                } else {
                    self.comment.clear();
                    self.state = BogusComment;
                }
                None
            }
            CommentStart => {
                match self.consume() {
                    Some('-') => self.state = CommentStartDash,
                    Some('>') => {
                        self.state = Data;
                        return self.emit_comment();
                    }
                    Some(c) => {
                        self.reconsume(c);
                        self.state = Comment;
                    }
                    None => self.state = Comment,
                }
                None
            }
            CommentStartDash => {
                match self.consume() {
                    Some('-') => self.state = CommentEnd,
                    Some('>') => {
                        self.state = Data;
                        return self.emit_comment();
                    }
                    Some(c) => {
                        self.comment.push('-');
                        self.reconsume(c);
                        self.state = Comment;
                    }
                    None => return self.comment_eof(),
                }
                None
            }
            Comment => {
                {
                    let bytes = self.input.as_bytes();
                    let start = self.pos;
                    let mut i = start;
                    while i < bytes.len() && bytes[i] != b'<' && bytes[i] != b'-' && bytes[i] != 0 {
                        i += 1;
                    }
                    if i > start {
                        self.comment.push_str(&self.input[start..i]);
                        self.pos = i;
                    }
                }
                match self.consume() {
                    Some('<') => {
                        self.comment.push('<');
                        self.state = CommentLessThan;
                    }
                    Some('-') => self.state = CommentEndDash,
                    Some('\0') => self.comment.push(REPLACEMENT),
                    Some(c) => self.comment.push(c),
                    None => return self.comment_eof(),
                }
                None
            }
            CommentLessThan => {
                match self.consume() {
                    Some('!') => {
                        self.comment.push('!');
                        self.state = CommentLessThanBang;
                    }
                    Some('<') => self.comment.push('<'),
                    Some(c) => {
                        self.reconsume(c);
                        self.state = Comment;
                    }
                    None => self.state = Comment,
                }
                None
            }
            CommentLessThanBang => {
                match self.consume() {
                    Some('-') => self.state = CommentLessThanBangDash,
                    Some(c) => {
                        self.reconsume(c);
                        self.state = Comment;
                    }
                    None => self.state = Comment,
                }
                None
            }
            CommentLessThanBangDash => {
                match self.consume() {
                    Some('-') => self.state = CommentLessThanBangDashDash,
                    Some(c) => {
                        self.reconsume(c);
                        self.state = CommentEndDash;
                    }
                    None => self.state = CommentEndDash,
                }
                None
            }
            CommentLessThanBangDashDash => {
                match self.consume() {
                    Some('>') => {
                        self.reconsume('>');
                        self.state = CommentEnd;
                    }
                    Some(c) => {
                        self.reconsume(c);
                        self.state = CommentEnd;
                    }
                    None => self.state = CommentEnd,
                }
                None
            }
            CommentEndDash => {
                match self.consume() {
                    Some('-') => self.state = CommentEnd,
                    Some(c) => {
                        self.comment.push('-');
                        self.reconsume(c);
                        self.state = Comment;
                    }
                    None => return self.comment_eof(),
                }
                None
            }
            CommentEnd => {
                match self.consume() {
                    Some('>') => {
                        self.state = Data;
                        return self.emit_comment();
                    }
                    Some('!') => self.state = CommentEndBang,
                    Some('-') => self.comment.push('-'),
                    Some(c) => {
                        self.comment.push_str("--");
                        self.reconsume(c);
                        self.state = Comment;
                    }
                    None => return self.comment_eof(),
                }
                None
            }
            CommentEndBang => {
                match self.consume() {
                    Some('-') => {
                        self.comment.push_str("--!");
                        self.state = CommentEndDash;
                    }
                    Some('>') => {
                        self.state = Data;
                        return self.emit_comment();
                    }
                    Some(c) => {
                        self.comment.push_str("--!");
                        self.reconsume(c);
                        self.state = Comment;
                    }
                    None => return self.comment_eof(),
                }
                None
            }
            DoctypeStart => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => self.state = BeforeDoctypeName,
                    Some('>') => {
                        self.reconsume('>');
                        self.state = BeforeDoctypeName;
                    }
                    Some(c) => {
                        self.reconsume(c);
                        self.state = BeforeDoctypeName;
                    }
                    None => {
                        self.doctype = Doctype {
                            force_quirks: true,
                            ..Default::default()
                        };
                        return self.doctype_eof();
                    }
                }
                None
            }
            BeforeDoctypeName => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => {}
                    Some('\0') => {
                        self.doctype = Doctype {
                            name: Some(REPLACEMENT.to_string()),
                            ..Default::default()
                        };
                        self.state = DoctypeName;
                    }
                    Some('>') => {
                        self.doctype = Doctype {
                            force_quirks: true,
                            ..Default::default()
                        };
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some(c) => {
                        self.doctype = Doctype {
                            name: Some(c.to_ascii_lowercase().to_string()),
                            ..Default::default()
                        };
                        self.state = DoctypeName;
                    }
                    None => {
                        self.doctype = Doctype {
                            force_quirks: true,
                            ..Default::default()
                        };
                        return self.doctype_eof();
                    }
                }
                None
            }
            DoctypeName => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => self.state = AfterDoctypeName,
                    Some('>') => {
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some('\0') => self
                        .doctype
                        .name
                        .get_or_insert_with(String::new)
                        .push(REPLACEMENT),
                    Some(c) => self
                        .doctype
                        .name
                        .get_or_insert_with(String::new)
                        .push(c.to_ascii_lowercase()),
                    None => {
                        self.doctype.force_quirks = true;
                        return self.doctype_eof();
                    }
                }
                None
            }
            AfterDoctypeName => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => {}
                    Some('>') => {
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some(c) => {
                        self.reconsume(c);
                        if self.starts_with_ci("PUBLIC") {
                            self.pos += 6;
                            self.state = AfterDoctypePublicKeyword;
                        } else if self.starts_with_ci("SYSTEM") {
                            self.pos += 6;
                            self.state = AfterDoctypeSystemKeyword;
                        } else {
                            self.consume();
                            self.doctype.force_quirks = true;
                            self.state = BogusDoctype;
                        }
                    }
                    None => {
                        self.doctype.force_quirks = true;
                        return self.doctype_eof();
                    }
                }
                None
            }
            AfterDoctypePublicKeyword => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => self.state = BeforeDoctypePublicIdentifier,
                    Some('"') => {
                        self.doctype.public_id = Some(String::new());
                        self.state = DoctypePublicIdentifierDoubleQuoted;
                    }
                    Some('\'') => {
                        self.doctype.public_id = Some(String::new());
                        self.state = DoctypePublicIdentifierSingleQuoted;
                    }
                    Some('>') => {
                        self.doctype.force_quirks = true;
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some(_) => {
                        self.doctype.force_quirks = true;
                        self.state = BogusDoctype;
                    }
                    None => {
                        self.doctype.force_quirks = true;
                        return self.doctype_eof();
                    }
                }
                None
            }
            BeforeDoctypePublicIdentifier => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => {}
                    Some('"') => {
                        self.doctype.public_id = Some(String::new());
                        self.state = DoctypePublicIdentifierDoubleQuoted;
                    }
                    Some('\'') => {
                        self.doctype.public_id = Some(String::new());
                        self.state = DoctypePublicIdentifierSingleQuoted;
                    }
                    Some('>') => {
                        self.doctype.force_quirks = true;
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some(_) => {
                        self.doctype.force_quirks = true;
                        self.state = BogusDoctype;
                    }
                    None => {
                        self.doctype.force_quirks = true;
                        return self.doctype_eof();
                    }
                }
                None
            }
            DoctypePublicIdentifierDoubleQuoted | DoctypePublicIdentifierSingleQuoted => {
                let quote = if self.state == DoctypePublicIdentifierDoubleQuoted {
                    '"'
                } else {
                    '\''
                };
                match self.consume() {
                    Some(c) if c == quote => self.state = AfterDoctypePublicIdentifier,
                    Some('\0') => self
                        .doctype
                        .public_id
                        .get_or_insert_with(String::new)
                        .push(REPLACEMENT),
                    Some('>') => {
                        self.doctype.force_quirks = true;
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some(c) => self
                        .doctype
                        .public_id
                        .get_or_insert_with(String::new)
                        .push(c),
                    None => {
                        self.doctype.force_quirks = true;
                        return self.doctype_eof();
                    }
                }
                None
            }
            AfterDoctypePublicIdentifier => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => {
                        self.state = BetweenDoctypePublicAndSystemIdentifiers
                    }
                    Some('>') => {
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some('"') => {
                        self.doctype.system_id = Some(String::new());
                        self.state = DoctypeSystemIdentifierDoubleQuoted;
                    }
                    Some('\'') => {
                        self.doctype.system_id = Some(String::new());
                        self.state = DoctypeSystemIdentifierSingleQuoted;
                    }
                    Some(_) => {
                        self.doctype.force_quirks = true;
                        self.state = BogusDoctype;
                    }
                    None => {
                        self.doctype.force_quirks = true;
                        return self.doctype_eof();
                    }
                }
                None
            }
            BetweenDoctypePublicAndSystemIdentifiers => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => {}
                    Some('>') => {
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some('"') => {
                        self.doctype.system_id = Some(String::new());
                        self.state = DoctypeSystemIdentifierDoubleQuoted;
                    }
                    Some('\'') => {
                        self.doctype.system_id = Some(String::new());
                        self.state = DoctypeSystemIdentifierSingleQuoted;
                    }
                    Some(_) => {
                        self.doctype.force_quirks = true;
                        self.state = BogusDoctype;
                    }
                    None => {
                        self.doctype.force_quirks = true;
                        return self.doctype_eof();
                    }
                }
                None
            }
            AfterDoctypeSystemKeyword => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => self.state = BeforeDoctypeSystemIdentifier,
                    Some('"') => {
                        self.doctype.system_id = Some(String::new());
                        self.state = DoctypeSystemIdentifierDoubleQuoted;
                    }
                    Some('\'') => {
                        self.doctype.system_id = Some(String::new());
                        self.state = DoctypeSystemIdentifierSingleQuoted;
                    }
                    Some('>') => {
                        self.doctype.force_quirks = true;
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some(_) => {
                        self.doctype.force_quirks = true;
                        self.state = BogusDoctype;
                    }
                    None => {
                        self.doctype.force_quirks = true;
                        return self.doctype_eof();
                    }
                }
                None
            }
            BeforeDoctypeSystemIdentifier => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => {}
                    Some('"') => {
                        self.doctype.system_id = Some(String::new());
                        self.state = DoctypeSystemIdentifierDoubleQuoted;
                    }
                    Some('\'') => {
                        self.doctype.system_id = Some(String::new());
                        self.state = DoctypeSystemIdentifierSingleQuoted;
                    }
                    Some('>') => {
                        self.doctype.force_quirks = true;
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some(_) => {
                        self.doctype.force_quirks = true;
                        self.state = BogusDoctype;
                    }
                    None => {
                        self.doctype.force_quirks = true;
                        return self.doctype_eof();
                    }
                }
                None
            }
            DoctypeSystemIdentifierDoubleQuoted | DoctypeSystemIdentifierSingleQuoted => {
                let quote = if self.state == DoctypeSystemIdentifierDoubleQuoted {
                    '"'
                } else {
                    '\''
                };
                match self.consume() {
                    Some(c) if c == quote => self.state = AfterDoctypeSystemIdentifier,
                    Some('\0') => self
                        .doctype
                        .system_id
                        .get_or_insert_with(String::new)
                        .push(REPLACEMENT),
                    Some('>') => {
                        self.doctype.force_quirks = true;
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some(c) => self
                        .doctype
                        .system_id
                        .get_or_insert_with(String::new)
                        .push(c),
                    None => {
                        self.doctype.force_quirks = true;
                        return self.doctype_eof();
                    }
                }
                None
            }
            AfterDoctypeSystemIdentifier => {
                match self.consume() {
                    Some(c) if is_whitespace(c) => {}
                    Some('>') => {
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some(_) => self.state = BogusDoctype,
                    None => {
                        self.doctype.force_quirks = true;
                        return self.doctype_eof();
                    }
                }
                None
            }
            BogusDoctype => {
                match self.consume() {
                    Some('>') => {
                        self.state = Data;
                        return self.emit_doctype();
                    }
                    Some(_) => {}
                    None => return self.doctype_eof(),
                }
                None
            }
            CdataSection => {
                self.take_until(b"]");
                match self.consume() {
                    Some(']') => self.state = CdataSectionBracket,
                    Some(c) => self.emit_char(c),
                    None => return self.emit_eof(),
                }
                None
            }
            CdataSectionBracket => {
                match self.consume() {
                    Some(']') => self.state = CdataSectionEnd,
                    Some(c) => {
                        self.emit_char(']');
                        self.reconsume(c);
                        self.state = CdataSection;
                    }
                    None => {
                        self.emit_char(']');
                        self.state = CdataSection;
                    }
                }
                None
            }
            CdataSectionEnd => {
                match self.consume() {
                    Some(']') => self.emit_char(']'),
                    Some('>') => self.state = Data,
                    Some(c) => {
                        self.emit_str("]]");
                        self.reconsume(c);
                        self.state = CdataSection;
                    }
                    None => {
                        self.emit_str("]]");
                        self.state = CdataSection;
                    }
                }
                None
            }
            CharacterReference => {
                self.temp.clear();
                self.temp.push('&');
                match self.consume() {
                    Some(c) if c.is_ascii_alphanumeric() => {
                        self.reconsume(c);
                        self.state = NamedCharacterReference;
                    }
                    Some('#') => {
                        self.temp.push('#');
                        self.state = NumericCharacterReference;
                    }
                    other => {
                        self.flush_char_ref();
                        if let Some(c) = other {
                            self.reconsume(c);
                        }
                        self.state = self.return_state;
                    }
                }
                None
            }
            NamedCharacterReference => {
                let rest = &self.input[self.pos..];
                match longest_named_reference(rest) {
                    Some((len, value)) => {
                        let matched_semicolon = rest.as_bytes()[len - 1] == b';';
                        let next = rest.as_bytes().get(len).copied();
                        if self.in_attribute()
                            && !matched_semicolon
                            && next.is_some_and(|b| b == b'=' || b.is_ascii_alphanumeric())
                        {
                            self.temp.push_str(&rest[..len]);
                            self.pos += len;
                            self.flush_char_ref();
                            self.state = self.return_state;
                        } else {
                            self.pos += len;
                            self.temp.clear();
                            self.temp.push_str(value);
                            self.flush_char_ref();
                            self.state = self.return_state;
                        }
                    }
                    None => {
                        self.flush_char_ref();
                        self.state = AmbiguousAmpersand;
                    }
                }
                None
            }
            AmbiguousAmpersand => {
                match self.consume() {
                    Some(c) if c.is_ascii_alphanumeric() => self.char_ref_char(c),
                    Some(c) => {
                        self.reconsume(c);
                        self.state = self.return_state;
                    }
                    None => self.state = self.return_state,
                }
                None
            }
            NumericCharacterReference => {
                self.char_ref_code = 0;
                match self.consume() {
                    Some(c @ ('x' | 'X')) => {
                        self.temp.push(c);
                        self.state = HexadecimalCharacterReferenceStart;
                    }
                    Some(c) if c.is_ascii_digit() => {
                        self.reconsume(c);
                        self.state = DecimalCharacterReference;
                    }
                    other => {
                        self.flush_char_ref();
                        if let Some(c) = other {
                            self.reconsume(c);
                        }
                        self.state = self.return_state;
                    }
                }
                None
            }
            HexadecimalCharacterReferenceStart => {
                match self.consume() {
                    Some(c) if c.is_ascii_hexdigit() => {
                        self.reconsume(c);
                        self.state = HexadecimalCharacterReference;
                    }
                    other => {
                        self.flush_char_ref();
                        if let Some(c) = other {
                            self.reconsume(c);
                        }
                        self.state = self.return_state;
                    }
                }
                None
            }
            HexadecimalCharacterReference => {
                match self.consume() {
                    Some(c) if c.is_ascii_hexdigit() => {
                        let d = c.to_digit(16).unwrap_or(0);
                        self.char_ref_code = self
                            .char_ref_code
                            .saturating_mul(16)
                            .saturating_add(d)
                            .min(0x110000);
                    }
                    Some(';') => self.state = NumericCharacterReferenceEnd,
                    other => {
                        if let Some(c) = other {
                            self.reconsume(c);
                        }
                        self.state = NumericCharacterReferenceEnd;
                    }
                }
                None
            }
            DecimalCharacterReference => {
                match self.consume() {
                    Some(c) if c.is_ascii_digit() => {
                        let d = c.to_digit(10).unwrap_or(0);
                        self.char_ref_code = self
                            .char_ref_code
                            .saturating_mul(10)
                            .saturating_add(d)
                            .min(0x110000);
                    }
                    Some(';') => self.state = NumericCharacterReferenceEnd,
                    other => {
                        if let Some(c) = other {
                            self.reconsume(c);
                        }
                        self.state = NumericCharacterReferenceEnd;
                    }
                }
                None
            }
            NumericCharacterReferenceEnd => {
                let c = numeric_reference_char(self.char_ref_code);
                self.temp.clear();
                self.temp.push(c);
                self.flush_char_ref();
                self.state = self.return_state;
                None
            }
        }
    }

    /// EOF inside a comment: emit the comment; `next` then returns EOF forever.
    fn comment_eof(&mut self) -> Option<Token> {
        let t = self.emit_comment();
        self.eof_emitted = true;
        t
    }

    /// EOF inside a doctype: emit the doctype; `next` then returns EOF forever.
    fn doctype_eof(&mut self) -> Option<Token> {
        let t = self.emit_doctype();
        self.eof_emitted = true;
        t
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(s: &str) -> Vec<Token> {
        let mut t = Tokenizer::new(s.to_owned());
        let mut out = Vec::new();
        loop {
            let tok = t.next();
            let eof = tok == Token::Eof;
            out.push(tok);
            if eof {
                break;
            }
        }
        out
    }

    fn tag(name: &str, attrs: &[(&str, &str)]) -> Tag {
        Tag {
            name: name.into(),
            attrs: attrs
                .iter()
                .map(|(n, v)| Attribute {
                    name: (*n).into(),
                    value: (*v).into(),
                })
                .collect(),
            self_closing: false,
        }
    }

    #[test]
    fn tags_and_attributes() {
        assert_eq!(
            tokens("<P Class=a b='c d' e=\"f\" g>x</p>"),
            vec![
                Token::StartTag(tag(
                    "p",
                    &[("class", "a"), ("b", "c d"), ("e", "f"), ("g", "")]
                )),
                Token::Chars("x".into()),
                Token::EndTag(tag("p", &[])),
                Token::Eof
            ]
        );
    }

    #[test]
    fn duplicate_attributes_dropped() {
        assert_eq!(
            tokens("<a x=1 x=2 y=3>"),
            vec![
                Token::StartTag(tag("a", &[("x", "1"), ("y", "3")])),
                Token::Eof
            ]
        );
    }

    #[test]
    fn self_closing_and_bogus() {
        assert_eq!(
            tokens("<br/>")[0],
            Token::StartTag(Tag {
                name: "br".into(),
                attrs: vec![],
                self_closing: true
            })
        );
        assert_eq!(tokens("<?php ?>")[0], Token::Comment("?php ?".into()));
        assert_eq!(tokens("</ x>")[0], Token::Comment(" x".into()));
        assert_eq!(tokens("<!x>")[0], Token::Comment("x".into()));
        assert_eq!(tokens("<!--a--!>")[0], Token::Comment("a".into()));
        assert_eq!(tokens("<!-->")[0], Token::Comment("".into()));
        assert_eq!(tokens("<!--<!--x-->")[0], Token::Comment("<!--x".into()));
    }

    #[test]
    fn doctype_forms() {
        let d = match &tokens("<!DOCTYPE html PUBLIC \"-//W3C//DTD HTML 4.01//EN\" 'http://x'>")[0]
        {
            Token::Doctype(d) => d.clone(),
            _ => panic!(),
        };
        assert_eq!(d.name.as_deref(), Some("html"));
        assert_eq!(d.public_id.as_deref(), Some("-//W3C//DTD HTML 4.01//EN"));
        assert_eq!(d.system_id.as_deref(), Some("http://x"));
        assert!(!d.force_quirks);
        assert!(
            matches!(&tokens("<!DOCTYPE>")[0], Token::Doctype(d) if d.force_quirks && d.name.is_none())
        );
    }

    #[test]
    fn character_references() {
        assert_eq!(
            tokens("&amp;&lt;&#x41;&#65;&#128;&#0;&#xD800;&#x110000;")[0],
            Token::Chars("&<AA\u{20AC}\u{FFFD}\u{FFFD}\u{FFFD}".into())
        );
        assert_eq!(
            tokens("I'm &notit; I")[0],
            Token::Chars("I'm \u{AC}it; I".into())
        );
        assert_eq!(
            tokens("&notin; &Aacute &bogus; &")[0],
            Token::Chars("\u{2209} \u{C1} &bogus; &".into())
        );
        assert_eq!(
            tokens("&CounterClockwiseContourIntegral;")[0],
            Token::Chars("\u{2233}".into())
        );
        assert_eq!(tokens("&#;&#x;")[0], Token::Chars("&#;&#x;".into()));
        // Two-codepoint entity.
        assert_eq!(
            tokens("&NotEqualTilde;")[0],
            Token::Chars("\u{2242}\u{338}".into())
        );
    }

    #[test]
    fn attribute_character_references() {
        // Legacy: no semicolon followed by alphanumeric or '=' is left alone in attributes.
        assert_eq!(
            tokens("<a href='?a=1&copy=2&amp;b=3&lt'>")[0],
            Token::StartTag(tag("a", &[("href", "?a=1&copy=2&b=3<")]))
        );
        assert_eq!(
            tokens("<a t='&copy;x'>")[0],
            Token::StartTag(tag("a", &[("t", "\u{A9}x")]))
        );
    }

    #[test]
    fn rawtext_and_script_escapes() {
        let mut t = Tokenizer::new("<style>a<b</style>x".into());
        assert!(matches!(t.next(), Token::StartTag(_)));
        t.state = State::Rawtext;
        assert_eq!(t.next(), Token::Chars("a<b".into()));
        assert_eq!(t.next(), Token::EndTag(tag("style", &[])));
        assert_eq!(t.next(), Token::Chars("x".into()));

        let mut t = Tokenizer::new("<script><!--<script></script>--></script>".into());
        assert!(matches!(t.next(), Token::StartTag(_)));
        t.state = State::ScriptData;
        assert_eq!(t.next(), Token::Chars("<!--<script></script>-->".into()));
        assert_eq!(t.next(), Token::EndTag(tag("script", &[])));
    }

    #[test]
    fn rcdata_and_plaintext() {
        let mut t = Tokenizer::new("<textarea>&amp;<b></TEXTAREA >".into());
        assert!(matches!(t.next(), Token::StartTag(_)));
        t.state = State::Rcdata;
        assert_eq!(t.next(), Token::Chars("&<b>".into()));
        assert_eq!(t.next(), Token::EndTag(tag("textarea", &[])));
        let mut t = Tokenizer::new("<b>\0</b>".into());
        t.state = State::Plaintext;
        assert_eq!(t.next(), Token::Chars("<b>\u{FFFD}</b>".into()));
    }

    #[test]
    fn cdata_only_in_foreign() {
        let mut t = Tokenizer::new("<![CDATA[x]]]>".into());
        t.allow_cdata = true;
        assert_eq!(t.next(), Token::Chars("x]".into()));
        let mut t = Tokenizer::new("<![CDATA[x]]>".into());
        assert_eq!(t.next(), Token::Comment("[CDATA[x]]".into()));
    }

    #[test]
    fn eof_forms() {
        assert_eq!(tokens("a<"), vec![Token::Chars("a<".into()), Token::Eof]);
        assert_eq!(tokens("</"), vec![Token::Chars("</".into()), Token::Eof]);
        assert_eq!(
            tokens("<!-- x"),
            vec![Token::Comment(" x".into()), Token::Eof]
        );
        assert_eq!(
            tokens("<!DOCTYPE htm"),
            vec![
                Token::Doctype(Doctype {
                    name: Some("htm".into()),
                    force_quirks: true,
                    ..Default::default()
                }),
                Token::Eof
            ]
        );
        assert_eq!(tokens("<a b='c"), vec![Token::Eof]);
    }
}
