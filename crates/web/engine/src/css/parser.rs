//! CSS Syntax Level 3 parsing (https://www.w3.org/TR/css-syntax-3/#parsing), in two
//! layers. The raw layer implements the specification's entry points over component
//! values (`parse_stylesheet_rules`, `parse_rule_list`, `parse_rule`,
//! `parse_declaration`, `parse_declaration_list`, `parse_block_contents`,
//! `parse_component_value_list`, `parse_component_value`) with its error recovery.
//! The typed layer turns raw rules into the `Stylesheet` model with selectors, media
//! queries and the at-rules the engine knows, flattening CSS Nesting on the way.

use super::media::{parse_supports_condition, Media, MediaQueryList, SupportsCondition};
use super::selector::{parse_relative_list, SelectorList};
use super::token::{ComponentValue, Declaration, Number, Token};
use super::tokenizer::{serialize_token, tokenize};
use crate::{Strictness, Unsupported, UnsupportedKind};

// ---------------------------------------------------------------------------
// Raw layer

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AtRule {
    pub name: String,
    pub prelude: Vec<ComponentValue>,
    /// `None` for statement at-rules (`@import ...;`).
    pub block: Option<Vec<ComponentValue>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualifiedRule {
    pub prelude: Vec<ComponentValue>,
    pub block: Vec<ComponentValue>,
}

/// One item of a rule list, declaration list or block contents. `Invalid` records a
/// parse error the specification says to discard, so the corpus tests can see it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Declaration(Declaration),
    AtRule(AtRule),
    QualifiedRule(QualifiedRule),
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    Invalid,
    ExtraInput,
}

struct Stream<'a> {
    values: &'a [ComponentValue],
    pos: usize,
}

