// A typical stdin filter: word frequencies, a CSV report and a formatted table.
const input = require('fs').readFileSync(0, 'utf8');
const [header, ...rows] = input.trim().split('\n').filter((l) => !l.startsWith('#'));
const cols = header.split(',');
const records = rows.map((line) => {
  const cells = line.split(',').map((c) => c.trim());
  return Object.fromEntries(cols.map((c, i) => [c, isNaN(cells[i]) ? cells[i] : Number(cells[i])]));
});
console.log(`${records.length} records, columns: ${cols.join(' | ')}`);

const byDept = new Map();
for (const r of records) {
  const d = byDept.get(r.dept) ?? { count: 0, total: 0, names: [] };
  d.count++;
  d.total += r.salary;
  d.names.push(r.name);
  byDept.set(r.dept, d);
}
const summary = [...byDept]
  .map(([dept, d]) => ({ dept, count: d.count, avg: Math.round(d.total / d.count), names: d.names.sort().join(';') }))
  .sort((a, b) => b.avg - a.avg || a.dept.localeCompare(b.dept));
console.table(summary);

const pad = (s, n, right) => (right ? String(s).padStart(n) : String(s).padEnd(n));
const width = Math.max(...records.map((r) => r.name.length));
for (const r of records.slice().sort((a, b) => a.name.localeCompare(b.name))) {
  const bar = '#'.repeat(Math.round(r.salary / 10000));
  console.log(`${pad(r.name, width)} ${pad(r.dept, 12)} ${pad(r.salary.toLocaleString('en-US'), 9, true)} ${bar}`);
}

const words = records.flatMap((r) => r.note.toLowerCase().match(/[a-z']+/g) ?? []);
const freq = words.reduce((m, w) => m.set(w, (m.get(w) || 0) + 1), new Map());
const top = [...freq].sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1)).slice(0, 5);
console.log('top words:', top.map(([w, n]) => `${w}=${n}`).join(', '));
console.log(JSON.stringify({ max: Math.max(...records.map((r) => r.salary)), depts: [...byDept.keys()] }));
process.stdout.write(`total payroll: ${records.reduce((s, r) => s + r.salary, 0).toFixed(2)}\n`);
