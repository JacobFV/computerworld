# React (TSX + Tailwind) versus hand-positioned Painter apps

The program's premise is that a coding agent builds higher-fidelity UIs in React than by
hand-positioning components with the native `Painter`. This note tests it on three of the
agent-written React apps in `crates/web/engine/tests/framework-parity/` by writing the same
three apps against `cw_applications::desktop_scene::Painter`, rendering both, and scoring
both with the same layout-fault checks. Measured on 2026-09-24 at commit `68b0ecd`.

## What was compared

| App | React source (non-blank, non-comment lines) | Painter source |
|---|---|---|
| analytics dashboard (`app-analytics`) | 537 (`App.tsx` 482 + `data.ts` 81, counted) | 392 |
| kanban board (`app-kanban`) | 292 | 293 |
| settings form with validation (`app-settings`) | 300 | 260 |

Line counts are from the study's own counter (`loc` in `src/main.rs`), not `wc -l`. The React
apps also share `shared/icons.tsx` (365 lines, generated from Lucide) and a 58-line build
(`build.sh`, `tailwind.config.js`, `input.css`); the Painter apps use the bundled
`symbol/*` icons for free.

- React: `crates/web/engine/tests/framework-parity/app-src/{analytics,kanban,settings}/`,
  run on the engine's realm (React 18 on cw-jsvm) and painted by cw-web. After the engine
  fixes of this workstream the engine lays these apps out as Chromium does (framework
  parity 98.8-100% of nodes in every state of these three apps, the misses being hover state), so the engine render is a
  faithful stand-in for the browser's.
- Painter: `research/studies/react-vs-painter/src/painter/{analytics,kanban,settings}.rs`,
  a standalone crate outside the workspace and the product. It drives each app through the
  same states as the React `steps.json` (13 states in all) with the same data.

Run it (release, about a second once built):

    cd research/studies/react-vs-painter
    CARGO_TARGET_DIR=../../../target/research/react-vs-painter-build cargo run --release > report.json

It writes every picture to `target/research/react-vs-painter/` and prints the report checked
in as `research/studies/react-vs-painter/report.json`.

## The fault checklist

Counted from the rendered `cw_scene::Scene` of each version, by the same code
(`faults` in `src/main.rs`), so neither side is judged by eye:

1. **truncated**: a label the renderer shortened with an ellipsis (text containing `…` that
   does not occur in the app's source).
2. **text_overlaps**: two text nodes whose glyph extents (natural advance by font size)
   intersect by more than 2 px each way, unless they are pieces of one line or an opaque
   surface painted between them (a dialog, a toast) hides the lower one.
3. **text_overflowing_its_box**: text whose natural extent runs more than 1 px past the box
   it sits on (the last filled or bordered box painted before it under its first pixel).
4. **near_miss_alignment**: same-size texts on the same surface within 120 px that sit
   1-3 px apart vertically, or an icon beside text whose centres differ by 2-4 px.
5. **inconsistent_spacing**: runs of three or more same-size boxes in a row or column
   whose gaps (all at most 64 px) differ by more than 1 px.
6. **cut_off_right**: nodes that cross the window's right edge and are not inside a
   narrower clip.

Every state is scored at the design size (1280 x 800) and in a smaller window
(1024 x 768), the second standing in for "the content or the window is not what the author
pictured".

## Results

| | React, 1280 x 800 | Painter, 1280 x 800 | React, 1024 x 768 | Painter, 1024 x 768 |
|---|---|---|---|---|
| truncated | 0 | 1 | 0 | 1 |
| text_overlaps | 0 | 0 | 0 | 0 |
| text_overflowing_its_box | 0 | 0 | 0 | 0 |
| near_miss_alignment | 0 | 0 | 0 | 0 |
| inconsistent_spacing | 0 | 0 | 0 | 0 |
| cut_off_right | 0 | 1 | 70 | 379 |

Totals over the 13 states per column. The details are in `report.json`:

- Painter, 1280: the settings form's username error is cut to
  `Usernames can only contain letters, num…` (the React field wraps it onto a second line
  and pushes the fields below down); the saved state's toast shadow crosses the right edge.
- React, 1024: all 70 are the kanban board's fourth column (14 nodes in each of 5 states),
  which sits in the board's `overflow-x-auto` scroll container and scrolls into view. The
  study's clip test cannot see that the container scrolls, so they are counted; by the
  source they are reachable. Analytics and settings reflow to the narrower window with
  nothing cut.
- Painter, 1024: every state loses its right-hand content for good (50-59 nodes in the
  analytics states, 26 in kanban, 15-20 in settings), including the settings form's
  Save button and the analytics Export button; nothing scrolls it back.

Pictures (checked in under `research/studies/react-vs-painter/pictures/`; Chromium's own
screenshots of the React apps are `crates/web/engine/tests/framework-parity/app-*.chromium.png`):

- `analytics.initial.react-engine.png` / `analytics.initial.painter.png`: the design size;
  close to each other. The Painter's area chart shows the vertical banding of drawing the
  gradient fill as 4 px strips, and its line is an integer-pixel polyline.
- `settings.errors.painter.png`: the truncated username error.
- `settings.errors@1024.react-engine.png` / `settings.errors@1024.painter.png`: the React
  form narrows its card and keeps its sticky footer at the window's bottom; the Painter
  form runs off the right edge and its footer, pinned at y = 731 for an 800 px window,
  covers the website field.
- `kanban.initial@1024.react-engine.png` / `kanban.initial@1024.painter.png`.

## What this does and does not show

- At the design size the checks find almost nothing on either side (0 React, 2 Painter).
  That result is biased in the Painter's favour: the Painter versions were written after
  the React apps had been rendered and dumped, and many coordinates in them are Chromium's
  (the range button at 978,114, the footer at y = 731). A coding agent writing a Painter app
  from a description has no such reference; it has to measure text, sum paddings and keep
  every coordinate consistent by hand, and the checks here would then be the ones to catch
  what it got wrong. That experiment (an agent writing both from the same prompt, scored
  blind) is the next step and was not run here.
- Off the design size the difference is large and structural: 0 lost nodes for React in
  analytics and settings and 70 reachable-by-scroll in kanban, against 379 unreachable for
  the Painter. Hand-positioned coordinates encode one window size; flex, grid and wrapping
  text encode intent. The Painter apps here ignore the window width entirely, which is how
  they were written; a Painter app can read `env.width` and lay out proportionally, at the
  cost of writing that arithmetic for every region.
- Effort by lines is roughly even (1,129 React lines against 945 Painter lines for the three
  apps, before the React side's shared icons and build), but the React lines buy more
  behaviour: real text inputs with caret and selection, hover and focus states,
  transitions, a scroll container, sticky positioning, wrapping error messages, and the
  DOM's accessibility tree. The Painter versions fake text entry (`type_text` appends to a
  string), have no hover or focus, and hard-code where each line of text goes. The Painter
  side needed one compile fix (an ambiguous float); the TSX is not type-checked by the
  build (esbuild strips types), so it got none.
- The checks are heuristics with known blind spots: they see the scene, not intent (the
  kanban scroll case), and a fault that looks right to the checker, such as the Painter's
  footer covering a field at 768 px high, is not counted.

In short: at the size the author targeted, both approaches can produce a clean layout, and
this study cannot separate them there without the blind experiment; as soon as the window
changes, the React apps keep working and the Painter apps lose content.
