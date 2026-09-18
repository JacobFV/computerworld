"""subprocess for the simulated interpreter.

Children run through the machine's own shell (so `ls`, `sh -c`, nested `python3`
and `node` all work) and complete before the call that started them returns;
their virtual run time is charged to this process's clock, which is what makes
`timeout=` meaningful. Output a child writes to an inherited descriptor lands in
our stream where CPython's would: after what we have flushed.
"""
import os
import sys
import _cw

__all__ = ['Popen', 'PIPE', 'STDOUT', 'DEVNULL', 'call', 'check_call', 'check_output',
           'getoutput', 'getstatusoutput', 'run', 'CalledProcessError', 'SubprocessError',
           'TimeoutExpired', 'CompletedProcess', 'list2cmdline']

PIPE = -1
STDOUT = -2
DEVNULL = -3


class SubprocessError(Exception):
    pass


class CalledProcessError(SubprocessError):
    def __init__(self, returncode, cmd, output=None, stderr=None):
        self.returncode = returncode
        self.cmd = cmd
        self.output = output
        self.stderr = stderr

    def __str__(self):
        if self.returncode and self.returncode < 0:
            return "Command '%s' died with signal %d." % (self.cmd, -self.returncode)
        return "Command '%s' returned non-zero exit status %d." % (self.cmd, self.returncode)

    @property
    def stdout(self):
        return self.output

    @stdout.setter
    def stdout(self, value):
        self.output = value


class TimeoutExpired(SubprocessError):
    def __init__(self, cmd, timeout, output=None, stderr=None):
        self.cmd = cmd
        self.timeout = timeout
        self.output = output
        self.stderr = stderr

    def __str__(self):
        return "Command '%s' timed out after %s seconds" % (self.cmd, self.timeout)

    @property
    def stdout(self):
        return self.output

    @stdout.setter
    def stdout(self, value):
        self.output = value


class CompletedProcess:
    def __init__(self, args, returncode, stdout=None, stderr=None):
        self.args = args
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr

    def __repr__(self):
        args = ['args={!r}'.format(self.args), 'returncode={!r}'.format(self.returncode)]
        if self.stdout is not None:
            args.append('stdout={!r}'.format(self.stdout))
        if self.stderr is not None:
            args.append('stderr={!r}'.format(self.stderr))
        return "{}({})".format(type(self).__name__, ', '.join(args))

    def check_returncode(self):
        if self.returncode:
            raise CalledProcessError(self.returncode, self.args, self.stdout, self.stderr)


def list2cmdline(seq):
    result = []
    needquote = False
    for arg in map(os.fsdecode, seq):
        bs_buf = []
        if result:
            result.append(' ')
        needquote = (" " in arg) or ("\t" in arg) or not arg
        if needquote:
            result.append('"')
        for c in arg:
            if c == '\\':
                bs_buf.append(c)
            elif c == '"':
                result.append('\\' * len(bs_buf) * 2)
                bs_buf = []
                result.append('\\"')
            else:
                if bs_buf:
                    result.extend(bs_buf)
                    bs_buf = []
                result.append(c)
        if bs_buf:
            result.extend(bs_buf)
        if needquote:
            result.extend(bs_buf)
            result.append('"')
    return ''.join(result)


class _Pipe:
    """The parent's end of a child's pipe. The child has already finished, so a
    read returns what it wrote; a write buffers input delivered on close."""

    def __init__(self, data, text, writable=False, on_close=None):
        self._data = data
        self._pos = 0
        self._text = text
        self._writable = writable
        self._written = [] if writable else None
        self._on_close = on_close
        self.closed = False

    def read(self, n=-1):
        if n is None or n < 0:
            out = self._data[self._pos:]
            self._pos = len(self._data)
            return out
        out = self._data[self._pos:self._pos + n]
        self._pos += len(out)
        return out

    def read1(self, n=-1):
        return self.read(n)

    def readline(self, limit=-1):
        nl = '\n' if self._text else b'\n'
        i = self._data.find(nl, self._pos)
        end = len(self._data) if i < 0 else i + 1
        if limit is not None and limit >= 0:
            end = min(end, self._pos + limit)
        out = self._data[self._pos:end]
        self._pos = end
        return out

    def readlines(self):
        out = []
        while True:
            line = self.readline()
            if not line:
                return out
            out.append(line)

    def __iter__(self):
        return self

    def __next__(self):
        line = self.readline()
        if not line:
            raise StopIteration
        return line

    def write(self, s):
        if not self._writable:
            raise OSError('File not open for writing')
        self._written.append(s)
        return len(s)

    def flush(self):
        pass

    def fileno(self):
        return 3

    def readable(self):
        return not self._writable

    def writable(self):
        return self._writable

    def close(self):
        if not self.closed:
            self.closed = True
            if self._on_close is not None:
                self._on_close(self)

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()


