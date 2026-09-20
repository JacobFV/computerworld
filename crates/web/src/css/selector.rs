//! Selectors Level 4: the grammar subset the engine matches, specificity, and
//! serialization back to text. Parsing works over component values (a rule prelude or
//! the arguments of `:not()`), so it needs no second tokenizer pass.

use super::token::{ComponentValue, Token};
use super::tokenizer::{serialize_identifier, serialize_string};
use std::fmt;

/// Specificity as the `(a, b, c)` triple; derived `Ord` compares lexicographically.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Specificity {
    pub a: u32,
    pub b: u32,
    pub c: u32,
}

impl Specificity {
    pub const ZERO: Specificity = Specificity { a: 0, b: 0, c: 0 };
    pub fn new(a: u32, b: u32, c: u32) -> Specificity {
        Specificity { a, b, c }
    }
    fn add(self, o: Specificity) -> Specificity {
        Specificity { a: self.a + o.a, b: self.b + o.b, c: self.c + o.c }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Combinator {
    Descendant,
    Child,
    NextSibling,
    SubsequentSibling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AttrOp {
    /// `[name]`
    Exists,
    /// `=`
    Equals,
    /// `~=`
    Includes,
    /// `|=`
    DashMatch,
    /// `^=`
    Prefix,
    /// `$=`
    Suffix,
    /// `*=`
    Substring,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AttrCase {
    /// Case-sensitive, unless the attribute is in HTML's case-insensitive list.
    #[default]
    Default,
    /// `i`
    Insensitive,
    /// `s`
    Sensitive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NthKind {
    Child,
    LastChild,
    OfType,
    LastOfType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    Ltr,
    Rtl,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum PseudoClass {
    Root,
    Empty,
    FirstChild,
    LastChild,
    OnlyChild,
    FirstOfType,
    LastOfType,
    OnlyOfType,
    /// `:nth-child(An+B [of S])` and its three siblings.
    Nth { kind: NthKind, a: i32, b: i32, of: Option<SelectorList> },
    Not(SelectorList),
    Is(SelectorList),
    Where(SelectorList),
    Has(Vec<RelativeSelector>),
    Hover,
    Active,
    Focus,
    FocusVisible,
    FocusWithin,
    Visited,
    Link,
    AnyLink,
    Target,
    Checked,
    Disabled,
    Enabled,
    Required,
    Optional,
    ReadOnly,
    ReadWrite,
    PlaceholderShown,
    Indeterminate,
    Default,
    Lang(Vec<String>),
    Dir(Direction),
    Scope,
    Defined,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoElement {
    Before,
    After,
    Marker,
    Placeholder,
    Selection,
    /// Parsed and matched, but the style track treats it as unsupported.
    FirstLine,
    /// Parsed and matched, but the style track treats it as unsupported.
    FirstLetter,
}

impl PseudoElement {
    pub fn name(self) -> &'static str {
        match self {
            PseudoElement::Before => "before",
            PseudoElement::After => "after",
            PseudoElement::Marker => "marker",
            PseudoElement::Placeholder => "placeholder",
            PseudoElement::Selection => "selection",
            PseudoElement::FirstLine => "first-line",
            PseudoElement::FirstLetter => "first-letter",
        }
    }
    /// False for `::first-line` and `::first-letter`, which no layout stage implements.
    pub fn supported(self) -> bool {
        !matches!(self, PseudoElement::FirstLine | PseudoElement::FirstLetter)
    }
    fn from_name(name: &str) -> Option<PseudoElement> {
        Some(match name.to_ascii_lowercase().as_str() {
            "before" => PseudoElement::Before,
            "after" => PseudoElement::After,
            "marker" => PseudoElement::Marker,
            "placeholder" => PseudoElement::Placeholder,
            "selection" => PseudoElement::Selection,
            "first-line" => PseudoElement::FirstLine,
            "first-letter" => PseudoElement::FirstLetter,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SimpleSelector {
    /// Type selector as written; matched case-insensitively against HTML elements.
    Type(String),
    Universal,
    Id(String),
    Class(String),
    Attribute { name: String, op: AttrOp, value: String, case: AttrCase },
    PseudoClass(PseudoClass),
    /// The nesting selector `&`; replaced by `:is(<parent>)` when a nested rule is
    /// flattened. At the top level it behaves as `:scope`.
    Nesting,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct CompoundSelector {
    pub simple: Vec<SimpleSelector>,
}

/// A complex selector: compounds left to right, `combinators[i]` sits between
/// `compounds[i]` and `compounds[i + 1]`. The optional pseudo-element is at the end.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ComplexSelector {
    pub compounds: Vec<CompoundSelector>,
    pub combinators: Vec<Combinator>,
    pub pseudo_element: Option<PseudoElement>,
}

/// A relative selector (the arguments of `:has()` and nested rule preludes).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RelativeSelector {
    pub combinator: Combinator,
    pub selector: ComplexSelector,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct SelectorList(pub Vec<ComplexSelector>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectorErrorKind {
    Empty,
    Syntax,
    UnsupportedPseudoClass,
    UnsupportedPseudoElement,
    ColumnCombinator,
    NestedHas,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectorError {
    pub kind: SelectorErrorKind,
    pub detail: String,
}

impl fmt::Display for SelectorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.detail)
    }
}

fn err(kind: SelectorErrorKind, detail: impl Into<String>) -> SelectorError {
    SelectorError { kind, detail: detail.into() }
}

// ---------------------------------------------------------------------------
// Parsing

/// Parses a selector list from text; `&` is accepted (top-level nesting selector).
pub fn parse_selector_list(src: &str) -> Result<SelectorList, SelectorError> {
    let values = super::parser::parse_component_value_list(src);
    SelectorList::parse(&values)
}

/// Parses a relative selector list (`> a, + b, c`) from text.
pub fn parse_relative_selector_list(src: &str) -> Result<Vec<RelativeSelector>, SelectorError> {
    let values = super::parser::parse_component_value_list(src);
    parse_relative_list(&values, false)
}

impl SelectorList {
    pub fn parse(values: &[ComponentValue]) -> Result<SelectorList, SelectorError> {
        let mut p = Parser { values, pos: 0, in_has: false };
        p.parse_list()
    }
    /// The forgiving form used by `:is()` and `:where()`: invalid selectors are dropped.
    pub fn parse_forgiving(values: &[ComponentValue]) -> SelectorList {
        Self::parse_forgiving_in(values, false)
    }
    fn parse_forgiving_in(values: &[ComponentValue], in_has: bool) -> SelectorList {
        let mut out = Vec::new();
        for group in split_commas(values) {
            let mut p = Parser { values: group, pos: 0, in_has };
            if let Ok(mut list) = p.parse_list() {
                out.append(&mut list.0);
            }
        }
        SelectorList(out)
    }
    pub fn iter(&self) -> impl Iterator<Item = &ComplexSelector> {
        self.0.iter()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// The highest specificity among the list, as `:is()` and `:not()` use.
    pub fn max_specificity(&self) -> Specificity {
        self.0.iter().map(|s| s.specificity()).max().unwrap_or_default()
    }
    /// Replaces `&` with `:is(<parent>)` in every selector; see
    /// `ComplexSelector::resolve_nesting`.
    pub fn resolve_nesting(&self, parent: Option<&SelectorList>) -> SelectorList {
        SelectorList(self.0.iter().map(|s| s.resolve_nesting(parent)).collect())
    }
    pub fn contains_nesting(&self) -> bool {
        self.0.iter().any(|s| s.contains_nesting())
    }
}

/// Parses relative selectors; a missing leading combinator means descendant.
pub fn parse_relative_list(values: &[ComponentValue], in_has: bool) -> Result<Vec<RelativeSelector>, SelectorError> {
    let mut p = Parser { values, pos: 0, in_has };
    let mut out = Vec::new();
    loop {
        p.skip_ws();
        let combinator = p.parse_combinator_token().unwrap_or(Combinator::Descendant);
        p.skip_ws();
        let selector = p.parse_complex()?;
        out.push(RelativeSelector { combinator, selector });
        p.skip_ws();
        match p.peek() {
            None => return Ok(out),
            Some(ComponentValue::Token(Token::Comma)) => {
                p.pos += 1;
            }
            Some(v) => return Err(err(SelectorErrorKind::Syntax, format!("unexpected {}", describe(v)))),
        }
    }
}

fn split_commas(values: &[ComponentValue]) -> Vec<&[ComponentValue]> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, v) in values.iter().enumerate() {
        if matches!(v, ComponentValue::Token(Token::Comma)) {
            out.push(&values[start..i]);
            start = i + 1;
        }
    }
    out.push(&values[start..]);
    out
}

fn describe(v: &ComponentValue) -> String {
    let mut s = String::new();
    super::parser::serialize_component_value(v, &mut s);
    s
}

struct Parser<'a> {
    values: &'a [ComponentValue],
    pos: usize,
    in_has: bool,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<&'a ComponentValue> {
        self.values.get(self.pos)
    }
    fn peek_at(&self, n: usize) -> Option<&'a ComponentValue> {
        self.values.get(self.pos + n)
    }
    fn next(&mut self) -> Option<&'a ComponentValue> {
        let v = self.values.get(self.pos);
        if v.is_some() {
            self.pos += 1;
        }
        v
    }
    fn skip_ws(&mut self) -> bool {
        let mut any = false;
        while matches!(self.peek(), Some(v) if v.is_whitespace()) {
            self.pos += 1;
            any = true;
        }
        any
    }
    fn is_delim(&self, n: usize, c: char) -> bool {
        matches!(self.peek_at(n), Some(ComponentValue::Token(Token::Delim(d))) if *d == c)
    }

    fn parse_list(&mut self) -> Result<SelectorList, SelectorError> {
        let mut out = Vec::new();
        loop {
            self.skip_ws();
            out.push(self.parse_complex()?);
            self.skip_ws();
            match self.next() {
                None => return Ok(SelectorList(out)),
                Some(ComponentValue::Token(Token::Comma)) => {}
                Some(v) => return Err(err(SelectorErrorKind::Syntax, format!("unexpected {}", describe(v)))),
            }
        }
    }

    /// A combinator token at the current position (whitespace already skipped).
    fn parse_combinator_token(&mut self) -> Option<Combinator> {
        if self.is_delim(0, '|') && self.is_delim(1, '|') {
            return None;
        }
        let c = match self.peek() {
            Some(ComponentValue::Token(Token::Delim('>'))) => Combinator::Child,
            Some(ComponentValue::Token(Token::Delim('+'))) => Combinator::NextSibling,
            Some(ComponentValue::Token(Token::Delim('~'))) => Combinator::SubsequentSibling,
            _ => return None,
        };
        self.pos += 1;
        Some(c)
    }

    fn parse_complex(&mut self) -> Result<ComplexSelector, SelectorError> {
        let mut compounds = Vec::new();
        let mut combinators = Vec::new();
        let mut pseudo_element = None;
        loop {
            let (compound, pe) = self.parse_compound()?;
            if compound.simple.is_empty() && pe.is_none() {
                return Err(err(SelectorErrorKind::Empty, "expected a compound selector"));
            }
            compounds.push(compound);
            if let Some(pe) = pe {
                pseudo_element = Some(pe);
                self.skip_ws();
                if !matches!(self.peek(), None | Some(ComponentValue::Token(Token::Comma))) {
                    return Err(err(SelectorErrorKind::Syntax, "a pseudo-element must end the selector"));
                }
                break;
            }
            let had_ws = self.skip_ws();
            if self.is_delim(0, '|') && self.is_delim(1, '|') {
                return Err(err(SelectorErrorKind::ColumnCombinator, "the column combinator `||` is not supported"));
            }
            match self.parse_combinator_token() {
                Some(c) => {
                    self.skip_ws();
                    combinators.push(c);
                }
                None => {
                    if had_ws && !matches!(self.peek(), None | Some(ComponentValue::Token(Token::Comma))) {
                        combinators.push(Combinator::Descendant);
                    } else {
                        break;
                    }
                }
            }
        }
        Ok(ComplexSelector { compounds, combinators, pseudo_element })
    }

    /// Parses one compound; returns the pseudo-element that ended it, if any.
    fn parse_compound(&mut self) -> Result<(CompoundSelector, Option<PseudoElement>), SelectorError> {
        let mut simple = Vec::new();
        // Optional namespace prefix followed by a type or universal selector.
        if let Some(t) = self.parse_type_or_universal()? {
            simple.push(t);
        }
        loop {
            match self.peek() {
                Some(ComponentValue::Token(Token::Hash { value, id })) => {
                    if !*id {
                        return Err(err(SelectorErrorKind::Syntax, format!("invalid id selector #{value}")));
                    }
                    simple.push(SimpleSelector::Id(value.clone()));
                    self.pos += 1;
                }
                Some(ComponentValue::Token(Token::Delim('.'))) => {
                    self.pos += 1;
                    match self.next() {
                        Some(ComponentValue::Token(Token::Ident(c))) => simple.push(SimpleSelector::Class(c.clone())),
                        _ => return Err(err(SelectorErrorKind::Syntax, "expected a class name after `.`")),
                    }
                }
                Some(ComponentValue::Token(Token::Delim('&'))) => {
                    self.pos += 1;
                    simple.push(SimpleSelector::Nesting);
                }
                Some(ComponentValue::Block { open: Token::OpenSquare, contents }) => {
                    self.pos += 1;
                    simple.push(parse_attribute(contents)?);
                }
                Some(ComponentValue::Token(Token::Colon)) => {
                    self.pos += 1;
                    let double = matches!(self.peek(), Some(ComponentValue::Token(Token::Colon)));
                    if double {
                        self.pos += 1;
                    }
                    match self.next() {
                        Some(ComponentValue::Token(Token::Ident(name))) => {
                            let lname = name.to_ascii_lowercase();
                            let legacy = matches!(lname.as_str(), "before" | "after" | "first-line" | "first-letter");
                            if double || legacy {
                                let Some(pe) = PseudoElement::from_name(&lname) else {
                                    return Err(err(SelectorErrorKind::UnsupportedPseudoElement, format!("::{name}")));
                                };
                                return Ok((CompoundSelector { simple }, Some(pe)));
                            }
                            simple.push(SimpleSelector::PseudoClass(parse_pseudo_class_ident(&lname, name)?));
                        }
                        Some(ComponentValue::Function { name, args }) => {
                            if double {
                                return Err(err(SelectorErrorKind::UnsupportedPseudoElement, format!("::{name}()")));
                            }
                            simple.push(SimpleSelector::PseudoClass(parse_pseudo_class_function(name, args, self.in_has)?));
                        }
                        _ => return Err(err(SelectorErrorKind::Syntax, "expected a pseudo-class name after `:`")),
                    }
                }
                _ => break,
            }
        }
        Ok((CompoundSelector { simple }, None))
    }

    fn parse_type_or_universal(&mut self) -> Result<Option<SimpleSelector>, SelectorError> {
        // Namespace prefixes: `ns|`, `*|`, `|`; the prefix is accepted and ignored.
        let has_prefix = match self.peek() {
            Some(ComponentValue::Token(Token::Ident(_))) | Some(ComponentValue::Token(Token::Delim('*'))) => self.is_delim(1, '|') && !self.is_delim(2, '|') && !self.is_delim(2, '='),
            Some(ComponentValue::Token(Token::Delim('|'))) => !self.is_delim(1, '|'),
            _ => false,
        };
        if has_prefix {
            if matches!(self.peek(), Some(ComponentValue::Token(Token::Delim('|')))) {
                self.pos += 1;
            } else {
                self.pos += 2;
            }
        }
        match self.peek() {
            Some(ComponentValue::Token(Token::Ident(name))) => {
                self.pos += 1;
                Ok(Some(SimpleSelector::Type(name.clone())))
            }
            Some(ComponentValue::Token(Token::Delim('*'))) => {
                self.pos += 1;
                Ok(Some(SimpleSelector::Universal))
            }
            _ if has_prefix => Err(err(SelectorErrorKind::Syntax, "expected a type after the namespace prefix")),
            _ => Ok(None),
        }
    }
}

fn parse_attribute(contents: &[ComponentValue]) -> Result<SimpleSelector, SelectorError> {
    let mut p = Parser { values: contents, pos: 0, in_has: false };
    p.skip_ws();
    // Namespace prefix on the attribute name is ignored.
    let prefixed = match p.peek() {
        Some(ComponentValue::Token(Token::Ident(_))) | Some(ComponentValue::Token(Token::Delim('*'))) => p.is_delim(1, '|') && !p.is_delim(2, '='),
        Some(ComponentValue::Token(Token::Delim('|'))) => true,
        _ => false,
    };
    if prefixed {
        if matches!(p.peek(), Some(ComponentValue::Token(Token::Delim('|')))) {
            p.pos += 1;
        } else {
            p.pos += 2;
        }
    }
    let name = match p.next() {
        Some(ComponentValue::Token(Token::Ident(n))) => n.clone(),
        _ => return Err(err(SelectorErrorKind::Syntax, "expected an attribute name")),
    };
    p.skip_ws();
    let op = match p.peek() {
        None => return Ok(SimpleSelector::Attribute { name, op: AttrOp::Exists, value: String::new(), case: AttrCase::Default }),
        Some(ComponentValue::Token(Token::Delim('='))) => {
            p.pos += 1;
            AttrOp::Equals
        }
        Some(ComponentValue::Token(Token::Delim(c))) if p.is_delim(1, '=') => {
            let op = match c {
                '~' => AttrOp::Includes,
                '|' => AttrOp::DashMatch,
                '^' => AttrOp::Prefix,
                '$' => AttrOp::Suffix,
                '*' => AttrOp::Substring,
                _ => return Err(err(SelectorErrorKind::Syntax, format!("unknown attribute operator {c}="))),
            };
            p.pos += 2;
            op
        }
        Some(v) => return Err(err(SelectorErrorKind::Syntax, format!("unexpected {} in attribute selector", describe(v)))),
    };
    p.skip_ws();
    let value = match p.next() {
        Some(ComponentValue::Token(Token::Ident(v))) | Some(ComponentValue::Token(Token::String(v))) => v.clone(),
        _ => return Err(err(SelectorErrorKind::Syntax, "expected an attribute value")),
    };
    p.skip_ws();
    let case = match p.next() {
        None => AttrCase::Default,
        Some(ComponentValue::Token(Token::Ident(f))) if f.eq_ignore_ascii_case("i") => AttrCase::Insensitive,
        Some(ComponentValue::Token(Token::Ident(f))) if f.eq_ignore_ascii_case("s") => AttrCase::Sensitive,
        Some(v) => return Err(err(SelectorErrorKind::Syntax, format!("unexpected {} after attribute value", describe(v)))),
    };
    p.skip_ws();
    if p.peek().is_some() {
        return Err(err(SelectorErrorKind::Syntax, "trailing content in attribute selector"));
    }
    Ok(SimpleSelector::Attribute { name, op, value, case })
}

fn parse_pseudo_class_ident(lname: &str, name: &str) -> Result<PseudoClass, SelectorError> {
    Ok(match lname {
        "root" => PseudoClass::Root,
        "empty" => PseudoClass::Empty,
        "first-child" => PseudoClass::FirstChild,
        "last-child" => PseudoClass::LastChild,
        "only-child" => PseudoClass::OnlyChild,
        "first-of-type" => PseudoClass::FirstOfType,
        "last-of-type" => PseudoClass::LastOfType,
        "only-of-type" => PseudoClass::OnlyOfType,
        "hover" => PseudoClass::Hover,
        "active" => PseudoClass::Active,
        "focus" => PseudoClass::Focus,
        "focus-visible" => PseudoClass::FocusVisible,
        "focus-within" => PseudoClass::FocusWithin,
        "visited" => PseudoClass::Visited,
        "link" => PseudoClass::Link,
        "any-link" => PseudoClass::AnyLink,
        "target" => PseudoClass::Target,
        "checked" => PseudoClass::Checked,
        "disabled" => PseudoClass::Disabled,
        "enabled" => PseudoClass::Enabled,
        "required" => PseudoClass::Required,
        "optional" => PseudoClass::Optional,
        "read-only" => PseudoClass::ReadOnly,
        "read-write" => PseudoClass::ReadWrite,
        "placeholder-shown" => PseudoClass::PlaceholderShown,
        "indeterminate" => PseudoClass::Indeterminate,
        "default" => PseudoClass::Default,
        "scope" => PseudoClass::Scope,
        "defined" => PseudoClass::Defined,
        _ => return Err(err(SelectorErrorKind::UnsupportedPseudoClass, format!(":{name}"))),
    })
}

fn parse_pseudo_class_function(name: &str, args: &[ComponentValue], in_has: bool) -> Result<PseudoClass, SelectorError> {
    let lname = name.to_ascii_lowercase();
    let sub = |args: &[ComponentValue]| -> Result<SelectorList, SelectorError> {
        let mut p = Parser { values: args, pos: 0, in_has };
        let list = p.parse_list()?;
        if list.0.iter().any(|s| s.pseudo_element.is_some()) {
            return Err(err(SelectorErrorKind::Syntax, format!(":{lname}() cannot contain a pseudo-element")));
        }
        Ok(list)
    };
    Ok(match lname.as_str() {
        "not" => PseudoClass::Not(sub(args)?),
        "is" | "matches" | "-webkit-any" | "-moz-any" => {
            let mut list = SelectorList::parse_forgiving_in(args, in_has);
            list.0.retain(|s| s.pseudo_element.is_none());
            PseudoClass::Is(list)
        }
        "where" => {
            let mut list = SelectorList::parse_forgiving_in(args, in_has);
            list.0.retain(|s| s.pseudo_element.is_none());
            PseudoClass::Where(list)
        }
        "has" => {
            if in_has {
                return Err(err(SelectorErrorKind::NestedHas, ":has() cannot be nested inside :has()"));
            }
            let rel = parse_relative_list(args, true)?;
            if rel.iter().any(|r| r.selector.pseudo_element.is_some()) {
                return Err(err(SelectorErrorKind::Syntax, ":has() cannot contain a pseudo-element"));
            }
            PseudoClass::Has(rel)
        }
        "nth-child" | "nth-last-child" | "nth-of-type" | "nth-last-of-type" => {
            let kind = match lname.as_str() {
                "nth-child" => NthKind::Child,
                "nth-last-child" => NthKind::LastChild,
                "nth-of-type" => NthKind::OfType,
                _ => NthKind::LastOfType,
            };
            let mut pos = 0;
            let (a, b) = parse_anb_prefix(args, &mut pos).ok_or_else(|| err(SelectorErrorKind::Syntax, format!("invalid An+B in :{name}()")))?;
            while matches!(args.get(pos), Some(v) if v.is_whitespace()) {
                pos += 1;
            }
            let of = match args.get(pos) {
                None => None,
                Some(ComponentValue::Token(Token::Ident(of))) if of.eq_ignore_ascii_case("of") && matches!(kind, NthKind::Child | NthKind::LastChild) => {
                    let list = sub(&args[pos + 1..])?;
                    if list.0.is_empty() {
                        return Err(err(SelectorErrorKind::Syntax, "empty selector list after `of`"));
                    }
                    Some(list)
                }
                Some(v) => return Err(err(SelectorErrorKind::Syntax, format!("unexpected {} in :{name}()", describe(v)))),
            };
            PseudoClass::Nth { kind, a, b, of }
        }
        "lang" => {
            let mut ranges = Vec::new();
            for group in split_commas(args) {
                let items: Vec<&ComponentValue> = group.iter().filter(|v| !v.is_whitespace()).collect();
                match items.as_slice() {
                    [ComponentValue::Token(Token::Ident(s))] | [ComponentValue::Token(Token::String(s))] => ranges.push(s.clone()),
                    [ComponentValue::Token(Token::Delim('*'))] => ranges.push("*".to_owned()),
                    _ => return Err(err(SelectorErrorKind::Syntax, "invalid :lang() argument")),
                }
            }
            if ranges.is_empty() {
                return Err(err(SelectorErrorKind::Syntax, ":lang() needs an argument"));
            }
            PseudoClass::Lang(ranges)
        }
        "dir" => {
            let items: Vec<&ComponentValue> = args.iter().filter(|v| !v.is_whitespace()).collect();
            match items.as_slice() {
                [ComponentValue::Token(Token::Ident(d))] if d.eq_ignore_ascii_case("ltr") => PseudoClass::Dir(Direction::Ltr),
                [ComponentValue::Token(Token::Ident(d))] if d.eq_ignore_ascii_case("rtl") => PseudoClass::Dir(Direction::Rtl),
                _ => return Err(err(SelectorErrorKind::Syntax, "invalid :dir() argument")),
            }
        }
        _ => return Err(err(SelectorErrorKind::UnsupportedPseudoClass, format!(":{name}()"))),
    })
}

// ---------------------------------------------------------------------------
// An+B

/// Parses the `An+B` microsyntax from text; `None` when invalid.
pub fn parse_anb(src: &str) -> Option<(i32, i32)> {
    let values = super::parser::parse_component_value_list(src);
    let mut pos = 0;
    let r = parse_anb_prefix(&values, &mut pos)?;
    while matches!(values.get(pos), Some(v) if v.is_whitespace()) {
        pos += 1;
    }
    if pos == values.len() {
        Some(r)
    } else {
        None
    }
}

fn int_value(text: &str, value: super::token::Number) -> Option<i32> {
    if !value.int || value.micro % 1_000_000 != 0 {
        return None;
    }
    let _ = text;
    i32::try_from(value.micro / 1_000_000).ok()
}

/// Digits after `n-` in an `ndashdigit` ident or unit: `n-5` gives `Some(-5)`.
fn ndash_digits(s: &str) -> Option<i32> {
    let rest = s.strip_prefix("n-")?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse::<i32>().ok().map(|v| -v)
}

/// Parses `An+B` at `*pos`, leaving `*pos` after it (whitespace after is not consumed).
pub fn parse_anb_prefix(values: &[ComponentValue], pos: &mut usize) -> Option<(i32, i32)> {
    let skip_ws = |pos: &mut usize| {
        while matches!(values.get(*pos), Some(v) if v.is_whitespace()) {
            *pos += 1;
        }
    };
    skip_ws(pos);
    // After an `n`, the optional B part.
    let rest = |a: i32, pos: &mut usize| -> Option<(i32, i32)> {
        let mut p = *pos;
        skip_ws(&mut p);
        match values.get(p) {
            Some(ComponentValue::Token(Token::Number { text, value })) if text.starts_with('+') || text.starts_with('-') => {
                let b = int_value(text, *value)?;
                *pos = p + 1;
                Some((a, b))
            }
            Some(ComponentValue::Token(Token::Delim(sign))) if *sign == '+' || *sign == '-' => {
                p += 1;
                skip_ws(&mut p);
                match values.get(p) {
                    Some(ComponentValue::Token(Token::Number { text, value })) if text.starts_with(|c: char| c.is_ascii_digit()) => {
                        let b = int_value(text, *value)?;
                        *pos = p + 1;
                        Some((a, if *sign == '-' { -b } else { b }))
                    }
                    _ => None,
                }
            }
            _ => Some((a, 0)),
        }
    };
    // After an `n-`: a signless integer must follow.
    let rest_dash = |a: i32, pos: &mut usize| -> Option<(i32, i32)> {
        let mut p = *pos;
        skip_ws(&mut p);
        match values.get(p) {
            Some(ComponentValue::Token(Token::Number { text, value })) if text.starts_with(|c: char| c.is_ascii_digit()) => {
                let b = int_value(text, *value)?;
                *pos = p + 1;
                Some((a, -b))
            }
            _ => None,
        }
    };
    let ident_case = |name: &str, sign: i32, pos: &mut usize| -> Option<(i32, i32)> {
        let l = name.to_ascii_lowercase();
        let (a, body) = if let Some(b) = l.strip_prefix('-') {
            if sign != 1 {
                return None;
            }
            (-1, b.to_owned())
        } else {
            (sign, l)
        };
        if body == "n" {
            return rest(a, pos);
        }
        if body == "n-" {
            return rest_dash(a, pos);
        }
        ndash_digits(&body).map(|b| (a, b))
    };
    match values.get(*pos)? {
        ComponentValue::Token(Token::Ident(name)) => {
            let l = name.to_ascii_lowercase();
            *pos += 1;
            if l == "odd" {
                return Some((2, 1));
            }
            if l == "even" {
                return Some((2, 0));
            }
            ident_case(name, 1, pos)
        }
        ComponentValue::Token(Token::Delim('+')) => match values.get(*pos + 1) {
            Some(ComponentValue::Token(Token::Ident(name))) if !name.starts_with('-') => {
                *pos += 2;
                ident_case(name, 1, pos)
            }
            _ => None,
        },
        ComponentValue::Token(Token::Number { text, value }) => {
            let b = int_value(text, *value)?;
            *pos += 1;
            Some((0, b))
        }
        ComponentValue::Token(Token::Dimension { text, value, unit }) => {
            let a = int_value(text, *value)?;
            let u = unit.to_ascii_lowercase();
            *pos += 1;
            if u == "n" {
                rest(a, pos)
            } else if u == "n-" {
                rest_dash(a, pos)
            } else {
                ndash_digits(&u).map(|b| (a, b))
            }
        }
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Specificity and nesting

impl ComplexSelector {
    pub fn specificity(&self) -> Specificity {
        let mut s = self.compounds.iter().fold(Specificity::ZERO, |acc, c| acc.add(c.specificity()));
        if self.pseudo_element.is_some() {
            s.c += 1;
        }
        s
    }
    pub fn rightmost(&self) -> &CompoundSelector {
        self.compounds.last().expect("a complex selector has at least one compound")
    }
    pub fn contains_nesting(&self) -> bool {
        self.compounds.iter().any(|c| c.contains_nesting())
    }
    /// CSS Nesting: every `&` becomes `:is(<parent>)`; a selector without `&` gets an
    /// implied `:is(<parent>) ` descendant prefix. With no parent, `&` becomes `:scope`.
    pub fn resolve_nesting(&self, parent: Option<&SelectorList>) -> ComplexSelector {
        RelativeSelector { combinator: Combinator::Descendant, selector: self.clone() }.resolve_nesting(parent)
    }
    pub fn simple_selectors(&self) -> impl Iterator<Item = &SimpleSelector> {
        self.compounds.iter().flat_map(|c| c.simple.iter())
    }
}

impl RelativeSelector {
    /// Resolves a nested rule's selector against its parent (see
    /// `ComplexSelector::resolve_nesting`); the relative combinator joins the parent.
    pub fn resolve_nesting(&self, parent: Option<&SelectorList>) -> ComplexSelector {
        let replacement = match parent {
            Some(p) => SimpleSelector::PseudoClass(PseudoClass::Is(p.clone())),
            None => SimpleSelector::PseudoClass(PseudoClass::Scope),
        };
        let mut out = self.selector.clone();
        let had = out.contains_nesting();
        for c in &mut out.compounds {
            for s in &mut c.simple {
                if let SimpleSelector::PseudoClass(pc) = s {
                    resolve_in_pseudo(pc, parent);
                }
                if *s == SimpleSelector::Nesting {
                    *s = replacement.clone();
                }
            }
        }
        if !had {
            if let Some(p) = parent {
                out.compounds.insert(0, CompoundSelector { simple: vec![SimpleSelector::PseudoClass(PseudoClass::Is(p.clone()))] });
                out.combinators.insert(0, self.combinator);
            }
        }
        out
    }
}

fn resolve_in_pseudo(pc: &mut PseudoClass, parent: Option<&SelectorList>) {
    match pc {
        PseudoClass::Not(l) | PseudoClass::Is(l) | PseudoClass::Where(l) => {
            if l.contains_nesting() {
                *l = l.resolve_nesting(parent);
            }
        }
        PseudoClass::Nth { of: Some(l), .. } => {
            if l.contains_nesting() {
                *l = l.resolve_nesting(parent);
            }
        }
        PseudoClass::Has(rel) => {
            for r in rel {
                if r.selector.contains_nesting() {
                    r.selector = r.selector.resolve_nesting(parent);
                }
            }
        }
        _ => {}
    }
}

impl CompoundSelector {
    pub fn specificity(&self) -> Specificity {
        self.simple.iter().fold(Specificity::ZERO, |acc, s| acc.add(s.specificity()))
    }
    pub fn contains_nesting(&self) -> bool {
        self.simple.iter().any(|s| match s {
            SimpleSelector::Nesting => true,
            SimpleSelector::PseudoClass(PseudoClass::Not(l) | PseudoClass::Is(l) | PseudoClass::Where(l)) => l.contains_nesting(),
            SimpleSelector::PseudoClass(PseudoClass::Nth { of: Some(l), .. }) => l.contains_nesting(),
            SimpleSelector::PseudoClass(PseudoClass::Has(rel)) => rel.iter().any(|r| r.selector.contains_nesting()),
            _ => false,
        })
    }
    pub fn id(&self) -> Option<&str> {
        self.simple.iter().find_map(|s| match s {
            SimpleSelector::Id(i) => Some(i.as_str()),
            _ => None,
        })
    }
    pub fn classes(&self) -> impl Iterator<Item = &str> {
        self.simple.iter().filter_map(|s| match s {
            SimpleSelector::Class(c) => Some(c.as_str()),
            _ => None,
        })
    }
    pub fn type_name(&self) -> Option<&str> {
        self.simple.iter().find_map(|s| match s {
            SimpleSelector::Type(t) => Some(t.as_str()),
            _ => None,
        })
    }
}

impl SimpleSelector {
    pub fn specificity(&self) -> Specificity {
        match self {
            SimpleSelector::Type(_) => Specificity::new(0, 0, 1),
            SimpleSelector::Universal | SimpleSelector::Nesting => Specificity::ZERO,
            SimpleSelector::Id(_) => Specificity::new(1, 0, 0),
            SimpleSelector::Class(_) | SimpleSelector::Attribute { .. } => Specificity::new(0, 1, 0),
            SimpleSelector::PseudoClass(pc) => match pc {
                PseudoClass::Where(_) => Specificity::ZERO,
                PseudoClass::Is(l) | PseudoClass::Not(l) => l.max_specificity(),
                PseudoClass::Has(rel) => rel.iter().map(|r| r.selector.specificity()).max().unwrap_or_default(),
                PseudoClass::Nth { of: Some(l), .. } => Specificity::new(0, 1, 0).add(l.max_specificity()),
                _ => Specificity::new(0, 1, 0),
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Serialization

impl fmt::Display for SelectorList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, s) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "{s}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Combinator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Combinator::Descendant => " ",
            Combinator::Child => " > ",
            Combinator::NextSibling => " + ",
            Combinator::SubsequentSibling => " ~ ",
        })
    }
}

impl fmt::Display for ComplexSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, c) in self.compounds.iter().enumerate() {
            if i > 0 {
                write!(f, "{}", self.combinators[i - 1])?;
            }
            write!(f, "{c}")?;
        }
        if let Some(pe) = self.pseudo_element {
            write!(f, "::{}", pe.name())?;
        }
        Ok(())
    }
}

impl fmt::Display for RelativeSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.combinator != Combinator::Descendant {
            f.write_str(self.combinator.to_string().trim_start())?;
        }
        write!(f, "{}", self.selector)
    }
}

