#!/usr/bin/env node
// Render mock.html with Chromium at 1280x800 and write chromium.png plus a capture.json
// (the box tree and ARIA snapshot compare.mjs reads) next to it.
//   PLAYWRIGHT_MODULE=... CHROME_BIN=... node shot.mjs
import {execFileSync} from 'node:child_process';
import {fileURLToPath} from 'node:url';
const here = fileURLToPath(new URL('.', import.meta.url));
const root = fileURLToPath(new URL('../..', import.meta.url));
execFileSync('node', [`${root}/scripts/dom-to-site/capture.mjs`, `${here}/mock.html`, `${here}/capture.json`, '--width', '1280', '--height', '800'], {stdio: 'inherit', env: process.env});
// capture.mjs names the screenshot capture.png; the deliverable is chromium.png.
execFileSync('cp', [`${here}/capture.png`, `${here}/chromium.png`]);
