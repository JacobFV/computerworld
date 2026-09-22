# Reads a header line with input(), then the remaining lines from sys.stdin.
import sys

name = input("Your name: ")
print(f"Hello, {name}!")
count = int(input())
numbers = [int(input()) for _ in range(count)]
print("sum:", sum(numbers), "max:", max(numbers))
rest = [line.rstrip("\n") for line in sys.stdin]
print("remaining lines:", rest)
words = " ".join(rest).split()
print("word count:", len(words))
try:
    input()
except EOFError as e:
    print("EOFError:", e)
