'use strict';
// A Prisma Client over one JSON file, for running a Prisma app's own code in the
// world without a database server. It is bundled in place of `@prisma/client`
// (scripts/oss-web/build.sh): the app's models, queries and relations are read
// from its own `schema.prisma`, and every query the app makes goes through the
// same client API (findUnique, findFirst, findMany, count, create, update,
// upsert, delete, their *Many forms and $transaction) with the same filters
// (equals/in/lt/contains..., AND/OR/NOT, relation some/every/none/is/isNot),
// select/include with nested _count, orderBy (by field or by a relation's
// _count), skip/take, and nested writes (connect, connectOrCreate, create,
// disconnect, set). The file is the app's whole database: it is read on the
// first query and written after every write, so a world service that keeps the
// file keeps the app's data (docs/oss-webapps.md).
//
// Storage: { "<Model>": [row, ...], "_links": { "<relation>": [[a, b], ...] },
// "_seq": { "<Model>": lastId } }. A row holds the model's scalar fields and
// foreign keys; an implicit many-to-many relation is a list of [a, b] id pairs,
// where `a` is the row on the side whose field name sorts first.
const fs = require('fs');
const path = require('path');
const SCHEMA = require('./schema.prisma');

const FILE = process.env.CW_PRISMA_STORE || '/data/db.json';

// ------------------------------------------------------------------ schema

function parseSchema(text) {
  const models = {};
  const blocks = text.replace(/\/\/[^\n]*/g, '').matchAll(/model\s+(\w+)\s*\{([^}]*)\}/g);
  for (const [, name, body] of blocks) {
    const fields = [];
    for (const line of body.split('\n')) {
      const m = line.trim().match(/^(\w+)\s+(\w+)(\[\])?(\?)?\s*(.*)$/);
      if (!m || m[1].startsWith('@@')) continue;
      const [, fname, type, list, optional, attrs] = m;
      const f = { name: fname, type, list: !!list, optional: !!optional, attrs };
      f.id = /@id\b/.test(attrs);
      f.unique = f.id || /@unique\b/.test(attrs);
      f.updatedAt = /@updatedAt\b/.test(attrs);
      const def = attrs.match(/@default\((.*?)\)(\s|$)/);
      if (def) f.default = def[1];
      const rel = attrs.match(/@relation\(([^)]*)\)/);
      if (rel) {
        const r = rel[1];
        const named = r.match(/^\s*"([^"]+)"/) || r.match(/name:\s*"([^"]+)"/);
        f.relationName = named ? named[1] : null;
        const fk = r.match(/fields:\s*\[([^\]]*)\]/);
        const refs = r.match(/references:\s*\[([^\]]*)\]/);
        if (fk) f.fk = fk[1].split(',').map((s) => s.trim());
        if (refs) f.refs = refs[1].split(',').map((s) => s.trim());
        f.cascade = /onDelete:\s*Cascade/.test(r);
      }
      fields.push(f);
    }
    models[name] = { name, fields, byName: Object.fromEntries(fields.map((f) => [f.name, f])) };
  }
  for (const model of Object.values(models)) {
    for (const f of model.fields) {
      f.relation = !!models[f.type];
      f.scalar = !f.relation;
    }
  }
  // Pair every relation field with its opposite.
  for (const model of Object.values(models)) {
    for (const f of model.fields) {
      if (!f.relation || f.opposite) continue;
      const target = models[f.type];
      const candidates = target.fields.filter((g) => g.relation && g.type === model.name && g !== f
        && (g.relationName || null) === (f.relationName || null) && !g.opposite);
      const g = candidates[0];
      if (!g) throw new Error(`prisma-store: ${model.name}.${f.name} has no opposite relation field`);
      f.opposite = g;
      g.opposite = f;
      f.model = model.name;
      g.model = target.name;
      const key = f.relationName || [`${model.name}.${f.name}`, `${target.name}.${g.name}`].sort().join('|');
      f.key = g.key = key;
      if (f.list && g.list) {
        f.kind = g.kind = 'many';
        // The side whose field name sorts first holds `a` of each pair.
        const first = [f, g].sort((x, y) => `${x.model}.${x.name}` < `${y.model}.${y.name}` ? -1 : 1)[0];
        f.sideA = first === f;
        g.sideA = first === g;
      } else if (f.fk) {
        f.kind = 'owner';
        g.kind = g.list ? 'back-list' : 'back-one';
      } else if (g.fk) {
        g.kind = 'owner';
        f.kind = f.list ? 'back-list' : 'back-one';
      } else {
        throw new Error(`prisma-store: relation ${key} names no foreign key`);
      }
    }
  }
  for (const model of Object.values(models)) {
    model.idField = model.fields.find((f) => f.id);
  }
  return models;
}

