# World and topology schema

The authoritative Rust schema is [`cw-protocol`](../crates/core/protocol/src/lib.rs).
JSON uses `schema_version: 1`; reject unsupported versions rather than silently
interpreting them. `WorldDefinition::from_json` parses and validates the definition.

| Field | Meaning |
|---|---|
| `id` | Nonempty world identity |
| `profiles` | OS definitions: `id`, `name`, `family`, `home`, `case_sensitive`, `shell` |
| `computers` | Machine `id`, `profile`, `address`, `user`, optional `node`, initial files, installed applications and packages, and an optional `presentation` (`desktop`, `laptop`, `phone`, `server`) |
| `network` | Nodes, links, DNS records, routes, explicit `implicit_lan` and gateway policy |
| `services` | Instance `id`, registered `kind`, placement `node`, domains, `port` (80), `tls` (default `true`: a second listener on 443 answers `https://`), initial state, and `search_entries`, the pages it offers the world's search engines |
| `internet` | Default `true`: `World::new` joins the [built-in internet](../worlds/internet) — its public sites, backbone and DNS — to the world, and the world may name the internet's five `INTERNET_ATTACHMENTS` (`edge-router`, `pop-west`, `pop-east`, `pop-eu`, `dns-public`) before it has. What the world declares itself wins. `false` is a world exactly as declared, and the only kind a build without `computerworld`'s `internet` feature boots |
| `internet_overlays` | By internet site id, the world's own content on that shared site: `initial_state` merged into the site's as a JSON merge patch (objects key by key; any other value, a list included, restated whole), and `search_entries` and `domains` appended. An overlay for a site the world does not join is an error |
| `metadata` | Owner-side JSON; not an actor observation channel. Two keys are load-bearing: `desktop_themes` maps a computer to an OS shell, and `desktop_apps` declares its application catalog. Both change what an actor sees and can launch — see [desktop GUI](desktop-gui.md). `device_presentations` maps a computer to `desktop`, `laptop`, `phone` or `server` (a computer's own `presentation` wins): laptops and phones show a battery, desktop computers and servers none. A phone shell is always a phone; a computer nothing is said about is a desktop computer. `search_engine` is the template the browser's address bar sends searches to (`%s` is the urlencoded query, default `https://google.com/search?q=%s`) — see [action families](action-families.md#the-omnibox). |

A computer's node defaults to its machine ID. Explicit nodes have an address and
zone (`local`, `internet`, `host`). Links are required by default; `network.implicit_lan: true` explicitly enables
implicit local reachability. A link names `from`/`to`, direction, logical
`latency_us` and `loss_per_million`. DNS records name an IP address or CNAME target, TTL in logical
microseconds, and optional resolver node. Service domains resolve to their
placement; clients still need reachable routes and listeners.

Validation checks duplicate identities, profile and node references, address
syntax and collisions, listener collisions, DNS definitions and link parameters.
The runtime also validates registered service kinds. OS profiles describe
synthetic path/shell behavior; they do not boot the corresponding operating system.

Definitions should keep all fixtures and initial state explicit. The seed can
vary initialization where an implementation uses its deterministic context; it is
not permission to draw host randomness. Runtime state, portable snapshots and
world blueprints are different artifacts: a blueprint constructs a world, while
a snapshot resumes one.

See [creating a world](custom-world.md) and the
[company blueprint](../worlds/company-2026/world.json).

`worlds/company-2026/world.json` is **generated**, not hand-written: it is resolved from
[`worlds/company-2026/world.yml`](../worlds/company-2026/world.yml) by `cw-world`, which
merges in `sites/*.json`, seeds the desktops from `home/` and derives each service's node,
link and DNS records; the search engines index the sites' `search_entries` themselves when
the world boots, and `scripts/build-content.sh` runs the pipeline. Edit the blueprint, not the
output; `scripts/test-all.sh` fails if the two have drifted.

A world of your own may be written either way. This document describes the definition,
which is plain JSON; [world blueprints](blueprint.md) describe the source form it can be
generated from — YAML, declared inputs, included fragments and copied directories.
