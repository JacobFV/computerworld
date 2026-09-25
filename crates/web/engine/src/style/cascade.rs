//! The cascade: rule matching, cascade ordering (origin, importance, layers,
//! specificity, source order), inheritance, custom properties, `var()`
//! substitution, computed-value fixups, pseudo-elements, and incremental restyle.
//!
//! Order of precedence for normal declarations, weakest first: user-agent,
//! presentational hints, author (layers in declaration order, unlayered last),
//! inline `style`. For `!important`: author (layers reversed, unlayered first),
//! inline, user-agent. Within a level: specificity, then source order.

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use super::computed::*;
use super::hints;
use super::properties::*;
use super::shorthands;
use super::ua;
use super::values::*;
use crate::css::{
    self, AncestorKeys, ComponentValue, Declaration, MatchContext, Media, Origin, PseudoElement,
    Rule, SelectorDeps, SelectorIndex, Specificity, Stylesheet, Token,
};
use crate::dom::{Document, Mutation, NodeId, NodeKind, QuirksMode};
use crate::geom::Au;
use crate::{Strictness, Unsupported, UnsupportedKind, Viewport};
use cw_scene::Color;

/// An `@font-face` rule as collected for the fonts module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FontFace {
    pub family: String,
    /// `src` URLs and `local()` names in order.
    pub src: Vec<String>,
    /// `font-weight` range (a single weight is `(w, w)`).
    pub weight: (u16, u16),
    pub style: FontStyle,
    pub declarations: Vec<Declaration>,
}

/// The cascade level a declaration sits at; the first sort key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Level {
    Ua = 0,
    Hints = 1,
    Author = 2,
    Inline = 3,
    AuthorImportant = 4,
    InlineImportant = 5,
    UaImportant = 6,
}

impl Level {
    fn is_ua(self) -> bool {
        matches!(self, Level::Ua | Level::UaImportant)
    }
}

/// A declaration parsed once, shared by every element the rule matches.
#[derive(Clone, Debug)]
enum ParsedDecl {
    Longhands(Vec<(LonghandId, Specified)>),
    /// A custom property (`--x`) with its declared tokens, or a CSS-wide keyword.
    Custom(String, CustomDeclared),
    /// A logical property or shorthand, expanded once the element's direction is known.
    Logical(String, Vec<ComponentValue>),
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum CustomDeclared {
    Tokens(Vec<ComponentValue>),
    Wide(CssWide),
}

#[derive(Clone, Debug)]
struct ParsedBlock {
    decls: Vec<(ParsedDecl, bool)>,
}

#[derive(Clone, Debug)]
struct RuleData {
    origin: Origin,
    /// Global layer index; `None` for unlayered.
    layer: Option<usize>,
    spec: Specificity,
    order: u32,
    block: Rc<ParsedBlock>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct SortKey {
    level: Level,
    layer: u32,
    spec: Specificity,
    order: u32,
}

struct Candidate<'a> {
    key: SortKey,
    decl: &'a ParsedDecl,
}

/// The winning declared value of every longhand and custom property on one element.
#[derive(Default)]
struct Winners {
    longhands: Vec<Option<Specified>>,
    /// The UA-level winner per longhand, for `revert`.
    ua: Vec<Option<Specified>>,
    /// The highest level that set each longhand.
    level: Vec<Option<Level>>,
    custom: BTreeMap<String, CustomDeclared>,
}

impl Winners {
    fn new() -> Winners {
        Winners {
            longhands: vec![None; LonghandId::COUNT],
            ua: vec![None; LonghandId::COUNT],
            level: vec![None; LonghandId::COUNT],
            custom: BTreeMap::new(),
        }
    }
    fn set(&mut self, id: LonghandId, v: Specified, level: Level) {
        let i = id as usize;
        let v = match v {
            Specified::CssWide(CssWide::Revert) => {
                if level.is_ua() {
                    Specified::CssWide(CssWide::Unset)
                } else {
                    self.ua[i]
                        .clone()
                        .unwrap_or(Specified::CssWide(CssWide::Unset))
                }
            }
            // The winner so far is the strongest declaration below this one; close to
            // `revert-layer`, which rolls back to the previous layer's value.
            Specified::CssWide(CssWide::RevertLayer) => match self.longhands[i].clone() {
                Some(v) => v,
                None => Specified::CssWide(CssWide::Unset),
            },
            v => v,
        };
        if level.is_ua() {
            self.ua[i] = Some(v.clone());
        }
        self.longhands[i] = Some(v);
        self.level[i] = Some(level);
    }
}

/// Everything the cascade needs for one document and one set of sheets.
struct Engine<'a> {
    doc: &'a Document,
    ctx: &'a MatchContext<'a>,
    strictness: Strictness,
    quirks: bool,
    elements: SelectorIndex<RuleData>,
    before: SelectorIndex<RuleData>,
    after: SelectorIndex<RuleData>,
    marker: SelectorIndex<RuleData>,
    placeholder: SelectorIndex<RuleData>,
    deps: SelectorDeps,
    layer_count: usize,
    unsupported: Vec<Unsupported>,
    font_faces: Vec<FontFace>,
    /// Families an `@font-face` rule downloads (a `url()` source), for `font-family`.
    web_fonts: Vec<String>,
    keyframes: BTreeMap<String, Keyframes>,
    viewport: (Au, Au),
    fonts: crate::css::FontEnvironment,
    body_text_color: Color,
    /// Parsed `style=""` attributes, cached per element per pass.
    inline_cache: std::cell::RefCell<BTreeMap<NodeId, Rc<ParsedBlock>>>,
}

