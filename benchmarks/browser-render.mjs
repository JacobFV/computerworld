/** Browser-only comparison. Both capture paths return PNG from the same browser.
 * No PNG screenshot / raw RGBA ratio is reported. */
import { createServer } from "node:http";
import { readFile, writeFile, stat } from "node:fs/promises";
import { resolve, extname, join } from "node:path";
import { pathToFileURL } from "node:url";
import { performance } from "node:perf_hooks";
import { execFileSync } from "node:child_process";
const root = resolve(".");
const { chromium } = await import(
  pathToFileURL(
    process.env.PLAYWRIGHT_MODULE ||
      "/tmp/computerworld-browser-tools/node_modules/playwright/index.mjs",
  )
);
const source =
  process.env.PREDECESSOR_ROOT ||
  "/tmp/computerworld-sources/synthux-mail-mock";
const { renderFolder } = await import(
  pathToFileURL(join(source, "src/render.mjs"))
);
const mail = renderFolder("inbox");
const server = createServer(async (req, res) => {
  res.setHeader("Cross-Origin-Opener-Policy", "same-origin");
  res.setHeader("Cross-Origin-Embedder-Policy", "require-corp");
  try {
    const pathname = new URL(req.url, "http://local").pathname;
    if (pathname === "/mail") {
      res.setHeader("Content-Type", "text/html");
      res.end(mail);
      return;
    }
    const path = resolve(root, "." + pathname);
    if (!path.startsWith(root + "/")) throw Error();
    res.setHeader(
      "Content-Type",
      { js: "text/javascript", wasm: "application/wasm", ttf: "font/ttf" }[
        extname(path).slice(1)
      ] || "application/octet-stream",
    );
    res.end(await readFile(path));
  } catch {
    res.statusCode = 404;
    res.end();
  }
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const origin = `http://127.0.0.1:${server.address().port}`;
const browser = await chromium.launch({
  executablePath: process.env.CHROME || "/usr/bin/google-chrome",
  headless: true,
  args: [
    "--no-sandbox",
    "--disable-gpu",
    "--disable-background-timer-throttling",
  ],
});
const page = await browser.newPage({
  viewport: { width: 1280, height: 720 },
  deviceScaleFactor: 1,
});
const results = [];
const runs = Number(process.env.BENCH_RUNS || 5),
  count = Number(process.env.BENCH_SAMPLES || 1000);
function record(name, run, samples) {
  const a = [...samples].sort((a, b) => a - b);
  results.push({
    name,
    run,
    count: a.length,
    p50_ns: a[Math.floor(a.length * 0.5)],
    p95_ns: a[Math.floor(a.length * 0.95)],
    mean_ns: a.reduce((a, b) => a + b, 0) / a.length,
    samples_ns: samples,
  });
}
try {
  await page.goto(origin + "/mail");
  await page.locator(".mail-row").first().waitFor();
  await page.addStyleTag({
    content: `@font-face{font-family:fixture;src:url('${origin}/crates/render/assets/DejaVuSansMono.ttf')}*{font-family:fixture!important;font-size:14px!important;font-weight:400!important;letter-spacing:0.571289px!important}`,
  });
  await page.evaluate(() => document.fonts.ready);
  const mailSemantic = await page.locator(".mail-row").allTextContents();
  for (let run = 0; run < runs; run++) {
    const samples = [];
    for (let i = 0; i < Math.min(count, 30) + 3; i++) {
      const t = performance.now();
      await page.evaluate((i) => {
        const row = document.querySelector(".mail-row");
        row.classList.toggle("unread", i % 2 === 0);
        row.classList.toggle("read", i % 2 !== 0);
      }, i);
      await page.screenshot({ type: "png" });
      if (i >= 3) samples.push((performance.now() - t) * 1e6);
    }
    record("predecessor.mail_update_png_capture", run, samples);
  }
  await page.screenshot({ path: "benchmarks/results/predecessor-mail.png" });
  const migratedMail = await page.evaluate(() => {
    const nodes = [];
    let id = 0;
    function color(s) {
      const n = s.match(/[\d.]+/g) || [];
      return [
        +(n[0] || 255),
        +(n[1] || 255),
        +(n[2] || 255),
        Math.round((n.length > 3 ? +n[3] : 1) * 255),
      ];
    }
    for (const e of document.body.querySelectorAll("*")) {
      const r = e.getBoundingClientRect(),
        style = getComputedStyle(e);
      if (r.width <= 0 || r.height <= 0) continue;
      let bg = color(style.backgroundColor);
      if (bg[3])
        nodes.push({
          id: id++,
          bounds: {
            x: Math.round(r.x),
            y: Math.round(r.y),
            width: Math.round(r.width),
            height: Math.round(r.height),
          },
          primitive: { kind: "box", fill: bg, border: null, border_width: 0 },
        });
    }
    const walk = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
    while (walk.nextNode()) {
      const t = walk.currentNode;
      if (
        !t.textContent.trim() ||
        ["STYLE", "SCRIPT"].includes(t.parentElement.tagName)
      )
        continue;
      const st = getComputedStyle(t.parentElement);
      for (let offset = 0; offset < t.textContent.length; offset++) {
        const ch = String.fromCodePoint(t.textContent.codePointAt(offset));
        if (!ch.trim()) continue;
        const range = document.createRange();
        range.setStart(t, offset);
        range.setEnd(t, offset + ch.length);
        offset += ch.length - 1;
        const r = range.getBoundingClientRect();
        if (r.width <= 0 || r.height <= 0 || r.y >= 720 || r.bottom <= 0)
          continue;
        nodes.push({
          id: id++,
          bounds: {
            x: Math.round(r.x),
            y: Math.round(r.y),
            width: 10,
            height: 20,
          },
          primitive: {
            kind: "text",
            text: ch,
            color: color(st.color),
            size: 14,
          },
        });
      }
    }
    const row = document.querySelector(".mail-row").getBoundingClientRect();
    const mutationIndex = nodes.findIndex(
      (n) =>
        n.primitive.kind === "box" &&
        n.bounds.x === Math.round(row.x) &&
        n.bounds.y === Math.round(row.y) &&
        n.bounds.width === Math.round(row.width),
    );
    if (mutationIndex < 0) throw Error("first mail row missing");
    return {
      width: 1280,
      height: 720,
      revision: 0,
      background: [255, 255, 255, 255],
      nodes,
      mutationIndex,
    };
  });

  // Separate primitive isolation: identical content, geometry and font source.
  await page.goto(origin + "/mail");
  await page.setContent(
    `<style>@font-face{font-family:fixture;src:url('${origin}/crates/render/assets/DejaVuSansMono.ttf')}body{margin:0;background:white}.row{position:absolute;height:22px;width:320px;font:16px/20px fixture;white-space:pre;overflow:hidden}</style><div id="dom"></div><canvas id="canvas" width="1280" height="720" style="display:none"></canvas>`,
  );
  await page.evaluate(async (origin) => {
    const wasm = await import(origin + "/pkg/web/computerworld.js");
    window.wasm = wasm;
    window.wasmMemory = (await wasm.default()).memory;
    window.renderer = new wasm.SceneRenderer();
    window.scene = {
      width: 1280,
      height: 720,
      revision: 0,
      background: [255, 255, 255, 255],
      nodes: [],
    };
    for (let i = 0; i < 100; i++) {
      let x = (i % 4) * 320,
        y = Math.floor(i / 4) * 22,
        text = `Computer row ${String(i).padStart(3, "0")}`;
      scene.nodes.push({
        id: i,
        bounds: { x, y, width: 320, height: 22 },
        primitive: { kind: "text", text, color: [0, 0, 0, 255], size: 16 },
      });
      const d = document.createElement("div");
      d.className = "row";
      d.style.left = x + "px";
      d.style.top = y + "px";
      d.textContent = text;
      document.querySelector("#dom").append(d);
    }
    await document.fonts.ready;
    window.ctx = document.querySelector("#canvas").getContext("2d");
    window.draw = () => {
      const frame = renderer.render(scene);
      ctx.putImageData(
        new ImageData(new Uint8ClampedArray(frame.rgba), 1280, 720),
        0,
        0,
      );
      frame.free();
    };
    draw();
    window.change = (i, n) => {
      for (let j = 0; j < n; j++) {
        scene.nodes[j].primitive.text = `Version ${String(i).padStart(5, "0")}`;
      }
    };
  }, origin);
  const worldDefinition = JSON.parse(
    await readFile("worlds/company-2026/world.json", "utf8"),
  );
  const browserWorldResults = await page.evaluate(
    ({ definition, count, runs }) => {
      let output = [];
      let initialMemory = wasmMemory.buffer.byteLength;
      for (let run = 0; run < runs; run++) {
        const start = performance.now();
        const world = new wasm.World(definition, 2026);
        const createNs = (performance.now() - start) * 1e6;
        const env = world.environment({
          actor: "alice",
          machines: ["alice-mac"],
          actions: ["terminal.v1"],
          observations: ["terminal.v1"],
        });
        let samples = [];
        for (let i = 0; i < count + 100; i++) {
          const t = performance.now();
          const r = env.step([
            {
              family: "terminal.v1",
              op: "execute",
              machine: "alice-mac",
              payload: { command: "pwd" },
            },
          ]);
          if (!r.outcomes[0].success) throw Error("wasm actor failed");
          if (i >= 100) samples.push((performance.now() - t) * 1e6);
        }
        output.push({ run, samples, createNs });
        env.free();
        world.free();
      }
      return {
        output,
        initialMemory,
        finalMemory: wasmMemory.buffer.byteLength,
      };
    },
    { definition: worldDefinition, count, runs },
  );
  for (const result of browserWorldResults.output)
    record("browser_wasm.terminal_step", result.run, result.samples);
  for (const n of [1, 10, 100])
    for (let run = 0; run < runs; run++) {
      const data = await page.evaluate(
        ({ n, count }) => {
          const dom = document.querySelector("#dom");
          dom.style.display = "block";
          document.querySelector("#canvas").style.display = "none";
          const children = [...dom.children];
          let samples = [];
          for (let i = 0; i < count + 100; i++) {
            const t = performance.now();
            for (let j = 0; j < n; j++)
              children[j].textContent = `Version ${String(i).padStart(5, "0")}`;
            for (let j = 0; j < n; j++) children[j].getBoundingClientRect();
            if (i >= 100) samples.push((performance.now() - t) * 1e6);
          }
          return samples;
        },
        { n, count },
      );
      record(`dom.update_layout_${n}_percent`, run, data);
      const wasmData = await page.evaluate(
        ({ n, count }) => {
          const samples = [];
          for (let i = 0; i < count + 100; i++) {
            change(i, n);
            const patch = {
              base_revision: scene.revision,
              revision: ++scene.revision,
              operations: scene.nodes
                .slice(0, n)
                .map((value) => ({ op: "upsert", value })),
            };
            const t = performance.now();
            const frame = renderer.patch(patch);
            const bytes = frame.rgba;
            frame.free();
            if (i >= 100) samples.push((performance.now() - t) * 1e6);
            if (!bytes.length) throw Error("empty frame");
          }
          return samples;
        },
        { n, count: Math.min(count, 200) },
      );
      record(`wasm.patch_raster_copy_${n}_percent`, run, wasmData);
    }
  for (const mode of [
    "dom",
    "wasm",
    "wasm_canvas_export",
    "wasm_canvas_incremental_export",
  ]) {
    await page.evaluate((mode) => {
      for (let i = 0; i < 100; i++) {
        const text = `Computer row ${String(i).padStart(3, "0")}`;
        scene.nodes[i].primitive.text = text;
        document.querySelector("#dom").children[i].textContent = text;
      }
      draw();
      document.querySelector("#dom").style.display =
        mode === "dom" ? "block" : "none";
      document.querySelector("#canvas").style.display =
        mode !== "dom" ? "block" : "none";
    }, mode);
    for (let run = 0; run < runs; run++) {
      let samples = [];
      for (let i = 0; i < Math.min(count, 30) + 3; i++) {
        const t = performance.now();
        if (mode === "wasm_canvas_incremental_export") {
          const png = await page.evaluate((i) => {
            change(i, 1);
            const patch = {
              base_revision: scene.revision,
              revision: ++scene.revision,
              operations: [{ op: "upsert", value: scene.nodes[0] }],
            };
            const frame = renderer.patch(patch);
            ctx.putImageData(
              new ImageData(new Uint8ClampedArray(frame.rgba), 1280, 720),
              0,
              0,
            );
            frame.free();
            return document.querySelector("#canvas").toDataURL("image/png");
          }, i);
          const bytes = Buffer.from(png.split(",")[1], "base64");
          if (bytes.readUInt32BE(16) !== 1280) throw Error("PNG width");
        } else {
          await page.evaluate(
            ({ mode, i }) => {
              if (mode === "dom") {
                document.querySelector(".row").textContent =
                  `Version ${String(i).padStart(5, "0")}`;
              } else {
                change(i, 1);
                draw();
              }
            },
            { mode, i },
          );
          if (mode === "wasm_canvas_export") {
            const png = await page.evaluate(() =>
              document.querySelector("#canvas").toDataURL("image/png"),
            );
            const bytes = Buffer.from(png.split(",")[1], "base64");
            if (
              bytes.readUInt32BE(16) !== 1280 ||
              bytes.readUInt32BE(20) !== 720
            )
              throw Error("PNG dimensions");
          } else {
            await page.screenshot({ type: "png" });
          }
        }
        if (i >= 3) samples.push((performance.now() - t) * 1e6);
      }
      record(`${mode}.update_png_capture`, run, samples);
    }
    await page.screenshot({ path: `benchmarks/results/primitive-${mode}.png` });
  }
  await page.evaluate((migrated) => {
    window.scene = migrated;
    document.querySelector("#dom").style.display = "none";
    document.querySelector("#canvas").style.display = "block";
    draw();
  }, migratedMail);
  for (let run = 0; run < runs; run++) {
    let samples = [];
    for (let i = 0; i < Math.min(count, 30) + 3; i++) {
      const t = performance.now();
      await page.evaluate((i) => {
        scene.nodes[scene.mutationIndex].primitive.fill =
          i % 2 ? [246, 248, 252, 255] : [255, 255, 255, 255];
        draw();
      }, i);
      await page.screenshot({ type: "png" });
      if (i >= 3) samples.push((performance.now() - t) * 1e6);
    }
    record("migrated_mail.update_png_capture", run, samples);
  }
  await page.screenshot({ path: "benchmarks/results/migrated-mail.png" });
  await writeFile(
    "benchmarks/results/migrated-mail-scene.json",
    JSON.stringify(migratedMail),
  );
  const meta = {
    browserWorld: browserWorldResults.output.map(({ samples, ...x }) => x),
    wasmMemory: {
      initial: browserWorldResults.initialMemory,
      final: browserWorldResults.finalMemory,
    },
    timestamp: new Date().toISOString(),
    chrome: await browser.version(),
    viewport: [1280, 720],
    dpr: 1,
    mail_commit: execFileSync("git", ["rev-parse", "HEAD"], {
      cwd: source,
      encoding: "utf8",
    }).trim(),
    font: "DejaVuSansMono.ttf bundled font; browser and fontdue antialiasing differ",
    mailSemantic,
    note: "DOM layout is not raster; WASM patch includes raster and JS buffer copy. No ratio between these unequal metrics. Equal PNG capture includes Playwright RPC/browser readback/encoding. 30 samples/run expensive captures; 200 wasm rasters; 1000 DOM updates. Legacy mail uses real predecessor fixture. Migrated layout captures per-character text/box geometry with normalized bundled font, but drops emoji fallback and CSS decorations; its PNG ratio is approximate workload only, not pixel-matched renderer speedup.",
  };
  await writeFile(
    process.env.BENCH_OUTPUT || "benchmarks/results/browser-render.json",
    JSON.stringify({ meta, results }, null, 2),
  );
  console.log(
    JSON.stringify(
      results.map(({ samples_ns, ...r }) => r),
      null,
      2,
    ),
  );
} finally {
  await browser.close();
  await new Promise((r) => server.close(r));
}
