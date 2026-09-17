"""Reduce raw runs without pretending run-mean quantiles are operation quantiles."""
import json,statistics,pathlib
rows=[]
for path in sorted(pathlib.Path('benchmarks/results').glob('*.json')):
 data=json.loads(path.read_text())
 if not isinstance(data,dict) or 'results' not in data:continue
 names=dict.fromkeys(r['name'] for r in data['results'])
 for name in names:
  rs=[r for r in data['results'] if r['name']==name]
  rows.append({'file':path.name,'name':name,'runs':len(rs),'count':sum(r['count'] for r in rs),'p50_ns':statistics.median(r['p50_ns'] for r in rs),'p95_ns':statistics.median(r['p95_ns'] for r in rs),'p50_min_ns':min(r['p50_ns'] for r in rs),'p50_max_ns':max(r['p50_ns'] for r in rs),'throughput_ops_s':1e9/statistics.median(r['mean_ns'] for r in rs)})
pathlib.Path('benchmarks/results/summary.json').write_text(json.dumps(rows,indent=2))
for r in rows: print(f"{r['file']:22} {r['name']:42} {r['p50_ns']/1000:10.2f} {r['p95_ns']/1000:10.2f} us ({r['count']} samples)")
