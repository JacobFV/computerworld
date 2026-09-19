// A JavaScript mirror of the checks in crates/protocol/src/lib.rs (`Page::validate`,
// `Style::validate`, `PageTheme::validate`) and services/static-site/src/lib.rs
// (`StaticSite::initialize`). The authoritative check is the Rust one; render.mjs runs it
// by loading the seed into the Wasm build, which parses every page through serde. This
// mirror gives a converter its errors without a build.
export const PAGE_ICONS = new Set(['arrow-left', 'arrow-right', 'arrow-up', 'bell', 'calendar', 'cast', 'chat', 'check', 'chevron-down', 'chevron-left', 'chevron-right', 'chevron-up', 'clock', 'close', 'compass', 'copy', 'document', 'download', 'edit', 'eye', 'filters', 'flag', 'folder', 'gear', 'globe', 'grid-view', 'headphones', 'heart', 'heart-fill', 'home', 'image', 'info', 'library', 'link', 'list-view', 'lock', 'menu', 'mic', 'minus', 'more', 'more-vertical', 'music', 'pause', 'person', 'play', 'plus', 'queue', 'radio', 'reload', 'repeat', 'repeat-one', 'reply', 'search', 'send', 'share', 'shuffle', 'skip-next', 'skip-previous', 'sliders', 'star', 'star-outline', 'tag', 'thumb-up', 'thumb-up-fill', 'trash', 'volume', 'volume-mute']);
const MAX_STYLE_SPAN = 64, MAX_STYLE_RADIUS = 512, MAX_PAGE_GAP = 128, MAX_GRID_COLUMNS = 12, MAX_PAGE_EXTENT = 8192;
const colour = v => /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/.test(v);
// `mono` and `pin: top` are in the working tree's protocol (after release 0.1.1); a seed
// that uses them needs a build that has them.
const STYLE_KEYS = new Set(['size', 'weight', 'color', 'background', 'border', 'radius', 'padding', 'align', 'width', 'height', 'flex', 'one_line', 'pin', 'scroll_x', 'italic', 'lang', 'mono']);
const FIELDS = {
  heading: ['id', 'text', 'level'], text: ['id', 'text'], link: ['id', 'text', 'url'], button: ['id', 'text', 'action'],
  input: ['id', 'label', 'value', 'placeholder'], form: ['id', 'action', 'children'], group: ['id', 'children'],
  image: ['id', 'source', 'alt', 'width', 'height'], row: ['id', 'children', 'gap', 'align', 'style'],
  grid: ['id', 'columns', 'children', 'gap', 'style'], card: ['id', 'children', 'style', 'action'],
  styled: ['id', 'text', 'style'], thumbnail: ['id', 'label', 'style', 'action'], badge: ['id', 'text', 'style'],
  divider: ['id', 'style'], icon: ['id', 'name', 'label', 'style', 'action'], spacer: ['id', 'height'],
};
const REQUIRED = {
  heading: ['text', 'level'], text: ['text'], link: ['text', 'url'], button: ['text', 'action'], input: ['label', 'value'],
  form: ['action', 'children'], group: ['children'], image: ['source', 'alt', 'width', 'height'], row: ['children'],
  grid: ['columns', 'children'], card: ['children'], styled: ['text'], thumbnail: ['label'], badge: ['text'],
  divider: [], icon: ['name', 'label'], spacer: [],
};

