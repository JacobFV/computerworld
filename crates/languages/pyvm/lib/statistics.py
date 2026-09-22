"""statistics for the simulated interpreter."""
import math
from fractions import Fraction
from collections import Counter

__all__ = ['StatisticsError', 'mean', 'fmean', 'geometric_mean', 'harmonic_mean',
           'median', 'median_low', 'median_high', 'median_grouped', 'mode', 'multimode',
           'pstdev', 'pvariance', 'stdev', 'variance', 'quantiles', 'correlation',
           'covariance', 'linear_regression']


class StatisticsError(ValueError):
    pass


def _exact(x):
    if isinstance(x, float):
        return Fraction(*x.as_integer_ratio())
    return Fraction(x)


def _convert(value, T):
    if T is float:
        return float(value)
    if T is int:
        if value.denominator == 1:
            return int(value.numerator)
        return float(value)
    if T is Fraction:
        return value
    return T(value)


def _result_type(data):
    T = int
    for x in data:
        if isinstance(x, float):
            T = float
        elif isinstance(x, Fraction) and T is int:
            T = Fraction
    return T


def mean(data):
    data = list(data)
    n = len(data)
    if n < 1:
        raise StatisticsError('mean requires at least one data point')
    T = _result_type(data)
    total = sum(_exact(x) for x in data)
    return _convert(total / n, T)


def fmean(data, weights=None):
    data = list(data)
    if weights is None:
        n = len(data)
        if not n:
            raise StatisticsError('fmean requires at least one data point')
        return math.fsum(data) / n
    weights = list(weights)
    return math.fsum(x * w for x, w in zip(data, weights)) / math.fsum(weights)


def geometric_mean(data):
    data = list(data)
    try:
        return math.exp(fmean([math.log(x) for x in data]))
    except ValueError:
        raise StatisticsError('geometric mean requires a non-empty dataset containing positive numbers') from None


def harmonic_mean(data, weights=None):
    data = list(data)
    if not data:
        raise StatisticsError('harmonic_mean requires at least one data point')
    if any(x < 0 for x in data):
        raise StatisticsError('harmonic mean does not support negative values')
    if any(x == 0 for x in data):
        return 0
    T = _result_type(data)
    total = sum(1 / _exact(x) for x in data)
    return _convert(len(data) / total, T)


