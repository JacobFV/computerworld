# Development commands

Run the common entry points from any directory:

| Command from the repository root | Purpose |
| --- | --- |
| `bash scripts/test-all.sh` | Workspace tests, isolation checks, lint, Wasm and generated content |
| `bash scripts/build-wasm.sh` | Browser and Node bindings in ignored `pkg/` |
| `bash scripts/build-content.sh` | World definitions, search indexes and the site's generated world |
| `bash scripts/smoke-bindings.sh` | Cross-language binding checks |

Other tools are grouped by responsibility:

- [site/](site/): documentation build, local server, browser checks and still generation.
- [content/](content/): generators used by `build-content.sh`.
- [release/](release/): npm packaging, release archives and release smoke checks.
- [checks/](checks/): simulation isolation, bindings smoke checks and policy verification.
- [dom-to-site/](dom-to-site/README.md): capture, convert and compare a source page.
- [web-parity/](web-parity/README.md): web engine comparison fixtures and tooling.

Examples: `node scripts/site/serve-site.mjs 8000`,
`node scripts/site/build-docs.mjs --check`, and
`python3 scripts/checks/check-boundaries.py`.

A nested script resolves the repository from its own location; caller-supplied
input/output paths retain the semantics documented by that command. Build output
belongs in ignored `target/`, `pkg/` or `site/docs/`, not beside these tools.
