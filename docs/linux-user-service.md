# Explicit Linux user-service bootstrap

`tools/linux_user_service.py` manages an opt-in systemd user service using
already installed, matching edition binaries. It does not install binaries,
enable login startup or configure models,
or run automatically from `install.sh`. Native Ubuntu ARM64 acceptance remains
pending; fixture tests on macOS do not establish systemd behavior.

On Linux, run with Python 3, an absolute installed CLI path and an explicit,
existing working directory:

```sh
python3 tools/linux_user_service.py install --cli /home/me/.local/bin/abbey --workspace /home/me/project
python3 tools/linux_user_service.py start --cli /home/me/.local/bin/abbey --workspace /home/me/project
python3 tools/linux_user_service.py status --cli /home/me/.local/bin/abbey --workspace /home/me/project
```

The same arguments support `stop`, `restart`, and `uninstall`. Use
`abbey-personal` for the personal edition; its matching daemon must be alongside
it. Identity probes run separately with `edition --name` and
`edition --daemon-name` inside a temporary private HOME with explicit config
and state roots for both editions. These probes can initialize CLI state; their
scratch roots are removed on success or failure and never enter the service
environment. Existing user or service state is neither adopted nor chmodded. Unknown pairs and missing executables fail closed.

Safe integration uses `~/.config/abbey-user-service/bearer`,
`~/.local/state/abbey`, and `~/.config/systemd/user/abbeyd.service`.
Personal integration uses `~/.config/abbey-personal-user-service/bearer`,
`~/.local/state/abbey-personal`, and
`~/.config/systemd/user/abbey-personal-daemon.service`. The socket lives in the
edition state directory's `daemon` subdirectory. HOME-based roots are explicit;
XDG config/state overrides are not consumed by this controller.

The controller creates managed directories with mode 0700 and files with mode
0600. Existing managed roots, units and credentials must be owner-only and
user-owned. Binary paths may be root-owned. Symlink path components, control
characters, foreign owners, and group/other writable path components are
refused (root-owned sticky ancestor directories such as `/tmp` are allowed).
This conservative policy can require installing real binaries rather than
symlinks. Existing unit content must match the exact generated configuration;
repeat management with the same CLI and workspace paths. Relocation and
adopting hand-edited or unrelated units are deliberately unavailable.

The unit starts the daemon without daemon arguments through `/usr/bin/env -i`,
with HOME, a fixed PATH, and the edition's bearer-file, socket and state
variables. No bearer bytes enter unit content, argv or diagnostic output.
Parent Abbey/provider settings and inline credentials are not inherited.
Systemd paths escape specifiers; ExecStart also escapes dollar substitution.
No shell is invoked. Units are staged and atomically replaced; failed staging
preserves the previous unit. If validation or daemon-reload fails after
publication, installation restores the previous unit bytes and mode, or removes
the newly created unit, then attempts another daemon-reload. Rollback or reload
failures are reported explicitly and require inspection before retry. Newly
generated credentials and runtime state remain retained after installation failure.

`start` makes at most three authenticated CLI `daemon status --json` probes,
each bounded to three seconds and 64 KiB of output, and validates the status
kind, ready state, exact edition, and the exact integer protocol/schema pairs
`(1, 2)` or `(2, 3)`. The existing typed CLI prefers current protocol 2 and
permits read-only protocol 1 compatibility only after explicit version rejection;
the controller adds no transport fallback or retry. Systemd activity
alone is insufficient. Failed readiness requests stop and returns failure;
stop failure also returns failure. `restart` explicitly stops before starting.
`status` performs one authenticated probe. Child diagnostics are suppressed to
avoid relaying sensitive output. Other subprocesses use the same bounds.

`uninstall` stops and disables only the exactly matched unit, removes that unit,
and reloads the user manager. It retains credentials, service configuration
directory, runtime state, memory, chats, audit and backups. It does not touch
another edition. No automatic replay, enablement, credential rotation, native
package distribution, service hardening beyond the private environment/umask,
or desktop startup is implemented.

The desktop has separate Linux-only, read-only discovery of this controller's
retained service configuration. With no explicit daemon credential or socket
environment settings, it resolves the effective user's native account home and
inspects the fixed edition paths above. XDG and Abbey state overrides do not
redirect discovery. Explicit bearer settings take precedence, including invalid
settings; a socket override without credentials preserves the existing
in-process route and skips discovery. macOS and Windows retain environment-only
selection.

Valid retained credentials select the daemon even when its socket is absent.
Invalid recognized configuration returns a configuration error; an unavailable
daemon returns a transport error. Neither falls back to in-process data.
Discovery never installs, starts, enables, repairs permissions or creates
credentials. Before using a discovered client, the desktop authenticates a
bounded read-only status request and checks the active edition. Connection
metadata inspection itself does not connect. Uninstall intentionally retains
the configuration, so discovery continues selecting it afterward; credential
presence is not evidence of an installed, enabled or ready service.

This discovery implementation has scratch Unix fixture coverage on macOS;
native Ubuntu ARM64 desktop acceptance remains pending.

Verification uses only temporary trees, fixture CLI binaries and fake
systemctl commands:

```sh
python3 -m unittest discover -s tools/tests -p test_linux_user_service.py
```

Before promoting Linux service acceptance, exercise real systemd user services
on native Ubuntu ARM64 with both editions concurrently, authenticated status,
run cancellation, restart/recovery, private-file inspection and uninstall
retention. Real daemon transport behavior remains covered separately by the
Rust daemon suites; these controller tests add no native transport evidence.