def median(data):
    data = sorted(data)
    n = len(data)
    if n == 0:
        raise StatisticsError("no median for empty data")
    if n % 2 == 1:
        return data[n // 2]
    i = n // 2
    return (data[i - 1] + data[i]) / 2


def median_low(data):
    data = sorted(data)
    n = len(data)
    if n == 0:
        raise StatisticsError("no median for empty data")
    if n % 2 == 1:
        return data[n // 2]
    return data[n // 2 - 1]


def median_high(data):
    data = sorted(data)
    n = len(data)
    if n == 0:
        raise StatisticsError("no median for empty data")
    return data[n // 2]


def median_grouped(data, interval=1.0):
    data = sorted(data)
    n = len(data)
    if not n:
        raise StatisticsError("no median for empty data")
    x = data[n // 2]
    L = x - interval / 2
    cf = sum(1 for v in data if v < x)
    f = sum(1 for v in data if v == x)
    return L + interval * (n / 2 - cf) / f


def mode(data):
    pairs = Counter(iter(data)).most_common(1)
    try:
        return pairs[0][0]
    except IndexError:
        raise StatisticsError('no mode for empty data') from None


def multimode(data):
    counts = Counter(iter(data))
    if not counts:
        return []
    maxcount = max(counts.values())
    return [value for value, count in counts.items() if count == maxcount]


def _ss(data, c=None):
    if c is None:
        c = sum(_exact(x) for x in data) / len(data)
    else:
        c = _exact(c)
    total = sum((_exact(x) - c) ** 2 for x in data)
    total2 = sum((_exact(x) - c) for x in data)
    total -= total2 ** 2 / len(data)
    return total


def variance(data, xbar=None):
    data = list(data)
    n = len(data)
    if n < 2:
        raise StatisticsError('variance requires at least two data points')
    T = _result_type(data)
    return _convert(_ss(data, xbar) / (n - 1), T)


def pvariance(data, mu=None):
    data = list(data)
    n = len(data)
    if n < 1:
        raise StatisticsError('pvariance requires at least one data point')
    T = _result_type(data)
    return _convert(_ss(data, mu) / n, T)


def _integer_sqrt_of_frac_rto(n, m):
    a = math.isqrt(n // m)
    return a | (a * a * m != n)


def _float_sqrt_of_frac(n, m):
    q = (n.bit_length() - m.bit_length() - 109) // 2
    if q >= 0:
        numerator = _integer_sqrt_of_frac_rto(n, m << 2 * q) << q
        denominator = 1
    else:
        numerator = _integer_sqrt_of_frac_rto(n << -2 * q, m)
        denominator = 1 << -q
    return numerator / denominator


def stdev(data, xbar=None):
    data = list(data)
    n = len(data)
    if n < 2:
        raise StatisticsError('stdev requires at least two data points')
    mss = _ss(data, xbar) / (n - 1)
    return _float_sqrt_of_frac(mss.numerator, mss.denominator)


def pstdev(data, mu=None):
    data = list(data)
    n = len(data)
    if n < 1:
        raise StatisticsError('pstdev requires at least one data point')
    mss = _ss(data, mu) / n
    return _float_sqrt_of_frac(mss.numerator, mss.denominator)


def quantiles(data, *, n=4, method='exclusive'):
    if n < 1:
        raise StatisticsError('n must be at least 1')
    data = sorted(data)
    ld = len(data)
    if ld < 2:
        raise StatisticsError('must have at least two data points')
    if method == 'inclusive':
        m = ld - 1
        result = []
        for i in range(1, n):
            j, delta = divmod(i * m, n)
            interpolated = (data[j] * (n - delta) + data[j + 1] * delta) / n
            result.append(interpolated)
        return result
    m = ld + 1
    result = []
    for i in range(1, n):
        j = i * m // n
        j = 1 if j < 1 else ld - 1 if j > ld - 1 else j
        delta = i * m - j * n
        interpolated = (data[j - 1] * (n - delta) + data[j] * delta) / n
        result.append(interpolated)
    return result


def covariance(x, y):
    n = len(x)
    if len(y) != n:
        raise StatisticsError('covariance requires that both inputs have same number of data points')
    if n < 2:
        raise StatisticsError('covariance requires at least two data points')
    xbar = fmean(x)
    ybar = fmean(y)
    sxy = math.fsum((xi - xbar) * (yi - ybar) for xi, yi in zip(x, y))
    return sxy / (n - 1)


def correlation(x, y):
    n = len(x)
    if len(y) != n:
        raise StatisticsError('correlation requires that both inputs have same number of data points')
    if n < 2:
        raise StatisticsError('correlation requires at least two data points')
    xbar = fmean(x)
    ybar = fmean(y)
    sxy = math.fsum((xi - xbar) * (yi - ybar) for xi, yi in zip(x, y))
    sxx = math.fsum((d := xi - xbar) * d for xi in x)
    syy = math.fsum((d := yi - ybar) * d for yi in y)
    try:
        return sxy / math.sqrt(sxx * syy)
    except ZeroDivisionError:
        raise StatisticsError('at least one of the inputs is constant')


class LinearRegression(tuple):
    slope = property(lambda s: s[0])
    intercept = property(lambda s: s[1])

    def __repr__(self):
        return f'LinearRegression(slope={self[0]!r}, intercept={self[1]!r})'


def linear_regression(x, y, /, *, proportional=False):
    n = len(x)
    if len(y) != n:
        raise StatisticsError('linear regression requires that both inputs have same number of data points')
    if n < 2:
        raise StatisticsError('linear regression requires at least two data points')
    if not proportional:
        xbar = fmean(x)
        ybar = fmean(y)
        x = [xi - xbar for xi in x]
        y = [yi - ybar for yi in y]
    sxy = math.fsum(xi * yi for xi, yi in zip(x, y))
    sxx = math.fsum(xi * xi for xi in x)
    try:
        slope = sxy / sxx
    except ZeroDivisionError:
        raise StatisticsError('x is constant')
    intercept = 0.0 if proportional else ybar - slope * xbar
    return LinearRegression((slope, intercept))
