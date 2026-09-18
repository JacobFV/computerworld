"""fractions for the simulated interpreter."""
import math
import numbers
import re

__all__ = ['Fraction']

_RATIONAL_FORMAT = re.compile(r"""
    \A\s*
    (?P<sign>[-+]?)
    (?=\d|\.\d)
    (?P<num>\d*|\d+(_\d+)*)
    (?:
        (?:\s*/\s*(?P<denom>\d+(_\d+)*))?
    |
        (?:\.(?P<decimal>\d*|\d+(_\d+)*))?
        (?:E(?P<exp>[-+]?\d+(_\d+)*))?
    )
    \s*\Z
""", re.VERBOSE | re.IGNORECASE)


class Fraction(numbers.Rational):
    def __new__(cls, numerator=0, denominator=None):
        self = object.__new__(cls)
        if denominator is None:
            if type(numerator) is int:
                self._numerator = numerator
                self._denominator = 1
                return self
            elif isinstance(numerator, Fraction):
                self._numerator = numerator._numerator
                self._denominator = numerator._denominator
                return self
            elif isinstance(numerator, float):
                if numerator != numerator or numerator in (float('inf'), float('-inf')):
                    raise ValueError(f"cannot convert {numerator!r} to integer ratio") if numerator != numerator else OverflowError("cannot convert Infinity to integer ratio")
                n, d = numerator.as_integer_ratio()
                self._numerator = n
                self._denominator = d
                return self
            elif isinstance(numerator, int):
                self._numerator = int(numerator)
                self._denominator = 1
                return self
            elif isinstance(numerator, str):
                m = _RATIONAL_FORMAT.match(numerator)
                if m is None:
                    raise ValueError('Invalid literal for Fraction: %r' % numerator)
                numerator = int(m.group('num') or '0')
                denom = m.group('denom')
                if denom:
                    denominator = int(denom)
                else:
                    denominator = 1
                    decimal = m.group('decimal')
                    if decimal:
                        decimal = decimal.replace('_', '')
                        scale = 10 ** len(decimal)
                        numerator = numerator * scale + int(decimal)
                        denominator *= scale
                    exp = m.group('exp')
                    if exp:
                        exp = int(exp)
                        if exp >= 0:
                            numerator *= 10 ** exp
                        else:
                            denominator *= 10 ** -exp
                if m.group('sign') == '-':
                    numerator = -numerator
            else:
                raise TypeError("argument should be a string or a Rational instance")
        elif type(numerator) is int is type(denominator):
            pass
        elif isinstance(numerator, Fraction) and isinstance(denominator, Fraction):
            numerator, denominator = (numerator._numerator * denominator._denominator,
                                      denominator._numerator * numerator._denominator)
        elif isinstance(numerator, int) and isinstance(denominator, int):
            numerator, denominator = int(numerator), int(denominator)
        else:
            raise TypeError("both arguments should be Rational instances")
        if denominator == 0:
            raise ZeroDivisionError('Fraction(%s, 0)' % numerator)
        g = math.gcd(numerator, denominator)
        if denominator < 0:
            g = -g
        numerator //= g
        denominator //= g
        self._numerator = numerator
        self._denominator = denominator
        return self

    @classmethod
    def from_float(cls, f):
        return cls(*f.as_integer_ratio())

    @classmethod
    def from_decimal(cls, dec):
        return cls(str(dec))

    def as_integer_ratio(self):
        return (self._numerator, self._denominator)

    def is_integer(self):
        return self._denominator == 1

    def limit_denominator(self, max_denominator=1000000):
        if max_denominator < 1:
            raise ValueError("max_denominator should be at least 1")
        if self._denominator <= max_denominator:
            return Fraction(self)
        p0, q0, p1, q1 = 0, 1, 1, 0
        n, d = self._numerator, self._denominator
        while True:
            a = n // d
            q2 = q0 + a * q1
            if q2 > max_denominator:
                break
            p0, q0, p1, q1 = p1, q1, p0 + a * p1, q2
            n, d = d, n - a * d
        k = (max_denominator - q0) // q1
        if 2 * d * (q0 + k * q1) <= self._denominator:
            return Fraction(p1, q1)
        else:
            return Fraction(p0 + k * p1, q0 + k * q1)

    numerator = property(lambda a: a._numerator)
    denominator = property(lambda a: a._denominator)

    def __repr__(self):
        return '%s(%s, %s)' % (self.__class__.__name__, self._numerator, self._denominator)

    def __str__(self):
        if self._denominator == 1:
            return str(self._numerator)
        return '%s/%s' % (self._numerator, self._denominator)

    def __format__(self, spec):
        if not spec:
            return str(self)
        return format(float(self), spec)

    def _operands(self, other):
        if isinstance(other, Fraction):
            return other
        if isinstance(other, int):
            return Fraction(other)
        return None

    def __add__(a, b):
        o = a._operands(b)
        if o is not None:
            return Fraction(a._numerator * o._denominator + o._numerator * a._denominator,
                            a._denominator * o._denominator)
        if isinstance(b, float):
            return float(a) + b
        if isinstance(b, complex):
            return complex(a) + b
        return NotImplemented

    def __radd__(b, a):
        return b.__add__(a)

    def __sub__(a, b):
        o = a._operands(b)
        if o is not None:
            return Fraction(a._numerator * o._denominator - o._numerator * a._denominator,
                            a._denominator * o._denominator)
        if isinstance(b, float):
            return float(a) - b
        return NotImplemented

    def __rsub__(b, a):
        o = b._operands(a)
        if o is not None:
            return o - b
        if isinstance(a, float):
            return a - float(b)
        return NotImplemented

    def __mul__(a, b):
        o = a._operands(b)
        if o is not None:
            return Fraction(a._numerator * o._numerator, a._denominator * o._denominator)
        if isinstance(b, float):
            return float(a) * b
        return NotImplemented

    def __rmul__(b, a):
        return b.__mul__(a)

    def __truediv__(a, b):
        o = a._operands(b)
        if o is not None:
            return Fraction(a._numerator * o._denominator, a._denominator * o._numerator)
        if isinstance(b, float):
            return float(a) / b
        return NotImplemented

    def __rtruediv__(b, a):
        o = b._operands(a)
        if o is not None:
            return o / b
        if isinstance(a, float):
            return a / float(b)
        return NotImplemented

    def __floordiv__(a, b):
        o = a._operands(b)
        if o is not None:
            return (a._numerator * o._denominator) // (a._denominator * o._numerator)
        return NotImplemented

    def __rfloordiv__(b, a):
        o = b._operands(a)
        if o is not None:
            return o // b
        return NotImplemented

    def __mod__(a, b):
        o = a._operands(b)
        if o is not None:
            da, db = a._denominator, o._denominator
            return Fraction((a._numerator * db) % (o._numerator * da), da * db)
        return NotImplemented

    def __divmod__(a, b):
        return (a // b, a % b)

    def __pow__(a, b):
        if isinstance(b, int) or (isinstance(b, Fraction) and b._denominator == 1):
            power = int(b) if not isinstance(b, int) else b
            if power >= 0:
                return Fraction(a._numerator ** power, a._denominator ** power)
            elif a._numerator > 0:
                return Fraction(a._denominator ** -power, a._numerator ** -power)
            elif a._numerator == 0:
                raise ZeroDivisionError('Fraction(%s, 0)' % a._denominator ** -power)
            else:
                return Fraction((-a._denominator) ** -power, (-a._numerator) ** -power)
        return float(a) ** float(b)

    def __rpow__(b, a):
        if b._denominator == 1 and b._numerator >= 0:
            return a ** b._numerator
        return a ** float(b)

    def __pos__(a):
        return Fraction(a._numerator, a._denominator)

    def __neg__(a):
        return Fraction(-a._numerator, a._denominator)

    def __abs__(a):
        return Fraction(abs(a._numerator), a._denominator)

    def __int__(a):
        if a._numerator < 0:
            return -(-a._numerator // a._denominator)
        return a._numerator // a._denominator

    def __trunc__(a):
        return a.__int__()

    def __floor__(a):
        return a._numerator // a._denominator

    def __ceil__(a):
        return -(-a._numerator // a._denominator)

    def __round__(self, ndigits=None):
        if ndigits is None:
            d = self._denominator
            floor, remainder = divmod(self._numerator, d)
            if remainder * 2 < d:
                return floor
            elif remainder * 2 > d:
                return floor + 1
            elif floor % 2 == 0:
                return floor
            else:
                return floor + 1
        shift = 10 ** abs(ndigits)
        if ndigits > 0:
            return Fraction(round(self * shift), shift)
        else:
            return Fraction(round(self / shift) * shift)

    def __float__(a):
        return a._numerator / a._denominator

    def __hash__(self):
        if self._denominator == 1:
            return hash(self._numerator)
        return hash(float(self))

    def __eq__(a, b):
        if isinstance(b, Fraction):
            return a._numerator == b._numerator and a._denominator == b._denominator
        if isinstance(b, int):
            return a._numerator == b and a._denominator == 1
        if isinstance(b, float):
            return float(a) == b
        return NotImplemented

    def _richcmp(self, other, op):
        if isinstance(other, (int, Fraction)):
            o = self._operands(other)
            return op(self._numerator * o._denominator, self._denominator * o._numerator)
        if isinstance(other, float):
            return op(float(self), other)
        return NotImplemented

    def __lt__(a, b):
        return a._richcmp(b, lambda x, y: x < y)

    def __gt__(a, b):
        return a._richcmp(b, lambda x, y: x > y)

    def __le__(a, b):
        return a._richcmp(b, lambda x, y: x <= y)

    def __ge__(a, b):
        return a._richcmp(b, lambda x, y: x >= y)

    def __bool__(a):
        return bool(a._numerator)
