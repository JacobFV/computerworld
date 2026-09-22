# Generated site content

`world-definition.js` is generated from the reference world by
`scripts/content/build-live-world.mjs`. Run `bash scripts/build-content.sh` from the
repository root to rebuild the world, its indexes and this derived module.

The module is intentionally committed so the static site has its world data in a
fresh checkout. Edit the blueprints and their inputs under `worlds/`, not this file.
The generated-content check in `scripts/test-all.sh` rejects drift.