def _encode(s, text, encoding, errors):
    if s is None:
        return None
    if text:
        if '\r\n' in s:
            s = s.replace('\r\n', '\n')
        return s
    return s.encode(encoding or 'utf-8', errors or 'strict')


class Popen:
    """A child process. It runs when its input is known: at construction when
    stdin is not a pipe, otherwise at `communicate()` / `wait()` / closing stdin."""

    def __init__(self, args, bufsize=-1, executable=None, stdin=None, stdout=None,
                 stderr=None, preexec_fn=None, close_fds=True, shell=False, cwd=None,
                 env=None, universal_newlines=None, startupinfo=None, creationflags=0,
                 restore_signals=True, start_new_session=False, pass_fds=(), *,
                 user=None, group=None, extra_groups=None, encoding=None, errors=None,
                 text=None, umask=-1, pipesize=-1, process_group=None):
        self.args = args
        self.returncode = None
        self._text = bool(text or universal_newlines or encoding or errors)
        self._encoding = encoding
        self._errors = errors
        self._stdin_mode = stdin
        self._stdout_mode = stdout
        self._stderr_mode = stderr
        self._shell = shell
        self._executable = executable
        self._env = env
        self._cwd = os.fspath(cwd) if cwd is not None else None
        self._result = None
        self._pending_input = ''
        self.stdin = None
        self.stdout = None
        self.stderr = None
        if self._cwd is not None and not os.path.isdir(self._cwd):
            if os.path.exists(self._cwd):
                raise NotADirectoryError(20, 'Not a directory', self._cwd)
            raise FileNotFoundError(2, 'No such file or directory', self._cwd)
        if shell:
            if isinstance(args, (str, bytes)):
                self._program = os.fsdecode(args)
            else:
                self._program = ' '.join(os.fsdecode(a) for a in args)
        else:
            if isinstance(args, (str, bytes)):
                argv = [os.fsdecode(args)]
            else:
                argv = [os.fsdecode(os.fspath(a)) for a in args]
            if executable is not None:
                argv[0] = os.fsdecode(executable)
            self._program = argv
            if not argv:
                raise IndexError('list index out of range')
        self.pid = 0
        if stdin == PIPE:
            # The child starts once its input is complete; a missing program is
            # reported then rather than here.
            self.stdin = _Pipe(None, self._text, writable=True, on_close=self._stdin_closed)
        elif stdin is not None and stdin != DEVNULL and hasattr(stdin, 'read'):
            data = stdin.read()
            self._pending_input = data.decode() if isinstance(data, bytes) else data
            self._run()
        else:
            self._run()

    def _stdin_closed(self, pipe):
        self._pending_input = ''.join(
            x.decode() if isinstance(x, bytes) else x for x in pipe._written)
        if self._result is None:
            self._run()

    def _run(self):
        if self._result is not None:
            return
        env = self._env
        if env is None:
            env = os.environ
        env_items = [(os.fsdecode(k), os.fsdecode(v)) for k, v in env.items()]
        out, err, code, took = _cw.spawn(self._program, self._shell, self._pending_input,
                                        self._cwd, env_items)
        self.pid = os.getpid() + 1
        self._took = took
        self._result = (out, err, code)
        self.returncode = code
        cap_out = cap_err = None
        if self._stdout_mode == PIPE:
            cap_out = out
        elif self._stdout_mode == DEVNULL:
            pass
        elif self._stdout_mode is None:
            _cw.child_output(out, '')
        elif hasattr(self._stdout_mode, 'write'):
            self._stdout_mode.write(out if self._text or 'b' not in getattr(self._stdout_mode, 'mode', '') else out.encode())
        if self._stderr_mode == PIPE:
            cap_err = err
        elif self._stderr_mode == STDOUT:
            if self._stdout_mode == PIPE:
                cap_out = (cap_out or '') + err
            elif self._stdout_mode is None:
                _cw.child_output(err, '')
        elif self._stderr_mode == DEVNULL:
            pass
        elif self._stderr_mode is None:
            _cw.child_output('', err)
        elif hasattr(self._stderr_mode, 'write'):
            self._stderr_mode.write(err)
        self._captured = (_encode(cap_out, self._text, self._encoding, self._errors),
                          _encode(cap_err, self._text, self._encoding, self._errors))
        if self._stdout_mode == PIPE:
            self.stdout = _Pipe(self._captured[0], self._text)
        if self._stderr_mode == PIPE:
            self.stderr = _Pipe(self._captured[1], self._text)

    def communicate(self, input=None, timeout=None):
        if input is not None:
            if self._stdin_mode != PIPE:
                raise ValueError('Cannot send input after starting communication')
            self._pending_input = input.decode() if isinstance(input, bytes) else input
        elif self.stdin is not None and not self.stdin.closed:
            self._pending_input = ''.join(
                x.decode() if isinstance(x, bytes) else x for x in self.stdin._written)
        if self.stdin is not None:
            self.stdin.closed = True
        self._run()
        if timeout is not None and self._took > timeout:
            raise TimeoutExpired(self.args, timeout, self._captured[0], self._captured[1])
        out = self.stdout.read() if self.stdout is not None else None
        err = self.stderr.read() if self.stderr is not None else None
        return (out, err)

    def poll(self):
        if self._result is None and self.stdin is not None and self.stdin.closed:
            self._run()
        return self.returncode

    def wait(self, timeout=None):
        if self._result is None:
            if self.stdin is not None:
                self.stdin.close()
            self._run()
        if timeout is not None and self._took > timeout:
            raise TimeoutExpired(self.args, timeout)
        return self.returncode

    def send_signal(self, sig):
        pass

    def terminate(self):
        pass

    def kill(self):
        pass

    def __enter__(self):
        return self

    def __exit__(self, exc_type, value, traceback):
        for f in (self.stdout, self.stderr, self.stdin):
            if f is not None:
                f.close()
        self.wait()

    def __repr__(self):
        return f"<Popen: returncode: {self.returncode} args: {self.args!r}>"


