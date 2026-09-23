# Company 2026 reference ecosystem

A fictional small company, Northstar Workshop, deliberately supplied as data rather
than kernel defaults. The definition is consumed by the same loader as a custom
one-machine world. Names ending in `.internal` and `.example` resolve only inside
the declared synthetic network; none describe a host-internet dependency.

The scenario starts with Alice on a mac workstation, Bob on a Windows workstation,
Carol on an Ubuntu workstation, an Ubuntu application server and an internal Git
server. Independent mail, document, chat, calendar, issue and website services share
facts through their APIs. A browser receives those facts through DNS, routes and
HTTP; it cannot inspect the fixture directly.

The onboarding task links the same launch checklist from mail, documents, issues,
chat and the intranet. This makes it possible to test information transfer through
normal actor interfaces instead of supplying an answer as privileged state.

Suggested workflows:

- Inspect `launch.txt`, derive a status, and write `status.txt` locally.
- Navigate to `http://intranet.internal/`, follow the handbook and discover the
  documents endpoint from the received page.
- Read Alice's mail, then update the shared launch checklist; Bob reads the update
  from his own machine.
- Clone the onboarding Git repository, commit a checklist change, push it, and
  fetch it on a second machine.
- Send a chat update and inspect it as another authorized user.
- Inspect a failed route or stopped service in the packet/event trace, correct the
  world/service state with an administrator interface, and retry the browser.

These examples demonstrate composition; objectives and success predicates do not
belong to the world kernel. All personal names, messages and organizations are
fictional fixture data.

## The wider web

The company sits on the [built-in internet](../internet), which every world joins unless
it sets `internet: false`: search engines, webmail, a video site, social feeds,
encyclopedias, forums, shops, an AI assistant and so on. `app-server` uplinks to it through
`edge-router`, and the company's own public sites — `northstar.example`,
`status.northstar.example`, `eng.northstar.example`, `guide.example` and the two staff
blogs — hang off its points of presence. Those, the `.internal` services and the home
speakers are the only sites under `sites/` here. The company's people and content on the
shared sites — Alice's inbox on `mail.google.com`, Northstar's repositories on `github.com`,
the team's posts, orders and videos — are its overlays under `overlays/`, one file per
site, named in `world.yml`'s `internet_overlays`.

## The directories

`world.yml` is the blueprint and `world.json` beside it is what it resolves to; the other
five directories are what the blueprint and the site build read.

| Directory | Read by | Becomes |
|---|---|---|
| `home/` | `world.yml`'s `copy:` blocks | The files the three desktops start with. `home/all` is overlaid with `home/macos`, `home/windows` or `home/ubuntu` per machine. |
| `sites/` | `world.yml`'s `include:` list | The company's own services, one per file, each placing its own node, link and DNS records. |
| `overlays/` | `world.yml`'s `internet_overlays:` | The company's own content on the internet's shared sites, merged into each as the internet joins. |
| `network/` | `world.yml`'s `include:` list | Topology with no service behind it — currently just the unplugged bedroom speaker. |
| `samples/` | `scripts/content/build-live-world.mjs` | The documents and media the live site's machines start with. |

`samples/` is the one that does not reach `world.json`. Its eight files are written by the
engines themselves rather than by hand — `Budget.xlsx` and `Sales.csv` by `cw-sheet`,
`Inventory.db` by `cw-sql`, the three `.apng` movies and two `.wav` sounds by `cw-video` —
so a spreadsheet a user opens in-world is real output of the spreadsheet engine and not a
prop. `crates/{sheet,sql,video}/tests/samples.rs` pin their bytes and regenerate them with
`CW_UPDATE_SAMPLES=1`; `scripts/content/build-live-world.mjs` base64-encodes them into each virtual
machine's `Documents/` and `Movies/` folders in `site/generated/world-definition.js`.

`cargo test -p computerworld --test internet_links` is the guard: it asks every declared
domain to answer, checks alternate domains serve the same page as their canonical one,
crawls every link two levels deep, and resolves every indexed search result. A dead
link in a world an agent is trained on is a silently wrong lesson, so that suite failing
is a release blocker.

Run `cargo run -p computerworld --example company` for the native actor workflow,
and `cargo test -p computerworld --test behaviors` for the cross-machine, browser,
Git, snapshot/replay and isolation behavior suite. The handbook's declared
`body_variants` select a release-review lead during deterministic service
initialization; this is task-relevant seed variation with stable references.