impl<'a> Stream<'a> {
    fn peek(&self) -> Option<&'a ComponentValue> {
        self.values.get(self.pos)
    }
    fn at_end(&self) -> bool {
        self.pos >= self.values.len()
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(v) if v.is_whitespace()) {
            self.pos += 1;
        }
    }
    fn is(&self, t: &Token) -> bool {
        matches!(self.peek(), Some(ComponentValue::Token(x)) if x == t)
    }
    fn is_open_curly(&self) -> bool {
        matches!(
            self.peek(),
            Some(ComponentValue::Token(Token::OpenCurly))
                | Some(ComponentValue::Block {
                    open: Token::OpenCurly,
                    ..
                })
        )
    }

    /// Consume a component value: a block or function is built from its tokens, an
    /// already-built block passes through.
    fn consume_component_value(&mut self) -> ComponentValue {
        let v = self.values[self.pos].clone();
        self.pos += 1;
        match v {
            ComponentValue::Token(Token::OpenCurly) => {
                self.consume_block(Token::OpenCurly, &Token::CloseCurly)
            }
            ComponentValue::Token(Token::OpenSquare) => {
                self.consume_block(Token::OpenSquare, &Token::CloseSquare)
            }
            ComponentValue::Token(Token::OpenParen) => {
                self.consume_block(Token::OpenParen, &Token::CloseParen)
            }
            ComponentValue::Token(Token::Function(name)) => {
                let mut args = Vec::new();
                while !self.at_end() {
                    if self.is(&Token::CloseParen) {
                        self.pos += 1;
                        break;
                    }
                    args.push(self.consume_component_value());
                }
                ComponentValue::Function { name, args }
            }
            v => v,
        }
    }

    fn consume_block(&mut self, open: Token, close: &Token) -> ComponentValue {
        let mut contents = Vec::new();
        while !self.at_end() {
            if self.is(close) {
                self.pos += 1;
                break;
            }
            contents.push(self.consume_component_value());
        }
        ComponentValue::Block { open, contents }
    }

    /// The block after a rule prelude: consumes a `{` block or passes through one.
    fn consume_curly_block(&mut self) -> Vec<ComponentValue> {
        match self.consume_component_value() {
            ComponentValue::Block { contents, .. } => contents,
            _ => unreachable!("caller checked for a curly block"),
        }
    }

    fn consume_at_rule(&mut self) -> AtRule {
        let name = match self.consume_component_value() {
            ComponentValue::Token(Token::AtKeyword(n)) => n,
            _ => unreachable!("caller checked for an at-keyword"),
        };
        let mut prelude = Vec::new();
        loop {
            if self.at_end() {
                return AtRule {
                    name,
                    prelude,
                    block: None,
                };
            }
            if self.is(&Token::Semicolon) {
                self.pos += 1;
                return AtRule {
                    name,
                    prelude,
                    block: None,
                };
            }
            if self.is_open_curly() {
                let block = self.consume_curly_block();
                return AtRule {
                    name,
                    prelude,
                    block: Some(block),
                };
            }
            prelude.push(self.consume_component_value());
        }
    }

    /// Consume a qualified rule. `nested` stops at an unmatched `}` (not consumed);
    /// `stop_semicolon` stops at `;` (not consumed). `None` is a parse error.
    fn consume_qualified_rule(
        &mut self,
        nested: bool,
        stop_semicolon: bool,
    ) -> Option<QualifiedRule> {
        let mut prelude: Vec<ComponentValue> = Vec::new();
        loop {
            if self.at_end() {
                return None;
            }
            if stop_semicolon && self.is(&Token::Semicolon) {
                return None;
            }
            if nested && self.is(&Token::CloseCurly) {
                return None;
            }
            if self.is_open_curly() {
                // A custom property declaration mistaken for a rule: discard.
                let non_ws: Vec<&ComponentValue> = prelude
                    .iter()
                    .filter(|v| !v.is_whitespace())
                    .take(2)
                    .collect();
                if let [ComponentValue::Token(Token::Ident(name)), ComponentValue::Token(Token::Colon)] =
                    non_ws.as_slice()
                {
                    if name.starts_with("--") {
                        if nested {
                            self.consume_bad_declaration_remnants(true);
                        } else {
                            self.consume_component_value();
                        }
                        return None;
                    }
                }
                let block = self.consume_curly_block();
                return Some(QualifiedRule { prelude, block });
            }
            prelude.push(self.consume_component_value());
        }
    }

    fn consume_bad_declaration_remnants(&mut self, nested: bool) {
        loop {
            if self.at_end() {
                return;
            }
            if self.is(&Token::Semicolon) {
                self.pos += 1;
                return;
            }
            if nested && self.is(&Token::CloseCurly) {
                return;
            }
            self.consume_component_value();
        }
    }

    /// Consume a declaration from the current position, stopping at `;` (when
    /// `stop_semicolon`), an unmatched `}` (when `nested`) or the end.
    fn consume_declaration(&mut self, nested: bool, stop_semicolon: bool) -> Option<Declaration> {
        self.skip_ws();
        let name = match self.peek() {
            Some(ComponentValue::Token(Token::Ident(n))) => n.clone(),
            _ => {
                self.consume_bad_declaration_remnants(nested);
                return None;
            }
        };
        self.pos += 1;
        self.skip_ws();
        if !self.is(&Token::Colon) {
            self.consume_bad_declaration_remnants(nested);
            return None;
        }
        self.pos += 1;
        self.skip_ws();
        let mut value = Vec::new();
        loop {
            if self.at_end() {
                break;
            }
            if stop_semicolon && self.is(&Token::Semicolon) {
                self.pos += 1;
                break;
            }
            if nested && self.is(&Token::CloseCurly) {
                break;
            }
            value.push(self.consume_component_value());
        }
        let custom = name.starts_with("--");
        let mut important = false;
        {
            let non_ws: Vec<usize> = value
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, v)| !v.is_whitespace())
                .map(|(i, _)| i)
                .take(2)
                .collect();
            if let [last, prev] = non_ws.as_slice() {
                let is_bang = matches!(&value[*prev], ComponentValue::Token(Token::Delim('!')));
                let is_important = matches!(&value[*last], ComponentValue::Token(Token::Ident(i)) if i.eq_ignore_ascii_case("important"));
                if is_bang && is_important {
                    value.truncate(*prev);
                    important = true;
                }
            }
        }
        while value.last().is_some_and(|v| v.is_whitespace()) {
            value.pop();
        }
        while value.first().is_some_and(|v| v.is_whitespace()) {
            value.remove(0);
        }
        if !custom {
            let has_curly = value.iter().any(|v| {
                matches!(
                    v,
                    ComponentValue::Block {
                        open: Token::OpenCurly,
                        ..
                    }
                )
            });
            if has_curly && value.iter().filter(|v| !v.is_whitespace()).count() > 1 {
                return None;
            }
        }
        let name = if custom {
            name
        } else {
            name.to_ascii_lowercase()
        };
        Some(Declaration {
            name,
            value,
            important,
        })
    }

    /// "Consume a list of declarations" (the 2021 CR algorithm used for `style=""`
    /// and declaration-only blocks): declarations and at-rules.
    fn consume_declaration_list(&mut self) -> Vec<Item> {
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None => return out,
                Some(v) if v.is_whitespace() => self.pos += 1,
                Some(ComponentValue::Token(Token::Semicolon)) => self.pos += 1,
                Some(ComponentValue::Token(Token::AtKeyword(_))) => {
                    let r = self.consume_at_rule();
                    out.push(Item::AtRule(r));
                }
                Some(ComponentValue::Token(Token::Ident(_))) => {
                    let start = self.pos;
                    let mut temp_end = start;
                    while !self.at_end() && !self.is(&Token::Semicolon) {
                        self.consume_component_value();
                        temp_end = self.pos;
                    }
                    let mut sub = Stream {
                        values: &self.values[start..temp_end],
                        pos: 0,
                    };
                    // The temporary list was built from consumed component values, but
                    // consuming again from the raw slice gives the same grouping.
                    match sub.consume_declaration(false, false) {
                        Some(d) => out.push(Item::Declaration(d)),
                        None => out.push(Item::Invalid),
                    }
                }
                Some(_) => {
                    out.push(Item::Invalid);
                    while !self.at_end() && !self.is(&Token::Semicolon) {
                        self.consume_component_value();
                    }
                }
            }
        }
    }

    /// "Consume a block's contents" (CSS Syntax 3 editor's draft, for nesting):
    /// declarations, at-rules and qualified rules mixed.
    fn consume_block_contents(&mut self) -> Vec<Item> {
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None => return out,
                Some(v) if v.is_whitespace() => self.pos += 1,
                Some(ComponentValue::Token(Token::Semicolon)) => self.pos += 1,
                Some(ComponentValue::Token(Token::CloseCurly)) => self.pos += 1,
                Some(ComponentValue::Token(Token::AtKeyword(_))) => {
                    let r = self.consume_at_rule();
                    out.push(Item::AtRule(r));
                }
                Some(_) => {
                    let mark = self.pos;
                    if let Some(d) = self.consume_declaration(true, true) {
                        out.push(Item::Declaration(d));
                        continue;
                    }
                    self.pos = mark;
                    match self.consume_qualified_rule(true, true) {
                        Some(r) => out.push(Item::QualifiedRule(r)),
                        None => {
                            out.push(Item::Invalid);
                            if self.is(&Token::Semicolon) {
                                self.pos += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    fn consume_rule_list(&mut self, top_level: bool) -> Vec<Item> {
        let mut out = Vec::new();
        loop {
            match self.peek() {
                None => return out,
                Some(v) if v.is_whitespace() => self.pos += 1,
                Some(ComponentValue::Token(Token::Cdo))
                | Some(ComponentValue::Token(Token::Cdc)) => {
                    if top_level {
                        self.pos += 1;
                    } else {
                        match self.consume_qualified_rule(false, false) {
                            Some(r) => out.push(Item::QualifiedRule(r)),
                            None => out.push(Item::Invalid),
                        }
                    }
                }
                Some(ComponentValue::Token(Token::AtKeyword(_))) => {
                    let r = self.consume_at_rule();
                    out.push(Item::AtRule(r));
                }
                Some(_) => match self.consume_qualified_rule(false, false) {
                    Some(r) => out.push(Item::QualifiedRule(r)),
                    None => out.push(Item::Invalid),
                },
            }
        }
    }
}

fn to_values(src: &str) -> Vec<ComponentValue> {
    tokenize(src)
        .into_iter()
        .map(ComponentValue::Token)
        .collect()
}

/// "Parse a list of component values".
pub fn parse_component_value_list(src: &str) -> Vec<ComponentValue> {
    let values = to_values(src);
    let mut s = Stream {
        values: &values,
        pos: 0,
    };
    let mut out = Vec::new();
    while !s.at_end() {
        out.push(s.consume_component_value());
    }
    out
}

/// "Parse a component value".
pub fn parse_component_value(src: &str) -> Result<ComponentValue, ParseError> {
    let values = to_values(src);
    let mut s = Stream {
        values: &values,
        pos: 0,
    };
    s.skip_ws();
    if s.at_end() {
        return Err(ParseError::Empty);
    }
    let v = s.consume_component_value();
    s.skip_ws();
    if s.at_end() {
        Ok(v)
    } else {
        Err(ParseError::ExtraInput)
    }
}

/// "Parse a declaration".
pub fn parse_declaration(src: &str) -> Result<Declaration, ParseError> {
    let values = to_values(src);
    parse_declaration_values(&values)
}

pub fn parse_declaration_values(values: &[ComponentValue]) -> Result<Declaration, ParseError> {
    let mut s = Stream { values, pos: 0 };
    s.skip_ws();
    if s.at_end() {
        return Err(ParseError::Empty);
    }
    if !matches!(s.peek(), Some(ComponentValue::Token(Token::Ident(_)))) {
        return Err(ParseError::Invalid);
    }
    s.consume_declaration(false, false)
        .ok_or(ParseError::Invalid)
}

/// "Parse a list of declarations" (declarations and at-rules; other content is
/// discarded up to the next `;` and recorded as `Item::Invalid`).
pub fn parse_declaration_list(src: &str) -> Vec<Item> {
    let values = to_values(src);
    parse_declaration_list_values(&values)
}

pub fn parse_declaration_list_values(values: &[ComponentValue]) -> Vec<Item> {
    Stream { values, pos: 0 }.consume_declaration_list()
}

/// "Parse a block's contents": declarations, at-rules and nested qualified rules.
pub fn parse_block_contents(src: &str) -> Vec<Item> {
    let values = to_values(src);
    parse_block_contents_values(&values)
}

pub fn parse_block_contents_values(values: &[ComponentValue]) -> Vec<Item> {
    Stream { values, pos: 0 }.consume_block_contents()
}

/// "Parse a rule": one at-rule or qualified rule.
pub fn parse_rule(src: &str) -> Result<Item, ParseError> {
    let values = to_values(src);
    let mut s = Stream {
        values: &values,
        pos: 0,
    };
    s.skip_ws();
    if s.at_end() {
        return Err(ParseError::Empty);
    }
    let item = if matches!(s.peek(), Some(ComponentValue::Token(Token::AtKeyword(_)))) {
        Item::AtRule(s.consume_at_rule())
    } else {
        match s.consume_qualified_rule(false, false) {
            Some(r) => Item::QualifiedRule(r),
            None => return Err(ParseError::Invalid),
        }
    };
    s.skip_ws();
    if s.at_end() {
        Ok(item)
    } else {
        Err(ParseError::ExtraInput)
    }
}

/// "Parse a list of rules" (CDO/CDC are not ignored).
pub fn parse_rule_list(src: &str) -> Vec<Item> {
    let values = to_values(src);
    Stream {
        values: &values,
        pos: 0,
    }
    .consume_rule_list(false)
}

pub fn parse_rule_list_values(values: &[ComponentValue]) -> Vec<Item> {
    Stream { values, pos: 0 }.consume_rule_list(false)
}

/// "Parse a stylesheet": the raw rules with CDO/CDC ignored at the top level.
pub fn parse_stylesheet_rules(src: &str) -> Vec<Item> {
    let values = to_values(src);
    Stream {
        values: &values,
        pos: 0,
    }
    .consume_rule_list(true)
}

/// Serializes a component value back to CSS text.
pub fn serialize_component_value(v: &ComponentValue, out: &mut String) {
    match v {
        ComponentValue::Token(t) => serialize_token(t, out),
        ComponentValue::Function { name, args } => {
            serialize_token(&Token::Function(name.clone()), out);
            serialize_component_values(args, out);
            out.push(')');
        }
        ComponentValue::Block { open, contents } => {
            serialize_token(open, out);
            serialize_component_values(contents, out);
            out.push(match open {
                Token::OpenCurly => '}',
                Token::OpenSquare => ']',
                _ => ')',
            });
        }
    }
}

pub fn serialize_component_values(values: &[ComponentValue], out: &mut String) {
    for v in values {
        serialize_component_value(v, out);
    }
}

pub fn component_values_to_string(values: &[ComponentValue]) -> String {
    let mut s = String::new();
    serialize_component_values(values, &mut s);
    s
}

// ---------------------------------------------------------------------------
// Typed layer

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Origin {
    UserAgent,
    #[default]
    Author,
    /// `style=""` attributes and script-set inline style.
    Inline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyframeSelector {
    From,
    To,
    Percentage(Number),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Rule {
    Style {
        selectors: SelectorList,
        declarations: Vec<Declaration>,
        /// Position among all style rules of the sheet, nested rules flattened, in
        /// source order; the cascade's tie-breaker.
        source_order: u32,
    },
    Media {
        query: MediaQueryList,
        rules: Vec<Rule>,
    },
    Import {
        url: String,
        media: MediaQueryList,
    },
    FontFace(Vec<Declaration>),
    Keyframes {
        name: String,
        frames: Vec<(Vec<KeyframeSelector>, Vec<Declaration>)>,
    },
    Supports {
        condition: SupportsCondition,
        rules: Vec<Rule>,
    },
    Layer {
        /// `@layer a, b;` lists several; a block form has one; an anonymous layer
        /// gets a synthesized `#anonN` name so it still has a place in `layer_order`.
        names: Vec<String>,
        rules: Vec<Rule>,
    },
    Page {
        /// Page selectors as written (`:first`, `wide:left`), empty for `@page {}`.
        selectors: Vec<String>,
        declarations: Vec<Declaration>,
    },
    Namespace {
        prefix: Option<String>,
        url: String,
    },
    Unknown {
        name: String,
        prelude: Vec<ComponentValue>,
        block: Option<Vec<ComponentValue>>,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
    pub origin: Origin,
    /// Fully qualified layer names (`base`, `base.forms`) in the order they were first
    /// declared; earlier layers lose to later ones in the cascade.
    pub layer_order: Vec<String>,
    /// What lenient parsing dropped or flagged.
    pub unsupported: Vec<Unsupported>,
}

/// A style rule reached through the sheet's conditional rules, with its layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleRuleRef<'a> {
    pub selectors: &'a SelectorList,
    pub declarations: &'a [Declaration],
    pub source_order: u32,
    /// Index into `Stylesheet::layer_order`; `None` for unlayered rules (which win
    /// over every layer for normal declarations).
    pub layer: Option<usize>,
}

impl Stylesheet {
    /// The style rules whose `@media` and `@supports` conditions hold, in source order.
    pub fn effective_style_rules<'a>(
        &'a self,
        media: &Media,
        is_supported: &dyn Fn(&str, &[ComponentValue]) -> bool,
    ) -> Vec<StyleRuleRef<'a>> {
        let mut out = Vec::new();
        self.walk_rules(
            &self.rules,
            media,
            is_supported,
            None,
            &mut Vec::new(),
            &mut out,
        );
        out
    }

    fn walk_rules<'a>(
        &'a self,
        rules: &'a [Rule],
        media: &Media,
        is_supported: &dyn Fn(&str, &[ComponentValue]) -> bool,
        layer: Option<usize>,
        layer_path: &mut Vec<String>,
        out: &mut Vec<StyleRuleRef<'a>>,
    ) {
        for r in rules {
            match r {
                Rule::Style {
                    selectors,
                    declarations,
                    source_order,
                } => out.push(StyleRuleRef {
                    selectors,
                    declarations,
                    source_order: *source_order,
                    layer,
                }),
                Rule::Media { query, rules } => {
                    if query.evaluate(media) {
                        self.walk_rules(rules, media, is_supported, layer, layer_path, out);
                    }
                }
                Rule::Supports { condition, rules } => {
                    if condition.evaluate(is_supported) {
                        self.walk_rules(rules, media, is_supported, layer, layer_path, out);
                    }
                }
                Rule::Layer { names, rules } => {
                    if rules.is_empty() {
                        continue;
                    }
                    let name = names.first().cloned().unwrap_or_default();
                    layer_path.push(name);
                    let full = layer_path.join(".");
                    let idx = self.layer_order.iter().position(|l| *l == full);
                    self.walk_rules(rules, media, is_supported, idx, layer_path, out);
                    layer_path.pop();
                }
                _ => {}
            }
        }
    }

    /// `@import` rules in order, for the loader to resolve.
    pub fn imports(&self) -> impl Iterator<Item = (&str, &MediaQueryList)> {
        self.rules.iter().filter_map(|r| match r {
            Rule::Import { url, media } => Some((url.as_str(), media)),
            _ => None,
        })
    }

    /// Every style rule in the sheet, conditional rules included, in source order.
    pub fn all_style_rules(&self) -> Vec<StyleRuleRef<'_>> {
        fn walk<'a>(rules: &'a [Rule], out: &mut Vec<StyleRuleRef<'a>>) {
            for r in rules {
                match r {
                    Rule::Style {
                        selectors,
                        declarations,
                        source_order,
                    } => out.push(StyleRuleRef {
                        selectors,
                        declarations,
                        source_order: *source_order,
                        layer: None,
                    }),
                    Rule::Media { rules, .. }
                    | Rule::Supports { rules, .. }
                    | Rule::Layer { rules, .. } => walk(rules, out),
                    _ => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.rules, &mut out);
        out
    }
}

/// Parses a stylesheet. In lenient mode nothing fails: unknown at-rules and invalid
/// selectors are dropped or kept as `Rule::Unknown` and recorded in `unsupported`.
/// In strict mode the first such construct is the error.
pub fn parse_stylesheet(
    src: &str,
    origin: Origin,
    strictness: Strictness,
) -> Result<Stylesheet, Unsupported> {
    let items = parse_stylesheet_rules(src);
    let mut c = Converter {
        strictness,
        unsupported: Vec::new(),
        order: 0,
        layer_order: Vec::new(),
        layer_path: Vec::new(),
        anon_layers: 0,
        seen_non_import: false,
    };
    let rules = c.convert_items(items, None, true)?;
    Ok(Stylesheet {
        rules,
        origin,
        layer_order: c.layer_order,
        unsupported: c.unsupported,
    })
}

/// Parses a `style=""` attribute: declarations only, at-rules dropped.
pub fn parse_declaration_block(src: &str) -> Vec<Declaration> {
    parse_declaration_list(src)
        .into_iter()
        .filter_map(|i| match i {
            Item::Declaration(d) => Some(d),
            _ => None,
        })
        .collect()
}

struct Converter {
    strictness: Strictness,
    unsupported: Vec<Unsupported>,
    order: u32,
    layer_order: Vec<String>,
    layer_path: Vec<String>,
    anon_layers: u32,
    seen_non_import: bool,
}

impl Converter {
    fn report(
        &mut self,
        kind: UnsupportedKind,
        name: impl Into<String>,
        detail: impl Into<String>,
    ) -> Result<(), Unsupported> {
        let u = Unsupported {
            kind,
            name: name.into(),
            detail: detail.into(),
        };
        if self.strictness == Strictness::Strict {
            return Err(u);
        }
        self.unsupported.push(u);
        Ok(())
    }

    fn register_layer(&mut self, name: &str) {
        let full = if self.layer_path.is_empty() {
            name.to_owned()
        } else {
            format!("{}.{}", self.layer_path.join("."), name)
        };
        // `@layer a.b` declares `a` as well.
        let mut prefix = String::new();
        for (i, part) in full.split('.').enumerate() {
            if i > 0 {
                prefix.push('.');
            }
            prefix.push_str(part);
            if !self.layer_order.contains(&prefix) {
                self.layer_order.push(prefix.clone());
            }
        }
    }

    /// Converts raw items. `parent` is the enclosing style rule's selector list when
    /// inside one (CSS Nesting); `top_level` allows `@import`, `@namespace`, `@charset`.
    fn convert_items(
        &mut self,
        items: Vec<Item>,
        parent: Option<&SelectorList>,
        top_level: bool,
    ) -> Result<Vec<Rule>, Unsupported> {
        let mut out = Vec::new();
        let mut pending_decls: Vec<Declaration> = Vec::new();
        for item in items {
            match item {
                Item::Invalid => {}
                Item::Declaration(d) => {
                    if parent.is_some() {
                        pending_decls.push(d);
                    }
                }
                Item::QualifiedRule(q) => {
                    self.seen_non_import = true;
                    if let Some(p) = parent {
                        if !pending_decls.is_empty() {
                            out.push(Rule::Style {
                                selectors: p.clone(),
                                declarations: std::mem::take(&mut pending_decls),
                                source_order: self.next_order(),
                            });
                        }
                    }
                    self.convert_style_rule(q, parent, &mut out)?;
                }
                Item::AtRule(a) => {
                    if let Some(p) = parent {
                        if !pending_decls.is_empty() {
                            out.push(Rule::Style {
                                selectors: p.clone(),
                                declarations: std::mem::take(&mut pending_decls),
                                source_order: self.next_order(),
                            });
                        }
                    }
                    if let Some(r) = self.convert_at_rule(a, parent, top_level)? {
                        out.push(r);
                    }
                }
            }
        }
        if let Some(p) = parent {
            if !pending_decls.is_empty() {
                out.push(Rule::Style {
                    selectors: p.clone(),
                    declarations: pending_decls,
                    source_order: self.next_order(),
                });
            }
        }
        Ok(out)
    }

    fn next_order(&mut self) -> u32 {
        let o = self.order;
        self.order += 1;
        o
    }

    fn convert_style_rule(
        &mut self,
        q: QualifiedRule,
        parent: Option<&SelectorList>,
        out: &mut Vec<Rule>,
    ) -> Result<(), Unsupported> {
        let prelude_text = component_values_to_string(&q.prelude);
        let selectors = match parent {
            None => match SelectorList::parse(&q.prelude) {
                Ok(l) => l.resolve_nesting(None),
                Err(e) => {
                    let kind = UnsupportedKind::Selector;
                    self.report(kind, prelude_text.trim(), e.to_string())?;
                    return Ok(());
                }
            },
            Some(p) => match parse_relative_list(&q.prelude, false) {
                Ok(rel) => SelectorList(rel.iter().map(|r| r.resolve_nesting(Some(p))).collect()),
                Err(e) => {
                    self.report(
                        UnsupportedKind::Selector,
                        prelude_text.trim(),
                        e.to_string(),
                    )?;
                    return Ok(());
                }
            },
        };
        for s in &selectors.0 {
            if let Some(pe) = s.pseudo_element {
                if !pe.supported() {
                    self.report(
                        UnsupportedKind::Selector,
                        format!("::{}", pe.name()),
                        "pseudo-element parsed but not rendered",
                    )?;
                }
            }
        }
        let items = parse_block_contents_values(&q.block);
        let mut declarations = Vec::new();
        let mut rest = Vec::new();
        let mut seen_rule = false;
        for item in items {
            match item {
                Item::Declaration(d) if !seen_rule => declarations.push(d),
                Item::Invalid => {}
                other => {
                    seen_rule = true;
                    rest.push(other);
                }
            }
        }
        let order = self.next_order();
        out.push(Rule::Style {
            selectors: selectors.clone(),
            declarations,
            source_order: order,
        });
        if !rest.is_empty() {
            let nested = self.convert_items(rest, Some(&selectors), false)?;
            out.extend(nested);
        }
        Ok(())
    }

    fn convert_at_rule(
        &mut self,
        a: AtRule,
        parent: Option<&SelectorList>,
        top_level: bool,
    ) -> Result<Option<Rule>, Unsupported> {
        let name = a.name.to_ascii_lowercase();
        let name = name
            .strip_prefix("-webkit-")
            .map(str::to_owned)
            .unwrap_or(name);
        match name.as_str() {
            "charset" => Ok(None),
            "import" => {
                if !top_level || self.seen_non_import {
                    return Ok(None);
                }
                if a.block.is_some() {
                    return Ok(None);
                }
                let mut items = a.prelude.iter().filter(|v| !v.is_whitespace());
                let url = match items.next() {
                    Some(ComponentValue::Token(Token::String(s)))
                    | Some(ComponentValue::Token(Token::Url(s))) => s.clone(),
                    Some(ComponentValue::Function { name, args })
                        if name.eq_ignore_ascii_case("url") =>
                    {
                        match args.iter().find(|v| !v.is_whitespace()) {
                            Some(ComponentValue::Token(Token::String(s))) => s.clone(),
                            _ => return Ok(None),
                        }
                    }
                    _ => return Ok(None),
                };
                // Optional `layer`, `layer(...)`, `supports(...)`; the rest is media.
                let rest: Vec<ComponentValue> = a
                    .prelude
                    .iter()
                    .skip_while(|v| v.is_whitespace())
                    .skip(1)
                    .cloned()
                    .collect();
                let mut rest = rest.as_slice();
                loop {
                    let trimmed_start = rest
                        .iter()
                        .position(|v| !v.is_whitespace())
                        .unwrap_or(rest.len());
                    rest = &rest[trimmed_start..];
                    match rest.first() {
                        Some(ComponentValue::Token(Token::Ident(i)))
                            if i.eq_ignore_ascii_case("layer") =>
                        {
                            rest = &rest[1..]
                        }
                        Some(ComponentValue::Function { name, .. })
                            if name.eq_ignore_ascii_case("layer")
                                || name.eq_ignore_ascii_case("supports") =>
                        {
                            rest = &rest[1..]
                        }
                        _ => break,
                    }
                }
                Ok(Some(Rule::Import {
                    url,
                    media: MediaQueryList::from_values(rest),
                }))
            }
            "namespace" => {
                if !top_level {
                    return Ok(None);
                }
                let items: Vec<&ComponentValue> =
                    a.prelude.iter().filter(|v| !v.is_whitespace()).collect();
                let url_of = |v: &ComponentValue| match v {
                    ComponentValue::Token(Token::String(s))
                    | ComponentValue::Token(Token::Url(s)) => Some(s.clone()),
                    ComponentValue::Function { name, args } if name.eq_ignore_ascii_case("url") => {
                        match args.iter().find(|v| !v.is_whitespace()) {
                            Some(ComponentValue::Token(Token::String(s))) => Some(s.clone()),
                            _ => None,
                        }
                    }
                    _ => None,
                };
                match items.as_slice() {
                    [u] => Ok(url_of(u).map(|url| Rule::Namespace { prefix: None, url })),
                    [ComponentValue::Token(Token::Ident(p)), u] => {
                        Ok(url_of(u).map(|url| Rule::Namespace {
                            prefix: Some(p.clone()),
                            url,
                        }))
                    }
                    _ => Ok(None),
                }
            }
            "media" => {
                let Some(block) = a.block else {
                    return Ok(None);
                };
                self.seen_non_import = true;
                let query = MediaQueryList::from_values(&a.prelude);
                let rules =
                    self.convert_items(parse_block_contents_values(&block), parent, false)?;
                Ok(Some(Rule::Media { query, rules }))
            }
            "supports" => {
                let Some(block) = a.block else {
                    return Ok(None);
                };
                self.seen_non_import = true;
                let Some(condition) = parse_supports_condition(&a.prelude) else {
                    self.report(
                        UnsupportedKind::AtRule,
                        "@supports",
                        format!(
                            "invalid condition `{}`",
                            component_values_to_string(&a.prelude).trim()
                        ),
                    )?;
                    return Ok(None);
                };
                let rules =
                    self.convert_items(parse_block_contents_values(&block), parent, false)?;
                Ok(Some(Rule::Supports { condition, rules }))
            }
            "layer" => {
                let names: Vec<String> = a
                    .prelude
                    .split(|v| matches!(v, ComponentValue::Token(Token::Comma)))
                    .map(|group| {
                        let idents: Vec<&ComponentValue> =
                            group.iter().filter(|v| !v.is_whitespace()).collect();
                        let mut s = String::new();
                        for (i, v) in idents.iter().enumerate() {
                            match v {
                                ComponentValue::Token(Token::Ident(x)) if i % 2 == 0 => {
                                    s.push_str(x)
                                }
                                ComponentValue::Token(Token::Delim('.')) if i % 2 == 1 => {
                                    s.push('.')
                                }
                                _ => return None,
                            }
                        }
                        if s.is_empty() || s.ends_with('.') {
                            None
                        } else {
                            Some(s)
                        }
                    })
                    .collect::<Option<Vec<String>>>()
                    .unwrap_or_default();
                match a.block {
                    None => {
                        if names.is_empty() {
                            return Ok(None);
                        }
                        for n in &names {
                            self.register_layer(n);
                        }
                        Ok(Some(Rule::Layer {
                            names,
                            rules: Vec::new(),
                        }))
                    }
                    Some(block) => {
                        self.seen_non_import = true;
                        let name = match names.as_slice() {
                            [] if a.prelude.iter().all(|v| v.is_whitespace()) => {
                                self.anon_layers += 1;
                                format!("#anon{}", self.anon_layers)
                            }
                            [n] => n.clone(),
                            _ => return Ok(None),
                        };
                        self.register_layer(&name);
                        self.layer_path.push(name.clone());
                        let rules =
                            self.convert_items(parse_block_contents_values(&block), parent, false);
                        self.layer_path.pop();
                        Ok(Some(Rule::Layer {
                            names: vec![name],
                            rules: rules?,
                        }))
                    }
                }
            }
            "font-face" => {
                let Some(block) = a.block else {
                    return Ok(None);
                };
                self.seen_non_import = true;
                Ok(Some(Rule::FontFace(declarations_only(&block))))
            }
            "keyframes" => {
                let Some(block) = a.block else {
                    return Ok(None);
                };
                self.seen_non_import = true;
                let items: Vec<&ComponentValue> =
                    a.prelude.iter().filter(|v| !v.is_whitespace()).collect();
                let name = match items.as_slice() {
                    [ComponentValue::Token(Token::Ident(n))]
                    | [ComponentValue::Token(Token::String(n))] => n.clone(),
                    _ => return Ok(None),
                };
                let mut frames = Vec::new();
                for item in parse_rule_list_values(&block) {
                    if let Item::QualifiedRule(q) = item {
                        let mut selectors = Vec::new();
                        let mut ok = true;
                        for group in q
                            .prelude
                            .split(|v| matches!(v, ComponentValue::Token(Token::Comma)))
                        {
                            let parts: Vec<&ComponentValue> =
                                group.iter().filter(|v| !v.is_whitespace()).collect();
                            match parts.as_slice() {
                                [ComponentValue::Token(Token::Ident(i))]
                                    if i.eq_ignore_ascii_case("from") =>
                                {
                                    selectors.push(KeyframeSelector::From)
                                }
                                [ComponentValue::Token(Token::Ident(i))]
                                    if i.eq_ignore_ascii_case("to") =>
                                {
                                    selectors.push(KeyframeSelector::To)
                                }
                                [ComponentValue::Token(Token::Percentage { value, .. })] => {
                                    selectors.push(KeyframeSelector::Percentage(*value))
                                }
                                _ => ok = false,
                            }
                        }
                        if ok && !selectors.is_empty() {
                            frames.push((selectors, declarations_only(&q.block)));
                        }
                    }
                }
                Ok(Some(Rule::Keyframes { name, frames }))
            }
            "page" => {
                let Some(block) = a.block else {
                    return Ok(None);
                };
                self.seen_non_import = true;
                let selectors = a
                    .prelude
                    .split(|v| matches!(v, ComponentValue::Token(Token::Comma)))
                    .map(|g| component_values_to_string(g).trim().to_owned())
                    .filter(|s| !s.is_empty())
                    .collect();
                Ok(Some(Rule::Page {
                    selectors,
                    declarations: declarations_only(&block),
                }))
            }
            _ => {
                self.seen_non_import = true;
                self.report(
                    UnsupportedKind::AtRule,
                    format!("@{}", a.name),
                    "unknown at-rule",
                )?;
                Ok(Some(Rule::Unknown {
                    name: a.name,
                    prelude: a.prelude,
                    block: a.block,
                }))
            }
        }
    }
}

fn declarations_only(block: &[ComponentValue]) -> Vec<Declaration> {
    parse_declaration_list_values(block)
        .into_iter()
        .filter_map(|i| match i {
            Item::Declaration(d) => Some(d),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decl(name: &str, value: &str, important: bool) -> Declaration {
        Declaration {
            name: name.into(),
            value: parse_component_value_list(value),
            important,
        }
    }

    #[test]
    fn component_values_and_blocks() {
        // Blocks nest by their own open/close pair: the `]` inside `( )` is a stray token.
        let v = parse_component_value_list("a [b (c] d) e");
        assert_eq!(v.len(), 3);
        match &v[2] {
            ComponentValue::Block {
                open: Token::OpenSquare,
                contents,
            } => {
                assert_eq!(contents.len(), 5);
                assert!(
                    matches!(&contents[2], ComponentValue::Block { open: Token::OpenParen, contents } if contents.len() == 4 && contents[1] == ComponentValue::Token(Token::CloseSquare))
                );
            }
            o => panic!("{o:?}"),
        }
        let v = parse_component_value_list("a [b] (c) {d} e");
        assert_eq!(v.len(), 9);
        assert_eq!(
            parse_component_value(" rgba(1, 2) "),
            Ok(ComponentValue::Function {
                name: "rgba".into(),
                args: parse_component_value_list("1, 2")
            })
        );
        assert_eq!(parse_component_value(""), Err(ParseError::Empty));
        assert_eq!(parse_component_value(".foo"), Err(ParseError::ExtraInput));
        assert_eq!(
            parse_component_value_list("(a"),
            vec![ComponentValue::Block {
                open: Token::OpenParen,
                contents: vec![ComponentValue::Token(Token::Ident("a".into()))]
            }]
        );
        assert_eq!(
            component_values_to_string(&parse_component_value_list("a [b (c] d) e")),
            "a [b (c] d) e]"
        );
        assert_eq!(
            component_values_to_string(&parse_component_value_list("a [b] (c) {d} e")),
            "a [b] (c) {d} e"
        );
    }

    #[test]
    fn declarations() {
        assert_eq!(
            parse_declaration("color: red"),
            Ok(decl("color", "red", false))
        );
        assert_eq!(
            parse_declaration("COLOR : red !important"),
            Ok(decl("color", "red", true))
        );
        assert_eq!(
            parse_declaration("color: red ! /**/ IMPORTANT "),
            Ok(decl("color", "red", true))
        );
        assert_eq!(
            parse_declaration("color: red !important!"),
            Ok(decl("color", "red !important!", false))
        );
        assert_eq!(
            parse_declaration("color: red important"),
            Ok(decl("color", "red important", false))
        );
        assert_eq!(
            parse_declaration("--Custom-Prop: { a }"),
            Ok(decl("--Custom-Prop", "{ a }", false))
        );
        assert_eq!(parse_declaration(""), Err(ParseError::Empty));
        assert_eq!(parse_declaration(" ;"), Err(ParseError::Invalid));
        assert_eq!(parse_declaration("@foo:"), Err(ParseError::Invalid));
        assert_eq!(parse_declaration("foo.. 9000"), Err(ParseError::Invalid));
        assert_eq!(
            parse_declaration("foo:;bar:;"),
            Ok(decl("foo", ";bar:;", false))
        );
    }

    #[test]
    fn declaration_list_recovery() {
        assert_eq!(
            parse_declaration_block("a:b; c:d 42!important;\n"),
            vec![decl("a", "b", false), decl("c", "d 42", true)]
        );
        assert_eq!(
            parse_declaration_block("z;a:b"),
            vec![decl("a", "b", false)]
        );
        assert_eq!(
            parse_declaration_block("z:x!;a:b"),
            vec![decl("z", "x!", false), decl("a", "b", false)]
        );
        assert_eq!(
            parse_declaration_block("a:b; c+:d"),
            vec![decl("a", "b", false)]
        );
        assert_eq!(
            parse_declaration_block("color: red; background: url(x;y); margin: 0"),
            vec![
                decl("color", "red", false),
                decl("background", "url(x;y)", false),
                decl("margin", "0", false)
            ]
        );
        assert_eq!(
            parse_declaration_block("color: rgb(1,2; margin: 0"),
            vec![decl("color", "rgb(1,2; margin: 0", false)]
        );
        assert_eq!(
            parse_declaration_block("@media screen { div{;}} a:b;; @media print{div{"),
            vec![decl("a", "b", false)]
        );
        assert_eq!(
            parse_declaration_block("@ media screen { div{;}} a:b;; @media print{div{"),
            vec![]
        );
        let items = parse_declaration_list("z;a:b");
        assert_eq!(items[0], Item::Invalid);
    }

    #[test]
    fn block_contents_mixes_declarations_and_rules() {
        let items = parse_block_contents("z:x;a b{c:d;;e:f}");
        assert_eq!(items.len(), 2);
        assert!(matches!(&items[1], Item::QualifiedRule(q) if q.prelude.len() == 3));
        let items = parse_block_contents("a:hover {c:1}");
        assert!(matches!(&items[0], Item::QualifiedRule(_)));
        let items = parse_block_contents("z:x;a b{c:d}e:f");
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[2], Item::Declaration(d) if d.name == "e"));
        let items = parse_block_contents("@ media screen { div{;}} a:b;; @media print{div{");
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[0], Item::QualifiedRule(_)));
        assert!(matches!(&items[2], Item::AtRule(a) if a.name == "media"));
    }

    #[test]
    fn rules_and_recovery() {
        assert_eq!(parse_rule(""), Err(ParseError::Empty));
        assert_eq!(parse_rule("foo"), Err(ParseError::Invalid));
        assert_eq!(
            parse_rule("div { color: #aaa; } p{}"),
            Err(ParseError::ExtraInput)
        );
        assert!(
            matches!(parse_rule("@foo bar;"), Ok(Item::AtRule(a)) if a.name == "foo" && a.block.is_none())
        );
        assert!(matches!(parse_rule("@foo { bar"), Ok(Item::AtRule(a)) if a.block.is_some()));
        assert!(
            matches!(parse_rule(" /**/ { color: #aaa  "), Ok(Item::QualifiedRule(q)) if q.prelude.is_empty())
        );
        let items = parse_rule_list("div {} -->");
        assert_eq!(items.len(), 2);
        assert_eq!(items[1], Item::Invalid);
        let items = parse_stylesheet_rules("<!-- --> div {} --> {}@a");
        assert_eq!(items.len(), 3);
        assert!(matches!(&items[2], Item::AtRule(a) if a.name == "a"));
    }

    fn sheet(src: &str) -> Stylesheet {
        parse_stylesheet(src, Origin::Author, Strictness::Lenient).unwrap()
    }
    fn style_rule(r: &Rule) -> (String, Vec<Declaration>, u32) {
        match r {
            Rule::Style {
                selectors,
                declarations,
                source_order,
            } => (selectors.to_string(), declarations.clone(), *source_order),
            other => panic!("not a style rule: {other:?}"),
        }
    }

    #[test]
    fn typed_stylesheet() {
        let s = sheet("@charset \"utf-8\"; @import url(a.css) screen; @import 'b.css'; @namespace svg url(http://www.w3.org/2000/svg);\n a, b > c { color: red; margin: 0 !important } @media (min-width: 600px) { .x { top: 0 } } @font-face { font-family: X; src: url(x.ttf) } @keyframes spin { from { a: 1 } 50%, to { a: 2 } } @supports (display: grid) { .g { display: grid } } @layer base, theme; @layer base { .b { a: 1 } @layer inner { .i { a: 2 } } } @page :first { margin: 1in } @foo bar { baz } .after {}");
        assert_eq!(s.origin, Origin::Author);
        assert!(
            matches!(&s.rules[0], Rule::Import { url, media } if url == "a.css" && media.to_string() == "screen")
        );
        assert!(
            matches!(&s.rules[1], Rule::Import { url, media } if url == "b.css" && media.is_empty())
        );
        assert!(
            matches!(&s.rules[2], Rule::Namespace { prefix: Some(p), url } if p == "svg" && url == "http://www.w3.org/2000/svg")
        );
        let (sel, decls, order) = style_rule(&s.rules[3]);
        assert_eq!(sel, "a, b > c");
        assert_eq!(
            decls,
            vec![decl("color", "red", false), decl("margin", "0", true)]
        );
        assert_eq!(order, 0);
        match &s.rules[4] {
            Rule::Media { query, rules } => {
                assert_eq!(query.to_string(), "(min-width: 600px)");
                assert_eq!(style_rule(&rules[0]).0, ".x");
            }
            o => panic!("{o:?}"),
        }
        assert!(matches!(&s.rules[5], Rule::FontFace(d) if d.len() == 2));
        match &s.rules[6] {
            Rule::Keyframes { name, frames } => {
                assert_eq!(name, "spin");
                assert_eq!(frames[0].0, vec![KeyframeSelector::From]);
                assert_eq!(
                    frames[1].0,
                    vec![
                        KeyframeSelector::Percentage(Number::from_i64(50)),
                        KeyframeSelector::To
                    ]
                );
                assert_eq!(frames[1].1, vec![decl("a", "2", false)]);
            }
            o => panic!("{o:?}"),
        }
        assert!(matches!(&s.rules[7], Rule::Supports { rules, .. } if rules.len() == 1));
        assert!(
            matches!(&s.rules[8], Rule::Layer { names, rules } if names == &["base", "theme"] && rules.is_empty())
        );
        assert!(
            matches!(&s.rules[9], Rule::Layer { names, rules } if names == &["base"] && rules.len() == 2)
        );
        assert!(
            matches!(&s.rules[10], Rule::Page { selectors, declarations } if selectors == &[":first"] && declarations.len() == 1)
        );
        assert!(matches!(&s.rules[11], Rule::Unknown { name, .. } if name == "foo"));
        assert_eq!(style_rule(&s.rules[12]).2, 5);
        assert_eq!(s.layer_order, vec!["base", "theme", "base.inner"]);
        assert_eq!(s.unsupported.len(), 1);
        assert_eq!(s.unsupported[0].kind, UnsupportedKind::AtRule);
        let eff = s.effective_style_rules(&Media::with_size(800, 600), &|_, _| true);
        let names: Vec<(String, Option<usize>)> = eff
            .iter()
            .map(|r| (r.selectors.to_string(), r.layer))
            .collect();
        assert_eq!(
            names,
            vec![
                ("a, b > c".into(), None),
                (".x".into(), None),
                (".g".into(), None),
                (".b".into(), Some(0)),
                (".i".into(), Some(2)),
                (".after".into(), None)
            ]
        );
        let eff = s.effective_style_rules(&Media::with_size(300, 600), &|_, _| false);
        assert_eq!(eff.len(), 4);
    }

    #[test]
    fn imports_only_at_the_start() {
        let s = sheet("a {} @import 'x.css';");
        assert_eq!(s.rules.len(), 1);
        let s = sheet("@layer a; @import 'x.css'; @layer b {} @import 'y.css';");
        assert_eq!(s.imports().count(), 1);
    }

    #[test]
    fn strict_mode_errors() {
        let e = parse_stylesheet("@foo {}", Origin::Author, Strictness::Strict).unwrap_err();
        assert_eq!(e.kind, UnsupportedKind::AtRule);
        assert_eq!(e.name, "@foo");
        let e = parse_stylesheet("a:host-context(x) {}", Origin::Author, Strictness::Strict)
            .unwrap_err();
        assert_eq!(e.kind, UnsupportedKind::Selector);
        let e = parse_stylesheet("p::first-line { x: 1 }", Origin::Author, Strictness::Strict)
            .unwrap_err();
        assert_eq!(e.name, "::first-line");
        assert!(parse_stylesheet(
            "a { color: red } @media print { b {} }",
            Origin::UserAgent,
            Strictness::Strict
        )
        .is_ok());
        let s = sheet("a:host-context(x) {} b || c {} p::first-line { x: 1 } d {}");
        assert_eq!(s.rules.len(), 2);
        assert_eq!(s.unsupported.len(), 3);
        assert_eq!(s.unsupported[0].name, "a:host-context(x)");
    }

    #[test]
    fn nesting_is_flattened() {
        let s = sheet(".a { color: red; &:hover { color: blue } .b { x: 1 } > .c, + .d { y: 2 } @media (hover) { z: 3; .e { w: 4 } } .f & { v: 5 } }");
        let flat: Vec<(String, Vec<Declaration>, u32)> =
            s.rules.iter().take(4).map(style_rule).collect();
        assert_eq!(flat[0], (".a".into(), vec![decl("color", "red", false)], 0));
        assert_eq!(
            flat[1],
            (
                ":is(.a):hover".into(),
                vec![decl("color", "blue", false)],
                1
            )
        );
        assert_eq!(
            flat[2],
            (":is(.a) .b".into(), vec![decl("x", "1", false)], 2)
        );
        assert_eq!(
            flat[3],
            (
                ":is(.a) > .c, :is(.a) + .d".into(),
                vec![decl("y", "2", false)],
                3
            )
        );
        match &s.rules[4] {
            Rule::Media { query, rules } => {
                assert_eq!(query.to_string(), "(hover)");
                assert_eq!(
                    style_rule(&rules[0]),
                    (".a".into(), vec![decl("z", "3", false)], 4)
                );
                assert_eq!(
                    style_rule(&rules[1]),
                    (":is(.a) .e".into(), vec![decl("w", "4", false)], 5)
                );
            }
            o => panic!("{o:?}"),
        }
        assert_eq!(style_rule(&s.rules[5]).0, ".f :is(.a)");
        // Declarations after a nested rule still apply to the parent (as a later rule).
        let s = sheet(".p { a: 1; .q { b: 2 } c: 3 }");
        assert_eq!(s.rules.len(), 3);
        assert_eq!(
            style_rule(&s.rules[2]),
            (".p".into(), vec![decl("c", "3", false)], 2)
        );
        // Top-level `&` is :scope; nested parents with lists.
        let s = sheet("& .x {} .a, .b { & + & {} }");
        assert_eq!(style_rule(&s.rules[0]).0, ":scope .x");
        assert_eq!(style_rule(&s.rules[2]).0, ":is(.a, .b) + :is(.a, .b)");
        // Specificity of the flattened selector follows :is().
        let sp = match &s.rules[2] {
            Rule::Style { selectors, .. } => selectors.0[0].specificity(),
            _ => unreachable!(),
        };
        assert_eq!((sp.a, sp.b, sp.c), (0, 2, 0));
    }

    #[test]
    fn media_nesting_and_layers_in_rules() {
        let s = sheet(
            "@media screen { @media (hover) { a { x: 1 } } @supports (a: b) { b { y: 2 } } }",
        );
        match &s.rules[0] {
            Rule::Media { rules, .. } => {
                assert!(matches!(&rules[0], Rule::Media { .. }));
                assert!(matches!(&rules[1], Rule::Supports { .. }));
            }
            o => panic!("{o:?}"),
        }
        let s = sheet("@layer { a {} } @layer x.y { b {} }");
        assert_eq!(s.layer_order, vec!["#anon1", "x", "x.y"]);
        assert!(matches!(&s.rules[1], Rule::Layer { names, .. } if names == &["x.y"]));
        let s = sheet("@supports (a: b) or { c {} } d {}");
        assert_eq!(s.rules.len(), 1);
        assert_eq!(s.unsupported[0].name, "@supports");
    }

    #[test]
    fn declaration_block_for_style_attribute() {
        let d = parse_declaration_block(
            "color: red; ; background : url( 'x.png' ) no-repeat !IMPORTANT; --My-Var: 1 ; bogus",
        );
        assert_eq!(d.len(), 3);
        assert_eq!(d[0], decl("color", "red", false));
        assert_eq!(d[1].name, "background");
        assert!(d[1].important);
        assert_eq!(d[2], decl("--My-Var", "1", false));
        assert_eq!(parse_declaration_block("{}"), vec![]);
        assert_eq!(parse_declaration_block("color: red {}"), vec![]);
    }
}
