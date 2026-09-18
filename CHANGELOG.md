# Changelog

## 0.1.0-alpha.2 — 2026-09-18

The first release whose wheels and Wasm bundles carry the desktop shell: the
`0.1.0-alpha.1` artifacts have no `window:*` or `shell:*` interaction targets, no
window manager and no native applications, so `examples/python/desktop_pixels.py`
and the pointer-driven parts of `examples/python/computer_interaction.py` need this
release or a source build.

Install by exact version. `target/wheels/` is a build directory and a
`computerworld*.whl` glob can pick up a wheel from an older revision — the usual
cause of an example failing with missing interactions or `application module
versions differ`. Package version `0.1.0a2` and engine version `0.1.0-alpha.2` are
the same release in PEP 440 and Cargo spellings. Snapshots from `0.1.0-alpha.1` are
not supported.

### Editors, application menus and page zoom

- Word wrap (`shell:toggle:word_wrap`, a machine setting): Notepad's View menu and
  its settings gear, TextEdit's View ▸ Wrap to Window and GNOME Text Editor's primary
  menu all toggle it; phone editors always wrap. Wrapped rows break after the last
  whole word that fits, the gutter numbers real lines only, clicks land on the
  wrapped row painted (`editor-text:<first row>:<columns>`), and the published caret
  sits on its visual row. The editor gained Up/Down arrows.
- Notepad's File, Edit and View buttons open real menus (they opened quick settings
  before): New window, Save, Close window; Edit ▸ Time/Date types the world clock;
  View ▸ Word wrap. Choosing a menu entry closes its menu.
- GNOME Text Editor and Terminal primary menus: New Window, Save, Wrap Text, Insert
  Date and Time, Close; Full Screen and Reset and Clear (`shell:terminal:clear`).
- Browser zoom (`shell:zoom:in|out|reset`, `Ctrl`/`Meta` + `=`/`-`/`0`), remembered
  per site. The page is laid out for the zoomed CSS viewport and text is rasterised
  at the new size, so it reflows and stays sharp. Safari's View menu and iOS Safari's
  "AA" menu carry it.
- The editor caret no longer measured its row pitch from toolbar text, which put the
  published caret on the wrong row in Notepad.

### Desktop shell

A real window manager and shell, driven entirely through `pointer.v1` /
`keyboard.v1` / `application.v1` against scene-node `interaction` targets:

- Windows with move, resize by edge and corner, minimize, maximize, close, focus and
  stacking; pointer capture for drags; `double_click` on a title bar maximizes.
- **Pointer semantics are click-to-select, double-click-to-open** on desktop themes;
  `ios` and `android` themes open on a single tap. Press identity is tracked between
  `down` and `up`, so releasing outside the pressed bounds cancels.
- A tabbed file manager (`FileTab`), with tabs held in the window rather than in the
  browser session: `window:<id>:content:files-newtab`, `files-tab:<i>`,
  `files-closetab:<i>`, plus `files-{home,up,root,back,forward,reload}`. Rows
  (`window:<id>:content:open:<i>`) open on double-click; folders open in place,
  documents need an editor installed. Browser tabs remain `shell:tab:*`.
- Shell panels (`shell:panel:*`), launcher and search, notification/control centre,
  device toggles (`shell:toggle:<setting>`) and levels (`shell:set:<setting>:<pct>`)
  that change real session state, and power actions (`shell:power:*`) that change
  what the screen shows.
- Touch gestures on phone themes synthesized from `down`/`up`: swipe for control
  centre, launcher, home and overview.
- Thirteen native applications — calendar, mail, chat, docs, notes, contacts,
  settings, calculator, clock, photos, music, maps, weather — each backed by a world
  service or the machine's own files rather than a static mock.
- Calendar panels page by month (`shell:month:{prev,next,today}`) and every day in
  them opens Calendar on that date (`shell:launch:calendar/YYYY-MM-DD`), on all five
  shells. Days before the world began carry no target.
- Share (`shell:share:{chat,mail}`) hands over the real thing: the page, document or
  selected file becomes the Messages draft or the body of a new Mail message, and the
  Finder and Explorer toolbars carry it for the selected item.
