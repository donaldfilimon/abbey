#!/usr/bin/env python3
"""Explicit, Linux-only systemd user integration for installed Abbey binaries."""
import argparse
import json
import os
from pathlib import Path
import secrets
import selectors
import shutil
import stat
import subprocess
import sys
import tempfile
import time


class ServiceError(Exception):
    pass


def checked(path, directory=False, private=True):
    """Never follow a symlink in any existing path component."""
    path = Path(path)
    if not path.is_absolute() or any(ord(c) < 32 or ord(c) == 127 for c in str(path)):
        raise ServiceError('paths must be absolute and contain no controls')
    for part in reversed((path, *path.parents)):
        info = part.lstat()
        trusted_sticky = (part != path and info.st_uid == 0
                          and stat.S_ISDIR(info.st_mode) and info.st_mode & stat.S_ISVTX)
        if stat.S_ISLNK(info.st_mode) or (info.st_mode & 0o022 and not trusted_sticky):
            raise ServiceError('unsafe path component')
        if info.st_uid not in (0, os.getuid()):
            raise ServiceError('foreign-owned path component')
    info = path.lstat()
    kind = stat.S_ISDIR if directory else stat.S_ISREG
    if not kind(info.st_mode) or info.st_uid not in ((os.getuid(),) if private else (0, os.getuid())):
        raise ServiceError('wrong path type or owner')
    if private and info.st_mode & 0o077:
        raise ServiceError('private path grants group or other access')
    return path


def mkdir(path):
    if not path.exists() and not path.is_symlink():
        mkdir(path.parent)
        path.mkdir(mode=0o700)
    checked(path, directory=True, private=False)


def quote(value, executable=False):
    # systemd specifiers and ExecStart environment substitution are distinct.
    value = str(value).replace('\\', '\\\\').replace('"', '\\"').replace('%', '%%')
    if executable:
        value = value.replace('$', '$$')
    return '"' + value + '"'