impl<'a> Engine<'a> {
    fn build(
        doc: &'a Document,
        sheets: &'a [Stylesheet],
        media: &Media,
        ctx: &'a MatchContext<'a>,
        strictness: Strictness,
    ) -> Result<Engine<'a>, Unsupported> {
        let _t = super::profile::span(super::profile::Phase::EngineBuild);
        let quirks = doc.quirks == QuirksMode::Quirks;
        let mut e = Engine {
            doc,
            ctx,
            strictness,
            quirks,
            elements: SelectorIndex::new(),
            before: SelectorIndex::new(),
            after: SelectorIndex::new(),
            marker: SelectorIndex::new(),
            placeholder: SelectorIndex::new(),
            deps: SelectorDeps::default(),
            layer_count: 0,
            unsupported: Vec::new(),
            font_faces: Vec::new(),
            web_fonts: Vec::new(),
            keyframes: BTreeMap::new(),
            viewport: (
                Au::from_px_i32(media.width_px),
                Au::from_px_i32(media.height_px),
            ),
            fonts: media.fonts,
            body_text_color: Color::BLACK,
            inline_cache: std::cell::RefCell::new(BTreeMap::new()),
        };
        if let Some(body) = doc.body() {
            if let Some(c) = doc.attr(body, "text").and_then(parse_legacy_color) {
                e.body_text_color = c;
            }
        }
        // Global layer order: first declaration wins the position, across sheets.
        let mut layers: Vec<String> = Vec::new();
        for s in sheets {
            for l in &s.layer_order {
                if !layers.contains(l) {
                    layers.push(l.clone());
                }
            }
        }
        e.layer_count = layers.len();
        let mut order: u32 = 0;
        let ua_sheets: Vec<&Stylesheet> = if quirks {
            vec![ua::sheet(), ua::quirks_sheet()]
        } else {
            vec![ua::sheet()]
        };
        let all: Vec<&Stylesheet> = ua_sheets.into_iter().chain(sheets.iter()).collect();
        for sheet in all {
            for u in &sheet.unsupported {
                e.record(u.clone());
            }
            e.collect_at_rules(&sheet.rules, media);
            let supported =
                |name: &str, value: &[ComponentValue]| is_supported_declaration(name, value);
            let sheet_strictness = if sheet.origin == Origin::UserAgent {
                Strictness::Lenient
            } else {
                strictness
            };
            for rule in sheet.effective_style_rules(media, &supported) {
                let layer = rule
                    .layer
                    .and_then(|i| sheet.layer_order.get(i))
                    .and_then(|name| layers.iter().position(|l| l == name));
                e.strictness = sheet_strictness;
                let block = Rc::new(e.parse_block(rule.declarations)?);
                e.strictness = strictness;
                order += 1;
                for sel in rule.selectors.iter() {
                    let data = RuleData {
                        origin: sheet.origin,
                        layer,
                        spec: sel.specificity(),
                        order,
                        block: block.clone(),
                    };
                    let index = match sel.pseudo_element {
                        None => &mut e.elements,
                        Some(PseudoElement::Before) => &mut e.before,
                        Some(PseudoElement::After) => &mut e.after,
                        Some(PseudoElement::Marker) => &mut e.marker,
                        Some(PseudoElement::Placeholder) => &mut e.placeholder,
                        Some(PseudoElement::Selection) => continue,
                        Some(p) => {
                            let u = Unsupported {
                                kind: UnsupportedKind::Selector,
                                name: format!("::{}", p.name()),
                                detail: sel.to_string(),
                            };
                            if strictness == Strictness::Strict {
                                return Err(u);
                            }
                            e.record(u);
                            continue;
                        }
                    };
                    let d = sel.dependencies();
                    e.deps.ids.extend(d.ids);
                    e.deps.classes.extend(d.classes);
                    e.deps.attributes.extend(d.attributes);
                    e.deps.tags.extend(d.tags);
                    e.deps.pseudo_classes.extend(d.pseudo_classes);
                    e.deps.structural |= d.structural;
                    e.deps.has |= d.has;
                    e.deps.state |= d.state;
                    e.deps.form |= d.form;
                    index.insert(sel.clone(), data);
                }
            }
        }
        Ok(e)
    }

    fn record(&mut self, u: Unsupported) {
        if !self.unsupported.contains(&u) {
            self.unsupported.push(u);
        }
    }

    fn collect_at_rules(&mut self, rules: &[Rule], media: &Media) {
        for r in rules {
            match r {
                Rule::FontFace(decls) => {
                    if let Some(f) = font_face(decls) {
                        if f.src.iter().any(|s| !s.starts_with("local:"))
                            && !self.web_fonts.contains(&f.family)
                        {
                            self.web_fonts.push(f.family.clone());
                        }
                        self.font_faces.push(f);
                    }
                }
                Rule::Keyframes { name, frames } => {
                    self.keyframes.insert(name.clone(), frames.clone());
                }
                Rule::Media { query, rules } => {
                    if query.evaluate(media) {
                        self.collect_at_rules(rules, media);
                    }
                }
                Rule::Supports { rules, .. } | Rule::Layer { rules, .. } => {
                    self.collect_at_rules(rules, media)
                }
                _ => {}
            }
        }
    }

    fn parse_block(&mut self, decls: &[Declaration]) -> Result<ParsedBlock, Unsupported> {
        let mut out = Vec::with_capacity(decls.len());
        for d in decls {
            let p = self.parse_declaration(d)?;
            out.push((p, d.important));
        }
        Ok(ParsedBlock { decls: out })
    }

    fn parse_declaration(&mut self, d: &Declaration) -> Result<ParsedDecl, Unsupported> {
        match parse_declaration(d) {
            Ok(p) => Ok(p),
            Err(u) => {
                if self.strictness == Strictness::Strict {
                    Err(u)
                } else {
                    self.record(u);
                    Ok(ParsedDecl::Invalid)
                }
            }
        }
    }

    fn inline_block(&self, node: NodeId) -> Result<Option<Rc<ParsedBlock>>, Unsupported> {
        let Some(src) = self.doc.attr(node, "style") else {
            return Ok(None);
        };
        if let Some(b) = self.inline_cache.borrow().get(&node) {
            return Ok(Some(b.clone()));
        }
        let decls = css::parse_declaration_block(src);
        let mut out = Vec::with_capacity(decls.len());
        for d in &decls {
            match parse_declaration(d) {
                Ok(p) => out.push((p, d.important)),
                Err(u) => {
                    if self.strictness == Strictness::Strict {
                        return Err(u);
                    }
                    // Lenient: dropped; recorded by the caller through `unsupported_inline`.
                    out.push((ParsedDecl::Invalid, d.important));
                }
            }
        }
        let b = Rc::new(ParsedBlock { decls: out });
        self.inline_cache.borrow_mut().insert(node, b.clone());
        Ok(Some(b))
    }

    /// Cascade candidates for an element (or, with `pseudo`, for one of its
    /// pseudo-elements, which take no hints and no inline style).
    fn winners(
        &self,
        node: NodeId,
        pseudo: Option<&SelectorIndex<RuleData>>,
        keys: &AncestorKeys,
        unsupported: &mut Vec<Unsupported>,
    ) -> Result<Winners, Unsupported> {
        let _t = super::profile::span(super::profile::Phase::Match);
        let index = pseudo.unwrap_or(&self.elements);
        let mut cands: Vec<Candidate> = Vec::new();
        for entry in index.matching_with(self.doc, node, self.ctx, keys) {
            let r = &entry.data;
            for (decl, important) in &r.block.decls {
                let level = match (r.origin, important) {
                    (Origin::UserAgent, false) => Level::Ua,
                    (Origin::UserAgent, true) => Level::UaImportant,
                    (Origin::Inline, false) => Level::Inline,
                    (Origin::Inline, true) => Level::InlineImportant,
                    (Origin::Author, false) => Level::Author,
                    (Origin::Author, true) => Level::AuthorImportant,
                };
                let layer = layer_key(r.layer, self.layer_count, *important);
                cands.push(Candidate {
                    key: SortKey {
                        level,
                        layer,
                        spec: r.spec,
                        order: r.order,
                    },
                    decl,
                });
            }
        }
        let hint_block;
        let inline_block;
        if pseudo.is_none() {
            let mut hint_decls = hints::presentational_hints(self.doc, node);
            // `<body link>` colours every `a[href]` at the hint level (the UA sheet's
            // `:link` colour is below it, author rules above it).
            if let Some(d) = hints::body_link_color(self.doc, node, "link") {
                hint_decls.push(d);
            }
            let mut parsed = Vec::with_capacity(hint_decls.len());
            for d in &hint_decls {
                match parse_declaration(d) {
                    Ok(p) => parsed.push((p, false)),
                    Err(u) => {
                        if self.strictness == Strictness::Strict {
                            return Err(u);
                        }
                        unsupported.push(u);
                    }
                }
            }
            hint_block = parsed;
            for (i, (decl, _)) in hint_block.iter().enumerate() {
                cands.push(Candidate {
                    key: SortKey {
                        level: Level::Hints,
                        layer: 0,
                        spec: Specificity::ZERO,
                        order: i as u32,
                    },
                    decl,
                });
            }
            inline_block = self.inline_block(node)?;
            if let Some(b) = &inline_block {
                for (i, (decl, important)) in b.decls.iter().enumerate() {
                    if matches!(decl, ParsedDecl::Invalid) {
                        if let Some(src) = self.doc.attr(node, "style") {
                            let d = css::parse_declaration_block(src).into_iter().nth(i);
                            if let Some(d) = d {
                                if let Err(u) = parse_declaration(&d) {
                                    unsupported.push(u);
                                }
                            }
                        }
                    }
                    let level = if *important {
                        Level::InlineImportant
                    } else {
                        Level::Inline
                    };
                    cands.push(Candidate {
                        key: SortKey {
                            level,
                            layer: 0,
                            spec: Specificity::ZERO,
                            order: i as u32,
                        },
                        decl,
                    });
                }
            }
        }
        cands.sort_by_key(|c| c.key);
        // Direction is needed to map logical properties: the strongest `direction`.
        let mut dir = Direction::Ltr;
        let mut parent_dir = None;
        if let Some(p) = self.doc.parent(node) {
            if self.doc.is_element(p) {
                parent_dir = Some(p);
            }
        }
        let _ = parent_dir;
        for c in &cands {
            if let ParsedDecl::Longhands(v) = c.decl {
                for (id, s) in v {
                    if *id == LonghandId::Direction {
                        if let Specified::Direction(d) = s {
                            dir = *d;
                        }
                    }
                }
            }
        }
        let mut w = Winners::new();
        for c in cands {
            match c.decl {
                ParsedDecl::Longhands(v) => {
                    for (id, s) in v {
                        w.set(*id, s.clone(), c.key.level);
                    }
                }
                ParsedDecl::Custom(name, v) => {
                    w.custom.insert(name.clone(), v.clone());
                }
                ParsedDecl::Logical(name, value) => {
                    if let Some(id) = shorthands::resolve_longhand(name, dir) {
                        if let Some(s) = parse_longhand(id, value) {
                            w.set(id, s, c.key.level);
                        }
                    } else if let Ok(v) = shorthands::expand(name, value, dir) {
                        for (id, s) in v {
                            w.set(id, s, c.key.level);
                        }
                    }
                }
                ParsedDecl::Invalid => {}
            }
        }
        Ok(w)
    }

    /// Computes one element's (or pseudo-element's) style from its winners.
    fn compute(
        &self,
        node: NodeId,
        w: &Winners,
        parent: &ComputedStyle,
        root_font_size: Option<Au>,
        is_pseudo: bool,
    ) -> ComputedStyle {
        let _t = super::profile::span(super::profile::Phase::Compute);
        let mut s = ComputedStyle::inherit_from(parent);
        // Custom properties first: everything else may reference them.
        s.custom = resolve_custom(&w.custom, &parent.custom);
        let is_root = root_font_size.is_none();
        let root_fs = root_font_size.unwrap_or(parent.font.size);
        // `ch` and `ex` come from the font's own metrics: the advance of `0` and the
        // x-height, not half an em.
        let lengths_for = |font: &Font, root_fs: Au| {
            let mut l = LengthContext::for_font_size(font.size, root_fs, self.viewport);
            l.ch = crate::layout::text::ch_unit(font);
            l.ex = crate::layout::text::font_metrics(font).x_height;
            l
        };
        let mut ctx = ComputeCtx {
            parent,
            lengths: lengths_for(&parent.font, root_fs),
            parent_lengths: lengths_for(&parent.font, root_fs),
            quirks: self.quirks,
            fonts: self.fonts,
            web_fonts: &self.web_fonts,
        };
        let apply_phase = |s: &mut ComputedStyle, ctx: &ComputeCtx, phase: u8| {
            for def in LONGHANDS.iter().filter(|d| d.phase == phase) {
                if let Some(v) = &w.longhands[def.id as usize] {
                    apply_value(s, def, v, ctx, parent);
                }
            }
        };
        apply_phase(&mut s, &ctx, 0);
        if !is_pseudo {
            if let Some(lang) = self
                .doc
                .attr(node, "lang")
                .or_else(|| self.doc.attr(node, "xml:lang"))
            {
                s.font.lang = cw_scene::Lang::from_tag(lang);
            }
        }
        let root_fs = if is_root { s.font.size } else { root_fs };
        ctx.lengths = lengths_for(&s.font, root_fs);
        apply_phase(&mut s, &ctx, 1);
        let lh = s.line_height_au(s.font.size.scale(12, 10));
        ctx.lengths.line_height = lh;
        if is_root {
            ctx.lengths.root_line_height = lh;
        }
        apply_phase(&mut s, &ctx, 2);
        self.fixups(node, &mut s, w, parent, is_root, is_pseudo);
        s
    }

    fn fixups(
        &self,
        node: NodeId,
        s: &mut ComputedStyle,
        w: &Winners,
        parent: &ComputedStyle,
        is_root: bool,
        is_pseudo: bool,
    ) {
        // The initial value of every colour but `color` is `currentcolor`: undeclared,
        // they compute to this element's own colour, not to black.
        let undeclared = |id: LonghandId| w.longhands[id as usize].is_none();
        for (id, side) in [
            (LonghandId::BorderTopColor, &mut s.border.top),
            (LonghandId::BorderRightColor, &mut s.border.right),
            (LonghandId::BorderBottomColor, &mut s.border.bottom),
            (LonghandId::BorderLeftColor, &mut s.border.left),
        ] {
            if undeclared(id) {
                side.color = s.color;
            }
        }
        if undeclared(LonghandId::OutlineColor) {
            s.outline.color = s.color;
        }
        // Computed border and outline widths are zero when the style draws nothing.
        for side in [
            &mut s.border.top,
            &mut s.border.right,
            &mut s.border.bottom,
            &mut s.border.left,
        ] {
            if !side.style.is_visible() {
                side.width = Au::ZERO;
            }
        }
        if !s.outline.style.is_visible() {
            s.outline.width = Au::ZERO;
        }
        // Absolutely positioned boxes do not float.
        if matches!(s.position, Position::Absolute | Position::Fixed) {
            s.float = Float::None;
        }
        // css-overflow-3 §3.1: `visible` on one axis computes to `auto` (and `clip`
        // to `hidden`) when the other axis is neither `visible` nor `clip`.
        let scrolls = |o: Overflow| !matches!(o, Overflow::Visible | Overflow::Clip);
        if scrolls(s.overflow_x) != scrolls(s.overflow_y) {
            for o in [&mut s.overflow_x, &mut s.overflow_y] {
                match *o {
                    Overflow::Visible => *o = Overflow::Auto,
                    Overflow::Clip => *o = Overflow::Hidden,
                    _ => {}
                }
            }
        }
        // A legacy `-webkit-box` that is vertical and line-clamped is not a flex
        // container in Blink: it lays out as a block that clamps its lines, and its
        // display computes to `flow-root` (`inline-block` for the inline form).
        // `-webkit-box` parses to `flex`, so a modern flex container carrying both
        // legacy properties is read the same way; line-clamp has no effect on one.
        if s.line_clamp.is_some() && s.box_orient_vertical {
            s.display = match s.display {
                Display::Flex => Display::FlowRoot,
                Display::InlineFlex => Display::InlineBlock,
                d => d,
            };
        }
        // Blockification: the root, floats, absolutes and flex/grid items.
        let parent_is_flex_or_grid = matches!(
            parent.display,
            Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
        ) && !is_pseudo
            || (is_pseudo
                && matches!(
                    parent.display,
                    Display::Flex | Display::InlineFlex | Display::Grid | Display::InlineGrid
                ));
        if is_root {
            if matches!(s.display, Display::Contents) {
                s.display = Display::Block;
            }
            s.display = s.display.blockify();
        } else if (s.is_out_of_flow() || parent_is_flex_or_grid)
            && !matches!(s.display, Display::Contents | Display::None)
        {
            s.inline_origin = s.display.is_inline_level();
            s.display = s.display.blockify();
        }
        // Blink's `AdjustStyleForDisplay`: a table does not inherit the legacy
        // `-webkit-center` alignment of a `<center>` or `align=center` ancestor; it
        // is centred as a block by the ancestor instead, and its cells start `start`.
        if matches!(s.display, Display::Table | Display::InlineTable)
            && s.text_align == TextAlign::WebkitCenter
        {
            s.text_align = TextAlign::Start;
        }
        // Quirks: tables take the document text colour unless the author says otherwise.
        if self.quirks && !is_pseudo && self.doc.is(node, "table") {
            let author_set = w.level[LonghandId::Color as usize].is_some_and(|l| !l.is_ua());
            if !author_set {
                s.color = self.body_text_color;
            }
        }
        // Text decoration propagation.
        let atomic_inline = matches!(
            s.display,
            Display::InlineBlock | Display::InlineTable | Display::InlineFlex | Display::InlineGrid
        );
        let mut eff = s.text_decoration;
        if !s.is_out_of_flow() && !atomic_inline {
            let p = parent.text_decoration_effective;
            eff.underline |= p.underline;
            eff.overline |= p.overline;
            eff.line_through |= p.line_through;
            if !s.text_decoration.any_line() && p.any_line() {
                eff.color = p.color;
                eff.style = p.style;
            }
        }
        // Colour resolves to a concrete value for paint.
        if eff.color.is_none() && eff.any_line() {
            eff.color = Some(if s.text_decoration.any_line() {
                s.color
            } else {
                parent.text_decoration_effective.color.unwrap_or(s.color)
            });
        }
        s.text_decoration_effective = eff;
    }

    /// Styles `root` and its subtree, reading the parent's style from `set`.
    fn style_subtree(
        &self,
        set: &mut StyleSet,
        root: NodeId,
        unsupported: &mut Vec<Unsupported>,
    ) -> Result<(), Unsupported> {
        let parent_style: Rc<ComputedStyle> = match self.styled_parent(set, root) {
            Some(p) => p,
            None => Rc::new(ComputedStyle::initial()),
        };
        let root_fs = if Some(root) == self.doc.document_element() {
            None
        } else {
            Some(set.root_font_size())
        };
        let keys = AncestorKeys::of(self.doc, root);
        self.style_node(set, root, parent_style, root_fs, &keys, unsupported)
    }

    fn styled_parent(&self, set: &StyleSet, node: NodeId) -> Option<Rc<ComputedStyle>> {
        let p = self.doc.parent(node)?;
        if !self.doc.is_element(p) {
            return None;
        }
        set.get_rc(p).cloned()
    }

    fn style_node(
        &self,
        set: &mut StyleSet,
        node: NodeId,
        parent: Rc<ComputedStyle>,
        root_font_size: Option<Au>,
        keys: &AncestorKeys,
        unsupported: &mut Vec<Unsupported>,
    ) -> Result<(), Unsupported> {
        match self.doc.kind(node) {
            NodeKind::Text(_) => {
                set.set(node, parent);
                return Ok(());
            }
            NodeKind::Element { .. } => {}
            _ => return Ok(()),
        }
        let w = self.winners(node, None, keys, unsupported)?;
        let style = self.compute(node, &w, &parent, root_font_size, false);
        let is_root = root_font_size.is_none();
        let root_fs = if is_root {
            style.font.size
        } else {
            root_font_size.unwrap()
        };
        if is_root {
            set.root_font_size_au = root_fs;
        }
        if self.quirks
            && self.doc.is(node, "table")
            && !w.level[LonghandId::Color as usize].is_some_and(|l| !l.is_ua())
        {
            set.quirk_table_color.insert(node);
        } else {
            set.quirk_table_color.remove(&node);
        }
        let style = Rc::new(style);
        set.set(node, style.clone());
        set.clear_pseudos(node);
        if !style.display.is_none() {
            for (index, kind) in [(&self.before, 0u8), (&self.after, 1u8)] {
                if index.is_empty() {
                    continue;
                }
                let pw = self.winners(node, Some(index), keys, unsupported)?;
                if pw.longhands[LonghandId::Content as usize].is_none() {
                    continue;
                }
                let ps = self.compute(node, &pw, &style, Some(root_fs), true);
                if matches!(ps.content, Content::Items(_)) && !ps.display.is_none() {
                    if kind == 0 {
                        set.set_before(node, Rc::new(ps));
                    } else {
                        set.set_after(node, Rc::new(ps));
                    }
                }
            }
            // `::placeholder` of a text control: the hint text the control paints
            // when it is empty, which the UA sheet gives a grey and an author rule
            // can recolour.
            if matches!(self.doc.tag(node), Some("input" | "textarea")) {
                let pw = self.winners(node, Some(&self.placeholder), keys, unsupported)?;
                let ps = self.compute(node, &pw, &style, Some(root_fs), true);
                set.set_placeholder(node, Rc::new(ps));
            }
            if matches!(style.display, Display::ListItem) {
                let mw = self.winners(node, Some(&self.marker), keys, unsupported)?;
                let mut ms = self.compute(node, &mw, &style, Some(root_fs), true);
                ms.display = Display::Inline;
                if mw.longhands[LonghandId::WhiteSpace as usize].is_none() {
                    ms.white_space = WhiteSpace::Pre;
                }
                if mw.longhands[LonghandId::TextTransform as usize].is_none() {
                    ms.text_transform = TextTransform::None;
                }
                set.set_marker(node, Rc::new(ms));
            }
        }
        let children: Vec<NodeId> = self.doc.children(node).collect();
        if children.is_empty() {
            return Ok(());
        }
        let child_keys = keys.under(self.doc, node);
        for c in children {
            self.style_node(
                set,
                c,
                style.clone(),
                Some(root_fs),
                &child_keys,
                unsupported,
            )?;
        }
        Ok(())
    }

    /// The subtree roots an incremental restyle must recompute, or `None` for the
    /// whole document.
    fn invalidation_roots(
        &self,
        set: &mut StyleSet,
        mutations: &[Mutation],
    ) -> Option<Vec<NodeId>> {
        let _t = super::profile::span(super::profile::Phase::Invalidation);
        let doc = self.doc;
        let mut roots: BTreeSet<NodeId> = BTreeSet::new();
        let mut whole = false;
        let structural = self.deps.structural;
        let parent_or_self = |n: NodeId| -> NodeId {
            match doc.parent(n) {
                Some(p) if doc.is_element(p) && structural => p,
                _ => n,
            }
        };
        for m in mutations {
            match m {
                Mutation::Inserted(n) => {
                    if !is_connected(doc, *n) {
                        continue;
                    }
                    if self.deps.has {
                        whole = true;
                    }
                    roots.insert(parent_or_self(*n));
                }
                Mutation::Removed { node, old_parent } => {
                    if !is_connected(doc, *node) {
                        for d in doc.descendants(*node) {
                            set.clear(d);
                        }
                    }
                    if self.deps.has {
                        whole = true;
                    }
                    if is_connected(doc, *old_parent)
                        && doc.is_element(*old_parent)
                        && (structural || set.get(*old_parent).is_none())
                    {
                        roots.insert(*old_parent);
                    } else if is_connected(doc, *old_parent) && !structural {
                        // Text-node siblings of the removed node keep the parent's style;
                        // nothing else changes without structural selectors.
                    }
                }
                Mutation::AttributeChanged { node, name, old } => {
                    if !is_connected(doc, *node) || !doc.is_element(*node) {
                        continue;
                    }
                    let affects =
                        match name.as_str() {
                            "style" => true,
                            "class" => {
                                let mut names: BTreeSet<&str> = doc.classes(*node).collect();
                                if let Some(o) = old {
                                    names.extend(o.split_ascii_whitespace());
                                }
                                names.iter().any(|c| self.deps.classes.contains(*c))
                                    || self.deps.attributes.contains("class")
                            }
                            "id" => {
                                let mut ids: Vec<&str> = Vec::new();
                                if let Some(i) = doc.attr(*node, "id") {
                                    ids.push(i);
                                }
                                if let Some(o) = old {
                                    ids.push(o);
                                }
                                ids.iter().any(|i| self.deps.ids.contains(*i))
                                    || self.deps.attributes.contains("id")
                            }
                            n => {
                                self.deps.attributes.contains(n)
                                    || hints::is_hint_attribute(doc, *node, n)
                                    || n == "lang"
                                    || n == "xml:lang"
                                    || (n == "href"
                                        && self.deps.pseudo_classes.iter().any(|p| {
                                            p == "link" || p == "any-link" || p == "visited"
                                        }))
                            }
                        };
                    if affects {
                        if self.deps.has {
                            whole = true;
                        }
                        roots.insert(parent_or_self(*node));
                    }
                }
                Mutation::TextChanged(n) => {
                    if !is_connected(doc, *n) {
                        continue;
                    }
                    if self.deps.has {
                        whole = true;
                    }
                    if self.deps.pseudo_classes.contains("empty") {
                        if let Some(p) = doc.parent(*n) {
                            if doc.is_element(p) {
                                roots.insert(parent_or_self(p));
                            }
                        }
                    }
                }
            }
        }
        if whole {
            return None;
        }
        // Drop roots inside other roots; restyle the nearest styled ancestor when a
        // root's parent has no style yet (it was inserted in the same batch).
        let mut out: Vec<NodeId> = Vec::new();
        for r in roots {
            let mut r = r;
            while let Some(p) = doc.parent(r) {
                if !doc.is_element(p) || set.get(p).is_some() {
                    break;
                }
                r = p;
            }
            if doc.ancestors(r).any(|a| out.contains(&a)) {
                continue;
            }
            out.retain(|o| !doc.ancestors(*o).any(|a| a == r));
            out.push(r);
        }
        Some(out)
    }
}

