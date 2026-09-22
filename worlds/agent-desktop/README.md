# Agent desktop

One Ubuntu workstation for one agent: a user, the everyday applications, two seeded
documents under `home/`, and an intranet wiki to browse. It is the middle size — bigger
than [`unrelated-lab`](../unrelated-lab), which has no network at all, and small enough
that a determinism run over it finishes in seconds where the
[reference company](../company-2026) would not.

`world.yml` is the blueprint and `world.json` beside it is generated, checked into git and
compared byte-for-byte by `cargo test -p cw-blueprint`:

    cargo run -p cw-blueprint --bin cw-world -- build worlds/agent-desktop/world.yml

`${AGENT_USER}` is a declared input: `--set AGENT_USER=noor` builds the same world under a
different name. [Blueprints](../../docs/blueprint.md) describes the format; the world is
the worked example in [writing a world of your own](../../docs/custom-world.md).

It appears in `cargo test -p computerworld --test agent_desktop` and as two of the
scenarios in the determinism corpus, where its state hashes are pinned per seed.
