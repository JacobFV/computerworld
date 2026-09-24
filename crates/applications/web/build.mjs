// Builds the desktop applications written as web apps: type-checks every source,
// then bundles each app's entry into `<app>/<app>.bundle.js`, which is checked in
// beside it and compiled into the simulator (`crates/applications/src/web_app`).
//
//   node crates/applications/web/build.mjs          # build every app
//   node crates/applications/web/build.mjs --check  # fail if a bundle is stale
//
// React is not bundled: the host loads React 18's production build first
// (`vendor/`), and `react` / `react-dom/client` resolve to the globals it defines.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const check = process.argv.includes("--check");
const APPS = [{ name: "notes", entry: "notes/Notes.tsx" }];

if (!existsSync(join(here, "node_modules", "esbuild"))) {
  execFileSync("npm", ["install", "--no-audit", "--no-fund", "--loglevel=error"], {
    cwd: here,
    stdio: "inherit",
  });
}
execFileSync(join(here, "node_modules", ".bin", "tsc"), ["-p", join(here, "tsconfig.json")], {
  cwd: here,
  stdio: "inherit",
});

const esbuild = await import(join(here, "node_modules", "esbuild", "lib", "main.js"));
let stale = [];
for (const app of APPS) {
  const result = await esbuild.build({
    entryPoints: [join(here, app.entry)],
    bundle: true,
    write: false,
    format: "iife",
    target: "es2020",
    jsx: "transform",
    jsxFactory: "React.createElement",
    jsxFragment: "React.Fragment",
    minify: false,
    legalComments: "none",
    charset: "utf8",
    alias: {
      react: join(here, "sdk", "react.ts"),
      "react-dom/client": join(here, "sdk", "react-dom-client.ts"),
    },
    banner: { js: `// Built by crates/applications/web/build.mjs from ${app.entry}. Do not edit.` },
  });
  const out = join(here, app.name, `${app.name}.bundle.js`);
  const text = result.outputFiles[0].text;
  const before = existsSync(out) ? readFileSync(out, "utf8") : "";
  if (check) {
    if (before !== text) stale.push(out);
  } else if (before !== text) {
    writeFileSync(out, text);
    console.log(`wrote ${out} (${text.length} bytes)`);
  } else {
    console.log(`${out} is current`);
  }
}
if (stale.length) {
  console.error(`stale bundles (run node crates/applications/web/build.mjs):\n${stale.join("\n")}`);
  process.exit(1);
}
