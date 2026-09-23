# World blueprints

A [world definition](world-schema.md) is a hermetic artifact. It is embedded in
every native and Wasm build, its bytes are hashed by the determinism corpus, and a
snapshot refuses to load into a world whose definition differs. That is what makes
a world reproducible — and it is also what makes one miserable to write by hand.
The reference company is three and a half megabytes of JSON.

A **blueprint** is the same document with four source-only conveniences, resolved
away before anything runs. `cw-world` reads the blueprint and writes the world:

    cargo run -p cw-blueprint --bin cw-world -- build worlds/company-2026/world.yml

The result is an ordinary world definition. Nothing downstream knows a blueprint
was involved.

## Writing one

A blueprint is YAML or JSON, and every key the world schema takes is a key a
blueprint takes. A world that needs none of the conveniences below is already a
blueprint — rename it and it builds unchanged.

```yaml
schema_version: 1
id: agent-desktop

inputs:
  AGENT_USER:
    default: ada
    description: the workstation's user

profiles:
  - id: ubuntu
    name: Ubuntu 24.04 workstation
    family: linux
    home: /home/{user}
    shell: posix

computers:
  - id: workstation
    profile: ubuntu
    address: 10.0.0.10
    user: ${AGENT_USER}
    copy:
      - from: home
    installed_apps: [terminal, browser, files, editor]

services:
  - id: wiki
    kind: wiki
    node: intranet-server
    domains: [wiki.internal]
    initial_state: {from_file: wiki/state.json}
    place:
      address: 10.0.1.10
      zone: local
      link: {from: workstation, latency_us: 10}
      dns:
        local: {ttl_us: 60000000, resolver: intranet-server}
```

Every path is relative to the blueprint's own directory and may not climb out of
it, so a world is built from the files that travel with it and from nothing else.
A symbolic link is refused rather than followed.

## `copy:` — a machine seeded from real files

`initial_files` is the right shape for the engine and the wrong shape for a
person: a seeded home folder written that way is a JSON string literal with `\n`
in it, undiffable and unopenable. `copy:` names directories instead.

```yaml
    copy:
      - from: home/all
      - from: home/ubuntu
        to: "~"
        exclude: ["*.tmp"]
```

Entries overlay in order, so `home/all` then `home/ubuntu` reads the way it is
written: where both hold the same path, the second wins. `to:` defaults to `~`,
the user's home folder, which is where a relative `initial_files` path already
resolves; an absolute `to:` such as `/etc` seeds an absolute path. A file whose
bytes are valid UTF-8 lands in `initial_files`, and anything else is base64 in
`initial_binary_files`. `.DS_Store`, `.gitkeep` and `.git` are never seeded.

A file stated in the blueprint beats one copied in bulk, so an exception does not
have to be deleted from the directory to be stated. Seeded paths are sorted, and
the whole set takes the position the `copy:` block held, so the resolved computer
reads in the order it was written.

There is a ceiling of 3 MiB of seeded bytes per computer, because the world file
rides in every build. Raise it deliberately:

```yaml
limits:
  bytes_per_computer: 8388608
```

### Why the copy happens at build time

It would be more convenient to copy files in after a machine boots. It would also
be wrong in three separate ways, and each of them is silent:

* `Runtime::reset` restores the definition's baseline, so files written after
  construction vanish at the first reset.
* Snapshot import guards on definition equality. Files outside the definition make
  two unlike worlds compare equal, and a snapshot from one loads into the other
  without complaint.
* The Wasm build has no filesystem to copy from, so a world that seeded itself
  that way could never run in the browser.

Folding the bytes into the definition is what keeps all three honest.

## `include:` and `extends:` — composing documents

`include:` merges fragments after the document's own declarations. `extends:`
loads another blueprint first and merges this one over it. Both obey one rule: a
later contribution replaces an earlier one, in the position the earlier one held.
Position matters because the world file is checked in — appending a service rather
than replacing it in place would reorder every service after it.

```yaml
extends: base/world.yml
include:
  - sites/*.json
  - devices/*.yml
```

A pattern may use `*` (within one path segment), `**` (across segments) or `?`;
matches are sorted, so a glob builds the same world twice. A pattern that matches
nothing is an error, as is a cycle.

Lists merge by identity where they have one — `profiles`, `computers`, `services`
and `network.nodes` by `id`, `network.dns` by `name`, `network.links` and
`network.routes` by their `from`/`to` pair. Every other list is replaced whole, so
`installed_apps: [terminal]` means those applications and not those on top of
whatever was there.

