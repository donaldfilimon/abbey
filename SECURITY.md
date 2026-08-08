# Security Policy

## Supported versions

Abbey is a personal project with a single active line. Only the current
release receives fixes; there are no maintained older branches.

| Version | Supported |
| ------- | --------- |
| 2.6.x   | yes       |
| < 2.6   | no        |

## Reporting a vulnerability

Report privately through this repository's **Security → Report a vulnerability**
tab (GitHub private vulnerability reporting). Please do not open a public issue
for a suspected vulnerability.

This is a personal project maintained by one person, so there is no response
SLA. Expect a best-effort acknowledgement; if a report is accepted, the fix
lands on `main` and the advisory is published from this repository.

## Scope worth knowing before you report

Abbey's threat model is shaped by what it actually does — it is a local CLI/TUI
that shells out to an executor (`cursor-agent`, `grok`, `fm`, or `abi`), not a
network service:

- **It executes local processes by design.** Prompts are passed to the
  configured backend binary, and `abbey os execute` runs allowlisted commands.
  Command execution through those documented paths is intended behaviour, not a
  vulnerability. Escaping the `os_control` allowlist, or executing without
  `--confirm`, *is* a vulnerability.
- **Prompt content reaching the backend in option position** is guarded (a
  leading-dash prompt is warned about, and the `abi` grammar uses a real `--`
  separator). A way to smuggle backend flags through prompt text is in scope.
- **Credentials** are never stored by Abbey; the backend CLI owns them. Abbey
  reading or logging a credential would be in scope.
- **State** lives under `~/.local/state/abbey` (chat ids, memory store, route
  log). Anything that lets another local user or a prompt influence writes
  outside that directory is in scope.
- **Third-party backends** (`cursor-agent`, `abi`, `fm`) have their own
  security contacts; issues inside those belong to their projects.