impl fmt::Display for CompoundSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.simple.is_empty() {
            return f.write_str("*");
        }
        for s in &self.simple {
            write!(f, "{s}")?;
        }
        Ok(())
    }
}

fn fmt_list(f: &mut fmt::Formatter<'_>, name: &str, l: &SelectorList) -> fmt::Result {
    write!(f, ":{name}({l})")
}

impl fmt::Display for SimpleSelector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SimpleSelector::Type(t) => f.write_str(&serialize_identifier(t)),
            SimpleSelector::Universal => f.write_str("*"),
            SimpleSelector::Nesting => f.write_str("&"),
            SimpleSelector::Id(i) => write!(f, "#{}", serialize_identifier(i)),
            SimpleSelector::Class(c) => write!(f, ".{}", serialize_identifier(c)),
            SimpleSelector::Attribute { name, op, value, case } => {
                write!(f, "[{}", serialize_identifier(name))?;
                let op_text = match op {
                    AttrOp::Exists => return f.write_str("]"),
                    AttrOp::Equals => "=",
                    AttrOp::Includes => "~=",
                    AttrOp::DashMatch => "|=",
                    AttrOp::Prefix => "^=",
                    AttrOp::Suffix => "$=",
                    AttrOp::Substring => "*=",
                };
                write!(f, "{op_text}{}", serialize_string(value))?;
                match case {
                    AttrCase::Default => {}
                    AttrCase::Insensitive => f.write_str(" i")?,
                    AttrCase::Sensitive => f.write_str(" s")?,
                }
                f.write_str("]")
            }
            SimpleSelector::PseudoClass(pc) => write!(f, "{pc}"),
        }
    }
}

