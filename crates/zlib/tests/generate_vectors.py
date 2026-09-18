#!/usr/bin/env python3
"""Records what the real libraries produce, for tests/vectors.json.

Run by hand (never from the Rust tests) on a machine with CPython 3.12 (system
zlib 1.3) and Node 24.21 (Chromium's zlib) on PATH:

    python3 crates/zlib/tests/generate_vectors.py

Inputs are generated from a fixed LCG that tests/byte_exact.rs reproduces, so the
file only stores each output's length and SHA-256 (plus the bytes of small ones).
"""
import hashlib
import json
import os
import subprocess
import sys
import zlib
import gzip

HERE = os.path.dirname(os.path.abspath(__file__))
WORDS = ("the of and to in is was that for on with as by at from his it an were are "
         "which this be or has had not but first one their its new after who they have "
         "her she two been other when there all during into school time may years more "
         "most only over city some world would where later up such used many can state "
         "about national out known university united then made").split()


def lcg(seed):
    x = seed & 0xFFFFFFFFFFFFFFFF
    while True:
        x = (x * 6364136223846793005 + 1442695040888963407) & 0xFFFFFFFFFFFFFFFF
        yield x >> 33


def inputs():
    out = {}
    out['empty'] = b''
    out['hello'] = b'hello world'
    out['repeat'] = b'abcabcabcabcabcabcabcabc' * 200
    g = lcg(1)
    out['random'] = bytes(next(g) & 0xFF for _ in range(3000))
    g = lcg(2)
    text = []
    n = 0
    while n < 120000:
        w = WORDS[next(g) % len(WORDS)]
        sep = '\n' if next(g) % 11 == 0 else ' '
        text.append(w + sep)
        n += len(w) + 1
    out['text'] = ''.join(text).encode()
    g = lcg(3)
    runs = bytearray()
    while len(runs) < 70000:
        b = next(g) & 0xFF
        runs += bytes([b]) * (1 + next(g) % 300)
    out['runs'] = bytes(runs)
    g = lcg(4)
    big = bytearray()
    while len(big) < 330000:
        k = next(g) % 4
        if k == 0:
            big += bytes(next(g) & 0xFF for _ in range(40))
        elif k == 1 and len(big) > 1000:
            start = next(g) % (len(big) - 500)
            big += big[start:start + 20 + next(g) % 200]
        else:
            big += WORDS[next(g) % len(WORDS)].encode() + b' '
    out['mixed'] = bytes(big)
    return out


def record(data):
    h = hashlib.sha256(data).hexdigest()
    rec = {'len': len(data), 'sha256': h}
    if len(data) <= 256:
        rec['hex'] = data.hex()
    return rec


PARAMS = []
for level in range(0, 10):
    PARAMS.append({'level': level, 'wbits': 15, 'mem': 8, 'strategy': 0})
for wbits in (-15, 31, 9, 12):
    for level in (1, 6, 9):
        PARAMS.append({'level': level, 'wbits': wbits, 'mem': 8, 'strategy': 0})
for strategy in (1, 2, 3, 4):
    PARAMS.append({'level': 6, 'wbits': 15, 'mem': 8, 'strategy': strategy})
for mem in (1, 5, 9):
    PARAMS.append({'level': 6, 'wbits': 15, 'mem': mem, 'strategy': 0})

NODE = r"""
const zlib = require('zlib');
const crypto = require('crypto');
const fs = require('fs');
const job = JSON.parse(fs.readFileSync(0, 'utf8'));
const out = [];
for (const c of job.cases) {
  const data = Buffer.from(c.input, 'hex');
  const r = zlib.deflateSync(data, { level: c.level, windowBits: Math.abs(c.wbits) > 15 ? c.wbits - 16 : Math.abs(c.wbits), memLevel: c.mem, strategy: c.strategy });
  let buf;
  if (c.wbits < 0) buf = zlib.deflateRawSync(data, { level: c.level, windowBits: -c.wbits, memLevel: c.mem, strategy: c.strategy });
  else if (c.wbits > 15) buf = zlib.gzipSync(data, { level: c.level, windowBits: c.wbits - 16, memLevel: c.mem, strategy: c.strategy });
  else buf = r;
  const rec = { len: buf.length, sha256: crypto.createHash('sha256').update(buf).digest('hex') };
  if (buf.length <= 256) rec.hex = buf.toString('hex');
  out.push(rec);
}
process.stdout.write(JSON.stringify(out));
"""


def main():
    ins = inputs()
    cases = []
    node_jobs = []
    for name, data in ins.items():
        for p in PARAMS:
            if p['wbits'] == 9 and len(data) > 100000:
                pass
            c = zlib.compressobj(p['level'], zlib.DEFLATED, p['wbits'], p['mem'], p['strategy'])
            py = c.compress(data) + c.flush()
            cases.append({'input': name, **p, 'lib': 'python', **record(py)})
            node_jobs.append({'input': data.hex(), **p, 'name': name})
    # CPython's one-shot zlib.compress and gzip.compress (mtime fixed).
    for name, data in ins.items():
        for level in (-1, 1, 9):
            cases.append({'input': name, 'level': level, 'wbits': 15, 'mem': 8, 'strategy': 0,
                          'lib': 'python-compress', **record(zlib.compress(data, level))})
        cases.append({'input': name, 'level': 9, 'wbits': 31, 'mem': 8, 'strategy': 0,
                      'lib': 'python-gzip', **record(gzip.compress(data, mtime=0))})
    res = subprocess.run(['node', '-e', NODE], input=json.dumps({'cases': node_jobs}).encode(),
                         capture_output=True, check=True)
    for job, rec in zip(node_jobs, json.loads(res.stdout)):
        cases.append({'input': job['name'], 'level': job['level'], 'wbits': job['wbits'],
                      'mem': job['mem'], 'strategy': job['strategy'], 'lib': 'node', **rec})
    versions = {
        'python': sys.version.split()[0], 'python_zlib': zlib.ZLIB_RUNTIME_VERSION,
        'node': subprocess.run(['node', '-p', 'process.version + " zlib " + process.versions.zlib'],
                               capture_output=True, text=True).stdout.strip(),
    }
    with open(os.path.join(HERE, 'vectors.json'), 'w') as f:
        json.dump({'versions': versions, 'cases': cases}, f, indent=0, sort_keys=True)
        f.write('\n')
    print(len(cases), 'cases written')


if __name__ == '__main__':
    main()