const MODELS = parseSchema(SCHEMA);

// ------------------------------------------------------------------ errors

class PrismaClientKnownRequestError extends Error {
  constructor(message, { code, meta } = {}) {
    super(message);
    this.name = 'PrismaClientKnownRequestError';
    this.code = code;
    this.meta = meta;
    this.clientVersion = '4.16.2';
  }
}
class PrismaClientValidationError extends Error {
  constructor(message) {
    super(message);
    this.name = 'PrismaClientValidationError';
    this.clientVersion = '4.16.2';
  }
}

// ------------------------------------------------------------------ storage

let db = null;
function load() {
  if (db) return db;
  let text = null;
  try { text = fs.readFileSync(FILE, 'utf8'); } catch (e) { if (e.code !== 'ENOENT') throw e; }
  db = text ? JSON.parse(text) : {};
  for (const name of Object.keys(MODELS)) if (!Array.isArray(db[name])) db[name] = [];
  if (!db._links) db._links = {};
  if (!db._seq) db._seq = {};
  return db;
}
function save() {
  fs.mkdirSync(path.dirname(FILE), { recursive: true });
  fs.writeFileSync(FILE, JSON.stringify(db));
}

const rows = (model) => load()[model];
const links = (key) => (load()._links[key] || (db._links[key] = []));
const clone = (v) => (v === undefined ? v : JSON.parse(JSON.stringify(v)));

// ------------------------------------------------------------------ values

function isDateField(f) { return f && f.type === 'DateTime'; }
function toStored(f, v) {
  if (v instanceof Date) return v.toISOString();
  if (isDateField(f) && typeof v === 'string') return new Date(v).toISOString();
  return v;
}
function toOutput(f, v) {
  if (isDateField(f) && typeof v === 'string') return new Date(v);
  return v === undefined ? null : v;
}
function cmp(a, b) {
  if (a === b) return 0;
  if (a === null || a === undefined) return -1;
  if (b === null || b === undefined) return 1;
  return a < b ? -1 : 1;
}

// ------------------------------------------------------------------ relations

function related(model, row, f) {
  const target = f.type;
  switch (f.kind) {
    case 'owner': {
      const fkv = f.fk.map((k) => row[k]);
      if (fkv.some((v) => v === null || v === undefined)) return null;
      return rows(target).find((r) => f.refs.every((k, i) => r[k] === fkv[i])) || null;
    }
    case 'back-list':
    case 'back-one': {
      const g = f.opposite;
      const list = rows(target).filter((r) => g.refs.every((k, i) => r[g.fk[i]] === row[k]));
      return f.kind === 'back-list' ? list : list[0] || null;
    }
    case 'many': {
      const id = row[MODELS[model].idField.name];
      const pairs = links(f.key);
      const ids = f.sideA ? pairs.filter((p) => p[0] === id).map((p) => p[1]) : pairs.filter((p) => p[1] === id).map((p) => p[0]);
      const tid = MODELS[target].idField.name;
      return rows(target).filter((r) => ids.includes(r[tid]));
    }
  }
  throw new Error(`prisma-store: unknown relation kind ${f.kind}`);
}

function link(model, row, f, other) {
  const g = f.opposite;
  if (f.kind === 'many') {
    const a = row[MODELS[model].idField.name];
    const b = other[MODELS[f.type].idField.name];
    const pair = f.sideA ? [a, b] : [b, a];
    const pairs = links(f.key);
    if (!pairs.some((p) => p[0] === pair[0] && p[1] === pair[1])) pairs.push(pair);
  } else if (f.kind === 'owner') {
    f.fk.forEach((k, i) => { row[k] = other[f.refs[i]]; });
  } else {
    g.fk.forEach((k, i) => { other[k] = row[g.refs[i]]; });
  }
}

