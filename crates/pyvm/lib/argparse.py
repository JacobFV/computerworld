"""argparse for the simulated interpreter (common features, CPython-style output)."""
import os as _os
import sys as _sys

__all__ = ['ArgumentParser', 'ArgumentError', 'Namespace', 'FileType', 'HelpFormatter',
           'RawDescriptionHelpFormatter', 'RawTextHelpFormatter', 'ArgumentDefaultsHelpFormatter',
           'SUPPRESS', 'OPTIONAL', 'ZERO_OR_MORE', 'ONE_OR_MORE', 'REMAINDER', 'BooleanOptionalAction']

SUPPRESS = '==SUPPRESS=='
OPTIONAL = '?'
ZERO_OR_MORE = '*'
ONE_OR_MORE = '+'
PARSER = 'A...'
REMAINDER = '...'


class Namespace:
    def __init__(self, **kwargs):
        for name in kwargs:
            setattr(self, name, kwargs[name])

    def __repr__(self):
        args = ', '.join(f'{k}={v!r}' for k, v in self.__dict__.items())
        return f'Namespace({args})'

    def __eq__(self, other):
        if not isinstance(other, Namespace):
            return NotImplemented
        return vars(self) == vars(other)

    def __contains__(self, key):
        return key in self.__dict__


class ArgumentError(Exception):
    def __init__(self, argument, message):
        self.argument_name = None if argument is None else _action_name(argument)
        self.message = message

    def __str__(self):
        if self.argument_name is None:
            return self.message
        return f'argument {self.argument_name}: {self.message}'


class ArgumentTypeError(Exception):
    pass


def _action_name(action):
    if action.option_strings:
        return '/'.join(action.option_strings)
    if action.metavar not in (None, SUPPRESS):
        return action.metavar
    if action.dest not in (None, SUPPRESS):
        return action.dest
    return None


class Action:
    def __init__(self, option_strings, dest, nargs=None, const=None, default=None, type=None,
                 choices=None, required=False, help=None, metavar=None, action=None):
        self.option_strings = option_strings
        self.dest = dest
        self.nargs = nargs
        self.const = const
        self.default = default
        self.type = type
        self.choices = choices
        self.required = required
        self.help = help
        self.metavar = metavar
        self.kind = action or 'store'


class FileType:
    def __init__(self, mode='r', bufsize=-1, encoding=None, errors=None):
        self._mode = mode

    def __call__(self, string):
        if string == '-':
            return _sys.stdin if 'r' in self._mode else _sys.stdout
        return open(string, self._mode)


class BooleanOptionalAction:
    pass


class HelpFormatter:
    def __init__(self, prog, indent_increment=2, max_help_position=24, width=None):
        self.prog = prog
        self.width = (width or 80) - 2
        self.max_help_position = min(max_help_position, max(self.width - 20, 2 * indent_increment))

    def _fill(self, text, width, indent):
        import textwrap
        text = ' '.join(text.split())
        return textwrap.fill(text, width, initial_indent=indent, subsequent_indent=indent)


class RawDescriptionHelpFormatter(HelpFormatter):
    def _fill(self, text, width, indent):
        return ''.join(indent + line for line in text.splitlines(keepends=True))


class RawTextHelpFormatter(RawDescriptionHelpFormatter):
    pass


class ArgumentDefaultsHelpFormatter(HelpFormatter):
    pass


class _MutuallyExclusiveGroup:
    def __init__(self, parser, required=False):
        self._parser = parser
        self.required = required

    def add_argument(self, *args, **kwargs):
        return self._parser.add_argument(*args, **kwargs)


class _ArgumentGroup:
    def __init__(self, parser, title=None, description=None):
        self._parser = parser
        self.title = title
        self.description = description
        self._actions = []

    def add_argument(self, *args, **kwargs):
        a = self._parser.add_argument(*args, _group=self, **kwargs)
        return a

    def add_mutually_exclusive_group(self, required=False):
        return _MutuallyExclusiveGroup(self, required)


class _SubParsersAction(Action):
    def __init__(self, parser, dest=SUPPRESS, title=None, description=None, help=None, metavar=None, required=False):
        super().__init__([], dest, nargs=PARSER, help=help, metavar=metavar, required=required, action='parsers')
        self._parent = parser
        self._choices = {}
        self.choices = self._choices
        self.title = title

    def add_parser(self, name, **kwargs):
        kwargs.setdefault('prog', f'{self._parent.prog} {name}')
        aliases = kwargs.pop('aliases', ())
        help_ = kwargs.pop('help', None)
        parser = ArgumentParser(**kwargs)
        self._choices[name] = parser
        for a in aliases:
            self._choices[a] = parser
        return parser