def run(*popenargs, input=None, capture_output=False, timeout=None, check=False, **kwargs):
    if input is not None:
        if kwargs.get('stdin') is not None:
            raise ValueError('stdin and input arguments may not both be used.')
        kwargs['stdin'] = PIPE
    if capture_output:
        if kwargs.get('stdout') is not None or kwargs.get('stderr') is not None:
            raise ValueError('stdout and stderr arguments may not be used '
                             'with capture_output.')
        kwargs['stdout'] = PIPE
        kwargs['stderr'] = PIPE
    with Popen(*popenargs, **kwargs) as process:
        stdout, stderr = process.communicate(input, timeout=timeout)
        retcode = process.poll()
        if check and retcode:
            raise CalledProcessError(retcode, process.args, output=stdout, stderr=stderr)
    return CompletedProcess(process.args, retcode, stdout, stderr)


def call(*popenargs, timeout=None, **kwargs):
    with Popen(*popenargs, **kwargs) as p:
        return p.wait(timeout=timeout)


def check_call(*popenargs, **kwargs):
    retcode = call(*popenargs, **kwargs)
    if retcode:
        cmd = kwargs.get("args")
        if cmd is None:
            cmd = popenargs[0]
        raise CalledProcessError(retcode, cmd)
    return 0


def check_output(*popenargs, timeout=None, **kwargs):
    if 'stdout' in kwargs:
        raise ValueError('stdout argument not allowed, it will be overridden.')
    if 'input' in kwargs and kwargs['input'] is None:
        kwargs['input'] = '' if kwargs.get('universal_newlines') or kwargs.get('text') \
            or kwargs.get('encoding') or kwargs.get('errors') else b''
    return run(*popenargs, stdout=PIPE, timeout=timeout, check=True, **kwargs).stdout


def getstatusoutput(cmd, *, encoding=None, errors=None):
    try:
        data = check_output(cmd, shell=True, text=True, stderr=STDOUT,
                            encoding=encoding, errors=errors)
        exitcode = 0
    except CalledProcessError as ex:
        data = ex.output
        exitcode = ex.returncode
    if data[-1:] == '\n':
        data = data[:-1]
    return exitcode, data


def getoutput(cmd, *, encoding=None, errors=None):
    return getstatusoutput(cmd, encoding=encoding, errors=errors)[1]
