import sys

print("before exit")
print("to stderr", file=sys.stderr)
try:
    sys.exit(4)
except SystemExit as e:
    print("caught exit", e.code)
sys.stdout.write("partial line")
sys.exit(3)