class ArgumentParser:
    def __init__(self, prog=None, usage=None, description=None, epilog=None, parents=(),
                 formatter_class=HelpFormatter, prefix_chars='-', fromfile_prefix_chars=None,
                 argument_default=None, conflict_handler='error', add_help=True,
                 allow_abbrev=True, exit_on_error=True):
        if prog is None:
            prog = _os.path.basename(_sys.argv[0]) if _sys.argv and _sys.argv[0] else 'main.py'
        self.prog = prog
        self.usage = usage
        self.description = description
        self.epilog = epilog
        self.formatter_class = formatter_class
        self.prefix_chars = prefix_chars
        self.argument_default = argument_default
        self.allow_abbrev = allow_abbrev
        self.exit_on_error = exit_on_error
        self._actions = []
        self._groups = []
        self._defaults = {}
        self._subparsers = None
        if add_help:
            self.add_argument('-h', '--help', action='help', default=SUPPRESS,
                              help='show this help message and exit')
        for parent in parents:
            for a in parent._actions:
                if a.kind != 'help':
                    self._actions.append(a)

    def add_argument(self, *args, _group=None, **kwargs):
        chars = self.prefix_chars
        if not args or len(args) == 1 and args[0][0] not in chars:
            if args and 'dest' in kwargs:
                raise ValueError('dest supplied twice for positional argument')
            dest = kwargs.pop('dest', args[0] if args else None)
            option_strings = []
            if kwargs.get('nargs') not in (OPTIONAL, ZERO_OR_MORE, REMAINDER) and 'required' not in kwargs:
                kwargs['required'] = True
            if kwargs.get('nargs') == ZERO_OR_MORE and 'default' not in kwargs:
                kwargs['required'] = False
        else:
            option_strings = list(args)
            dest = kwargs.pop('dest', None)
            if dest is None:
                long_opts = [o for o in option_strings if len(o) > 1 and o[1] in chars]
                first = (long_opts or option_strings)[0]
                dest = first.lstrip(chars).replace('-', '_')
        action = kwargs.pop('action', 'store')
        if action is BooleanOptionalAction:
            action = 'boolean_optional'
            option_strings = option_strings + ['--no-' + o[2:] for o in option_strings if o.startswith('--')]
        if 'default' not in kwargs:
            if action in ('store_true',):
                kwargs['default'] = False
            elif action == 'store_false':
                kwargs['default'] = True
            elif dest in self._defaults:
                kwargs['default'] = self._defaults[dest]
            else:
                kwargs['default'] = self.argument_default
        if action == 'store_const' and 'const' not in kwargs:
            raise TypeError("__init__() missing 1 required positional argument: 'const'")
        if action == 'version':
            kwargs.setdefault('help', "show program's version number and exit")
            self._version = kwargs.pop('version', None)
        if action == 'count' and kwargs.get('default') is None:
            pass
        typ = kwargs.get('type')
        if typ is not None and not callable(typ):
            raise ValueError(f'{typ!r} is not callable')
        a = Action(option_strings, dest, action=action, **kwargs)
        if action in ('store_true', 'store_false', 'store_const', 'count', 'help', 'version',
                      'append_const', 'boolean_optional'):
            a.nargs = 0
        if action == 'store_true':
            a.const = True
        if action == 'store_false':
            a.const = False
        for existing in self._actions:
            for o in option_strings:
                if o in existing.option_strings:
                    raise ArgumentError(a, f'conflicting option string: {o}')
        self._actions.append(a)
        if _group is not None:
            _group._actions.append(a)
        return a

    def add_argument_group(self, title=None, description=None):
        g = _ArgumentGroup(self, title, description)
        self._groups.append(g)
        return g

    def add_mutually_exclusive_group(self, required=False):
        return _MutuallyExclusiveGroup(self, required)

    def add_subparsers(self, **kwargs):
        dest = kwargs.pop('dest', SUPPRESS)
        sp = _SubParsersAction(self, dest=dest, **kwargs)
        self._subparsers = sp
        self._actions.append(sp)
        return sp

    def set_defaults(self, **kwargs):
        self._defaults.update(kwargs)
        for a in self._actions:
            if a.dest in kwargs:
                a.default = kwargs[a.dest]

    def get_default(self, dest):
        for a in self._actions:
            if a.dest == dest and a.default is not None:
                return a.default
        return self._defaults.get(dest, None)

    # ---- help ----
    def _metavar(self, a, positional=False):
        if a.metavar is not None:
            return a.metavar
        if a.choices is not None and a.kind != 'parsers':
            return '{' + ','.join(map(str, a.choices)) + '}'
        if a.kind == 'parsers':
            return '{' + ','.join(a.choices) + '}'
        return a.dest if positional else a.dest.upper()

    def _args_str(self, a, positional=False):
        m = self._metavar(a, positional)
        n = a.nargs
        if n is None:
            return m
        if n == OPTIONAL:
            return f'[{m}]'
        if n == ZERO_OR_MORE:
            return f'[{m} ...]'
        if n == ONE_OR_MORE:
            return f'{m} [{m} ...]'
        if n == REMAINDER:
            return '...'
        if n == PARSER:
            return f'{m} ...'
        if n == 0:
            return ''
        return ' '.join([m] * n)

    def format_usage(self):
        return self._usage_text() + '\n'

    def _usage_text(self):
        if self.usage is not None:
            return 'usage: ' + self.usage.replace('%(prog)s', self.prog)
        parts = []
        for a in self._actions:
            if a.help == SUPPRESS:
                continue
            if a.option_strings:
                s = a.option_strings[0]
                args = self._args_str(a)
                if args:
                    s += ' ' + args
                parts.append(s if a.required else f'[{s}]')
        for a in self._actions:
            if not a.option_strings and a.help != SUPPRESS:
                parts.append(self._args_str(a, True))
        return 'usage: ' + ' '.join([self.prog] + [p for p in parts if p])

    def _invocation(self, a):
        if not a.option_strings:
            return self._metavar(a, True)
        args = self._args_str(a)
        if not args:
            return ', '.join(a.option_strings)
        return ', '.join(f'{o} {args}' for o in a.option_strings)

    def format_help(self):
        fmt = self.formatter_class(self.prog)
        lines = [self._usage_text(), '']
        if self.description:
            lines.append(fmt._fill(self.description, fmt.width, ''))
            lines.append('')
        positionals = [a for a in self._actions if not a.option_strings and a.help != SUPPRESS]
        optionals = [a for a in self._actions if a.option_strings and a.help != SUPPRESS]
        invs = [self._invocation(a) for a in positionals + optionals]
        max_len = max([len(i) + 2 for i in invs] + [0])
        help_pos = min(max_len + 2, fmt.max_help_position)

        def section(title, actions):
            out = [title + ':']
            for a in actions:
                inv = self._invocation(a)
                help_text = a.help or ''
                if help_text and '%(default)' in help_text:
                    help_text = help_text.replace('%(default)s', str(a.default))
                if help_text and '%(prog)' in help_text:
                    help_text = help_text.replace('%(prog)s', self.prog)
                if isinstance(fmt, ArgumentDefaultsHelpFormatter) and a.default not in (None, SUPPRESS) and help_text:
                    help_text += f' (default: {a.default})'
                line = '  ' + inv
                if not help_text:
                    out.append(line)
                elif len(line) <= help_pos - 2:
                    out.append(line.ljust(help_pos) + help_text)
                else:
                    out.append(line)
                    out.append(' ' * help_pos + help_text)
            return out
        if positionals:
            lines += section('positional arguments', positionals)
            lines.append('')
        if optionals:
            lines += section('options', optionals)
            lines.append('')
        if self.epilog:
            lines.append(fmt._fill(self.epilog, fmt.width, ''))
            lines.append('')
        return '\n'.join(lines).rstrip('\n') + '\n'

    def print_usage(self, file=None):
        (file or _sys.stdout).write(self.format_usage())

    def print_help(self, file=None):
        (file or _sys.stdout).write(self.format_help())

    def exit(self, status=0, message=None):
        if message:
            _sys.stderr.write(message)
        _sys.exit(status)

    def error(self, message):
        self.print_usage(_sys.stderr)
        self.exit(2, f'{self.prog}: error: {message}\n')

    # ---- parsing ----
    def parse_args(self, args=None, namespace=None):
        args, argv = self.parse_known_args(args, namespace)
        if argv:
            self.error('unrecognized arguments: ' + ' '.join(argv))
        return args

    def _convert(self, a, value):
        if a.type is None:
            v = value
        else:
            try:
                v = a.type(value)
            except ArgumentTypeError as e:
                raise ArgumentError(a, str(e))
            except (TypeError, ValueError):
                name = getattr(a.type, '__name__', repr(a.type))
                raise ArgumentError(a, f'invalid {name} value: {value!r}')
        if a.choices is not None and v not in a.choices:
            choices = ', '.join(map(repr, a.choices))
            raise ArgumentError(a, f'invalid choice: {v!r} (choose from {choices})')
        return v

    def _match_option(self, arg):
        for a in self._actions:
            if arg in a.option_strings:
                return a, None
        if '=' in arg and arg.startswith('--'):
            name, value = arg.split('=', 1)
            for a in self._actions:
                if name in a.option_strings:
                    return a, value
        if arg.startswith('-') and not arg.startswith('--') and len(arg) > 2:
            for a in self._actions:
                if arg[:2] in a.option_strings:
                    return a, arg[2:]
        if self.allow_abbrev and arg.startswith('--'):
            name = arg.split('=', 1)[0]
            matches = [a for a in self._actions for o in a.option_strings if o.startswith(name)]
            uniq = []
            for m in matches:
                if m not in uniq:
                    uniq.append(m)
            if len(uniq) == 1:
                return uniq[0], arg.split('=', 1)[1] if '=' in arg else None
            if len(uniq) > 1:
                opts = ', '.join(o for m in uniq for o in m.option_strings if o.startswith(name))
                self.error(f'ambiguous option: {name} could match {opts}')
        return None, None

    def parse_known_args(self, args=None, namespace=None):
        if args is None:
            args = _sys.argv[1:]
        else:
            args = list(args)
        ns = namespace if namespace is not None else Namespace()
        for a in self._actions:
            if a.dest is not SUPPRESS and a.dest != SUPPRESS and not hasattr(ns, a.dest):
                if a.default is not SUPPRESS:
                    setattr(ns, a.dest, a.default)
        for k, v in self._defaults.items():
            if not hasattr(ns, k):
                setattr(ns, k, v)
        try:
            extras = self._parse(args, ns)
        except ArgumentError as e:
            if not self.exit_on_error:
                raise
            self.error(str(e))
        return ns, extras

    def _parse(self, args, ns):
        positionals = [a for a in self._actions if not a.option_strings]
        extras = []
        pos_values = []
        seen = set()
        i = 0
        only_positional = False
        while i < len(args):
            arg = args[i]
            if arg == '--' and not only_positional:
                only_positional = True
                i += 1
                continue
            if not only_positional and arg.startswith('-') and arg != '-' and not _is_number(arg):
                a, attached = self._match_option(arg)
                if a is None:
                    extras.append(arg)
                    i += 1
                    continue
                seen.add(a)
                i += 1
                kind = a.kind
                if kind == 'help':
                    self.print_help()
                    self.exit()
                if kind == 'version':
                    _sys.stdout.write(str(self._version).replace('%(prog)s', self.prog) + '\n')
                    self.exit()
                if kind in ('store_true', 'store_false', 'store_const'):
                    setattr(ns, a.dest, a.const)
                    continue
                if kind == 'boolean_optional':
                    setattr(ns, a.dest, not arg.startswith('--no-'))
                    continue
                if kind == 'count':
                    setattr(ns, a.dest, (getattr(ns, a.dest, None) or 0) + 1)
                    continue
                if kind == 'append_const':
                    lst = list(getattr(ns, a.dest, None) or [])
                    lst.append(a.const)
                    setattr(ns, a.dest, lst)
                    continue
                values = []
                if attached is not None:
                    values = [attached]
                else:
                    n = a.nargs
                    if n is None:
                        if i >= len(args) or (args[i].startswith('-') and args[i] != '-' and not _is_number(args[i])):
                            raise ArgumentError(a, 'expected one argument')
                        values = [args[i]]
                        i += 1
                    elif n == OPTIONAL:
                        if i < len(args) and not (args[i].startswith('-') and not _is_number(args[i])):
                            values = [args[i]]
                            i += 1
                        else:
                            setattr(ns, a.dest, a.const)
                            continue
                    elif n in (ZERO_OR_MORE, ONE_OR_MORE, REMAINDER):
                        while i < len(args) and (n == REMAINDER or not (args[i].startswith('-') and not _is_number(args[i]))):
                            values.append(args[i])
                            i += 1
                        if n == ONE_OR_MORE and not values:
                            raise ArgumentError(a, 'expected at least one argument')
                    else:
                        for _ in range(n):
                            if i >= len(args) or args[i].startswith('-') and not _is_number(args[i]):
                                raise ArgumentError(a, f'expected {n} arguments')
                            values.append(args[i])
                            i += 1
                converted = [self._convert(a, v) for v in values]
                if kind == 'append' or kind == 'extend':
                    lst = list(getattr(ns, a.dest, None) or [])
                    if kind == 'extend' or a.nargs not in (None, OPTIONAL):
                        if kind == 'extend':
                            lst.extend(converted)
                        else:
                            lst.append(converted)
                    else:
                        lst.append(converted[0])
                    setattr(ns, a.dest, lst)
                elif a.nargs in (None, OPTIONAL):
                    setattr(ns, a.dest, converted[0])
                else:
                    setattr(ns, a.dest, converted)
            else:
                pos_values.append(arg)
                i += 1
                if self._subparsers is not None and len(pos_values) > sum(1 for p in positionals if p.kind != 'parsers'):
                    pass
        # Assign positionals.
        pi = 0
        remaining = pos_values
        for idx, a in enumerate(positionals):
            n = a.nargs
            if a.kind == 'parsers':
                if not remaining:
                    if a.required:
                        raise ArgumentError(None, f'the following arguments are required: {self._metavar(a, True)}')
                    continue
                name = remaining[0]
                if name not in a.choices:
                    choices = ', '.join(map(repr, a.choices))
                    raise ArgumentError(a, f'invalid choice: {name!r} (choose from {choices})')
                if a.dest is not SUPPRESS:
                    setattr(ns, a.dest, name)
                sub = a.choices[name]
                sub_ns, sub_extras = sub.parse_known_args(remaining[1:] + extras, ns)
                extras = sub_extras
                remaining = []
                continue
            later_min = sum(_min_count(p.nargs) for p in positionals[idx + 1:])
            if n is None:
                if not remaining:
                    break
                setattr(ns, a.dest, self._convert(a, remaining[0]))
                remaining = remaining[1:]
                seen.add(a)
            elif n == OPTIONAL:
                if len(remaining) > later_min:
                    setattr(ns, a.dest, self._convert(a, remaining[0]))
                    remaining = remaining[1:]
                elif a.default is not None:
                    setattr(ns, a.dest, a.default)
                else:
                    setattr(ns, a.dest, a.const)
                seen.add(a)
            elif n in (ZERO_OR_MORE, ONE_OR_MORE, REMAINDER):
                take = max(0, len(remaining) - later_min)
                vals = remaining[:take]
                remaining = remaining[take:]
                if n == ONE_OR_MORE and not vals:
                    break
                if vals or n != ZERO_OR_MORE or a.default is None:
                    setattr(ns, a.dest, [self._convert(a, v) for v in vals])
                seen.add(a)
            else:
                if len(remaining) < n:
                    break
                setattr(ns, a.dest, [self._convert(a, v) for v in remaining[:n]])
                remaining = remaining[n:]
                seen.add(a)
        missing = [a for a in self._actions if a.required and a not in seen and a.kind != 'parsers']
        if missing:
            names = ', '.join(_action_name(a) if a.option_strings else self._metavar(a, True) for a in missing)
            raise ArgumentError(None, f'the following arguments are required: {names}')
        for a in self._actions:
            if isinstance(getattr(ns, a.dest, None), str) and a.type is not None and a not in seen and a.default is getattr(ns, a.dest, None):
                setattr(ns, a.dest, self._convert(a, a.default))
        return remaining + extras


def _min_count(n):
    if n is None:
        return 1
    if n in (OPTIONAL, ZERO_OR_MORE, REMAINDER):
        return 0
    if n == ONE_OR_MORE:
        return 1
    if n == PARSER:
        return 1
    return n


def _is_number(s):
    try:
        float(s)
        return True
    except ValueError:
        return False
