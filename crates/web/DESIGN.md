# cw-web: contracts between the modules

Read docs/web-engine-plan.md first. This file fixes the interfaces the modules meet at,
so they can be built in parallel. A module may add to its own API freely; the shared
types in `geom.rs`, `dom/`, and `style/computed.rs` change only by agreement (edit and
tell the other owners).

## Pipeline

```
bytes --html::parse--> dom::Document
dom::Document + [css::Stylesheet] --style::cascade--> style::StyleSet (ComputedStyle per NodeId)
dom::Document + StyleSet + Viewport --layout::layout--> layout::FragmentTree (Au geometry)
FragmentTree --paint::paint--> cw_scene::Scene (nodes; semantics from the DOM)
```

Everything is deterministic: no floats in layout (`geom::Au`, 1/64 px, i32), no host
time, no host fonts, no HashMap iteration order that reaches output (use BTreeMap or
sort). Text is measured through `cw_scene::metrics` (`text_width`, `advance`, `wrap`)
with pixel results converted to `Au`.

## Ownership

| Module | Owns | Public entry points |
|---|---|---|
| `html` | tokenizer, tree builder, entities, fragment parsing | `html::parse(&str) -> Document`, `html::parse_fragment(&Document, context: NodeId, &str) -> Vec<NodeId>` |
| `dom` | node arena, tree ops, attributes, mutation log (shared; written by the scaffold, extended by `html`) | see `dom/mod.rs` |
| `css` | tokenizer, parser, selectors, at-rules, stylesheet model, specificity, selector matching against `dom` | `css::parse_stylesheet(&str, Origin) -> Stylesheet`, `css::parse_declarations(&str) -> Vec<Declaration>`, `css::matches(&Document, NodeId, &Selector, &MatchContext) -> bool` |
| `style` | property table (name, syntax, initial, inherited, computed form), shorthands, value parsing, the user-agent sheet, presentational hints, cascade, inheritance, custom properties, `calc`, colours, `ComputedStyle` | `style::cascade(&Document, &[Stylesheet], &Media) -> StyleSet`, `style::ua::sheet()`, `StyleSet::get(NodeId) -> &ComputedStyle` |
| `layout` | box tree generation, block, inline, tables, replaced elements, intrinsic sizes, fragment tree | `layout::layout(&Document, &StyleSet, Viewport) -> FragmentTree` |
| `paint` | fragment tree to `cw_scene::Scene`, stacking, backgrounds, borders, text decoration, semantics | `paint::paint(&Document, &StyleSet, &FragmentTree, Viewport) -> cw_scene::Scene` |
| `page` | `cw_protocol::Page` to HTML plus stylesheet (migration bridge) | `page::to_html(&Page) -> (String, String)` |

## Strictness

`css::parse_stylesheet` and `style::cascade` take `Strictness::Lenient` (ignore and log
unknowns, browser behaviour) or `Strictness::Strict` (return an error naming the first
unsupported property, value, selector, at-rule or element). Authored sites build with
Strict. Captured pages run Lenient. A `Vec<Unsupported>` is collected in both modes.

## Fixed point

`Au` is `i32` in 1/64 px. Pixel conversions round half away from zero. Layout never
divides by zero and saturates instead of overflowing. `Au::from_px(f)` is only for
parsing literal CSS numbers, which are parsed as decimal strings, never `f64` math in
layout.

## Fonts

`ComputedStyle::font_family` is the resolved `cw_scene::Typeface` (via
`cw_scene::fonts::resolve_family`, from M0a) plus the original family list for
`getComputedStyle`. `font_weight` is 100..=900; the renderer has 400 and 700 and
synthesises the rest as bold when >= 600. Text runs are measured with
`cw_scene::metrics::text_width(typeface, Style{bold, italic, lang}, text, size_px)`.

## Testing

Each module has unit tests. `tests/` holds reftests (`tests/ref/<name>.html` and
`<name>-ref.html` must paint to identical scene digests) and layout parity fixtures
(`tests/parity/<name>.html` with `<name>.chromium.json`, a dump of each element's
`getBoundingClientRect` and chosen computed values made by
`scripts/web-parity/dump.mjs`). `cargo test -p cw-web` runs all of them.
