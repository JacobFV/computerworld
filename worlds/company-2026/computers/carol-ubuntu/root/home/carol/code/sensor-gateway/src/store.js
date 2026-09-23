'use strict';
/** A JSON file of readings: load, append a batch, save. */
const fs = require('fs');
const path = require('path');

/**
 * @param {string} file
 * @returns {import('./readings').Reading[]}
 */
function load(file) {
  if (!fs.existsSync(file)) return [];
  const text = fs.readFileSync(file, 'utf8');
  return text.trim() === '' ? [] : JSON.parse(text);
}

/**
 * @param {string} file
 * @param {import('./readings').Reading[]} readings
 */
function save(file, readings) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, `${JSON.stringify(readings, null, 2)}\n`);
}

module.exports = { load, save };