def run(argv, env):
    # Drain a pipe with both an output ceiling and a wall-clock deadline.
    try:
        child = subprocess.Popen([str(a) for a in argv], env=env, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
    except OSError as error:
        raise ServiceError('command unavailable') from error
    output = bytearray()
    deadline = time.monotonic() + 3
    try:
        with selectors.DefaultSelector() as selector:
            selector.register(child.stdout, selectors.EVENT_READ)
            while selector.get_map():
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise ServiceError('command timed out')
                for key, _ in selector.select(remaining):
                    data = os.read(key.fileobj.fileno(), 4096)
                    if not data:
                        selector.unregister(key.fileobj)
                    else:
                        output.extend(data)
                        if len(output) > 65536:
                            raise ServiceError('command output exceeds limit')
            if child.wait(timeout=max(0.001, deadline - time.monotonic())):
                raise ServiceError('command failed')
    except subprocess.TimeoutExpired as error:
        raise ServiceError('command timed out') from error
    finally:
        if child.poll() is None:
            child.kill()
        child.wait()
        child.stdout.close()
    return output.decode('utf-8', errors='strict').strip()


class Controller:
    def __init__(self, cli, home, workspace, systemctl, *, platform=None):
        if (sys.platform if platform is None else platform) != 'linux':
            raise ServiceError('Linux systemd user services only')
        self.home = checked(Path(home), directory=True, private=False)
        self.workspace = checked(Path(workspace), directory=True, private=False)
        self.cli = checked(Path(cli), private=False)
        self.systemctl = checked(Path(systemctl), private=False)
        self.env = {'HOME': str(self.home), 'PATH': '/usr/bin:/bin'}
        # Transport context only; no Abbey/model/credential environment inheritance.
        for key in ('XDG_RUNTIME_DIR', 'DBUS_SESSION_BUS_ADDRESS'):
            if key in os.environ:
                self.env[key] = os.environ[key]
        # CLI initialization precedes edition dispatch and may create/read state.
        # Identity discovery must never touch the intended service/user roots.
        with tempfile.TemporaryDirectory(prefix='abbey-service-identity-') as scratch:
            probe = Path(scratch).resolve()
            roots = {key: probe / leaf for key, leaf in (
                ('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'),
                ('XDG_STATE_HOME', 'state'), ('XDG_DATA_HOME', 'data'),
                ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime'),
                ('ABBEY_STATE_DIR', 'safe-state'),
                ('ABBEY_PERSONAL_STATE_DIR', 'personal-state'))}
            for root in roots.values():
                root.mkdir(mode=0o700)
            probe_env = {key: str(root) for key, root in roots.items()}
            probe_env.update({'PATH': '/usr/bin:/bin', 'USERPROFILE': str(roots['HOME']),
                              'ABBEY_CONFIG': str(probe / 'safe-config.toml'),
                              'ABBEY_PERSONAL_CONFIG': str(probe / 'personal-config.toml')})
            name = run([self.cli, 'edition', '--name'], probe_env)
            daemon = run([self.cli, 'edition', '--daemon-name'], probe_env)
        pairs = {'abbey': ('abbeyd', 'standard', 'ABBEYD', 'ABBEY'),
                 'abbey-personal': ('abbey-personal-daemon', 'personal', 'ABBEY_PERSONAL_DAEMON', 'ABBEY_PERSONAL')}
        if name not in pairs or pairs[name][0] != daemon or self.cli.name != name:
            raise ServiceError('unknown or inconsistent installed edition')
        _, self.edition, prefix, state_prefix = pairs[name]
        self.daemon = checked(self.cli.parent / daemon, private=False)
        if not os.access(self.cli, os.X_OK) or not os.access(self.daemon, os.X_OK):
            raise ServiceError('installed binaries must be executable')
        self.config = self.home / '.config' / (name + '-user-service')
        self.state = self.home / '.local' / 'state' / name
        self.token = self.config / 'bearer'
        self.unit = self.home / '.config' / 'systemd' / 'user' / (daemon + '.service')
        self.marker = '# Abbey linux_user_service.py v1 ' + name + '\n'
        self.service_env = dict(self.env)
        self.service_env.update({prefix + '_BEARER_TOKEN_FILE': str(self.token),
                                 prefix + '_SOCKET_PATH': str(self.state / 'daemon' / (daemon + '.sock')),
                                 state_prefix + '_STATE_DIR': str(self.state)})

    def content(self):
        args = ['/usr/bin/env', '-i', *[key + '=' + value for key, value in sorted(self.service_env.items())
                                      if key not in ('DBUS_SESSION_BUS_ADDRESS', 'XDG_RUNTIME_DIR')], str(self.daemon)]
        return (self.marker + '[Unit]\nDescription=Abbey local daemon\n[Service]\nType=simple\nUMask=0077\n'
                + 'WorkingDirectory=' + quote(self.workspace) + '\nExecStart=' + ' '.join(quote(arg, executable=True) for arg in args)
                + '\nRestart=no\n[Install]\nWantedBy=default.target\n')

    def validate(self):
        for path in (self.config, self.state):
            checked(path, directory=True)
        checked(self.token)
        token = self.token.read_bytes()
        if not 32 <= len(token) <= 4096 or any(c < 32 or c == 127 for c in token):
            raise ServiceError('invalid retained bearer')
        checked(self.unit)
        if self.unit.read_text() != self.content():
            raise ServiceError('unit is not the exact installer-owned configuration')

    def ctl(self, action):
        run([self.systemctl, '--user', action, self.unit.name], self.env)

    def publish_unit(self, content, mode=0o600):
        fd, staged = tempfile.mkstemp(prefix='.abbey-unit-', dir=self.unit.parent)
        try:
            with os.fdopen(fd, 'wb') as unit:
                unit.write(content); unit.flush(); os.fchmod(unit.fileno(), mode)
                os.fsync(unit.fileno())
            os.replace(staged, self.unit)
        finally:
            if os.path.exists(staged): os.unlink(staged)

    def install(self):
        for path in (self.config, self.state, self.unit.parent):
            mkdir(path)
            checked(path, directory=True)
        previous = None
        if self.unit.exists() or self.unit.is_symlink():
            self.validate()
            previous = (self.unit.read_bytes(), stat.S_IMODE(self.unit.stat().st_mode))
        if self.token.exists() or self.token.is_symlink():
            checked(self.token)
            token = self.token.read_bytes()
            if not 32 <= len(token) <= 4096 or any(c < 32 or c == 127 for c in token):
                raise ServiceError('invalid retained bearer')
        else:
            fd = os.open(self.token, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with os.fdopen(fd, 'w') as token:
                token.write(secrets.token_hex(32))
        self.publish_unit(self.content().encode('utf-8'))
        try:
            self.validate()
            run([self.systemctl, '--user', 'daemon-reload'], self.env)
        except (ServiceError, OSError, UnicodeError) as error:
            try:
                if previous is None:
                    self.unit.unlink()
                else:
                    self.publish_unit(*previous)
            except (ServiceError, OSError) as rollback_error:
                raise ServiceError('installation failed; unit rollback failed; inspect unit before retry') from rollback_error
            try:
                run([self.systemctl, '--user', 'daemon-reload'], self.env)
            except (ServiceError, OSError, UnicodeError) as reload_error:
                raise ServiceError('installation failed; previous unit state restored; rollback daemon-reload failed') from reload_error
            raise ServiceError('installation failed; previous unit state restored and reloaded') from error

    def status(self):
        self.validate()
        try:
            event = json.loads(run([self.cli, 'daemon', 'status', '--json'], self.service_env))
            payload = event['payload']
            protocol, schema = payload['protocol_version'], payload['schema_version']
            if (event['type'] != 'status' or payload['edition'] != self.edition
                    or type(protocol) is not int or type(schema) is not int
                    or (protocol, schema) not in ((1, 2), (2, 3))
                    or payload['state'] != 'ready'):
                raise ValueError()
        except (ValueError, KeyError, TypeError) as error:
            raise ServiceError('authenticated status does not match ready edition contract') from error
        return event

    def start(self):
        self.validate(); self.ctl('start')
        for attempt in range(3):
            try: return self.status()
            except ServiceError:
                if attempt == 2:
                    # Explicit start failure cleanup; never claim readiness.
                    self.ctl('stop')
                    raise ServiceError('service failed authenticated readiness; stop requested')
                time.sleep(0.1)

    def stop(self):
        self.validate(); self.ctl('stop')

    def restart(self):
        self.stop(); return self.start()

    def uninstall(self):
        self.validate(); self.ctl('stop'); self.ctl('disable')
        self.unit.unlink()
        run([self.systemctl, '--user', 'daemon-reload'], self.env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=('install', 'start', 'status', 'stop', 'restart', 'uninstall'))
    parser.add_argument('--cli', required=True, type=Path)
    parser.add_argument('--workspace', required=True, type=Path)
    args = parser.parse_args()
    try:
        if sys.platform != 'linux': raise ServiceError('Linux systemd user services only')
        systemctl = shutil.which('systemctl', path='/usr/bin:/bin')
        if systemctl is None: raise ServiceError('systemctl unavailable')
        controller = Controller(args.cli, Path.home(), args.workspace, Path(systemctl))
        getattr(controller, args.action)()
        print(args.action + ' succeeded' + ('; authenticated ready' if args.action in ('start', 'restart', 'status') else ''))
    except (ServiceError, OSError, UnicodeError) as error:
        print('Abbey service operation refused or failed: ' + str(error), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