A fragment is either a whole world document or **one service on its own**,
recognised by its `kind`, which no world document has. That is what lets a
directory of service files be a directory of service files.

## `from_file:` — a value that lives in its own file

Anywhere a value belongs, `{from_file: <path>}` becomes that file's contents:

```yaml
    initial_state: {from_file: wiki/state.json}
```

Substitution does not reach inside a loaded file: data is data.

## `place:` — a service's node, link and records

Stating a service's placement three times is what makes a large world unwritable.
The reference company has ninety-three services and paid for them with a hundred
and two nodes, a hundred and two links and two hundred and thirty DNS records,
none of which said anything the service had not already implied.

```yaml
defaults:
  place:
    zone: internet
    link: {latency_us: 1500}
    dns:
      internet: {ttl_us: 300000000, resolver: dns-public}
      local: {ttl_us: 60000000, resolver: app-server}
```

Each service then says only what is its own:

```yaml
  - id: airbnb
    kind: shop
    node: airbnb
    domains: [airbnb.com, www.airbnb.com]
    place:
      address: 198.51.100.112
      link: {from: pop-west}
```

which adds the node, the link from `pop-west`, and an address record for each
domain with the TTL and resolver its zone declares. A record stated by hand wins:
an alias to another name is exactly what a derived address record would destroy.
Omit `link:` and no link is made. DNS records come out in name order, so adding a
service moves one line rather than appending to a list nobody can scan.

## `inputs:` — the environment, declared

A blueprint may not reach into the ambient environment. Every variable it
substitutes is declared up front, so `cw-world inputs` can say what a world needs
before it needs it, and so nothing depends on the machine that built it.

```yaml
inputs:
  COMPANY_DOMAIN:
    default: northstar.example
    description: the domain the intranet answers on
  REGION:
    default: us
    one_of: [us, eu]
  BUILD_TAG:
    optional: true
  FIXTURE_ROOT:
    secret: true
```

`${NAME}` and `${NAME:-fallback}` substitute into any string, keys included.
Values come from the environment, or from `--set NAME=VALUE`, which wins.

* An **undeclared** name is an error, and the message lists what is declared.
* A declared name with neither a value nor a `default:` is an error, and every
  unsatisfied input is reported at once — being told about one missing variable
  per run is how a five-variable world takes five runs to build.
* An `optional:` input may be unset, and then every `${NAME}` reading it must
  carry its own fallback. Substituting an empty string is how a world quietly
  builds wrong.
* A `secret:` input may never reach the resolved world. It is usable in a source
  path such as a `copy.from`, and refused anywhere it would be written into the
  artifact — a host token baked into an actor-visible file is the leak this
  prevents.
* `$${` is a literal `${`. Beyond that, `${...}` substitutes only when what it
  wraps is a name or a name with a fallback, so a seeded workflow's
  `${{ matrix.os }}` and a shell script's `${1}` survive unchanged.

Only the blueprint the build was pointed at declares inputs; a fragment may use
them. One file therefore tells you everything a world needs to be built.

## `cw-world`

```
cw-world build     <blueprint>   resolve it and write the world definition
cw-world check     <blueprint>   resolve it and verify the written world is current
cw-world validate  <blueprint>   resolve it and report, writing nothing
cw-world inputs    <blueprint>   list the inputs it declares
```

`-o <path>` chooses where to write (default: `world.json` beside the blueprint),
`--root <dir>` chooses the directory paths resolve against (default: the
blueprint's own directory), and `--set NAME=VALUE` supplies an input.

`build` and `validate` both run the engine's own validation — duplicate
identities, unknown profiles, address and listener collisions, DNS shape — so a
blueprint fails at build time rather than at `World::new`. An unknown key is
refused too, and the message names the key it was probably meant to be: a
blueprint that silently drops a misspelled `intial_files` is worse than no schema.

`check` is what CI runs, through `scripts/test-all.sh`: a hand-edited `world.json`
would be overwritten by the next build, so the drift is caught instead.

## Two worked examples

[`worlds/agent-desktop/world.yml`](../worlds/agent-desktop/world.yml)
is one machine, one service and one input, and builds
[`worlds/agent-desktop/world.json`](../worlds/agent-desktop/world.json).

[`worlds/company-2026/world.yml`](../worlds/company-2026/world.yml) is the
reference company: six hundred lines that resolve to the three-and-a-half-megabyte
world file beside it. Three desktops seeded from `home/`, eighty-seven services
each in its own file under `sites/`, and forty-six of its hundred and two nodes,
forty-six of its hundred and two links and a hundred and eighteen of its two
hundred and thirty DNS records derived from a `place:` block rather than written.