- The Android keyboard's suggestion strip completes the word before the caret; each
  chip is a `shell:insert:` of the rest of the word.

New: [docs/action-families.md](docs/action-families.md) tabulates every action
family, op, payload, return value and interaction target, and the
privileged-versus-actor split.

### Services and the simulated web

Eleven new service crates — `assistant`, `bank`, `drive`, `forum`, `geo`, `media`,
`press`, `search`, `shop`, `social`, `wiki` — join the existing set. The reference
world is no longer an intranet with a handful of pages: it declares many more
service instances and a browsable synthetic web of independent sites under
`worlds/company-2026/sites/`, built by `scripts/build-world.mjs` and
`scripts/build-search-index.mjs`. Counts change per revision; read
`worlds/company-2026/world.json` rather than quoting a number.

### Page model

`PageElement` gained `Row`, `Grid`, `Card`, `Styled`, `Thumbnail`, `Badge`,
`Divider` and `Spacer`, and pages gained a `PageTheme` (accent, background, surface,
ink, muted, content width) plus a `Style` presentation hint on text. Pages still
describe documents, not arbitrary geometry: the extents are bounded
(`MAX_STYLE_SPAN`, `MAX_PAGE_GAP`, `MAX_GRID_COLUMNS`, `MAX_PAGE_EXTENT`) and colors
must be `#rrggbb`/`#rrggbbaa`. Existing pages remain valid; consumers that
exhaustively match on `PageElement` must handle the new variants.

