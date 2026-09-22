# Migrating a service from `Page` to HTML

Milestone 5 of [web-engine-plan.md](contracts/web-engine-plan.md): services serve HTML, the
browser renders it through the `cw-web` engine, and `Page` stays behind a converter
(`cw_web::page::to_document`) for what has not moved yet. This is the recipe, with
`crates/services/search` (google.com, bing.com, duckduckgo.com) as the worked example.

## What to replace

| `Page` code | HTML |
| --- | --- |
| `web::themed_page(title, theme, elements)` | `Document::new(title).lang("en").stylesheet(CSS).root_style("--accent: ..").body_class("skin-x").body(nodes)`, returned with `html::page(&doc)` |
| `web::styled(id, text, style)` | `el("p").id(id).class("stats").text(text)`; the `Style` becomes a rule in the `.css` file |
| `web::row`, `web::grid`, `web::column`, `web::card` | `div("row")`, `div("tiles")`, ...: a class each, laid out by the stylesheet with `display: flex` or `grid` |
| `web::card_action(id, style, visit(url), children)` | `el("a").id(id).class("hit").attr("href", url).child(..)`: a block anchor, so the card stays one link with the same id |
| `web::link` / `styled_link` / `inline_link` | `link(id, href, text)`, styled by class |
| `PageElement::Form { id, action, children }` | `form(id, action.url, "get"\|"post")`; fields are `text_input(id, name, value)`, `hidden(name, value)`; `$field` references become the input's `name` |
| `PageElement::Input { id, label, value, placeholder }` | `text_input(id, name, value).attr("aria-label", label).attr("placeholder", ..)` (or a `<label for>`) |
| `PageElement::Button { id, text, action }` | `button(id, text)` inside the form; a button that posts elsewhere gets `.attr("formaction", url)` |
| `PageAction { method: "GET", url }` on a card | an `href` |
| `web::thumbnail`, `web::badge`, `web::icon` | a `span` with a class; a synthetic picture is a tinted box with the label inside |
| `web::divider`, `web::spacer` | margins in the stylesheet |
| `PageTheme` colours read at render time | custom properties on `<html style>` (`--accent`, `--ink`, `--muted`, `--surface`, `--paper`), read by the sheet with `var()` |
| a query string built by hand | `href("/search", &[("q", query), ("v", vertical)])` |

The stylesheet is a real file next to the source (`crates/services/search/src/search.css`),
included with `include_str!` and handed to `Document::stylesheet`, which emits one
`<style>` in `<head>`. It uses only what the engine renders, which the strict
validator checks; `crates/web/src/style/properties/mod.rs` lists the longhands and
`crates/web/src/style/shorthands.rs` the shorthands.

## Keeping the ids

Ids are the agent API: `browser.v1` `click`, `fill` and `submit` name elements by id,
and the semantic observation lists links, buttons, inputs and forms by the same id.
Keep every id the `Page` version had on the element that plays the same role:

- the form keeps its id (`search`), the input its id and a `name` (`q`), the buttons
  theirs (`search-go`, `search-lucky`);
- a card that was one click target becomes a block `<a id>` with the same id, and the
  texts inside keep their `-title`, `-site`, `-snippet` ids as spans;
- tabs, footer links and history links keep theirs (`tab-<vertical>`, `foot-<n>`,
  `recent-<n>`);
- a button that posted with no fields becomes a one-button form (`recent-clear-form`
  around `recent-clear`);
- ids are unique per page (the validator checks).

## The worked example: search

`crates/services/search/src/view.rs` before: `Chrome::mark` built a `Row` of coloured
`Styled` letters, `box_` a `PageElement::Form` with an `Input` and two `Button`s whose
`PageAction`s carried `$q`, `snippet` a `card_action` with three `styled` texts.

After: `mark` is `<div id="mark" class="logo">` with one `<span class="c0">` per
letter, coloured by `.logo .c0 { color: #4285f4 }`; `box_` is `form("search",
"/search", "get")` around `div("box")` holding `text_input("q", "q", query)` and the
CSS-drawn icons, with `button("search-go", ..)` and `button("search-lucky",
..).attr("formaction", "/lucky")` beside it; `snippet` is `el("a").id("hit-0")
.class("hit").attr("href", url)` with `span("crumb")`, `span("title")` and
`span("snip")` inside. The palette from the seed's `theme` goes on `<html style>` as
custom properties; the skin (`google`, `bing`, `ddg`, `plain`) is `body.skin-<name>`,
and `search.css` keys the letter-spacing, the icons and the radii on it. Long result
lists are paged (`page-<n>`, `page-prev`, `page-next`, `?p=<n>`).

The tests (`crates/services/search/src/lib.rs`) parse every response with
`cw_web::html::parse`, find elements with `Document::by_id`, read `href`, `action`
and `value` attributes, and run `validate_strict` on every page they fetch. The
browser-level test (`crates/computerworld/tests/google_search.rs`) navigates to
google.com, reads the semantic tree, clicks the box, types with `keyboard.v1`,
presses Enter, and clicks `hit-0`.

## Checklist

- [ ] Ids preserved: every id the `Page` version had is on the element with the same role; unique per page.
- [ ] Forms preserved: same `action` and method, same field names, the same buttons (`formaction` for a button that goes elsewhere).
- [ ] Stylesheet in a `.css` file next to the source, included with `include_str!`, one `<style>` per page; per-instance colours as custom properties on `<html>`.
- [ ] Every page of every skin runs through `html::validate_strict` in the service's tests (`cw-service-common` with `features = ["validate"]` under `[dev-dependencies]`).
- [ ] Service tests parse HTML (`cw_web::html::parse`, `by_id`, attributes, `text_content`) instead of reading Page JSON.
- [ ] A browser-level test in `crates/computerworld/tests` drives the site through `browser.v1` and `keyboard.v1`: navigate, read the semantic tree, fill or type, press Enter, click a result.
- [ ] The semantic tree still lists the links, buttons, inputs and forms an agent used, with their ids and labels (`aria-label` or `<label for>` on inputs).
- [ ] Text from requests and seeds goes through `text(..)` or `.text(..)`, never `raw`.
- [ ] `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings` are green.

## Further

[service-sdk.md](service-sdk.md#html-services) describes the template layer, the
stylesheet convention and the strict validator in full; [custom-service.md](custom-service.md)
is the end-to-end example. What the engine does with the HTML afterwards — parse,
cascade, lay out, paint, run the scripts — is [rendering](rendering.md) and the `cw-web`
entry in [architecture](architecture.md). The ids this recipe is so careful to keep are
the ones [action families](action-families.md) addresses.
