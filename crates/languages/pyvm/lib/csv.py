"""csv for the simulated interpreter (reader/writer/DictReader/DictWriter)."""

QUOTE_MINIMAL, QUOTE_ALL, QUOTE_NONNUMERIC, QUOTE_NONE, QUOTE_STRINGS, QUOTE_NOTNULL = range(6)


class Error(Exception):
    pass


class Dialect:
    delimiter = ','
    quotechar = '"'
    escapechar = None
    doublequote = True
    skipinitialspace = False
    lineterminator = '\r\n'
    quoting = QUOTE_MINIMAL
    strict = False


class excel(Dialect):
    pass


class excel_tab(excel):
    delimiter = '\t'


class unix_dialect(Dialect):
    lineterminator = '\n'
    quoting = QUOTE_ALL


_dialects = {'excel': excel, 'excel-tab': excel_tab, 'unix': unix_dialect}


def register_dialect(name, dialect=None, **fmtparams):
    _dialects[name] = dialect or type(name, (Dialect,), fmtparams)


def get_dialect(name):
    return _dialects[name]


def list_dialects():
    return list(_dialects)


class _Params:
    def __init__(self, dialect, fmtparams):
        if isinstance(dialect, str):
            dialect = _dialects[dialect]
        for name in ('delimiter', 'quotechar', 'escapechar', 'doublequote', 'skipinitialspace',
                     'lineterminator', 'quoting', 'strict'):
            setattr(self, name, fmtparams.get(name, getattr(dialect, name)))


class reader:
    def __init__(self, f, dialect='excel', **fmtparams):
        self._it = iter(f)
        self._p = _Params(dialect, fmtparams)
        self.line_num = 0

    def __iter__(self):
        return self

    def __next__(self):
        p = self._p
        line = next(self._it)
        self.line_num += 1
        fields = []
        field = ''
        in_quotes = False
        quoted = False
        i = 0
        while True:
            if i >= len(line):
                if in_quotes:
                    try:
                        line += next(self._it)
                        self.line_num += 1
                        continue
                    except StopIteration:
                        if p.strict:
                            raise Error('unexpected end of data')
                        break
                break
            c = line[i]
            if in_quotes:
                if p.escapechar and c == p.escapechar and i + 1 < len(line):
                    field += line[i + 1]
                    i += 2
                    continue
                if c == p.quotechar:
                    if p.doublequote and i + 1 < len(line) and line[i + 1] == p.quotechar:
                        field += c
                        i += 2
                        continue
                    in_quotes = False
                    i += 1
                    continue
                field += c
                i += 1
                continue
            if c == p.delimiter:
                fields.append(self._convert(field, quoted))
                field = ''
                quoted = False
                i += 1
                if p.skipinitialspace:
                    while i < len(line) and line[i] == ' ':
                        i += 1
                continue
            if c in '\r\n':
                i += 1
                continue
            if c == p.quotechar and p.quoting != QUOTE_NONE and field == '':
                in_quotes = True
                quoted = True
                i += 1
                continue
            if p.escapechar and c == p.escapechar and i + 1 < len(line):
                field += line[i + 1]
                i += 2
                continue
            field += c
            i += 1
        if fields or field or quoted or line.strip('\r\n'):
            fields.append(self._convert(field, quoted))
        return fields

    def _convert(self, field, quoted):
        if self._p.quoting == QUOTE_NONNUMERIC and not quoted and field != '':
            return float(field)
        return field


class writer:
    def __init__(self, f, dialect='excel', **fmtparams):
        self._f = f
        self._p = _Params(dialect, fmtparams)

    def _quote(self, value):
        p = self._p
        if value is None:
            s = ''
        elif isinstance(value, float):
            s = repr(value)
        else:
            s = str(value)
        need = p.quoting == QUOTE_ALL or (
            p.quoting == QUOTE_NONNUMERIC and not isinstance(value, (int, float))) or (
            p.quoting == QUOTE_STRINGS and isinstance(value, str)) or (
            p.quoting == QUOTE_NOTNULL and value is not None)
        if not need and p.quoting != QUOTE_NONE:
            need = (s == '' and False) or any(ch in s for ch in (p.delimiter, p.quotechar, '\n', '\r')) or (s.startswith(' ') and False)
        if p.quoting == QUOTE_NONE:
            if p.escapechar:
                for ch in (p.escapechar, p.delimiter, p.quotechar):
                    if ch:
                        s = s.replace(ch, p.escapechar + ch)
            return s
        if need:
            if p.doublequote:
                s = s.replace(p.quotechar, p.quotechar * 2)
            elif p.escapechar:
                s = s.replace(p.quotechar, p.escapechar + p.quotechar)
            return p.quotechar + s + p.quotechar
        return s

    def writerow(self, row):
        line = self._p.delimiter.join(self._quote(v) for v in row) + self._p.lineterminator
        return self._f.write(line)

    def writerows(self, rows):
        for row in rows:
            self.writerow(row)


class DictReader:
    def __init__(self, f, fieldnames=None, restkey=None, restval=None, dialect='excel', **kwds):
        self._fieldnames = fieldnames
        self.restkey = restkey
        self.restval = restval
        self.reader = reader(f, dialect, **kwds)
        self.line_num = 0

    @property
    def fieldnames(self):
        if self._fieldnames is None:
            try:
                self._fieldnames = next(self.reader)
            except StopIteration:
                pass
        self.line_num = self.reader.line_num
        return self._fieldnames

    def __iter__(self):
        return self

    def __next__(self):
        if self.line_num == 0:
            self.fieldnames
        row = next(self.reader)
        self.line_num = self.reader.line_num
        while row == []:
            row = next(self.reader)
        d = dict(zip(self.fieldnames, row))
        lf = len(self.fieldnames)
        lr = len(row)
        if lf < lr:
            d[self.restkey] = row[lf:]
        elif lf > lr:
            for key in self.fieldnames[lr:]:
                d[key] = self.restval
        return d


class DictWriter:
    def __init__(self, f, fieldnames, restval="", extrasaction="raise", dialect="excel", **kwds):
        self.fieldnames = fieldnames
        self.restval = restval
        self.extrasaction = extrasaction
        self.writer = writer(f, dialect, **kwds)

    def writeheader(self):
        return self.writer.writerow(self.fieldnames)

    def _dict_to_list(self, rowdict):
        if self.extrasaction == "raise":
            wrong_fields = rowdict.keys() - self.fieldnames
            if wrong_fields:
                raise ValueError("dict contains fields not in fieldnames: " + ", ".join([repr(x) for x in wrong_fields]))
        return (rowdict.get(key, self.restval) for key in self.fieldnames)

    def writerow(self, rowdict):
        return self.writer.writerow(self._dict_to_list(rowdict))

    def writerows(self, rowdicts):
        for r in rowdicts:
            self.writerow(r)


class Sniffer:
    def sniff(self, sample, delimiters=None):
        for d in (delimiters or ',;\t|'):
            if d in sample:
                class _D(Dialect):
                    delimiter = d
                return _D
        raise Error("Could not determine delimiter")

    def has_header(self, sample):
        return True
