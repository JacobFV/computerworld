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

Beyond the company's own `.internal` services, the world declares a synthetic public
internet on recognisable domains — search engines, webmail, a video site, social
feeds, encyclopedias, forums, shops, an AI assistant and so on. Every one is an
ordinary service reached through the same DNS, routes and HTTP an actor already uses;
none of them touches the host network.

**Addresses are fictional even where names are not.** Every node sits in a range IANA
reserves for documentation — `203.0.113.0/24`, `198.51.100.0/24`, `192.0.2.0/24` — so
no simulated host resembles a real one. Alternate spellings are DNS aliases pointing
at the same node, exactly as they are on the real internet.

Internet nodes are reached through an edge router and three points of presence, so
latency is realistically higher than the LAN's 10 µs: a few milliseconds to the
company's own site, more to a distant one.

### The sites

Every site is a seed file under `sites/`, grouped here by the service kind that renders it
(the variant in parentheses is its `mode`, `layout` or `skin`). Alternate spellings
(`www.`, `m.`, `youtu.be`, `twitter.com`, `youtubemusic.com`, …) are extra `domains` on the
same seed.

| Kind | Sites |
|---|---|
| assistant | chatgpt.com, claude.ai |
| bank | northwind.example, paypal.com |
| calendar (gcal) | calendar.google.com |
| chat / slack / discord / messages | chat.internal, slack.com, discord.com, messages.internal |
| docs (gdocs, plain) | docs.google.com, notion.so |
| drive (gdrive, dropbox) | drive.google.com, dropbox.com |
| forum (subreddits) | reddit.com, yelp.com |
| forum (qa) | stackoverflow.com, quora.com |
| forum (linkfeed) | news.ycombinator.com, craigslist.org |
| geo (maps, weather) | maps.google.com, openstreetmap.org, weather.com |
| git (github, plain) | github.com, gitlab.com |
| issues (linear) | linear.app |
| mail (gmail, outlook, plain) | mail.google.com, outlook.com, mail.com |
| media (video) | youtube.com, tiktok.com, twitch.tv, vimeo.com, netflix.com |
| media (audio, music) | spotify.com, soundcloud.com, music.youtube.com |
| press (wire) | reuters.com, bbc.com, news.google.com |
| press (magazine) | theverge.com, arstechnica.com, nytimes.com, cnn.com |
| press (blog) | alicechen.dev, bmartinez.net, eng.northstar.example, medium.com, substack.com |
| search (google, bing, ddg) | google.com, bing.com, duckduckgo.com |
| shop (retail) | amazon.com, etsy.com, ebay.com, booking.com, airbnb.com, uber.com, doordash.com |
| shop (tickets) | ticketmaster.com |
| social (microblog) | x.com, mastodon.social, facebook.com, bsky.app |
| social (photos) | instagram.com, pinterest.com |
| social (professional) | linkedin.com |
| speaker | kitchen, livingroom and office-tv `.speaker.internal` (cast targets; `bedroom.speaker.internal` is declared in DNS but unplugged: nothing listens) |
| static-site | intranet.internal, northstar.example, status.northstar.example, guide.example, apple.com, microsoft.com, zoom.us, whatsapp.com, figma.com, cloudflare.com, aws.amazon.com, stripe.com, npmjs.com, pypi.org, crates.io, docs.rs |
| wiki | wikipedia.org, imdb.com, archive.org |

### Adding or editing a site

Seed data lives in one file per site under `worlds/company-2026/sites/<service-id>.json`,
never directly in `world.json`. The basename must equal the `id`.

```json
{
  "id": "theverge", "kind": "press", "node": "theverge",
  "domains": ["theverge.com", "www.theverge.com"], "port": 80,
  "initial_state": { "layout": "magazine", "brand": "The Verge" },
  "search_entries": [
    {"url": "http://theverge.com/2026/atlas-determinism", "title": "…",
     "snippet": "…", "vertical": "news", "authority": 6, "keywords": ["atlas"]}
  ]
}
```

`search_entries` is the site's sitemap: every search engine seeded without `documents`
indexes all of them when the world boots, filing each under the site's first domain and
applying its own `authority_overrides`. That is what makes a search result a link that
really resolves. Its `vertical` is one of the
engines' `all`, `news`, `videos` or `images`. `place` lets a site bring its own host on a
documentation-range address, and derives the node, the link and a DNS A record for every
domain the site declares, so no name depends on the runtime's auto-add — see
[world blueprints](../../docs/blueprint.md). In `world.json` a service that came from its
own file is written on one line — the seed file is the reviewable copy — while the
hand-written computers, network and `.internal` services stay pretty-printed.
`scripts/build-content.sh` runs the index, the world build and the live site world's
regeneration, and is idempotent.

## The directories

`world.yml` is the blueprint and `world.json` beside it is what it resolves to; the other
four directories are what the blueprint and the site build read.

| Directory | Read by | Becomes |
|---|---|---|
| `home/` | `world.yml`'s `copy:` blocks | The files the three desktops start with. `home/all` is overlaid with `home/macos`, `home/windows` or `home/ubuntu` per machine. |
| `sites/` | `world.yml`'s `include:` list | One service per file, each placing its own node, link and DNS records. |
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