/// Serializes `An+B` in canonical form (`2n+1`, `-n+3`, `4`).
pub fn format_anb(a: i32, b: i32) -> String {
    if a == 0 {
        return b.to_string();
    }
    let mut s = match a {
        1 => "n".to_owned(),
        -1 => "-n".to_owned(),
        _ => format!("{a}n"),
    };
    if b > 0 {
        s.push_str(&format!("+{b}"));
    } else if b < 0 {
        s.push_str(&b.to_string());
    }
    s
}

impl fmt::Display for PseudoClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PseudoClass::Root => f.write_str(":root"),
            PseudoClass::Empty => f.write_str(":empty"),
            PseudoClass::FirstChild => f.write_str(":first-child"),
            PseudoClass::LastChild => f.write_str(":last-child"),
            PseudoClass::OnlyChild => f.write_str(":only-child"),
            PseudoClass::FirstOfType => f.write_str(":first-of-type"),
            PseudoClass::LastOfType => f.write_str(":last-of-type"),
            PseudoClass::OnlyOfType => f.write_str(":only-of-type"),
            PseudoClass::Nth { kind, a, b, of } => {
                let name = match kind {
                    NthKind::Child => "nth-child",
                    NthKind::LastChild => "nth-last-child",
                    NthKind::OfType => "nth-of-type",
                    NthKind::LastOfType => "nth-last-of-type",
                };
                write!(f, ":{name}({}", format_anb(*a, *b))?;
                if let Some(l) = of {
                    write!(f, " of {l}")?;
                }
                f.write_str(")")
            }
            PseudoClass::Not(l) => fmt_list(f, "not", l),
            PseudoClass::Is(l) => fmt_list(f, "is", l),
            PseudoClass::Where(l) => fmt_list(f, "where", l),
            PseudoClass::Has(rel) => {
                f.write_str(":has(")?;
                for (i, r) in rel.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{r}")?;
                }
                f.write_str(")")
            }
            PseudoClass::Hover => f.write_str(":hover"),
            PseudoClass::Active => f.write_str(":active"),
            PseudoClass::Focus => f.write_str(":focus"),
            PseudoClass::FocusVisible => f.write_str(":focus-visible"),
            PseudoClass::FocusWithin => f.write_str(":focus-within"),
            PseudoClass::Visited => f.write_str(":visited"),
            PseudoClass::Link => f.write_str(":link"),
            PseudoClass::AnyLink => f.write_str(":any-link"),
            PseudoClass::Target => f.write_str(":target"),
            PseudoClass::Checked => f.write_str(":checked"),
            PseudoClass::Disabled => f.write_str(":disabled"),
            PseudoClass::Enabled => f.write_str(":enabled"),
            PseudoClass::Required => f.write_str(":required"),
            PseudoClass::Optional => f.write_str(":optional"),
            PseudoClass::ReadOnly => f.write_str(":read-only"),
            PseudoClass::ReadWrite => f.write_str(":read-write"),
            PseudoClass::PlaceholderShown => f.write_str(":placeholder-shown"),
            PseudoClass::Indeterminate => f.write_str(":indeterminate"),
            PseudoClass::Default => f.write_str(":default"),
            PseudoClass::Lang(ranges) => {
                f.write_str(":lang(")?;
                for (i, r) in ranges.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    let is_ident = !r.is_empty() && r.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !r.starts_with(|c: char| c.is_ascii_digit()) && !r.starts_with('-');
                    if is_ident {
                        f.write_str(r)?;
                    } else {
                        f.write_str(&serialize_string(r))?;
                    }
                }
                f.write_str(")")
            }
            PseudoClass::Dir(Direction::Ltr) => f.write_str(":dir(ltr)"),
            PseudoClass::Dir(Direction::Rtl) => f.write_str(":dir(rtl)"),
            PseudoClass::Scope => f.write_str(":scope"),
            PseudoClass::Defined => f.write_str(":defined"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> SelectorList {
        parse_selector_list(s).unwrap_or_else(|e| panic!("{s}: {e}"))
    }
    fn spec(s: &str) -> (u32, u32, u32) {
        let sp = parse(s).0[0].specificity();
        (sp.a, sp.b, sp.c)
    }

    #[test]
    fn parses_simple_and_compound() {
        let l = parse("div.a#b[x=y i]:hover, *, ns|p, *|*, |q");
        assert_eq!(l.0.len(), 5);
        assert_eq!(l.0[0].compounds[0].simple.len(), 5);
        assert_eq!(l.0[1].compounds[0].simple, vec![SimpleSelector::Universal]);
        assert_eq!(l.0[2].compounds[0].simple, vec![SimpleSelector::Type("p".into())]);
        assert_eq!(l.0[3].compounds[0].simple, vec![SimpleSelector::Universal]);
        assert_eq!(l.0[4].compounds[0].simple, vec![SimpleSelector::Type("q".into())]);
    }

    #[test]
    fn parses_combinators() {
        let l = parse("a b>c+d~e");
        assert_eq!(l.0[0].combinators, vec![Combinator::Descendant, Combinator::Child, Combinator::NextSibling, Combinator::SubsequentSibling]);
        let l = parse("a  >  b\n~\tc");
        assert_eq!(l.0[0].combinators, vec![Combinator::Child, Combinator::SubsequentSibling]);
        assert_eq!(parse_selector_list("a || b").unwrap_err().kind, SelectorErrorKind::ColumnCombinator);
        assert_eq!(parse_selector_list("a >").unwrap_err().kind, SelectorErrorKind::Empty);
        assert_eq!(parse_selector_list("").unwrap_err().kind, SelectorErrorKind::Empty);
        assert_eq!(parse_selector_list("a,").unwrap_err().kind, SelectorErrorKind::Empty);
    }

    #[test]
    fn parses_attributes() {
        let attr = |s: &str| match &parse(s).0[0].compounds[0].simple[0] {
            SimpleSelector::Attribute { name, op, value, case } => (name.clone(), *op, value.clone(), *case),
            other => panic!("{other:?}"),
        };
        assert_eq!(attr("[a]"), ("a".into(), AttrOp::Exists, "".into(), AttrCase::Default));
        assert_eq!(attr("[ a = b ]"), ("a".into(), AttrOp::Equals, "b".into(), AttrCase::Default));
        assert_eq!(attr("[a~='b c' i]"), ("a".into(), AttrOp::Includes, "b c".into(), AttrCase::Insensitive));
        assert_eq!(attr("[a|=b S]"), ("a".into(), AttrOp::DashMatch, "b".into(), AttrCase::Sensitive));
        assert_eq!(attr("[a^=b]").1, AttrOp::Prefix);
        assert_eq!(attr("[a$=b]").1, AttrOp::Suffix);
        assert_eq!(attr("[a*=b]").1, AttrOp::Substring);
        assert_eq!(attr("[*|a=b]").0, "a");
        assert_eq!(attr("[svg|href]").0, "href");
        assert!(parse_selector_list("[a=]").is_err());
        assert!(parse_selector_list("[a b]").is_err());
        assert!(parse_selector_list("[a=b x]").is_err());
        assert!(parse_selector_list("[a==b]").is_err());
    }

    #[test]
    fn parses_pseudo_classes_and_elements() {
        let l = parse("a:not(.x, [y]):is(b, c):where(d):has(> e, ~ f):nth-child(2n+1 of .g)::before");
        let c = &l.0[0].compounds[0].simple;
        assert!(matches!(c[1], SimpleSelector::PseudoClass(PseudoClass::Not(ref n)) if n.0.len() == 2));
        assert!(matches!(c[2], SimpleSelector::PseudoClass(PseudoClass::Is(_))));
        assert!(matches!(c[3], SimpleSelector::PseudoClass(PseudoClass::Where(_))));
        match &c[4] {
            SimpleSelector::PseudoClass(PseudoClass::Has(r)) => {
                assert_eq!(r[0].combinator, Combinator::Child);
                assert_eq!(r[1].combinator, Combinator::SubsequentSibling);
            }
            o => panic!("{o:?}"),
        }
        assert!(matches!(c[5], SimpleSelector::PseudoClass(PseudoClass::Nth { kind: NthKind::Child, a: 2, b: 1, of: Some(_) })));
        assert_eq!(l.0[0].pseudo_element, Some(PseudoElement::Before));
        assert_eq!(parse("p:first-line").0[0].pseudo_element, Some(PseudoElement::FirstLine));
        assert_eq!(parse("p:after").0[0].pseudo_element, Some(PseudoElement::After));
        assert_eq!(parse("p::first-letter").0[0].pseudo_element, Some(PseudoElement::FirstLetter));
        assert!(!PseudoElement::FirstLetter.supported());
        assert_eq!(parse_selector_list("::before:hover").unwrap_err().kind, SelectorErrorKind::Syntax);
        assert_eq!(parse_selector_list("a::foo").unwrap_err().kind, SelectorErrorKind::UnsupportedPseudoElement);
        assert_eq!(parse_selector_list("a:host").unwrap_err().kind, SelectorErrorKind::UnsupportedPseudoClass);
        assert_eq!(parse_selector_list(":has(:has(a))").unwrap_err().kind, SelectorErrorKind::NestedHas);
        assert_eq!(parse_selector_list(":has(:not(:has(a)))").unwrap_err().kind, SelectorErrorKind::NestedHas);
        // Forgiving lists drop a nested :has() instead of failing.
        assert_eq!(parse_selector_list(":has(:is(:has(a), b))").unwrap().to_string(), ":has(:is(b))");
        assert_eq!(parse_selector_list(":not(::before)").unwrap_err().kind, SelectorErrorKind::Syntax);
        assert_eq!(parse_selector_list("#0a").unwrap_err().kind, SelectorErrorKind::Syntax);
        // Forgiving lists drop what they cannot parse.
        match &parse(":is(a, :host, b)").0[0].compounds[0].simple[0] {
            SimpleSelector::PseudoClass(PseudoClass::Is(l)) => assert_eq!(l.0.len(), 2),
            o => panic!("{o:?}"),
        }
        assert!(matches!(parse(":lang(en, \"fr-*\")").0[0].compounds[0].simple[0], SimpleSelector::PseudoClass(PseudoClass::Lang(ref r)) if r == &["en", "fr-*"]));
        assert!(matches!(parse(":dir(RTL)").0[0].compounds[0].simple[0], SimpleSelector::PseudoClass(PseudoClass::Dir(Direction::Rtl))));
        for name in ["root", "empty", "first-child", "last-child", "only-child", "first-of-type", "last-of-type", "only-of-type", "hover", "active", "focus", "focus-visible", "focus-within", "visited", "link", "any-link", "target", "checked", "disabled", "enabled", "required", "optional", "read-only", "read-write", "placeholder-shown", "indeterminate", "default", "scope", "defined"] {
            parse(&format!(":{name}"));
        }
    }

    #[test]
    fn anb() {
        assert_eq!(parse_anb("odd"), Some((2, 1)));
        assert_eq!(parse_anb(" EVEN "), Some((2, 0)));
        assert_eq!(parse_anb("3"), Some((0, 3)));
        assert_eq!(parse_anb("-14"), Some((0, -14)));
        assert_eq!(parse_anb("n"), Some((1, 0)));
        assert_eq!(parse_anb("-n"), Some((-1, 0)));
        assert_eq!(parse_anb("+n"), Some((1, 0)));
        assert_eq!(parse_anb("+ n"), None);
        assert_eq!(parse_anb("3n"), Some((3, 0)));
        assert_eq!(parse_anb("-3n+1"), Some((-3, 1)));
        assert_eq!(parse_anb("3n + 1"), Some((3, 1)));
        assert_eq!(parse_anb("3n +1"), Some((3, 1)));
        assert_eq!(parse_anb("3n+ 1"), Some((3, 1)));
        assert_eq!(parse_anb("3n - 1"), Some((3, -1)));
        assert_eq!(parse_anb("3n-1"), Some((3, -1)));
        assert_eq!(parse_anb("3n- 1"), Some((3, -1)));
        assert_eq!(parse_anb("n-1"), Some((1, -1)));
        assert_eq!(parse_anb("-n- 1"), Some((-1, -1)));
        assert_eq!(parse_anb("-n-1"), Some((-1, -1)));
        assert_eq!(parse_anb("3.1n"), None);
        assert_eq!(parse_anb("3 n"), None);
        assert_eq!(parse_anb("n + +1"), None);
        assert_eq!(parse_anb("n+1 2"), None);
        assert_eq!(parse_anb("ödd"), None);
        assert_eq!(parse_anb("n-"), None);
    }

    #[test]
    fn specificity_rules() {
        assert_eq!(spec("*"), (0, 0, 0));
        assert_eq!(spec("li"), (0, 0, 1));
        assert_eq!(spec("ul li"), (0, 0, 2));
        assert_eq!(spec("h1 + *[rel=up]"), (0, 1, 1));
        assert_eq!(spec("ul ol li.red"), (0, 1, 3));
        assert_eq!(spec("li.red.level"), (0, 2, 1));
        assert_eq!(spec("#x34y"), (1, 0, 0));
        assert_eq!(spec("#s12:not(FOO)"), (1, 0, 1));
        assert_eq!(spec(".foo :is(.bar, #baz)"), (1, 1, 0));
        assert_eq!(spec(":where(#a)"), (0, 0, 0));
        assert_eq!(spec(":not(#a, .b)"), (1, 0, 0));
        assert_eq!(spec(":has(> #a, .b)"), (1, 0, 0));
        assert_eq!(spec(":nth-child(even of li, .important)"), (0, 2, 0));
        assert_eq!(spec("p::before"), (0, 0, 2));
        assert_eq!(spec("a:hover::after"), (0, 1, 2));
        assert!(Specificity::new(1, 0, 0) > Specificity::new(0, 99, 99));
    }

    #[test]
    fn serialize_round_trip() {
        for s in [
            "div.a#b[x=\"y\" i]:hover",
            "a b > c + d ~ e",
            "*",
            "a:not(.x, [y]):is(b, c):where(d):has(> e, ~ f, + g, h):nth-child(2n+1 of .g)::before",
            ":nth-child(2n+1)",
            ":nth-last-child(-n+3)",
            ":nth-of-type(4)",
            ":nth-last-of-type(n)",
            ":lang(en, \"fr-*\", \"*-CH\")",
            ":dir(rtl)",
            "p::first-letter",
            "[data-x~=\"a b\" s]",
            ".\\31 23",
            "#\\-",
            "a::marker",
            ":root > :empty",
        ] {
            let l = parse(s);
            let text = l.to_string();
            let again = parse(&text);
            assert_eq!(l, again, "{s} -> {text}");
            assert_eq!(again.to_string(), text);
        }
        assert_eq!(parse(":nth-child(odd)").to_string(), ":nth-child(2n+1)");
        assert_eq!(parse(":nth-child(even)").to_string(), ":nth-child(2n)");
        assert_eq!(parse("A>B").to_string(), "A > B");
        assert_eq!(parse("a[b=c]").to_string(), "a[b=\"c\"]");
    }

    #[test]
    fn nesting_resolution() {
        let parent = parse(".a, .b");
        let nested = parse("&:hover");
        let r = nested.resolve_nesting(Some(&parent));
        assert_eq!(r.to_string(), ":is(.a, .b):hover");
        let bare = parse(".c");
        assert_eq!(bare.resolve_nesting(Some(&parent)).to_string(), ":is(.a, .b) .c");
        let rel = parse_relative_selector_list("> .c, + .d").unwrap();
        assert_eq!(rel[0].resolve_nesting(Some(&parent)).to_string(), ":is(.a, .b) > .c");
        assert_eq!(rel[1].resolve_nesting(Some(&parent)).to_string(), ":is(.a, .b) + .d");
        assert_eq!(parse(".x &").resolve_nesting(Some(&parent)).to_string(), ".x :is(.a, .b)");
        assert_eq!(parse("&").resolve_nesting(None).to_string(), ":scope");
        assert_eq!(parse(":not(&)").resolve_nesting(Some(&parent)).to_string(), ":not(:is(.a, .b))");
        // Specificity of `&` follows :is().
        let sp = parse("& .c").resolve_nesting(Some(&parse("#p, .q"))).0[0].specificity();
        assert_eq!((sp.a, sp.b, sp.c), (1, 1, 0));
    }
}
