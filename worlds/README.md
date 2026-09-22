# Worlds

A world is data, not code: one JSON definition the loader consumes, with whatever
directories of seed content its blueprint copies in. The kernel holds no defaults from
any of them, which is why there is more than one here.

| World | Size | What it is for |
|---|---|---|
| [`company-2026`](company-2026) | 5 machines, 3 OS families, ~100 sites | The reference ecosystem. Compiled into the library, and what the site, the examples and most tests run against. |
| [`agent-desktop`](agent-desktop) | 1 machine, 1 intranet wiki | One desktop an agent can use. The blueprint worked example, and the fast half of the determinism corpus. |
| [`unrelated-lab`](unrelated-lab) | 1 machine, no network | Proof that nothing reference-world is baked into the kernel, and the smallest world to copy. |

Each holds its own `world.json`. Where a world has a `world.yml` beside it, the JSON is
generated from that blueprint and is not edited by hand; `scripts/build-content.sh`
rebuilds every one of them, and `cargo test -p cw-blueprint` fails if a checked-in
`world.json` has drifted from its blueprint.

To write your own, start from [writing a world of your own](../docs/custom-world.md) and
the [world schema](../docs/world-schema.md).