fn is_connected(doc: &Document, n: NodeId) -> bool {
    if doc.node(n).detached {
        return false;
    }
    doc.ancestors(n).last() == Some(Document::ROOT) || n == Document::ROOT
}

/// The layer sort key at a level: for normal declarations layers in order and
/// unlayered last; for important ones reversed.
fn layer_key(layer: Option<usize>, count: usize, important: bool) -> u32 {
    match (layer, important) {
        (None, false) => count as u32,
        (Some(i), false) => i as u32,
        (None, true) => 0,
        (Some(i), true) => (count - i) as u32,
    }
}

/// Parses one declaration into longhands (or a custom/logical marker).
fn parse_declaration(d: &Declaration) -> Result<ParsedDecl, Unsupported> {
    if d.name.starts_with("--") {
        let mut p = Parser::new(&d.value);
        if let Some(k) = parse_css_wide(&mut p) {
            return Ok(ParsedDecl::Custom(d.name.clone(), CustomDeclared::Wide(k)));
        }
        return Ok(ParsedDecl::Custom(
            d.name.clone(),
            CustomDeclared::Tokens(d.value.clone()),
        ));
    }
    let name = normalize_property_name(&d.name);
    let invalid = || Unsupported {
        kind: UnsupportedKind::Value,
        name: name.clone(),
        detail: serialize_component_values(&d.value),
    };
    if is_logical_name(&name) {
        // Validate against the LTR mapping so invalid values are reported now.
        if let Some(id) = shorthands::resolve_longhand(&name, Direction::Ltr) {
            if parse_longhand(id, &d.value).is_none() {
                return Err(invalid());
            }
            return Ok(ParsedDecl::Logical(name, d.value.clone()));
        }
        return match shorthands::expand(&name, &d.value, Direction::Ltr) {
            Ok(_) => Ok(ParsedDecl::Logical(name, d.value.clone())),
            Err(shorthands::ShorthandError::Invalid) => Err(invalid()),
            Err(_) => Err(Unsupported {
                kind: UnsupportedKind::Property,
                name,
                detail: "unknown property".into(),
            }),
        };
    }
    if let Some(id) = LonghandId::by_name(&name) {
        return match parse_longhand(id, &d.value) {
            Some(v) => Ok(ParsedDecl::Longhands(vec![(id, v)])),
            None => Err(invalid()),
        };
    }
    match shorthands::expand(&name, &d.value, Direction::Ltr) {
        Ok(v) => Ok(ParsedDecl::Longhands(v)),
        Err(shorthands::ShorthandError::Invalid) => Err(invalid()),
        Err(shorthands::ShorthandError::Unsupported) => Err(Unsupported {
            kind: UnsupportedKind::Property,
            name,
            detail: "not implemented".into(),
        }),
        Err(shorthands::ShorthandError::NotShorthand) => Err(Unsupported {
            kind: UnsupportedKind::Property,
            name,
            detail: "unknown property".into(),
        }),
    }
}

