// Builds the desktop applications written as web apps: type-checks every source
// against `types/cw.d.ts` and React's types, then compiles each app with cw-tsx
// (crates/web/tsx) into `<app>/<app>.js`, the script React 18's production build
// runs, and — when the app is inside the compiled subset — `<app>/<app>.ui.json`,
// the IR cw-ui runs without a VM. `<app>/<app>.diagnostics.json` says why an app is
// not compiled. All three are checked in beside the source and compiled into the
// simulator (`crates/applications/src/web_app`).
//
//   node crates/applications/web/build.mjs          # build every app
//   node crates/applications/web/build.mjs --check  # fail if an output is stale
//
// cw-tsx is run with `cargo run` unless CW_TSX names a built binary.
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = join(here, "..", "..", "..");
const check = process.argv.includes("--check");
const APPS = [{ name: "notes", entry: "notes/Notes.tsx" }];

if (!existsSync(join(here, "node_modules", "typescript"))) {
  execFileSync("npm", ["install", "--no-audit", "--no-fund", "--loglevel=error"], {
    cwd: here,
    stdio: "inherit",
  });
}
execFileSync(join(here, "node_modules", ".bin", "tsc"), ["-p", join(here, "tsconfig.json")], {
  cwd: here,
  stdio: "inherit",
});

function cwTsx(args) {
  const [cmd, pre] = process.env.CW_TSX
    ? [process.env.CW_TSX, []]
    : ["cargo", ["run", "-q", "-p", "cw-tsx", "--bin", "cw-tsx", "--"]];
  try {
    // Diagnostics go to the checked-in `<app>.diagnostics.json`, not the terminal.
    execFileSync(cmd, [...pre, ...args], { cwd: repo, stdio: ["ignore", "ignore", "ignore"] });
    return 0;
  } catch (e) {
    return e.status;
  }
}

const stale = [];
for (const app of APPS) {
  const out = mkdtempSync(join(tmpdir(), "cw-web-app-"));
  // 0: script and IR; 3: the script only (the app is outside the compiled subset).
  const status = cwTsx(["build", join(here, app.entry), "-o", out, "--name", app.name]);
  if (status !== 0 && status !== 3) throw new Error(`cw-tsx could not build ${app.entry}`);
  const diagnostics = JSON.parse(readFileSync(join(out, `${app.name}.diagnostics.json`), "utf8"));
  console.log(
    status === 0
      ? `${app.name}: compiled for cw-ui, with its React fallback`
      : `${app.name}: runs on React (${diagnostics.length} diagnostics keep it outside the compiled subset)`,
  );
  for (const file of [`${app.name}.js`, `${app.name}.ui.json`, `${app.name}.diagnostics.json`]) {
    const built = existsSync(join(out, file)) ? readFileSync(join(out, file), "utf8") : null;
    const target = join(here, app.name, file);
    const before = existsSync(target) ? readFileSync(target, "utf8") : null;
    if (built === before) continue;
    if (check) {
      stale.push(target);
    } else if (built === null) {
      rmSync(target);
      console.log(`removed ${target}`);
    } else {
      writeFileSync(target, built);
      console.log(`wrote ${target} (${built.length} bytes)`);
    }
  }
  rmSync(out, { recursive: true });
}
if (stale.length) {
  console.error(`stale outputs (run node crates/applications/web/build.mjs):\n${stale.join("\n")}`);
  process.exit(1);
}
