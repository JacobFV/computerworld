import gzip
import struct
import zlib

text = ("The quick brown fox jumps over the lazy dog. " * 40).encode()
words = " ".join(str(i * i % 977) for i in range(3000)).encode()

for level in (-1, 0, 1, 6, 9):
    c = zlib.compress(words, level)
    print(level, len(c), c[:6].hex(), zlib.crc32(c), zlib.decompress(c) == words)

for wbits in (-15, 9, 31):
    co = zlib.compressobj(6, zlib.DEFLATED, wbits)
    out = co.compress(text[:300]) + co.compress(text[300:]) + co.flush()
    print(wbits, len(out), zlib.adler32(out))
    print(zlib.decompress(out, wbits) == text)

for strategy in (zlib.Z_FILTERED, zlib.Z_HUFFMAN_ONLY, zlib.Z_RLE, zlib.Z_FIXED):
    co = zlib.compressobj(level=6, strategy=strategy)
    print(strategy, (co.compress(words) + co.flush()).hex()[:40])

co = zlib.compressobj()
parts = [co.compress(text[:100]), co.flush(zlib.Z_SYNC_FLUSH), co.compress(text[100:]),
         co.flush(zlib.Z_FULL_FLUSH), co.flush()]
print([len(p) for p in parts], b"".join(parts)[-8:].hex())

d = zlib.decompressobj()
stream = zlib.compress(words) + b"trailing"
got = d.decompress(stream, 100)
print(len(got), len(d.unconsumed_tail) > 1000, d.eof)
while d.unconsumed_tail and not d.eof:
    got += d.decompress(d.unconsumed_tail, 1000)
print(len(got) == len(words), d.eof, d.unused_data, d.unconsumed_tail)
print(d.decompress(b"more"), d.unused_data, d.unconsumed_tail)

zd = b"quick brown fox lazy dog"
co = zlib.compressobj(zdict=zd)
cd = co.compress(text) + co.flush()
print(len(cd), zlib.decompressobj(zdict=zd).decompress(cd) == text)

for bad in (b"not zlib at all", zlib.compress(text)[:20], zlib.compress(text)[:-1] + b"\x00"):
    try:
        zlib.decompress(bad)
    except zlib.error as e:
        print("zlib.error:", e)

g = gzip.compress(text, mtime=0)
print(g[:10].hex(), len(g), gzip.decompress(g + gzip.compress(b"!", mtime=0))[-5:])
g2 = gzip.compress(text, compresslevel=1, mtime=123456)
print(g2[:10].hex(), gzip.decompress(g2) == text)
with gzip.open("out.txt.gz", "wb") as f:
    f.write(words)
with open("out.txt.gz", "rb") as f:
    raw = f.read()
print(raw[3], raw[8:10].hex(), raw[10:21])
with gzip.open("out.txt.gz", "rb") as f:
    print(f.read(10), f.readline()[:10])
try:
    gzip.decompress(b"\x1f\x8b\x08\x00garbage")
except Exception as e:
    print(type(e).__name__, e)

print(struct.pack("<hHiIqQ", -2, 2, -3, 3, -4, 4).hex())
print(struct.pack(">fd?c5s3p", 1.5, -2.25, True, b"x", b"hello", b"ab").hex())
print(struct.unpack("<3B2x2h", bytes(range(9))))
print(struct.calcsize("@bid"), struct.calcsize("=bid"), struct.calcsize("!8s2H"))
s = struct.Struct("!HHI")
print(s.size, s.unpack(s.pack(1, 2, 3)), list(struct.iter_unpack("<H", b"\x01\x00\x02\x00")))
for fmt, v in (("B", 256), ("h", 40000), ("I", -1)):
    try:
        struct.pack(fmt, v)
    except struct.error as e:
        print("struct.error:", e)
try:
    struct.unpack("<I", b"ab")
except struct.error as e:
    print("struct.error:", e)
