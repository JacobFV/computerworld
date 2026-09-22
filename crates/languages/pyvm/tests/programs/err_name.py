def compute(values):
    result = 0
    for v in values:
        result += v * factr
    return result


factor = 2
print(compute([1, 2, 3]))
