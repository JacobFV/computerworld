# String formatting: f-strings, format specs, str.format, % formatting, reprs.
import math

name, age, pi = "Ada", 36, math.pi
print(f"{name} is {age} years old; pi={pi:.3f}")
print(f"{name!r} {name!s} {name!a} {'é'!a}")
print(f"{age:5d}|{age:<5d}|{age:^5d}|{age:>5}|{age:05d}|{-age:05d}|{age:+d}")
print(f"{pi:10.4f}|{pi:<10.2f}|{pi:e}|{pi:.2E}|{pi:g}|{1e20:g}|{0.00001234:g}")
print(f"{1234567.891:,.2f}|{1234567:_}|{255:x}|{255:X}|{255:#o}|{5:b}|{255:#010b}")
print(f"{0.25:%}|{0.5:.0%}|{3:.1%}")
print(f"{'left':<10}|{'right':>10}|{'center':^10}|{'x':*^9}|{'pad':_<6}")
print(f"{12.5:.0f} {13.5:.0f} {2.675:.2f} {1.005:.2f} {-0.0:.1f}")
width, prec = 12, 3
print(f"{pi:{width}.{prec}f}|{'nested':>{width}}")
items = {"apple": 1.5, "banana": 0.25, "cherry": 10}
for k, v in items.items():
    print(f"{k:<8}${v:>7.2f}")
print(f"{age = }, {pi = :.2f}, {name=}")
print(f"{{literal braces}} {len(items)} {[x * 2 for x in range(3)]} {'yes' if age > 18 else 'no'}")
print("{} and {}".format("a", "b"), "{1} {0} {1}".format("x", "y"), "{name}-{n:03}".format(name="id", n=7))
print("{0[0]} {0[1]} {p.real}".format(["first", "second"], p=3 + 4j))
print("{:>6.2f}|{:<6}|{:^6}".format(3.14159, "ab", "c"))
print("%s is %d years, %.2f%% done, %5s|%-5s|%05.1f" % ("Bob", 42, 99.5, "r", "l", 3.14159))
print("%x %X %o %e %g %c %r" % (255, 255, 8, 12345.678, 0.0001, 65, "q"))
print("%(a)s + %(b)s" % {"a": 1, "b": 2}, "%%", "%5.1f%%" % 12.34)
print(str(1.0), str(1e16), str(1e-5), str(0.1 + 0.2), str(1/3), str(100.0), repr(1e100), repr(-0.0))
print(1e15, 1e16, 123456789012345678.0, 0.000123, 0.0001, 2.5e-7, float("inf"), float("-inf"), float("nan"))
print(round(2.5), round(3.5), round(-2.5), round(2.675, 2), round(1234.5678, -2), round(7, -1), round(15, -1))
print(repr("it's"), repr('say "hi"'), repr("both ' \""), repr("tab\tnewline\n\\"), repr("\x00\x7fé中"))
print(str(b"bytes\x00\xff"), repr(bytearray(b"ab")), b"abc".decode(), "héllo".encode())
print("|".join(["a", "b", "c"]), "a,b,,c".split(","), "  x  y  ".split(), "a b c".split(" ", 1), "a.b.c".rsplit(".", 1))
print("Hello World".lower(), "hello world".title(), "hello".capitalize(), "Hello".swapcase(), "ß".upper(), "ǅ".lower())
print("abc".center(9, "*"), "abc".ljust(6, "-"), "abc".rjust(6), "42".zfill(5), "-42".zfill(5))
print("  strip  ".strip(), "xxhixx".strip("x"), "hello".replace("l", "L"), "hello".replace("l", "L", 1))
print("hello".find("l"), "hello".rfind("l"), "hello".find("z"), "hello".count("l"), "hello".index("e"))
print("hello".startswith("he"), "hello".endswith(("lo", "x")), "123".isdigit(), "abc".isalpha(), "a1".isalnum(), " ".isspace())
print("Hello"[1:4], "Hello"[::-1], "Hello"[-3:], "Hello"[::2], "Hello"[10:], "abcdef"[1:-1:2])
print("tab\tsep".expandtabs(4), "line1\nline2\r\nline3".splitlines(), "a-b-c".partition("-"), "a-b-c".rpartition("-"))
print("%s" % [1, 2], "%s" % ((1, 2),), "{!r:>10}".format("x"), format(3.14159, ".2f"), format(42, "08b"), format("s", "^5"))
table = [("Name", "Qty"), ("apple", 3), ("kiwi", 12)]
for row in table:
    print("{:<8}{:>5}".format(*row))
print(f"{3+4j}", f"{complex(1, -1)}", str(2j), (1 + 2j) * (3 - 1j), abs(3 + 4j))
print(f"{10**20}", f"{-7 // 2}", f"{7 % -3}", f"{2 ** -1}", f"{divmod(-7, 2)}", f"{int('ff', 16)}", f"{int('0b101', 0)}")
print(ascii("日本"), chr(9731), ord("€"), hex(-255), bin(10), oct(64), int(" 42 "), float(" 3.5 "), int(-3.9))