function unlink(model, row, f, other) {
  const g = f.opposite;
  if (f.kind === 'many') {
    const a = row[MODELS[model].idField.name];
    const b = other[MODELS[f.type].idField.name];
    const pair = f.sideA ? [a, b] : [b, a];
    db._links[f.key] = links(f.key).filter((p) => !(p[0] === pair[0] && p[1] === pair[1]));
  } else if (f.kind === 'owner') {
    f.fk.forEach((k) => { row[k] = null; });
  } else {
    g.fk.forEach((k) => { other[k] = null; });
  }
}

// ------------------------------------------------------------------ filters

function scalarMatch(f, value, cond) {
  if (cond === null || typeof cond !== 'object' || cond instanceof Date) {
    return cmp(value, toStored(f, cond)) === 0 && (value === null) === (cond === null);
  }
  const insensitive = cond.mode === 'insensitive';
  const norm = (v) => (insensitive && typeof v === 'string' ? v.toLowerCase() : v);
  const v = norm(value);
  for (const [op, raw] of Object.entries(cond)) {
    if (op === 'mode') continue;
    const x = Array.isArray(raw) ? raw.map((y) => norm(toStored(f, y))) : norm(toStored(f, raw));
    const ok = {
      equals: () => v === x,
      not: () => (raw !== null && typeof raw === 'object' ? !scalarMatch(f, value, raw) : v !== x),
      in: () => x.includes(v),
      notIn: () => !x.includes(v),
      lt: () => v !== null && v < x,
      lte: () => v !== null && v <= x,
      gt: () => v !== null && v > x,
      gte: () => v !== null && v >= x,
      contains: () => typeof v === 'string' && v.includes(x),
      startsWith: () => typeof v === 'string' && v.startsWith(x),
      endsWith: () => typeof v === 'string' && v.endsWith(x),
    }[op];
    if (!ok) throw new PrismaClientValidationError(`Unknown filter \`${op}\` on field \`${f.name}\``);
    if (!ok()) return false;
  }
  return true;
}

function matches(model, row, where) {
  if (!where) return true;
  const m = MODELS[model];
  for (const [key, cond] of Object.entries(where)) {
    if (cond === undefined) continue;
    if (key === 'AND') {
      const list = Array.isArray(cond) ? cond : [cond];
      if (!list.every((w) => matches(model, row, w))) return false;
      continue;
    }
    if (key === 'OR') {
      if (!cond.some((w) => matches(model, row, w))) return false;
      continue;
    }
    if (key === 'NOT') {
      const list = Array.isArray(cond) ? cond : [cond];
      if (list.some((w) => matches(model, row, w))) return false;
      continue;
    }
    const f = m.byName[key];
    if (!f) throw new PrismaClientValidationError(`Unknown argument \`${key}\` in where of ${model}`);
    if (f.scalar) {
      if (!scalarMatch(f, row[key] === undefined ? null : row[key], cond)) return false;
      continue;
    }
    if (f.list) {
      const list = related(model, row, f);
      if (cond.some && !list.some((r) => matches(f.type, r, cond.some))) return false;
      if (cond.every && !list.every((r) => matches(f.type, r, cond.every))) return false;
      if (cond.none && list.some((r) => matches(f.type, r, cond.none))) return false;
      continue;
    }
    const one = related(model, row, f);
    if (cond === null) { if (one) return false; continue; }
    if ('is' in cond || 'isNot' in cond) {
      if ('is' in cond && !(cond.is === null ? !one : one && matches(f.type, one, cond.is))) return false;
      if ('isNot' in cond && (cond.isNot === null ? !one : one && matches(f.type, one, cond.isNot))) return false;
      continue;
    }
    if (!one || !matches(f.type, one, cond)) return false;
  }
  return true;
}

// ------------------------------------------------------------------ reads

function sortRows(model, list, orderBy) {
  if (!orderBy) return list;
  const orders = Array.isArray(orderBy) ? orderBy : [orderBy];
  const keys = [];
  for (const o of orders) {
    for (const [field, dir] of Object.entries(o)) {
      const f = MODELS[model].byName[field];
      if (f && f.relation && dir && typeof dir === 'object' && dir._count) {
        keys.push({ get: (r) => related(model, r, f).length, desc: dir._count === 'desc' });
      } else {
        const d = typeof dir === 'object' ? dir.sort : dir;
        keys.push({ get: (r) => r[field], desc: d === 'desc' });
      }
    }
  }
  // Decorated sort, so ties keep insertion order (as the database's row order).
  return list
    .map((r, i) => ({ r, i, k: keys.map((k) => k.get(r)) }))
    .sort((x, y) => {
      for (let j = 0; j < keys.length; j++) {
        const c = cmp(x.k[j], y.k[j]);
        if (c) return keys[j].desc ? -c : c;
      }
      return x.i - y.i;
    })
    .map((x) => x.r);
}