fn is_logical_name(name: &str) -> bool {
    name.contains("-inline")
        || name.contains("-block")
        || name == "inline-size"
        || name == "block-size"
}

/// For `@supports`: whether a declaration parses.
pub fn is_supported_declaration(name: &str, value: &[ComponentValue]) -> bool {
    let d = Declaration {
        name: name.to_ascii_lowercase(),
        value: value.to_vec(),
        important: false,
    };
    parse_declaration(&d).is_ok()
}

/// Whether a property name (longhand, shorthand or custom) is one the engine knows.
pub fn is_known_property(name: &str) -> bool {
    name.starts_with("--")
        || shorthands::resolve_longhand(name, Direction::Ltr).is_some()
        || shorthands::is_shorthand(name)
}

/// Applies one longhand's winning value, handling the CSS-wide keywords and pending
/// `var()` substitution. Invalid at computed-value time means `unset`.
fn apply_value(
    s: &mut ComputedStyle,
    def: &PropertyDef,
    v: &Specified,
    ctx: &ComputeCtx,
    parent: &ComputedStyle,
) {
    let initial = ComputedStyle::initial();
    let unset = |s: &mut ComputedStyle| {
        if def.inherited {
            (def.copy)(s, parent);
        } else {
            (def.copy)(s, &initial);
        }
    };
    match v {
        Specified::CssWide(CssWide::Initial) => (def.copy)(s, &initial),
        Specified::CssWide(CssWide::Inherit) => (def.copy)(s, parent),
        Specified::CssWide(_) => unset(s),
        Specified::Pending { property, value } => {
            let Some(tokens) = substitute_var(value, &s.custom, 0) else {
                unset(s);
                return;
            };
            let resolved = if let Some(id) = shorthands::resolve_longhand(property, s.direction) {
                if id == def.id {
                    parse_longhand(id, &tokens)
                } else {
                    None
                }
            } else {
                shorthands::expand(property, &tokens, s.direction)
                    .ok()
                    .and_then(|v| v.into_iter().find(|(id, _)| *id == def.id).map(|(_, v)| v))
            };
            match resolved {
                Some(Specified::Pending { .. }) | None => unset(s),
                Some(r) => apply_value(s, def, &r, ctx, parent),
            }
        }
        other => {
            if !(def.apply)(s, other, ctx) {
                unset(s);
            }
        }
    }
}

/// Resolves the declared custom properties against the inherited ones, substituting
/// `var()` references among them and invalidating cycles.
fn resolve_custom(
    declared: &BTreeMap<String, CustomDeclared>,
    inherited: &BTreeMap<String, Vec<ComponentValue>>,
) -> BTreeMap<String, Vec<ComponentValue>> {
    let mut out: BTreeMap<String, Vec<ComponentValue>> = inherited.clone();
    // Keywords first.
    let mut pending: BTreeMap<&str, &Vec<ComponentValue>> = BTreeMap::new();
    for (name, v) in declared {
        match v {
            CustomDeclared::Wide(CssWide::Initial) => {
                out.remove(name);
            }
            CustomDeclared::Wide(_) => {
                match inherited.get(name) {
                    Some(v) => out.insert(name.clone(), v.clone()),
                    None => out.remove(name),
                };
            }
            CustomDeclared::Tokens(t) => {
                pending.insert(name, t);
            }
        }
    }
    let mut resolved: BTreeMap<String, Option<Vec<ComponentValue>>> = BTreeMap::new();
    let names: Vec<&str> = pending.keys().copied().collect();
    for name in names {
        let mut visiting: Vec<String> = Vec::new();
        let _ = resolve_one(name, &pending, inherited, &mut resolved, &mut visiting);
    }
    for (name, v) in resolved {
        match v {
            Some(t) => {
                out.insert(name, t);
            }
            None => {
                out.remove(&name);
            }
        }
    }
    out
}

fn resolve_one(
    name: &str,
    pending: &BTreeMap<&str, &Vec<ComponentValue>>,
    inherited: &BTreeMap<String, Vec<ComponentValue>>,
    resolved: &mut BTreeMap<String, Option<Vec<ComponentValue>>>,
    visiting: &mut Vec<String>,
) -> Result<Option<Vec<ComponentValue>>, ()> {
    if let Some(r) = resolved.get(name) {
        return Ok(r.clone());
    }
    let Some(tokens) = pending.get(name) else {
        return Ok(inherited.get(name).cloned());
    };
    if visiting.iter().any(|v| v == name) {
        // A cycle: every property on the stack from the repeated name is invalid.
        let start = visiting.iter().position(|v| v == name).unwrap();
        for v in &visiting[start..] {
            resolved.insert(v.clone(), None);
        }
        return Err(());
    }
    visiting.push(name.to_owned());
    let r = substitute_custom(tokens, pending, inherited, resolved, visiting, 0);
    visiting.pop();
    match r {
        Ok(v) => {
            resolved.entry(name.to_owned()).or_insert(Some(v));
            Ok(resolved.get(name).cloned().unwrap())
        }
        Err(()) => {
            resolved.entry(name.to_owned()).or_insert(None);
            if resolved.get(name) == Some(&None) {
                Err(())
            } else {
                Ok(resolved.get(name).cloned().unwrap())
            }
        }
    }
}

/// `var()` substitution inside a custom property's value during resolution.
fn substitute_custom(
    tokens: &[ComponentValue],
    pending: &BTreeMap<&str, &Vec<ComponentValue>>,
    inherited: &BTreeMap<String, Vec<ComponentValue>>,
    resolved: &mut BTreeMap<String, Option<Vec<ComponentValue>>>,
    visiting: &mut Vec<String>,
    depth: u32,
) -> Result<Vec<ComponentValue>, ()> {
    if depth > 32 {
        return Err(());
    }
    let mut out = Vec::with_capacity(tokens.len());
    for t in tokens {
        match t {
            ComponentValue::Function { name, args } if name.eq_ignore_ascii_case("var") => {
                let (var_name, fallback) = split_var_args(args).ok_or(())?;
                let value = match resolve_one(&var_name, pending, inherited, resolved, visiting) {
                    Ok(v) => v,
                    Err(()) => {
                        // A cycle: this property is invalid too, fallback or not.
                        return Err(());
                    }
                };
                match value {
                    Some(v) => out.extend(v),
                    None => match fallback {
                        Some(f) => out.extend(substitute_custom(
                            &f,
                            pending,
                            inherited,
                            resolved,
                            visiting,
                            depth + 1,
                        )?),
                        None => return Err(()),
                    },
                }
            }
            ComponentValue::Function { name, args } => {
                out.push(ComponentValue::Function {
                    name: name.clone(),
                    args: substitute_custom(
                        args,
                        pending,
                        inherited,
                        resolved,
                        visiting,
                        depth + 1,
                    )?,
                });
            }
            ComponentValue::Block { open, contents } => {
                out.push(ComponentValue::Block {
                    open: open.clone(),
                    contents: substitute_custom(
                        contents,
                        pending,
                        inherited,
                        resolved,
                        visiting,
                        depth + 1,
                    )?,
                });
            }
            t => out.push(t.clone()),
        }
    }
    Ok(out)
}

/// `var(--name [, fallback])` arguments.
fn split_var_args(args: &[ComponentValue]) -> Option<(String, Option<Vec<ComponentValue>>)> {
    let mut i = 0;
    while i < args.len() && args[i].is_whitespace() {
        i += 1;
    }
    let name = match args.get(i)? {
        ComponentValue::Token(Token::Ident(s)) if s.starts_with("--") => s.clone(),
        _ => return None,
    };
    i += 1;
    while i < args.len() && args[i].is_whitespace() {
        i += 1;
    }
    if i >= args.len() {
        return Some((name, None));
    }
    if !matches!(args[i], ComponentValue::Token(Token::Comma)) {
        return None;
    }
    let fallback: Vec<ComponentValue> = args[i + 1..].to_vec();
    Some((name, Some(trim_ws(fallback))))
}

fn trim_ws(mut v: Vec<ComponentValue>) -> Vec<ComponentValue> {
    while v.first().is_some_and(|t| t.is_whitespace()) {
        v.remove(0);
    }
    while v.last().is_some_and(|t| t.is_whitespace()) {
        v.pop();
    }
    v
}

/// Substitutes `var()` in a regular property's value from resolved custom properties.
pub fn substitute_var(
    tokens: &[ComponentValue],
    custom: &BTreeMap<String, Vec<ComponentValue>>,
    depth: u32,
) -> Option<Vec<ComponentValue>> {
    if depth > 32 {
        return None;
    }
    let mut out = Vec::with_capacity(tokens.len());
    for t in tokens {
        match t {
            ComponentValue::Function { name, args } if name.eq_ignore_ascii_case("var") => {
                let (var_name, fallback) = split_var_args(args)?;
                match custom.get(&var_name) {
                    Some(v) => out.extend(v.iter().cloned()),
                    None => out.extend(substitute_var(&fallback?, custom, depth + 1)?),
                }
            }
            ComponentValue::Function { name, args } if name.eq_ignore_ascii_case("env") => {
                return None
            }
            ComponentValue::Function { name, args } => out.push(ComponentValue::Function {
                name: name.clone(),
                args: substitute_var(args, custom, depth + 1)?,
            }),
            ComponentValue::Block { open, contents } => out.push(ComponentValue::Block {
                open: open.clone(),
                contents: substitute_var(contents, custom, depth + 1)?,
            }),
            t => out.push(t.clone()),
        }
    }
    Some(trim_ws(out))
}

