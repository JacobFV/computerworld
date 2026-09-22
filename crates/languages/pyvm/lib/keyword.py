"""keyword for the simulated interpreter."""

kwlist = [
    'False', 'None', 'True', 'and', 'as', 'assert', 'async', 'await', 'break',
    'class', 'continue', 'def', 'del', 'elif', 'else', 'except', 'finally',
    'for', 'from', 'global', 'if', 'import', 'in', 'is', 'lambda', 'nonlocal',
    'not', 'or', 'pass', 'raise', 'return', 'try', 'while', 'with', 'yield'
]

softkwlist = ['_', 'case', 'match', 'type']

_kwset = frozenset(kwlist)
_softset = frozenset(softkwlist)


def iskeyword(s):
    return s in _kwset


def issoftkeyword(s):
    return s in _softset