function page(list, args) {
  const skip = args.skip || 0;
  return args.take === undefined ? list.slice(skip) : list.slice(skip, skip + args.take);
}

function shape(model, row, args = {}) {
  const m = MODELS[model];
  const out = {};
  const { select, include } = args;
  if (select) {
    for (const [key, spec] of Object.entries(select)) {
      if (!spec) continue;
      if (key === '_count') { out._count = counts(model, row, spec); continue; }
      const f = m.byName[key];
      if (!f) throw new PrismaClientValidationError(`Unknown field \`${key}\` in select of ${model}`);
      out[key] = f.scalar ? toOutput(f, row[key]) : relatedShape(model, row, f, spec);
    }
    return out;
  }
  for (const f of m.fields) if (f.scalar) out[f.name] = toOutput(f, row[f.name]);
  if (include) {
    for (const [key, spec] of Object.entries(include)) {
      if (!spec) continue;
      if (key === '_count') { out._count = counts(model, row, spec); continue; }
      const f = m.byName[key];
      if (!f || f.scalar) throw new PrismaClientValidationError(`Unknown relation \`${key}\` in include of ${model}`);
      out[key] = relatedShape(model, row, f, spec);
    }
  }
  return out;
}

function relatedShape(model, row, f, spec) {
  const nested = spec === true ? {} : spec;
  const r = related(model, row, f);
  if (!f.list) return r ? shape(f.type, r, nested) : null;
  let list = r.filter((x) => matches(f.type, x, nested.where));
  list = page(sortRows(f.type, list, nested.orderBy), nested);
  return list.map((x) => shape(f.type, x, nested));
}

function counts(model, row, spec) {
  const m = MODELS[model];
  const fields = spec === true ? m.fields.filter((f) => f.list) : Object.keys(spec.select || {}).filter((k) => spec.select[k]).map((k) => m.byName[k]);
  const out = {};
  for (const f of fields) {
    const sel = spec === true ? true : spec.select[f.name];
    const where = sel && typeof sel === 'object' ? sel.where : undefined;
    out[f.name] = related(model, row, f).filter((x) => matches(f.type, x, where)).length;
  }
  return out;
}

function findRows(model, args = {}) {
  let list = rows(model).filter((r) => matches(model, r, args.where));
  list = sortRows(model, list, args.orderBy);
  if (args.distinct) {
    const seen = new Set();
    const keys = Array.isArray(args.distinct) ? args.distinct : [args.distinct];
    list = list.filter((r) => {
      const k = JSON.stringify(keys.map((x) => r[x]));
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    });
  }
  return page(list, args);
}

function uniqueRow(model, where) {
  const list = rows(model).filter((r) => matches(model, r, where));
  return list[0] || null;
}

// ------------------------------------------------------------------ writes

function nowIso() { return new Date().toISOString(); }

function applyDefaults(model, row) {
  const m = MODELS[model];
  for (const f of m.fields) {
    if (!f.scalar || row[f.name] !== undefined) continue;
    if (f.default !== undefined) {
      const d = f.default;
      if (d === 'autoincrement()') {
        const seq = (load()._seq[model] || rows(model).reduce((n, r) => Math.max(n, r[f.name] || 0), 0)) + 1;
        db._seq[model] = seq;
        row[f.name] = seq;
      } else if (d === 'now()') row[f.name] = nowIso();
      else if (d === 'uuid()' || d === 'cuid()') row[f.name] = require('crypto').randomUUID();
      else if (d === 'true' || d === 'false') row[f.name] = d === 'true';
      else if (/^-?\d+(\.\d+)?$/.test(d)) row[f.name] = Number(d);
      else if (/^".*"$/.test(d)) row[f.name] = JSON.parse(d);
      else row[f.name] = d;
    } else if (f.updatedAt) {
      row[f.name] = nowIso();
    } else if (f.optional || f.list) {
      row[f.name] = null;
    }
  }
  // Foreign keys of owning relations default to null.
  for (const f of m.fields) if (f.kind === 'owner') for (const k of f.fk) if (row[k] === undefined) row[k] = null;
}