fn font_face(decls: &[Declaration]) -> Option<FontFace> {
    let mut family = None;
    let mut src = Vec::new();
    let mut weight = (400u16, 400u16);
    let mut style = FontStyle::Normal;
    for d in decls {
        let mut p = Parser::new(&d.value);
        match d.name.as_str() {
            "font-family" => family = super::properties::parse::family_name(&mut p),
            "src" => {
                let _ = p.comma_list(|p| {
                    if let Some(u) = p.expect_url() {
                        // Format and tech hints are skipped.
                        while let Some(v) = p.peek() {
                            if matches!(v, ComponentValue::Token(Token::Comma)) {
                                break;
                            }
                            p.next();
                        }
                        src.push(u);
                        return Some(());
                    }
                    if let Some(args) = p.expect_function_named("local") {
                        let mut a = Parser::new(args);
                        let name = a
                            .expect_string()
                            .map(str::to_owned)
                            .or_else(|| super::properties::parse::family_name(&mut a))?;
                        src.push(format!("local:{name}"));
                        return Some(());
                    }
                    None
                });
            }
            "font-weight" => {
                if let Some(super::properties::FontWeightSpec::Absolute(a)) =
                    super::properties::parse::font_weight_spec(&mut p)
                {
                    let b = match super::properties::parse::font_weight_spec(&mut p) {
                        Some(super::properties::FontWeightSpec::Absolute(b)) => b,
                        _ => a,
                    };
                    weight = (a, b);
                }
            }
            "font-style" => {
                if let Some(s) = super::properties::parse::font_style_keyword(&mut p) {
                    style = s;
                }
            }
            _ => {}
        }
    }
    Some(FontFace {
        family: family?,
        src,
        weight,
        style,
        declarations: decls.to_vec(),
    })
}

impl StyleSet {
    fn clear_pseudos(&mut self, id: NodeId) {
        self.before.remove(&id);
        self.after.remove(&id);
        self.marker.remove(&id);
        self.placeholder.remove(&id);
    }
}

/// Computes the style of every element in the document.
pub fn cascade(
    doc: &Document,
    sheets: &[Stylesheet],
    media: &Media,
    ctx: &MatchContext,
    strictness: Strictness,
) -> Result<StyleSet, Unsupported> {
    let engine = Engine::build(doc, sheets, media, ctx, strictness)?;
    let mut set = StyleSet::new();
    set.viewport = Viewport {
        width: media.width_px.max(0) as u32,
        height: media.height_px.max(0) as u32,
        scale: 1,
        zoom: 100,
    };
    set.font_faces = engine.font_faces.clone();
    set.keyframes = engine.keyframes.clone();
    for u in &engine.unsupported {
        set.record_unsupported(u.clone());
    }
    let mut unsupported = Vec::new();
    if let Some(root) = doc.document_element() {
        engine.style_subtree(&mut set, root, &mut unsupported)?;
    }
    for u in unsupported {
        set.record_unsupported(u);
    }
    Ok(set)
}

/// Recomputes only what `mutations` can have changed, using the selectors'
/// dependencies: class/id/attribute/`style` changes restyle the element's subtree
/// (its parent's when sibling combinators or structural pseudo-classes exist),
/// insertions and removals likewise, `:has()` anywhere restyles the document.
pub fn restyle(
    doc: &Document,
    set: &mut StyleSet,
    mutations: &[Mutation],
    sheets: &[Stylesheet],
    media: &Media,
    ctx: &MatchContext,
    strictness: Strictness,
) -> Result<(), Unsupported> {
    let engine = Engine::build(doc, sheets, media, ctx, strictness)?;
    set.font_faces = engine.font_faces.clone();
    set.keyframes = engine.keyframes.clone();
    for u in &engine.unsupported {
        set.record_unsupported(u.clone());
    }
    set.viewport = Viewport {
        width: media.width_px.max(0) as u32,
        height: media.height_px.max(0) as u32,
        scale: 1,
        zoom: 100,
    };
    let mut unsupported = Vec::new();
    match engine.invalidation_roots(set, mutations) {
        None => {
            if let Some(root) = doc.document_element() {
                engine.style_subtree(set, root, &mut unsupported)?;
            }
        }
        Some(roots) => {
            for r in roots {
                if is_connected(doc, r) {
                    engine.style_subtree(set, r, &mut unsupported)?;
                }
            }
        }
    }
    for u in unsupported {
        set.record_unsupported(u);
    }
    Ok(())
}

/// Restyles the subtrees of the given elements (for `MatchContext` state changes
/// such as hover and focus: pass the elements whose state flipped, plus their old
/// counterparts). With no state-dependent selectors this is a no-op.
pub fn restyle_state(
    doc: &Document,
    set: &mut StyleSet,
    changed: &[NodeId],
    sheets: &[Stylesheet],
    media: &Media,
    ctx: &MatchContext,
    strictness: Strictness,
) -> Result<(), Unsupported> {
    let engine = Engine::build(doc, sheets, media, ctx, strictness)?;
    if !engine.deps.state && !engine.deps.form {
        return Ok(());
    }
    let mut unsupported = Vec::new();
    if engine.deps.has {
        if let Some(root) = doc.document_element() {
            engine.style_subtree(set, root, &mut unsupported)?;
        }
    } else {
        let mut roots: Vec<NodeId> = Vec::new();
        for c in changed {
            if !is_connected(doc, *c) {
                continue;
            }
            let r = if engine.deps.structural {
                doc.parent(*c).filter(|p| doc.is_element(*p)).unwrap_or(*c)
            } else {
                *c
            };
            if doc.ancestors(r).any(|a| roots.contains(&a)) || roots.contains(&r) {
                continue;
            }
            roots.retain(|o| !doc.ancestors(*o).any(|a| a == r));
            roots.push(r);
        }
        for r in roots {
            engine.style_subtree(set, r, &mut unsupported)?;
        }
    }
    for u in unsupported {
        set.record_unsupported(u);
    }
    Ok(())
}

