# File I/O on the machine's filesystem: write, append, read modes, os and pathlib.
import os
import shutil
from pathlib import Path

os.makedirs("data/sub", exist_ok=True)
with open("data/notes.txt", "w") as f:
    f.write("first line\n")
    f.writelines(["second line\n", "third line\n"])
    print("written:", f.name, f.mode, f.closed)
print("closed after with:", f.closed)
with open("data/notes.txt", "a") as f:
    print("appended", file=f)
with open("data/notes.txt") as f:
    print(repr(f.readline()))
    print(f.readlines())
with open("data/notes.txt") as f:
    for i, line in enumerate(f, 1):
        print(i, line.rstrip())
print(open("data/notes.txt").read().count("line"))
with open("data/blob.bin", "wb") as f:
    f.write(bytes(range(5)) + b"\xff")
data = open("data/blob.bin", "rb").read()
print(data, len(data), data[-1], list(data[:3]))
print(os.path.exists("data/notes.txt"), os.path.isdir("data"), os.path.isfile("data"), os.path.getsize("data/blob.bin"))
print(sorted(os.listdir("data")), os.path.basename("data/notes.txt"), os.path.dirname("/a/b/c.txt"))
os.rename("data/blob.bin", "data/sub/blob.bin")
print(sorted(os.listdir("data/sub")))
p = Path("data") / "sub" / "hello.txt"
p.write_text("hi from pathlib\n")
print(p, p.name, p.stem, p.suffix, p.parent, p.exists(), p.read_text().strip())
print(sorted(str(x) for x in Path("data").iterdir()), sorted(q.name for q in Path("data").rglob("*.txt")))
shutil.copy("data/notes.txt", "data/copy.txt")
print(open("data/copy.txt").read() == open("data/notes.txt").read())
for root, dirs, files in os.walk("data"):
    print(root, sorted(dirs), sorted(files))
os.remove("data/copy.txt")
try:
    open("data/copy.txt")
except FileNotFoundError as e:
    print(e.errno, e.strerror, e.filename)
try:
    os.mkdir("data")
except FileExistsError as e:
    print("exists:", e)
try:
    open("data")
except IsADirectoryError as e:
    print("isdir:", e)
shutil.rmtree("data")
print(os.path.exists("data"), os.getcwd())
with open("log.txt", "w") as f:
    print("line A", file=f)
    print("line B", end="", file=f)
print(repr(open("log.txt").read()))
f = open("unclosed.txt", "w")
f.write("flushed at exit")
