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
speakers are the only services it runs, each a `service.json` under `services/<id>/`. The company's people and content on the
shared sites — Alice's inbox on `mail.google.com`, Northstar's repositories on `github.com`,
the team's posts, orders and videos — are its overlays, each an `overlay.json` in the
directory named for the site: `services/google-mail/overlay.json`, `services/github/overlay.json`.

## The directories

`world.yml` is the blueprint and `world.json` beside it is what it resolves to; the other
three directories are what the blueprint and the site build read.

| Directory | Read by | Becomes |
|---|---|---|
| `computers/` | `world.yml`'s `include:` list | One directory per machine: `computer.json` is its definition, and `root/` holds the files it starts with at the paths they have on it — `alice-mac/root/Users/alice`, `bob-windows/root/C/Users/bob` (the top directory is the drive), `carol-ubuntu/root/home/carol`. |
| `services/` | `world.yml`'s `include:` list | One directory per service. `service.json` is a service the company runs, placing its own node, link and DNS records; `overlay.json` is the company's content on the internet's service of that name, merged into it as the internet joins. |
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