/// The style a detached or unstyled element would get as a child of `parent`, from
/// a declaration block alone (used for `getComputedStyle`-like queries by script
/// before layout, and by tests).
pub fn compute_from_declarations(decls: &[Declaration], parent: &ComputedStyle) -> ComputedStyle {
    let mut w = Winners::new();
    for d in decls {
        if let Ok(p) = parse_declaration(d) {
            match p {
                ParsedDecl::Longhands(v) => {
                    for (id, s) in v {
                        w.set(id, s, Level::Author);
                    }
                }
                ParsedDecl::Custom(n, v) => {
                    w.custom.insert(n, v);
                }
                ParsedDecl::Logical(name, value) => {
                    if let Some(id) = shorthands::resolve_longhand(&name, Direction::Ltr) {
                        if let Some(s) = parse_longhand(id, &value) {
                            w.set(id, s, Level::Author);
                        }
                    } else if let Ok(v) = shorthands::expand(&name, &value, Direction::Ltr) {
                        for (id, s) in v {
                            w.set(id, s, Level::Author);
                        }
                    }
                }
                ParsedDecl::Invalid => {}
            }
        }
    }
    let doc = Document::new();
    let ctx = MatchContext::new();
    let engine = Engine {
        doc: &doc,
        ctx: &ctx,
        strictness: Strictness::Lenient,
        quirks: false,
        elements: SelectorIndex::new(),
        before: SelectorIndex::new(),
        after: SelectorIndex::new(),
        marker: SelectorIndex::new(),
        placeholder: SelectorIndex::new(),
        deps: SelectorDeps::default(),
        layer_count: 0,
        unsupported: Vec::new(),
        font_faces: Vec::new(),
        web_fonts: Vec::new(),
        keyframes: BTreeMap::new(),
        viewport: (Au::from_px_i32(1280), Au::from_px_i32(800)),
        fonts: crate::css::FontEnvironment::Bundled,
        body_text_color: Color::BLACK,
        inline_cache: std::cell::RefCell::new(BTreeMap::new()),
    };
    engine.compute(Document::ROOT, &w, parent, Some(parent.font.size), true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dom::Attribute;

    fn sheet(css: &str) -> Stylesheet {
        css::parse_stylesheet(css, Origin::Author, Strictness::Lenient).unwrap()
    }

    fn styled(html: &str, css: &str) -> (Document, StyleSet) {
        let doc = crate::html::parse(html);
        let set = cascade(
            &doc,
            &[sheet(css)],
            &Media::default(),
            &MatchContext::new(),
            Strictness::Lenient,
        )
        .unwrap();
        (doc, set)
    }

    fn by_id(doc: &Document, id: &str) -> NodeId {
        doc.by_id(id)[0]
    }

    fn ser(doc: &Document, set: &StyleSet, id: &str, prop: &str) -> String {
        set.get(by_id(doc, id)).unwrap().serialize(prop).unwrap()
    }

    #[test]
    fn specificity_ties_and_source_order() {
        let (d, s) = styled(
            r#"<div id="a" class="x y"><p id="p" class="x">t</p></div>"#,
            ".x { color: red } .y { color: blue } #a { color: green } .x.y { color: purple } div p.x { color: rgb(1,2,3) } p { color: yellow }",
        );
        assert_eq!(ser(&d, &s, "a", "color"), "rgb(0, 128, 0)");
        assert_eq!(ser(&d, &s, "p", "color"), "rgb(1, 2, 3)");
        // Same specificity: the later wins.
        let (d, s) = styled(
            r#"<p id="p" class="a b">t</p>"#,
            ".b { color: blue } .a { color: red }",
        );
        assert_eq!(ser(&d, &s, "p", "color"), "rgb(255, 0, 0)");
    }

    #[test]
    fn font_kerning_and_the_kern_feature_cascade() {
        let (d, s) = styled(
            r#"<div id="a"><p id="p">t</p></div><p id="b">t</p><p id="c">t</p><p id="e">t</p>"#,
            "#a { font-kerning: none; font-feature-settings: \"kern\" off } \
             #b { font-feature-settings: \"liga\" 0, \"kern\" } \
             #c { font-feature-settings: \"kern\" 0, \"liga\" } \
             #e { font-kerning: none; font-feature-settings: \"kerning\" }",
        );
        assert_eq!(ser(&d, &s, "p", "font-kerning"), "none");
        assert_eq!(ser(&d, &s, "p", "font-feature-settings"), "\"kern\" 0");
        let font = |id: &str| s.get(by_id(&d, id)).unwrap().font.clone();
        assert!(!font("p").kerns(), "inherited from the div");
        assert!(font("b").kerns() && font("b").kern_feature == 1);
        assert!(!font("c").kerns());
        // An invalid tag drops the declaration; `font-kerning: none` still applies.
        assert_eq!(font("e").kern_feature, -1);
        assert!(!font("e").kerns());
    }

    #[test]
    fn glyphs_are_scaled_at_the_size_truncated_to_a_64th() {
        let (d, s) = styled(
            r#"<p id="a">t</p><div id="b"><p id="c">t</p></div><p id="e">t</p>"#,
            "#a { font-size: 8pt } #b { font-size: 22.4px } #c { font-size: 50% } #e { font-size: 20px }",
        );
        let font = |id: &str| s.get(by_id(&d, id)).unwrap().font.clone();
        // 8pt is 10.6667 px: laid out at 10.671875 (rounded), set at 10.65625, as
        // Chromium's FreeType does.
        assert_eq!(font("a").size, Au(683));
        assert_eq!(font("a").glyph_size(), Au(682));
        assert_eq!(font("b").size, Au(1434));
        assert_eq!(font("b").glyph_size(), Au(1433));
        // Relative to the parent's unrounded size: 11.2 px exactly.
        assert_eq!(font("c").glyph_size(), Au(716));
        assert_eq!(font("e").glyph_size(), Au(1280));
        // A size set directly, which the unrounded one no longer describes.
        let mut f = font("a");
        f.size = Au(1024);
        assert_eq!(f.glyph_size(), Au(1024));
    }

    #[test]
    fn important_and_inline() {
        let (d, s) = styled(
            r#"<p id="p" style="color: blue">t</p>"#,
            "#p { color: red !important } p { color: green }",
        );
        assert_eq!(ser(&d, &s, "p", "color"), "rgb(255, 0, 0)");
        let (d, s) = styled(
            r#"<p id="p" style="color: blue !important">t</p>"#,
            "#p { color: red !important }",
        );
        assert_eq!(ser(&d, &s, "p", "color"), "rgb(0, 0, 255)");
        let (d, s) = styled(
            r#"<p id="p" style="color: blue">t</p>"#,
            "#p { color: red }",
        );
        assert_eq!(ser(&d, &s, "p", "color"), "rgb(0, 0, 255)");
    }

    #[test]
    fn layers_order_and_importance_reversal() {
        let css = "@layer base, theme; @layer theme { p { color: red } } @layer base { p { color: blue } } ";
        let (d, s) = styled(r#"<p id="p">t</p>"#, css);
        assert_eq!(ser(&d, &s, "p", "color"), "rgb(255, 0, 0)");
        let css = "@layer base { p { color: blue !important } } @layer theme { p { color: red !important } } p { color: green !important }";
        let (d, s) = styled(r#"<p id="p">t</p>"#, css);
        assert_eq!(ser(&d, &s, "p", "color"), "rgb(0, 0, 255)");
        let css = "@layer base { p { color: blue } } p { color: green }";
        let (d, s) = styled(r#"<p id="p">t</p>"#, css);
        assert_eq!(ser(&d, &s, "p", "color"), "rgb(0, 128, 0)");
    }

    #[test]
    fn inheritance_chain_and_relative_units() {
        let html = r#"<div id="a" style="font-size: 20px; color: red"><p id="b" style="font-size: 1.5em; padding: 1em; margin: 1rem"><span id="c">t</span></p></div>"#;
        let (d, s) = styled(html, "");
        assert_eq!(ser(&d, &s, "b", "font-size"), "30px");
        assert_eq!(ser(&d, &s, "b", "padding-top"), "30px");
        assert_eq!(ser(&d, &s, "b", "margin-top"), "16px");
        assert_eq!(ser(&d, &s, "c", "font-size"), "30px");
        assert_eq!(ser(&d, &s, "c", "color"), "rgb(255, 0, 0)");
        // Text nodes take the parent's style.
        let text = d.first_child(by_id(&d, "c")).unwrap();
        assert_eq!(s.get(text).unwrap().color, Color(255, 0, 0, 255));
        // Non-inherited properties reset.
        assert_eq!(ser(&d, &s, "c", "padding-top"), "0px");
        // `inherit` on a non-inherited property, `initial` on an inherited one.
        let (d, s) = styled(
            r#"<div id="a" style="padding: 5px; color: red"><p id="b" style="padding: inherit; color: initial">t</p></div>"#,
            "",
        );
        assert_eq!(ser(&d, &s, "b", "padding-left"), "5px");
        assert_eq!(ser(&d, &s, "b", "color"), "rgb(0, 0, 0)");
        // rem against the root font size.
        let (d, s) = styled(
            r#"<div id="a" style="width: 2rem">t</div>"#,
            "html { font-size: 10px }",
        );
        assert_eq!(ser(&d, &s, "a", "width"), "20px");
        assert_eq!(s.root_font_size(), Au::from_px_i32(10));
    }

    #[test]
    fn font_keywords_weights_and_line_height() {
        let (d, s) = styled(
            r#"<div id="a" style="font-weight: bold; font-size: large"><b id="b" style="font-weight: bolder">x</b><i id="c" style="font-weight: lighter; font-size: smaller; line-height: 150%">y</i></div><pre id="p">z</pre><p id="q" style="line-height: 1.5; font-size: 10px">w</p>"#,
            "",
        );
        assert_eq!(ser(&d, &s, "a", "font-size"), "18px");
        assert_eq!(ser(&d, &s, "b", "font-weight"), "900");
        assert_eq!(ser(&d, &s, "c", "font-weight"), "400");
        assert_eq!(ser(&d, &s, "c", "font-size"), "15px");
        assert_eq!(ser(&d, &s, "c", "line-height"), "22.5px");
        // The monospace quirk: medium is 13px for `pre`.
        assert_eq!(ser(&d, &s, "p", "font-size"), "13px");
        assert_eq!(ser(&d, &s, "q", "line-height"), "15px");
        assert_eq!(
            s.get(by_id(&d, "q")).unwrap().line_height,
            LineHeight::Number(1500)
        );
    }

    #[test]
    fn custom_properties_fallback_and_cycles() {
        let html = r#"<div id="a" style="--c: red; --w: 10px; --x: var(--y); --y: var(--x); --z: var(--missing, 4px)"><p id="b" style="color: var(--c); width: var(--w); height: var(--x, 7px); padding-top: var(--z); margin-top: var(--nope); border-top-width: var(--w) solid">t</p></div>"#;
        let (d, s) = styled(html, "");
        assert_eq!(ser(&d, &s, "b", "color"), "rgb(255, 0, 0)");
        assert_eq!(ser(&d, &s, "b", "width"), "10px");
        // The cycle makes --x invalid, so the fallback applies.
        assert_eq!(ser(&d, &s, "b", "height"), "7px");
        assert_eq!(ser(&d, &s, "b", "padding-top"), "4px");
        // Missing without fallback: invalid at computed-value time, so unset (initial).
        assert_eq!(ser(&d, &s, "b", "margin-top"), "0px");
        // Substituted but invalid for the property: unset.
        assert_eq!(ser(&d, &s, "b", "border-top-width"), "0px");
        assert_eq!(ser(&d, &s, "b", "--c"), "red");
        assert_eq!(ser(&d, &s, "b", "--x"), "");
        // Custom properties through a shorthand and calc.
        let (d, s) = styled(
            r#"<p id="p" style="--m: 2px; margin: var(--m) calc(var(--m) * 3)">t</p>"#,
            "",
        );
        assert_eq!(ser(&d, &s, "p", "margin-right"), "6px");
        assert_eq!(ser(&d, &s, "p", "margin-top"), "2px");
    }

    #[test]
    fn calc_with_mixed_units_and_percentages() {
        let (d, s) = styled(
            r#"<div id="a" style="font-size: 10px; width: calc(100% - 2em); height: calc(1em + 1rem); padding-left: calc(2 * 3px)">t</div>"#,
            "",
        );
        assert_eq!(ser(&d, &s, "a", "width"), "calc(100% - 20px)");
        assert_eq!(ser(&d, &s, "a", "height"), "26px");
        assert_eq!(ser(&d, &s, "a", "padding-left"), "6px");
    }

    #[test]
    fn quirks_font_size_and_table_color() {
        let html = r#"<body text="blue"><font id="f" size="+1">x</font><table id="t"><tr><td id="c">y</td></tr></table><div style="color: red"><table id="u"><tr><td>z</td></tr></table></div></body>"#;
        let doc = crate::html::parse(html);
        assert_eq!(doc.quirks, QuirksMode::Quirks);
        let set = cascade(
            &doc,
            &[],
            &Media::default(),
            &MatchContext::new(),
            Strictness::Lenient,
        )
        .unwrap();
        assert_eq!(ser(&doc, &set, "f", "font-size"), "18px");
        assert_eq!(ser(&doc, &set, "t", "color"), "rgb(0, 0, 255)");
        assert_eq!(ser(&doc, &set, "c", "color"), "rgb(0, 0, 255)");
        // The quirk resets the font-size on tables to the initial.
        assert_eq!(ser(&doc, &set, "t", "font-size"), "16px");
        let u = by_id(&doc, "u");
        assert_eq!(set.get(u).unwrap().color, Color(0, 0, 255, 255));
        // Standards mode: the table inherits red.
        let doc = crate::html::parse(&format!("<!DOCTYPE html>{html}"));
        assert_eq!(doc.quirks, QuirksMode::NoQuirks);
        let set = cascade(
            &doc,
            &[],
            &Media::default(),
            &MatchContext::new(),
            Strictness::Lenient,
        )
        .unwrap();
        assert_eq!(
            set.get(by_id(&doc, "u")).unwrap().color,
            Color(255, 0, 0, 255)
        );
    }

    #[test]
    fn presentational_hints_lose_to_author_css() {
        let (d, s) = styled(
            r#"<p id="a" align="center">x</p><p id="b" align="center">y</p><table id="t" width="300" bgcolor="red"></table>"#,
            "#b { text-align: right } table { width: 100px }",
        );
        assert_eq!(ser(&d, &s, "a", "text-align"), "-webkit-center");
        assert_eq!(ser(&d, &s, "b", "text-align"), "right");
        assert_eq!(ser(&d, &s, "t", "width"), "100px");
        assert_eq!(ser(&d, &s, "t", "background-color"), "rgb(255, 0, 0)");
        // Hints beat the UA sheet.
        let (d, s) = styled(
            r#"<table><tr><td id="c" align="right">x</td></tr></table>"#,
            "",
        );
        assert_eq!(ser(&d, &s, "c", "text-align"), "right");
    }

    #[test]
    fn ua_sheet_basics() {
        let (d, s) = styled(
            r##"<div id="d"><h1 id="h">t</h1><p id="p">x</p><a id="a" href="#">l</a><b id="b">s</b><table id="t"><tr id="r"><td id="c">1</td></tr></table><ul><li id="li">i</li></ul><span id="hid" hidden>h</span></div>"##,
            "",
        );
        assert_eq!(ser(&d, &s, "d", "display"), "block");
        assert_eq!(ser(&d, &s, "h", "font-size"), "32px");
        assert_eq!(ser(&d, &s, "h", "font-weight"), "700");
        // 0.67em of 32px is 21.44px; Au quantises it to 1/64 px.
        assert_eq!(ser(&d, &s, "h", "margin-top"), "21.4375px");
        assert_eq!(ser(&d, &s, "p", "margin-top"), "16px");
        assert_eq!(ser(&d, &s, "a", "color"), "rgb(0, 0, 238)");
        assert_eq!(ser(&d, &s, "a", "text-decoration-line"), "underline");
        assert_eq!(ser(&d, &s, "b", "font-weight"), "700");
        assert_eq!(ser(&d, &s, "t", "display"), "table");
        assert_eq!(ser(&d, &s, "t", "border-spacing"), "2px 2px");
        assert_eq!(ser(&d, &s, "r", "display"), "table-row");
        assert_eq!(ser(&d, &s, "c", "display"), "table-cell");
        assert_eq!(ser(&d, &s, "c", "padding-top"), "1px");
        assert_eq!(ser(&d, &s, "li", "display"), "list-item");
        assert!(s.marker(by_id(&d, "li")).is_some());
        assert_eq!(ser(&d, &s, "hid", "display"), "none");
        let body = d.body().unwrap();
        assert_eq!(
            s.get(body).unwrap().serialize("margin-left").unwrap(),
            "8px"
        );
    }

    #[test]
    fn pseudo_elements_only_with_content() {
        let (d, s) = styled(r#"<p id="a">x</p><p id="b">y</p><p id="c">z</p>"#, "#a::before { content: \"[\"; color: red } #b::before { color: red } #c::after { content: none } p::after { content: counter(n) }");
        let a = by_id(&d, "a");
        let before = s.before(a).unwrap();
        assert_eq!(
            before.content,
            Content::Items(vec![ContentItem::Text("[".into())])
        );
        assert_eq!(before.color, Color(255, 0, 0, 255));
        assert!(s.before(by_id(&d, "b")).is_none());
        assert!(s.after(by_id(&d, "c")).is_none());
        assert!(s.after(a).is_some());
        // Pseudo-elements inherit from the originating element.
        let (d, s) = styled(
            r#"<p id="a" style="color: blue; font-size: 20px">x</p>"#,
            "p::before { content: \"a\"; font-size: 2em }",
        );
        let b = s.before(by_id(&d, "a")).unwrap();
        assert_eq!(b.color, Color(0, 0, 255, 255));
        assert_eq!(b.font.size, Au::from_px_i32(40));
    }

    #[test]
    fn ch_and_ex_come_from_the_font() {
        // Arimo's `0` is 1139/2048 em and its x-height 1082/2048 em, not half an em;
        // a monospace face's `0` is its cell.
        let css = "#a { font: 100px Arial; width: 10ch; height: 10ex } #m { font: 100px 'Courier New'; width: 10ch } #s { font: 100px Arial; font-size: 2ch }";
        let (doc, set) = styled(
            r#"<div id="a"></div><div id="m"></div><div style="font: 50px Arial"><div id="s"></div></div>"#,
            css,
        );
        let px = |id: &str, prop: &str| {
            ser(&doc, &set, id, prop)
                .trim_end_matches("px")
                .parse::<f64>()
                .unwrap()
        };
        assert!(
            (px("a", "width") - 556.0).abs() < 1.0,
            "10ch in Arimo at 100px: {}",
            px("a", "width")
        );
        assert!(
            (px("a", "height") - 528.0).abs() < 2.0,
            "10ex in Arimo at 100px: {}",
            px("a", "height")
        );
        assert!(
            (px("m", "width") - 600.0).abs() < 1.0,
            "10ch in Cousine at 100px: {}",
            px("m", "width")
        );
        // `font-size: 2ch` measures the parent's font (50px Arimo).
        assert!(
            (px("s", "font-size") - 55.6).abs() < 0.5,
            "2ch of the parent: {}",
            px("s", "font-size")
        );
    }

    #[test]
    fn the_device_decides_which_families_are_installed() {
        let doc = crate::html::parse(r#"<p id="p" style="font-family: Inter, sans-serif">x</p>"#);
        let world = cascade(
            &doc,
            &[],
            &Media::default(),
            &MatchContext::new(),
            Strictness::Lenient,
        )
        .unwrap();
        assert_eq!(
            world.get(by_id(&doc, "p")).unwrap().font.typeface,
            cw_scene::Typeface::Inter
        );
        let linux = Media {
            fonts: crate::css::FontEnvironment::LinuxBaseline,
            ..Media::default()
        };
        let linux = cascade(&doc, &[], &linux, &MatchContext::new(), Strictness::Lenient).unwrap();
        assert_eq!(
            linux.get(by_id(&doc, "p")).unwrap().font.typeface,
            cw_scene::Typeface::Arimo
        );
    }

    #[test]
    fn aspect_ratio_and_line_clamp_compute() {
        let css = "#a { aspect-ratio: 16 / 9 } #b { aspect-ratio: 1 } #c { aspect-ratio: auto 1.5 } #d { aspect-ratio: 0 / 1 } #e { aspect-ratio: -1 } \
                   #t { display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2 } #u { display: -webkit-box } #v { display: flex; -webkit-line-clamp: 3 }";
        let (doc, set) = styled(
            r#"<p id="a"></p><p id="b"></p><p id="c"></p><p id="d"></p><p id="e"></p><p id="t"></p><p id="u"></p><p id="v"></p>"#,
            css,
        );
        assert_eq!(ser(&doc, &set, "a", "aspect-ratio"), "16 / 9");
        assert_eq!(ser(&doc, &set, "b", "aspect-ratio"), "1 / 1");
        assert_eq!(ser(&doc, &set, "c", "aspect-ratio"), "auto 1.5 / 1");
        assert_eq!(ser(&doc, &set, "d", "aspect-ratio"), "auto");
        assert_eq!(ser(&doc, &set, "e", "aspect-ratio"), "auto");
        // Blink computes a vertical, line-clamped legacy box to `flow-root`.
        assert_eq!(ser(&doc, &set, "t", "display"), "flow-root");
        assert_eq!(ser(&doc, &set, "t", "-webkit-line-clamp"), "2");
        assert_eq!(ser(&doc, &set, "u", "display"), "flex");
        assert_eq!(ser(&doc, &set, "v", "display"), "flex");
    }

    #[test]
    fn media_query_gating() {
        let css = "@media (max-width: 600px) { p { color: red } } @media (min-width: 601px) { p { color: blue } }";
        let doc = crate::html::parse(r#"<p id="p">x</p>"#);
        let narrow = cascade(
            &doc,
            &[sheet(css)],
            &Media::with_size(400, 800),
            &MatchContext::new(),
            Strictness::Lenient,
        )
        .unwrap();
        assert_eq!(ser(&doc, &narrow, "p", "color"), "rgb(255, 0, 0)");
        let wide = cascade(
            &doc,
            &[sheet(css)],
            &Media::with_size(1000, 800),
            &MatchContext::new(),
            Strictness::Lenient,
        )
        .unwrap();
        assert_eq!(ser(&doc, &wide, "p", "color"), "rgb(0, 0, 255)");
        assert_eq!(wide.viewport().width, 1000);
    }

    #[test]
    fn display_fixups_and_text_decoration_propagation() {
        let html = r#"<span id="f" style="float: left">a</span><span id="p" style="position: absolute; display: inline-block">b</span><div id="flex" style="display: flex"><span id="item">c</span><span id="contents" style="display: contents">d</span></div><u id="u"><span id="in">e</span><span id="ib" style="display: inline-block">f</span><span id="abs" style="position: absolute">g</span></u>"#;
        let (d, s) = styled(html, "");
        assert_eq!(ser(&d, &s, "f", "display"), "block");
        assert_eq!(ser(&d, &s, "p", "display"), "block");
        assert_eq!(ser(&d, &s, "item", "display"), "block");
        assert_eq!(ser(&d, &s, "contents", "display"), "contents");
        let root = d.document_element().unwrap();
        assert_eq!(s.get(root).unwrap().display, Display::Block);
        assert!(
            s.get(by_id(&d, "in"))
                .unwrap()
                .text_decoration_effective
                .underline
        );
        assert!(!s.get(by_id(&d, "in")).unwrap().text_decoration.underline);
        assert!(
            !s.get(by_id(&d, "ib"))
                .unwrap()
                .text_decoration_effective
                .underline
        );
        assert!(
            !s.get(by_id(&d, "abs"))
                .unwrap()
                .text_decoration_effective
                .underline
        );
    }

    #[test]
    fn currentcolor_and_border_width_fixup() {
        let (d, s) = styled(
            r#"<p id="a" style="color: red; border: 3px solid; outline: 2px; border-left-style: none">x</p>"#,
            "",
        );
        assert_eq!(ser(&d, &s, "a", "border-top-color"), "rgb(255, 0, 0)");
        assert_eq!(ser(&d, &s, "a", "border-top-width"), "3px");
        assert_eq!(ser(&d, &s, "a", "border-left-width"), "0px");
        assert_eq!(ser(&d, &s, "a", "outline-width"), "0px");
    }

    #[test]
    fn one_scrolling_overflow_axis_makes_the_other_auto() {
        let (d, s) = styled(
            r#"<pre id="a" style="overflow-x: auto">x</pre><div id="b" style="overflow-y: clip">y</div><div id="c" style="overflow-x: hidden; overflow-y: clip">z</div>"#,
            "",
        );
        assert_eq!(ser(&d, &s, "a", "overflow-y"), "auto");
        assert_eq!(ser(&d, &s, "b", "overflow-x"), "visible");
        assert_eq!(ser(&d, &s, "c", "overflow-y"), "hidden");
    }

    #[test]
    fn strictness_reports_unknowns() {
        let doc =
            crate::html::parse(r#"<p id="p" style="colr: red; color: nope; width: 10px">x</p>"#);
        let set = cascade(
            &doc,
            &[],
            &Media::default(),
            &MatchContext::new(),
            Strictness::Lenient,
        )
        .unwrap();
        assert_eq!(ser(&doc, &set, "p", "width"), "10px");
        assert!(set
            .unsupported
            .iter()
            .any(|u| u.kind == UnsupportedKind::Property && u.name == "colr"));
        assert!(set
            .unsupported
            .iter()
            .any(|u| u.kind == UnsupportedKind::Value && u.name == "color"));
        let err = cascade(
            &doc,
            &[],
            &Media::default(),
            &MatchContext::new(),
            Strictness::Strict,
        )
        .unwrap_err();
        assert_eq!(err.kind, UnsupportedKind::Property);
        assert_eq!(err.name, "colr");
        let doc = crate::html::parse(r#"<p>x</p>"#);
        let s = sheet("p { columns: 2 }");
        let err = cascade(
            &doc,
            &[s],
            &Media::default(),
            &MatchContext::new(),
            Strictness::Strict,
        )
        .unwrap_err();
        assert_eq!(err.name, "columns");
        let s = sheet("p { width: 10px }");
        assert!(cascade(
            &doc,
            &[s],
            &Media::default(),
            &MatchContext::new(),
            Strictness::Strict
        )
        .is_ok());
    }

    #[test]
    fn font_faces_and_keyframes_are_collected() {
        let css = "@font-face { font-family: \"My Face\"; src: url(a.woff2) format(\"woff2\"), local(Arial); font-weight: 700 } @keyframes spin { from { opacity: 0 } to { opacity: 1 } }";
        let (_, s) = styled("<p>x</p>", css);
        assert_eq!(s.font_faces.len(), 1);
        assert_eq!(s.font_faces[0].family, "My Face");
        assert_eq!(
            s.font_faces[0].src,
            vec!["a.woff2".to_string(), "local:Arial".to_string()]
        );
        assert_eq!(s.font_faces[0].weight, (700, 700));
        assert!(s.keyframes.contains_key("spin"));
    }

    #[test]
    fn logical_properties_follow_direction() {
        let (d, s) = styled(
            r#"<p id="a" style="margin-inline-start: 5px" dir="rtl">x</p><p id="b" style="margin-inline-start: 5px">y</p>"#,
            "",
        );
        assert_eq!(ser(&d, &s, "a", "margin-right"), "5px");
        assert_eq!(ser(&d, &s, "a", "margin-left"), "0px");
        assert_eq!(ser(&d, &s, "b", "margin-left"), "5px");
    }

    #[test]
    fn transitions_and_lang() {
        let (d, s) = styled(
            r#"<p id="a" style="transition: opacity 0.3s ease-in, width 1s" lang="ja">x</p>"#,
            "",
        );
        let st = s.get(by_id(&d, "a")).unwrap();
        let items = st.transitions.items();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].property, "opacity");
        assert_eq!(items[0].duration_ms, 300);
        assert_eq!(items[1].timing, TimingFunction::Ease);
        assert_eq!(st.font.lang, cw_scene::Lang::Ja);
        assert_eq!(ser(&d, &s, "a", "transition-duration"), "0.3s, 1s");
    }

    fn assert_same(doc: &Document, a: &StyleSet, b: &StyleSet) {
        for n in doc.descendants(Document::ROOT) {
            if doc.node(n).detached {
                continue;
            }
            assert_eq!(a.get(n), b.get(n), "node {n:?} {:?}", doc.kind(n));
            assert_eq!(a.before(n), b.before(n), "before {n:?}");
            assert_eq!(a.after(n), b.after(n), "after {n:?}");
            assert_eq!(a.marker(n), b.marker(n), "marker {n:?}");
        }
    }

    #[test]
    fn incremental_restyle_matches_full_cascade() {
        let css = ".red { color: red } .big { font-size: 2em } #x > span { color: blue } p + p { margin-top: 0 } li:nth-child(2n) { color: green } .sib ~ span { font-weight: bold } div:empty { display: none } [data-k=v] { padding: 1px } ul li::before { content: \"-\" } .red::after { content: \"!\" }";
        let mut doc = crate::html::parse(
            r#"<div id="x"><p class="a">1<span>s</span></p><p class="b">2</p><ul><li>a</li><li>b</li><li>c</li></ul><span class="sib">q</span><span>r</span><div id="e"></div></div>"#,
        );
        let sheets = [sheet(css)];
        let media = Media::default();
        let ctx = MatchContext::new();
        let mut set = cascade(&doc, &sheets, &media, &ctx, Strictness::Lenient).unwrap();
        doc.drain_mutations();
        // A deterministic sequence of mutations, checking after each batch.
        let mut seed: u64 = 12345;
        let mut rand = move |n: u64| {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) % n
        };
        for step in 0..60 {
            let elements: Vec<NodeId> = doc
                .descendants(Document::ROOT)
                .filter(|n| {
                    doc.is_element(*n)
                        && !doc.is(*n, "html")
                        && !doc.is(*n, "body")
                        && !doc.is(*n, "head")
                })
                .collect();
            let target = elements[rand(elements.len() as u64) as usize];
            match rand(8) {
                0 => doc.set_attr(
                    target,
                    "class",
                    ["red", "big", "sib", "a", ""][rand(5) as usize],
                ),
                1 => doc.set_attr(
                    target,
                    "style",
                    ["color: purple", "font-size: 20px", "", "display: contents"][rand(4) as usize],
                ),
                2 => {
                    let n = doc.create_element(
                        "span",
                        vec![Attribute {
                            name: "class".into(),
                            value: "red".into(),
                        }],
                    );
                    doc.append(target, n);
                }
                3 => {
                    let n = doc.create_element("li", vec![]);
                    let t = doc.create_text("x");
                    doc.append(n, t);
                    let before = doc.first_child(target);
                    doc.insert_before(target, n, before);
                }
                4 => {
                    if let Some(c) = doc.first_child(target) {
                        doc.detach(c);
                    }
                }
                5 => doc.set_attr(target, "data-k", ["v", "w"][rand(2) as usize]),
                6 => doc.remove_attr(target, "class"),
                _ => {
                    if let Some(t) = doc.children(target).find(|c| doc.text(*c).is_some()) {
                        doc.set_text(t, if step % 2 == 0 { "" } else { "y" });
                    }
                }
            }
            if rand(3) == 0 {
                continue; // batch several mutations
            }
            let muts = doc.drain_mutations();
            restyle(
                &doc,
                &mut set,
                &muts,
                &sheets,
                &media,
                &ctx,
                Strictness::Lenient,
            )
            .unwrap();
            let full = cascade(&doc, &sheets, &media, &ctx, Strictness::Lenient).unwrap();
            assert_same(&doc, &set, &full);
        }
        let muts = doc.drain_mutations();
        restyle(
            &doc,
            &mut set,
            &muts,
            &sheets,
            &media,
            &ctx,
            Strictness::Lenient,
        )
        .unwrap();
        let full = cascade(&doc, &sheets, &media, &ctx, Strictness::Lenient).unwrap();
        assert_same(&doc, &set, &full);
    }

    #[test]
    fn restyle_state_hover() {
        let doc = crate::html::parse(
            r##"<div id="d"><a id="a" href="#">x<span id="s">y</span></a></div>"##,
        );
        let sheets = [sheet(
            "a:hover { color: red } a:hover span { font-weight: bold }",
        )];
        let media = Media::default();
        let mut ctx = MatchContext::new();
        let mut set = cascade(&doc, &sheets, &media, &ctx, Strictness::Lenient).unwrap();
        let a = by_id(&doc, "a");
        ctx.set_hovered(&doc, Some(a));
        restyle_state(
            &doc,
            &mut set,
            &[a],
            &sheets,
            &media,
            &ctx,
            Strictness::Lenient,
        )
        .unwrap();
        assert_eq!(ser(&doc, &set, "a", "color"), "rgb(255, 0, 0)");
        assert_eq!(ser(&doc, &set, "s", "font-weight"), "700");
        let full = cascade(&doc, &sheets, &media, &ctx, Strictness::Lenient).unwrap();
        assert_same(&doc, &set, &full);
    }

    #[test]
    fn serialization_matches_get_computed_style_forms() {
        let (d, s) = styled(
            r#"<div id="a" style="width: 50%; margin: 0 auto; border-radius: 4px / 8px; background: url(x.png) no-repeat center / cover, linear-gradient(to right, red, blue); box-shadow: 0 1px 2px rgba(0,0,0,.5), inset 0 0 0 1px red; transform: translate(10px, 20%) rotate(45deg); grid-template-columns: [a] 1fr repeat(auto-fill, minmax(100px, 1fr)) [b]; grid-area: 1 / 2 / span 2 / c; opacity: .5; letter-spacing: 1px; content: 'x' attr(title) counter(c, upper-roman); font: italic 700 12px/1.5 'Helvetica Neue', serif; animation: spin 1s infinite">x</div>"#,
            "",
        );
        assert_eq!(ser(&d, &s, "a", "width"), "50%");
        assert_eq!(ser(&d, &s, "a", "margin-left"), "auto");
        assert_eq!(ser(&d, &s, "a", "border-top-left-radius"), "4px 8px");
        assert_eq!(
            ser(&d, &s, "a", "background-image"),
            "url(\"x.png\"), linear-gradient(90deg, rgb(255, 0, 0), rgb(0, 0, 255))"
        );
        assert_eq!(ser(&d, &s, "a", "background-repeat"), "no-repeat, repeat");
        assert_eq!(ser(&d, &s, "a", "background-size"), "cover, auto");
        assert_eq!(ser(&d, &s, "a", "background-position-x"), "50%, 0%");
        assert_eq!(
            ser(&d, &s, "a", "box-shadow"),
            "rgba(0, 0, 0, 0.5) 0px 1px 2px 0px, rgb(255, 0, 0) 0px 0px 0px 1px inset"
        );
        // A percentage translate keeps the function list (the box is not laid out);
        // lengths-only lists resolve to the composed matrix, as Chromium reports.
        assert_eq!(
            ser(&d, &s, "a", "transform"),
            "translate(10px, 20%) rotate(45deg)"
        );
        let (d2, s2) = styled(
            r#"<p id="b" style="transform: translateX(1rem) rotate(0deg) skewX(0deg) skewY(0deg) scale(0.95, 1) scale(1, 0.95)">x</p>"#,
            "",
        );
        assert_eq!(
            ser(&d2, &s2, "b", "transform"),
            "matrix(0.95, 0, 0, 0.95, 16, 0)"
        );
        assert_eq!(
            ser(&d, &s, "a", "grid-template-columns"),
            "[a] 1fr repeat(auto-fill, minmax(100px, 1fr)) [b]"
        );
        assert_eq!(ser(&d, &s, "a", "grid-row-start"), "1");
        assert_eq!(ser(&d, &s, "a", "grid-column-start"), "2");
        assert_eq!(ser(&d, &s, "a", "grid-row-end"), "span 2");
        assert_eq!(ser(&d, &s, "a", "grid-column-end"), "c");
        assert_eq!(ser(&d, &s, "a", "opacity"), "0.5");
        assert_eq!(ser(&d, &s, "a", "letter-spacing"), "1px");
        assert_eq!(
            ser(&d, &s, "a", "content"),
            "\"x\" attr(title) counter(c, upper-roman)"
        );
        assert_eq!(ser(&d, &s, "a", "font-family"), "\"Helvetica Neue\", serif");
        assert_eq!(ser(&d, &s, "a", "font-style"), "italic");
        assert_eq!(ser(&d, &s, "a", "font-weight"), "700");
        assert_eq!(ser(&d, &s, "a", "line-height"), "18px");
        assert_eq!(ser(&d, &s, "a", "animation-name"), "spin");
        assert_eq!(ser(&d, &s, "a", "animation-iteration-count"), "infinite");
        assert_eq!(
            s.get(by_id(&d, "a")).unwrap().font.typeface,
            cw_scene::Typeface::Arimo
        );
    }
}