function checkUnique(model, row) {
  const m = MODELS[model];
  for (const f of m.fields) {
    if (!f.unique || row[f.name] === null || row[f.name] === undefined) continue;
    if (rows(model).some((r) => r !== row && r[f.name] === row[f.name])) {
      throw new PrismaClientKnownRequestError(
        `Unique constraint failed on the fields: (\`${f.name}\`)`,
        { code: 'P2002', meta: { target: [f.name] } },
      );
    }
  }
}

function requireRow(model, where, what) {
  const row = uniqueRow(model, where);
  if (!row) {
    throw new PrismaClientKnownRequestError(`An operation failed because it depends on one or more records that were required but not found. ${what}`,
      { code: 'P2025', meta: { cause: what } });
  }
  return row;
}

function nestedWrites(model, row, relData) {
  const m = MODELS[model];
  for (const [key, ops] of relData) {
    const f = m.byName[key];
    const target = f.type;
    const many = (x) => (Array.isArray(x) ? x : [x]);
    if (ops.set !== undefined) {
      for (const other of related(model, row, f) ? [].concat(related(model, row, f)) : []) unlink(model, row, f, other);
      for (const w of many(ops.set)) link(model, row, f, requireRow(target, w, `No '${target}' record was found for a set.`));
    }
    if (ops.disconnect !== undefined) {
      if (ops.disconnect === true) {
        const other = related(model, row, f);
        if (other) unlink(model, row, f, other);
      } else {
        for (const w of many(ops.disconnect)) {
          const other = uniqueRow(target, w);
          if (other) unlink(model, row, f, other);
        }
      }
    }
    if (ops.connect !== undefined) {
      for (const w of many(ops.connect)) link(model, row, f, requireRow(target, w, `No '${target}' record was found for a connect.`));
    }
    if (ops.connectOrCreate !== undefined) {
      for (const c of many(ops.connectOrCreate)) {
        const other = uniqueRow(target, c.where) || createRow(target, c.create);
        link(model, row, f, other);
      }
    }
    if (ops.create !== undefined) {
      for (const d of many(ops.create)) {
        const other = createRow(target, d, { via: f.opposite, parent: row });
        link(model, row, f, other);
      }
    }
    if (ops.delete !== undefined) {
      for (const w of many(ops.delete)) deleteRow(target, requireRow(target, w, `No '${target}' record was found for a nested delete.`));
    }
  }
}

function splitData(model, data) {
  const m = MODELS[model];
  const scalars = {};
  const relData = [];
  for (const [key, v] of Object.entries(data || {})) {
    if (v === undefined) continue;
    const f = m.byName[key];
    if (!f && !MODELS[model].fields.some((g) => g.fk && g.fk.includes(key))) {
      throw new PrismaClientValidationError(`Unknown argument \`${key}\` in data of ${model}`);
    }
    if (f && f.relation) relData.push([key, v]);
    else scalars[key] = v;
  }
  return { scalars, relData };
}

function createRow(model, data, via) {
  const { scalars, relData } = splitData(model, data);
  const m = MODELS[model];
  const row = {};
  for (const [k, v] of Object.entries(scalars)) row[k] = toStored(m.byName[k], v);
  // A row created through its parent's back relation points at the parent.
  if (via && via.via && via.via.kind === 'owner') link(model, row, via.via, via.parent);
  applyDefaults(model, row);
  // Owning relations are resolved before the row exists (they set its foreign keys).
  const owners = relData.filter(([k]) => m.byName[k].kind === 'owner');
  const rest = relData.filter(([k]) => m.byName[k].kind !== 'owner');
  nestedWrites(model, row, owners);
  checkUnique(model, row);
  rows(model).push(row);
  nestedWrites(model, row, rest);
  return row;
}

function updateRow(model, row, data) {
  const { scalars, relData } = splitData(model, data);
  const m = MODELS[model];
  for (const [k, v] of Object.entries(scalars)) {
    const f = m.byName[k];
    if (v !== null && typeof v === 'object' && !(v instanceof Date) && !Array.isArray(v)) {
      const cur = row[k] || 0;
      if ('set' in v) row[k] = toStored(f, v.set);
      else if ('increment' in v) row[k] = cur + v.increment;
      else if ('decrement' in v) row[k] = cur - v.decrement;
      else if ('multiply' in v) row[k] = cur * v.multiply;
      else if ('divide' in v) row[k] = cur / v.divide;
      else row[k] = v;
    } else {
      row[k] = toStored(f, v);
    }
  }
  for (const f of m.fields) if (f.updatedAt && scalars[f.name] === undefined) row[f.name] = nowIso();
  checkUnique(model, row);
  nestedWrites(model, row, relData);
  return row;
}

