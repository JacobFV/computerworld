"""shlex: POSIX-shell-like lexing (split, quote, join)."""
import re

__all__ = ["shlex", "split", "quote", "join"]

_find_unsafe = re.compile(r'[^\w@%+=:,./-]', re.ASCII).search


def quote(s):
    if not s:
        return "''"
    if _find_unsafe(s) is None:
        return s
    return "'" + s.replace("'", "'\"'\"'") + "'"


def join(split_command):
    return ' '.join(quote(arg) for arg in split_command)


class shlex:
    def __init__(self, instream=None, infile=None, posix=False, punctuation_chars=False):
        if isinstance(instream, str):
            self._text = instream
        elif instream is None:
            import sys
            self._text = sys.stdin.read()
        else:
            self._text = instream.read()
        self.posix = posix
        self.whitespace_split = False
        self.commenters = '#'
        self.wordchars = ('abcdfeghijklmnopqrstuvwxyz'
                          'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_')
        self.whitespace = ' \t\r\n'
        self.quotes = '\'"'
        self.escape = '\\'
        self.escapedquotes = '"'
        self.eof = None if posix else ''
        self._tokens = None

    def _lex(self):
        text = self._text
        tokens = []
        i = 0
        n = len(text)
        while i < n:
            c = text[i]
            if c in self.whitespace:
                i += 1
                continue
            if c in self.commenters:
                while i < n and text[i] != '\n':
                    i += 1
                continue
            word = []
            quoted = False
            if not self.whitespace_split and c not in self.wordchars and c not in self.quotes \
                    and not (self.posix and c in self.escape):
                tokens.append(c)
                i += 1
                continue
            while i < n:
                c = text[i]
                if c in self.whitespace:
                    break
                if not self.whitespace_split and c not in self.wordchars \
                        and c not in self.quotes and not (self.posix and c in self.escape) \
                        and c not in '.-/~:@%+=,*?[]':
                    break
                if c in self.quotes:
                    quoted = True
                    q = c
                    i += 1
                    if not self.posix:
                        word.append(q)
                    while True:
                        if i >= n:
                            raise ValueError("No closing quotation")
                        c = text[i]
                        if c == q:
                            i += 1
                            if not self.posix:
                                word.append(q)
                            break
                        if self.posix and c in self.escape and q in self.escapedquotes:
                            if i + 1 >= n:
                                raise ValueError("No escaped character")
                            nxt = text[i + 1]
                            if nxt in (q, c):
                                word.append(nxt)
                            else:
                                word.append(c)
                                word.append(nxt)
                            i += 2
                            continue
                        word.append(c)
                        i += 1
                    continue
                if self.posix and c in self.escape:
                    if i + 1 >= n:
                        raise ValueError("No escaped character")
                    word.append(text[i + 1])
                    i += 2
                    continue
                word.append(c)
                i += 1
            if word or quoted:
                tokens.append(''.join(word))
        return tokens

    def get_token(self):
        if self._tokens is None:
            self._tokens = self._lex()
        if self._tokens:
            return self._tokens.pop(0)
        return self.eof

    def __iter__(self):
        return self

    def __next__(self):
        token = self.get_token()
        if token == self.eof:
            raise StopIteration
        return token


def split(s, comments=False, posix=True):
    if s is None:
        raise ValueError("s argument must not be None")
    lex = shlex(s, posix=posix)
    lex.whitespace_split = True
    if not comments:
        lex.commenters = ''
    return list(lex)