Layout is responsive: rows give flex children at least their min-content width and
wrap when they cannot, grids of cards drop columns on narrow viewports, and on a
phone-width page a sidebar stacks under the content. Links wrap. Badges that only
colour their text are labels rather than accent pills (Gmail's unread counts and
Amazon's "Two-day" were unreadable accent-on-accent before), and empty badges draw
nothing. Forms take their title and submit label from their id (Search, Send,
Reply, RSVP, Create) instead of a generic "Update"/"Save changes", and a form with
no fields is just its button. `cw_service_common::column` stacks children; the
YouTube watch page used a `Row` for its columns and a zero-width player, and now
shows the player above the title with Up next beside it.

### Shell

`find` applied its root and **silently ignored every predicate**, so
`find /home/agent -name nope` returned the whole tree. It now implements `-name`,
`-iname`, `-type` and `-maxdepth`, and refuses an unsupported predicate by name with a
non-zero exit rather than returning a wrong answer. New: `stat` (with `-c`), `du`, `df`,
`which`, `nproc`, `uptime`, `clear`, `ip`, `sudo`; `grep` works non-recursively and takes
`-i -v -n -c -l -L -F -E -q -s -h -H -w -x -r -R -e`; `sed` takes `-n -i -e` with `Np`,
`N,Mp`, `$p`, `Nd` addresses; `ls` takes the usual flag clusters with real `-l` rows;
`date` returns a clock-shaped string derived from the tick; and `2>`, `2>>`, `2>&1`,
`&>` and `/dev/null` work, with descriptors resolved in order so `>f 2>&1` and
`2>&1 >f` differ as in bash.

**Exit codes were not truthful.** The command dispatcher returned
`Result<String, String>`, so the error arm had nowhere to carry a status and every
failure was reported as `1`; `git`, `sh` and script execution discarded the inner code
as well. Commands now return a real status: `0` success, `1` a modelled negative,
`2` outside the simulated surface or malformed, `126` not executable, `127` no such
command. New: [docs/shell.md](docs/shell.md) publishes the supported command and flag
matrix, marking each behaviour *modelled* or *fixed*, with a conformance test that keeps
the document honest and asserts every refusal names the flag it rejected.

### Terminal

The terminal never echoed a command and had no prompt, so an agent watching the screen
could not tell when one finished. `AppState::Terminal` now holds a transcript of
entries carrying the prompt as it was, the command, stdout, stderr and the exit code;
the prompt is real (`user@host:cwd$`, or `PS C:\path>` under powershell) and is captured
before the command runs. A non-zero exit is shown as `[exit N]` on screen and emitted for
every entry in the semantic projection, so success is never inferred from the absence of
a line. `clear` is a real program that empties the transcript and leaves history and cwd
intact.

### Perception

`Scene` gained an accessibility-shaped view (`accessibility()` merging nodes into
`AxNode` with role, name, value, enabled/focusable/focused/checked/selected/expanded and
the owning window), focus and caret geometry (`Focus`, `Caret`, `Keyboard` reporting
where the next keystroke goes), occlusion (`hit_stack`, `hit_reaches`, per-window
`occluded_by`/`exposed`), window identity and bounds (`SceneWindow`), whole-pane text
(`TextBuffer`) and line metadata (`TextLine`) whose `continuation` flag marks a wrapped
fragment — joining visual lines previously corrupted text with no signal. Change
detection is `SceneDelta` plus per-node `revision` and a scene `digest`, all **content
derived rather than counted**, so a snapshot restore cannot desynchronise them.

`ActionOutcome` gained `effect: Option<ActionEffect>`, a coarse per-action summary of
what changed: tags, windows opened and closed, focused window, browser URL and a digest
of the actor-visible state. An accepted-but-inert action reports `changed: []`, which is
a real answer; a denied action reports `None`, because it never reached a machine. The
digest crosses the wire as text: a `u64` reaches JavaScript as a `BigInt`, which
`JSON.stringify` refuses.

### Rendering legibility

Hyphens in identifiers were lost to OCR because a hyphen straddled two pixel rows at
most terminal sizes; `~` rasterised to two rows and was indistinguishable from `-` once
downsampled; the dotted zero's dot was faint against `8`. Fixed-pitch glyphs are now
grid-fitted with integer-only arithmetic: bars snap to one whole row, `~` is rasterised
at 2x and box-filtered back for real amplitude, and a dotted zero's interior mark is
scaled to full opacity. Output stays bit-for-bit reproducible.
[crates/render/assets/README.md](crates/render/assets/README.md) records which glyphs
remain ambiguous, at which sizes, with measurements.

### Rendering

Higher-fidelity OS shells. Each platform now has a system-like bundled typeface with
measured, word-wrapped and ellipsized text; frosted-glass materials (`Primitive::Backdrop`),
tintable symbols (`Primitive::Symbol`), rounded window clipping, antialiased paths and
filtered icon/wallpaper scaling in the deterministic renderer; and rebuilt macOS, Windows 11,
Ubuntu, iOS and Android shells, window frames, browser chrome and application views.
Phones always present applications full screen, Android gained its notification shade, and
service pages resolve their header icons. Scene JSON gains optional `typeface` and
`rounded_clip` fields; pixel output of desktop scenes changes. `cargo run --example
os-gallery` renders every shell to PNG for review.

### Distribution size

The browser bundle is about 4.9 MB gzipped, down from 13.8 MB: wallpapers ship as
JPEG (decoded with `jpeg-decoder`'s platform-independent path, so native and Wasm
pixels still match), the bundled DejaVu faces are subset to Latin, Greek, Cyrillic,
symbols, box drawing and Braille, and the Wasm module is built for size with
rendering kept at full optimisation. Text outside the subset (Hebrew, Arabic, Thai,
CJK) draws as a hollow box. Wasm renders are roughly 40% slower than a full-speed
build; see [docs/performance.md](docs/performance.md).

### Compatibility

Snapshots from before these changes are rejected on restore with `application module
versions differ` — registered module identities are part of the checkpoint boundary.
Pixel output of every desktop scene changed. Wheel, Wasm bundle and any recorded
checkpoint must come from the same revision; `scripts/smoke-bindings.sh` builds them
in that order.

## 0.1.0-alpha.1

First public prerelease. A single Rust runtime exposes synthetic computer worlds
through native, Python and JavaScript/Wasm APIs, with deterministic replay,
snapshots, synthetic networking/services, structured/pixel observations and
interactive desktop/mobile examples.

See [release notes](docs/releases/v0.1.0-alpha.1.md) for the supported scope,
installation, verification and alpha compatibility limits. Python package version:
`0.1.0a1`. No API or cross-version checkpoint stability guarantee yet.
