"""numbers: the numeric abstract base classes."""
from abc import ABCMeta


class Number(metaclass=ABCMeta):
    __hash__ = None


class Complex(Number):
    pass


class Real(Complex):
    pass


class Rational(Real):
    pass


class Integral(Rational):
    pass


Complex.register(complex)
Real.register(float)
Integral.register(int)
Rational.register(int)
Real.register(int)
Complex.register(int)
Complex.register(float)
Number.register(int)
Number.register(float)
Number.register(complex)
