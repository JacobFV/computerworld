# argparse, logging, pprint, unittest-style checks, asyncio, typing, abc.
import argparse
import asyncio
import logging
import pprint
import sys
from abc import ABC, abstractmethod
from typing import Dict, List, NamedTuple, Optional

parser = argparse.ArgumentParser(prog="tool", description="Process some numbers.")
parser.add_argument("numbers", type=int, nargs="+", help="numbers to add")
parser.add_argument("--scale", type=float, default=1.0, help="multiplier")
parser.add_argument("-v", "--verbose", action="store_true")
parser.add_argument("--mode", choices=["sum", "max"], default="sum")
args = parser.parse_args(["3", "4", "5", "--scale", "2", "-v"])
print(args, args.numbers, args.scale * sum(args.numbers))
print(parser.parse_args(["1", "--mode", "max"]).mode)
print(parser.format_help())
try:
    parser.parse_args(["x"])
except SystemExit as e:
    print("exit code", e.code)
try:
    parser.parse_args([])
except SystemExit as e:
    print("exit code", e.code)

logging.basicConfig(level=logging.INFO, format="%(levelname)s:%(name)s:%(message)s", stream=sys.stdout)
log = logging.getLogger("app")
log.debug("hidden")
log.info("processing %d items", 3)
log.warning("careful: %s", "low disk")
logging.getLogger("app.db").error("connection failed")

config = {"servers": [{"host": "alpha.example.com", "ports": [80, 443], "tags": ["web", "primary"]},
                      {"host": "beta.example.com", "ports": [22], "tags": []}],
          "retries": 3, "name": "cluster", "zeta": None}
pprint.pprint(config)
pprint.pprint(list(range(30)), width=40)
print(pprint.pformat({"b": 1, "a": 2}))


class Shape(ABC):
    @abstractmethod
    def area(self) -> float:
        ...

    def describe(self) -> str:
        return f"{type(self).__name__} area={self.area()}"


class Sq(Shape):
    def __init__(self, s: float) -> None:
        self.s = s

    def area(self) -> float:
        return self.s ** 2


print(Sq(3).describe())
try:
    Shape()
except TypeError as e:
    print("TypeError:", e)


class Employee(NamedTuple):
    name: str
    dept: str = "eng"


def team(members: List[Employee], lead: Optional[str] = None) -> Dict[str, int]:
    out: Dict[str, int] = {}
    for m in members:
        out[m.dept] = out.get(m.dept, 0) + 1
    return out


staff = [Employee("a"), Employee("b", "ops"), Employee("c")]
print(team(staff), staff[1], staff[0].dept, team.__annotations__["return"])


async def fetch(name, delay, log):
    log.append(f"start {name}")
    await asyncio.sleep(delay)
    log.append(f"done {name}")
    return name.upper()


async def main():
    log = []
    results = await asyncio.gather(fetch("a", 0.2, log), fetch("b", 0.1, log), fetch("c", 0.3, log))
    print(results)
    print(log)
    task = asyncio.create_task(fetch("d", 0.05, log))
    print(await task, task.done())
    q = asyncio.Queue()

    async def producer():
        for i in range(3):
            await q.put(i)
            await asyncio.sleep(0.01)
        await q.put(None)

    async def consumer():
        got = []
        while (item := await q.get()) is not None:
            got.append(item)
        return got
    _, got = await asyncio.gather(producer(), consumer())
    print("consumed", got)
    try:
        await asyncio.wait_for(asyncio.sleep(10), timeout=0.5)
    except asyncio.TimeoutError:
        print("timed out")
    return "main done"


print(asyncio.run(main()))