function deleteRow(model, row) {
  const m = MODELS[model];
  // Rows whose required foreign key points here cascade (or are left dangling
  // only where the schema says SetNull, which this store treats as null).
  for (const other of Object.values(MODELS)) {
    for (const f of other.fields) {
      if (f.kind !== 'owner' || f.type !== model) continue;
      const dependants = rows(other.name).filter((r) => f.refs.every((k, i) => r[f.fk[i]] === row[k]));
      for (const d of dependants) {
        if (f.cascade || !f.optional) deleteRow(other.name, d);
        else f.fk.forEach((k) => { d[k] = null; });
      }
    }
  }
  for (const f of m.fields) {
    if (f.kind === 'many') {
      const id = row[m.idField.name];
      db._links[f.key] = links(f.key).filter((p) => (f.sideA ? p[0] : p[1]) !== id);
    }
  }
  const list = rows(model);
  const i = list.indexOf(row);
  if (i >= 0) list.splice(i, 1);
}

// ------------------------------------------------------------------ client

function delegate(model) {
  const read = (fn) => (args) => Promise.resolve().then(() => fn(args || {}));
  const write = (fn) => (args) => Promise.resolve().then(() => { const r = fn(args || {}); save(); return r; });
  return {
    findUnique: read((a) => { const r = uniqueRow(model, a.where); return r ? shape(model, r, a) : null; }),
    findUniqueOrThrow: read((a) => shape(model, requireRow(model, a.where, 'Record not found'), a)),
    findFirst: read((a) => { const r = findRows(model, { ...a, take: 1 })[0]; return r ? shape(model, r, a) : null; }),
    findFirstOrThrow: read((a) => {
      const r = findRows(model, { ...a, take: 1 })[0];
      if (!r) throw new PrismaClientKnownRequestError('No record found', { code: 'P2025' });
      return shape(model, r, a);
    }),
    findMany: read((a) => findRows(model, a).map((r) => shape(model, r, a))),
    count: read((a) => findRows(model, a).length),
    create: write((a) => shape(model, createRow(model, a.data), a)),
    createMany: write((a) => {
      let count = 0;
      for (const d of Array.isArray(a.data) ? a.data : [a.data]) {
        try { createRow(model, d); count += 1; } catch (e) { if (!(a.skipDuplicates && e.code === 'P2002')) throw e; }
      }
      return { count };
    }),
    update: write((a) => shape(model, updateRow(model, requireRow(model, a.where, 'Record to update not found.'), a.data), a)),
    updateMany: write((a) => {
      const list = rows(model).filter((r) => matches(model, r, a.where));
      for (const r of list) updateRow(model, r, a.data);
      return { count: list.length };
    }),
    upsert: write((a) => {
      const r = uniqueRow(model, a.where);
      return shape(model, r ? updateRow(model, r, a.update) : createRow(model, a.create), a);
    }),
    delete: write((a) => {
      const r = requireRow(model, a.where, 'Record to delete does not exist.');
      const out = shape(model, r, a);
      deleteRow(model, r);
      return out;
    }),
    deleteMany: write((a) => {
      const list = rows(model).filter((r) => matches(model, r, a.where));
      for (const r of list) deleteRow(model, r);
      return { count: list.length };
    }),
  };
}

class PrismaClient {
  constructor() {
    for (const name of Object.keys(MODELS)) {
      this[name[0].toLowerCase() + name.slice(1)] = delegate(name);
    }
  }
  $connect() { return Promise.resolve(); }
  $disconnect() { return Promise.resolve(); }
  $on() {}
  $use() {}
  $transaction(arg) {
    if (Array.isArray(arg)) return Promise.all(arg);
    return Promise.resolve().then(() => arg(this));
  }
}

const Prisma = { PrismaClientKnownRequestError, PrismaClientValidationError };

module.exports = { PrismaClient, Prisma, PrismaClientKnownRequestError, PrismaClientValidationError };