function checkStyle(s, where, problems) {
  if (s === undefined) return;
  if (typeof s !== 'object' || s === null) return problems.push(`${where}: style must be an object`);
  for (const k of Object.keys(s)) if (!STYLE_KEYS.has(k)) problems.push(`${where}: unknown style key ${k}`);
  for (const k of ['color', 'background', 'border']) if (s[k] !== undefined && !colour(s[k])) problems.push(`${where}: invalid colour ${k}=${s[k]}`);
  if (s.radius > MAX_STYLE_RADIUS) problems.push(`${where}: radius exceeds ${MAX_STYLE_RADIUS}`);
  if (s.padding > MAX_STYLE_SPAN) problems.push(`${where}: padding exceeds ${MAX_STYLE_SPAN}`);
  if (s.width > MAX_PAGE_EXTENT || s.height > MAX_PAGE_EXTENT) problems.push(`${where}: width or height exceeds ${MAX_PAGE_EXTENT}`);
  if (s.flex > 64) problems.push(`${where}: flex exceeds 64`);
  if (s.size !== undefined && (s.size < 6 || s.size > 96)) problems.push(`${where}: size must be 6 through 96`);
  if (s.pin !== undefined && !['top', 'bottom'].includes(s.pin)) problems.push(`${where}: pin must be top or bottom`);
  if (s.weight !== undefined && !['regular', 'medium', 'bold'].includes(s.weight)) problems.push(`${where}: weight must be regular, medium or bold`);
  if (s.align !== undefined && !['left', 'center', 'right'].includes(s.align)) problems.push(`${where}: align must be left, center or right`);
  for (const k of ['radius', 'padding', 'width', 'height', 'flex', 'size']) if (s[k] !== undefined && (!Number.isInteger(s[k]) || s[k] < 0)) problems.push(`${where}: ${k} must be a non-negative integer`);
}
function checkAction(a, where, problems) {
  if (typeof a !== 'object' || a === null) return problems.push(`${where}: action must be an object`);
  if (typeof a.method !== 'string' || typeof a.url !== 'string') problems.push(`${where}: action needs method and url`);
  if (a.fields !== undefined && (typeof a.fields !== 'object' || Object.values(a.fields).some(v => typeof v !== 'string'))) problems.push(`${where}: action fields must map strings to strings`);
}
export function validatePage(page, where = 'page') {
  const problems = [];
  if (page.version !== 1) problems.push(`${where}: version must be 1`);
  if (typeof page.title !== 'string') problems.push(`${where}: title must be a string`);
  if (page.theme) {
    for (const k of ['accent', 'background', 'surface', 'ink', 'muted']) if (page.theme[k] !== undefined && !colour(page.theme[k])) problems.push(`${where}: invalid theme colour ${k}=${page.theme[k]}`);
    if (page.theme.content_width > MAX_PAGE_EXTENT) problems.push(`${where}: theme content width exceeds ${MAX_PAGE_EXTENT}`);
    for (const k of Object.keys(page.theme)) if (!['accent', 'background', 'surface', 'ink', 'muted', 'content_width'].includes(k)) problems.push(`${where}: unknown theme key ${k}`);
  }
  const ids = new Set();
  const visit = (elements, depth, path) => {
    if (depth > 64) return problems.push(`${path}: nesting exceeds 64 levels`);
    if (!Array.isArray(elements)) return problems.push(`${path}: children must be an array`);
    elements.forEach((e, i) => {
      const at = `${path}[${i}]`;
      if (!e || typeof e !== 'object') return problems.push(`${at}: not an object`);
      const fields = FIELDS[e.kind];
      if (!fields) return problems.push(`${at}: unknown kind ${e.kind}`);
      for (const k of Object.keys(e)) if (k !== 'kind' && !fields.includes(k)) problems.push(`${at}: unknown field ${k} on ${e.kind}`);
      for (const k of REQUIRED[e.kind]) if (e[k] === undefined) problems.push(`${at}: ${e.kind} needs ${k}`);
      if (typeof e.id !== 'string' || !e.id) problems.push(`${at}: empty id`);
      else if (ids.has(e.id)) problems.push(`${at}: duplicate id ${e.id}`);
      ids.add(e.id);
      if (ids.size > 100000) problems.push(`${at}: page exceeds element budget`);
      if (e.kind === 'heading' && !(e.level >= 1 && e.level <= 6)) problems.push(`${at}: heading level must be 1 through 6`);
      if (['row', 'grid'].includes(e.kind) && e.gap > MAX_PAGE_GAP) problems.push(`${at}: gap exceeds ${MAX_PAGE_GAP}`);
      if (e.kind === 'grid' && !(e.columns >= 1 && e.columns <= MAX_GRID_COLUMNS)) problems.push(`${at}: grid columns must be 1 through 12`);
      if (e.kind === 'icon') {
        if (!PAGE_ICONS.has(e.name)) problems.push(`${at}: unknown icon ${e.name}`);
        if (!String(e.label ?? '').trim()) problems.push(`${at}: icon needs a label`);
      }
      if (e.kind === 'spacer' && e.height > MAX_PAGE_EXTENT) problems.push(`${at}: spacer height exceeds ${MAX_PAGE_EXTENT}`);
      if (e.kind === 'image' && !Number.isInteger(e.width)) problems.push(`${at}: image width must be an integer`);
      if (['row', 'grid', 'card', 'styled', 'thumbnail', 'badge', 'divider', 'icon'].includes(e.kind)) checkStyle(e.style, at, problems);
      if (e.action !== undefined && (e.kind === 'button' || e.kind === 'form' || e.action !== null)) checkAction(e.action, at, problems);
      if (e.children) visit(e.children, depth + 1, `${at}.children`);
    });
  };
  visit(page.elements ?? [], 0, `${where}.elements`);
  return problems;
}
/// The site-file shape build-world.mjs splices, with the static-site service's own checks.
export function validateSite(site) {
  const problems = [];
  for (const k of ['id', 'kind', 'node']) if (!site[k]) problems.push(`site: ${k} is required`);
  if (!Array.isArray(site.domains) || !site.domains.length) problems.push('site: domains must be a non-empty array');
  if (site.network_node && (!site.network_node.address || !site.network_node.link?.from)) problems.push('site: network_node needs address and link.from');
  if (site.kind === 'static-site') {
    const state = site.initial_state ?? {};
    for (const k of ['pages', 'records', 'assets']) if (state[k] !== undefined && (typeof state[k] !== 'object' || Array.isArray(state[k]))) problems.push(`site: ${k} must be an object`);
    for (const [path, page] of Object.entries(state.pages ?? {})) {
      if (!path.startsWith('/')) problems.push(`site: page path ${path} must be absolute`);
      problems.push(...validatePage(page, `pages[${path}]`));
    }
    for (const [path, asset] of Object.entries(state.assets ?? {})) {
      if (!path.startsWith('/') || !asset.content_type || (asset.json === undefined && asset.bytes === undefined)) problems.push(`site: asset ${path} needs an absolute path, content_type and json or bytes`);
      if (asset.json?.rgba && asset.json.rgba.length !== asset.json.width * asset.json.height * 4) problems.push(`site: asset ${path} rgba length mismatch`);
      if (asset.json?.rgba && asset.json.rgba.length > 4 * 1024 * 1024) problems.push(`site: asset ${path} exceeds the 4 MiB image budget`);
    }
  }
  return problems;
}
