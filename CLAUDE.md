# Working in this repository

## Keep going

Work continuously to the end of what was asked. Do not stop to check in, ask
whether to proceed, or hand back a plan when the next step is clear — decide it
and do it. Ask only when two readings of the request would produce materially
different work, or when an action is destructive and was not authorised.

Where something turns out to be blocked, finish everything else in full and say
plainly at the end what was left and why.

## Commit each unit of work

Commit as soon as a unit of work is complete and verified — not at the end of a
session, and not in one commit covering several unrelated things. A unit is one
change that stands on its own: a fix with its test, an extraction that leaves
behaviour identical, one milestone of a port. Run the checks that cover it
first; a commit that has not been verified is not a unit of work.

Commit messages here are a declarative sentence, no prefix and no ticket, and a
body in prose that says what was wrong and what it is now. Numbers in the body
are measured, never estimated.

Commit; do not push unless asked.

## Verifying

Cargo runs one at a time and under a shared lock, because parallel builds have
OOMed this machine:

    export CARGO_BUILD_JOBS=4 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
    flock /home/brandonin/.claude/jobs/cargo.lock cargo test -p computerworld --features render

CI lints on Rust 1.98, so check `cargo +1.98.0 clippy` as well as stable. A long
run (`cargo test -p cw-web --features pipeline,wpt` is ~27 minutes) is built
under the lock with `--no-run` and then run from `target/debug/deps/` outside it,
so it does not block other work.

The site's Wasm bundle is not in git. After any engine change the site depends
on:

    bash scripts/build-wasm.sh && cp -r pkg/web/. site/pkg/

Browser work needs Playwright and Chrome from outside the project:

    PLAYWRIGHT_MODULE=/usr/lib/chatgpt/resources/cua_node/lib/node_modules/playwright/index.mjs
    CHROME_BIN=/usr/bin/google-chrome

`scripts/check-site.mjs` is the one thing in CI that opens the page: it asserts
that machines come up in both hosts the page can use (workers with an
OffscreenCanvas, and the tab), that a pointer over one machine redraws that
machine and none beside it, that fullscreen and retirement behave, and that a
page whose worker will not load falls back rather than spinning. Run it after
any change to `site/live.js`, `site/worker.js`, `site/engine.js` or `site/app.js`.

`scripts/render-site-stills.mjs` renders every machine's still by opening the
real site and letting the scenes boot; `--check` compares instead of writing,
and the nightly `stills` workflow runs it over the whole cast, because the
pictures drift silently as the engine's rendering changes.
`scripts/serve-site.mjs` serves `site/` with the media types the bundle needs.

## Profile before fixing

Hot paths here have repeatedly been slow for a reason nobody guessed: a browser
digest re-hashing each tab's history twice per action, a renderer diffing one
machine's screen against another's. Measure first, and put the number in the
commit message. When changing an engine hot path, run the determinism corpus
(`crates/computerworld/tests/determinism_corpus.rs`) — state hashes must come
out byte-identical.
