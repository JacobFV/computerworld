# Regular expressions, JSON round-trips, CSV, and text munging.
import csv
import io
import json
import re

log = """2024-01-05 12:00:01 INFO  user=alice action=login ip=10.0.0.1
2024-01-05 12:03:15 ERROR user=bob action=upload size=1024 ip=10.0.0.7
2024-01-05 12:04:00 WARN  user=alice action=download size=2048
2024-01-06 08:30:44 ERROR user=carol action=login ip=192.168.1.20"""

pattern = re.compile(r"^(?P<date>\d{4}-\d{2}-\d{2}) (?P<time>[\d:]+) (?P<level>\w+)\s+(?P<rest>.*)$", re.M)
for m in pattern.finditer(log):
    fields = dict(kv.split("=") for kv in m.group("rest").split())
    print(m.group("level"), m["date"], fields.get("user"), fields.get("size", "-"))
print(re.findall(r"user=(\w+)", log))
print(re.findall(r"(\d+)\.(\d+)\.(\d+)\.(\d+)", log))
print(sorted(set(re.findall(r"ip=([\d.]+)", log))))
print(re.sub(r"\d{4}-(\d{2})-(\d{2})", r"\2/\1", "on 2024-01-05 and 2023-12-31"))
print(re.sub(r"(?P<word>\b\w{5}\b)", lambda m: m.group("word").upper(), "hello there world of regex"))
print(re.split(r"[,;]\s*", "a, b;c,  d"), re.split(r"(\s)", "a b"), re.split(r"x*", "axbc"))
print(re.match(r"\d+", "123abc").group(), re.match(r"\d+", "abc"), re.fullmatch(r"[a-z]+", "abc") is not None)
print(re.search(r"(a)(b)?", "ac").groups(), re.search(r"(a)(b)?", "ac").groups("-"), re.search(r"colou?r", "my color").span())
print(re.findall(r"\bfoo\b", "foo foobar barfoo foo."), re.findall(r"a.c", "abc a\nc", re.S), re.findall(r"^\w", "ab\ncd", re.M))
print(re.findall(r"(?i)hello", "Hello HELLO hello"), re.sub(r"\s+", " ", "  lots   of   space  ").strip())
print(re.findall(r"<.*?>", "<a><b></b>"), re.findall(r"<.*>", "<a><b></b>"), re.findall(r"(\w)\1", "aabbcd"))
print(re.escape("1+1=2?"), re.subn(r"o", "0", "foo boo"), re.findall(r"(?<=\$)\d+", "$10 and $20"))
print(re.findall(r"\w+(?=!)", "hi! bye. wow!"), re.findall(r"q(?!u)", "quit qat"), bool(re.search(r"^$", "")))
email = re.compile(r"[\w.+-]+@[\w-]+\.[\w.]+")
print(email.findall("contact: a.b+c@ex-ample.co.uk, bad@, x@y.io"))
print(pattern.pattern[:10], pattern.groups, sorted(pattern.groupindex.items()))
try:
    re.compile("(unclosed")
except re.error as e:
    print("re.error:", e)

record = {"name": "Widget", "price": 9.99, "tags": ["a", "b"], "stock": None, "active": True,
          "dims": {"w": 1.5, "h": 2}, "unicode": "café ☕", "big": 12345678901234567890}
encoded = json.dumps(record)
print(encoded)
print(json.dumps(record, indent=2, sort_keys=True, ensure_ascii=False))
decoded = json.loads(encoded)
print(decoded == record, decoded["dims"]["w"], type(decoded["big"]).__name__)
print(json.dumps([1, "two", 3.0, [None]], separators=(",", ":")), json.dumps("quote\"slash\\\n"), json.dumps(1e100))
print(json.loads('{"a": [1, 2, {"b": null}], "c": "\\u00e9\\n", "d": -1.5e3, "e": true}'))
try:
    json.loads('{"a": 1,}')
except json.JSONDecodeError as e:
    print("JSONDecodeError:", e, e.pos, e.lineno, e.colno)
try:
    json.dumps({1, 2})
except TypeError as e:
    print("TypeError:", e)
print(json.dumps({"z": 1, "a": 2}, sort_keys=True), json.dumps({1: "int key", True: "bool"}), json.loads("[]"), json.loads(" 42 "))


class Point:
    def __init__(self, x, y):
        self.x, self.y = x, y


print(json.dumps(Point(1, 2), default=lambda o: o.__dict__))

buf = io.StringIO()
writer = csv.writer(buf)
writer.writerow(["name", "quote", "n"])
writer.writerow(["Ann", 'said "hi", then left', 3])
writer.writerow(["Bob", "plain", 4.5])
print(repr(buf.getvalue()))
rows = list(csv.reader(io.StringIO(buf.getvalue())))
print(rows)
reader = csv.DictReader(io.StringIO("id,score\n1,90\n2,85\n"))
print([dict(r) for r in reader])
out = io.StringIO()
dw = csv.DictWriter(out, fieldnames=["k", "v"])
dw.writeheader()
dw.writerows([{"k": "x", "v": 1}, {"k": "y", "v": 2}])
print(out.getvalue().splitlines())

text = """It was the best of times, it was the worst of times,
it was the age of wisdom, it was the age of foolishness."""
freq = {}
for word in re.findall(r"[a-z]+", text.lower()):
    freq[word] = freq.get(word, 0) + 1
top = sorted(freq.items(), key=lambda kv: (-kv[1], kv[0]))[:5]
print(top)
print(" ".join(w.capitalize() for w in "snake_case_to_title".split("_")), "CamelCaseString".swapcase())
print(re.sub(r"(?<!^)(?=[A-Z])", "_", "CamelCaseString").lower())
print("".join(ch for ch in "Hello, World!" if ch.isalnum()), "racecar" == "racecar"[::-1], sorted("banana"), "".join(sorted(set("banana"))))
