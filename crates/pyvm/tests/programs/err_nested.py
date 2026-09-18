# An uncaught error deep in a call chain: full traceback with carets, exit 1.
import json


def parse(text):
    return json.loads(text)


def total(orders):
    return sum(o["qty"] * o["price"] for o in orders)


def report(raw):
    orders = parse(raw)
    print("orders:", len(orders))
    return total(orders)


print("start")
print(report('[{"qty": 2, "price": 3.5}, {"qty": 1, "price": 10}]'))
print(report('[{"qty": 2, "price": 3.5}, {"qty": 1}]'))
print("unreachable")
