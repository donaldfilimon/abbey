# Abbey for Claude Code

Connect Claude Code to an installed Abbey CLI. The plugin uses Abbey's stdio MCP server and provides four skills:

| Command | Purpose |
| --- | --- |
| `/abbey:status` | Read runtime status, capability claims, and host support via MCP |
| `/abbey:doctor` | Diagnose the selected backend and effective model |
| `/abbey:memory <query>` | Search up to ten local memory results |
| `/abbey:runs <operation>` | Explicit bounded daemon run submit/status/events/cancel |

## Run from this checkout

Install Abbey using the repository's [installation instructions](../../README.md), and ensure `abbey` is on the PATH inherited by Claude Code. From the repository root:

```sh
claude --plugin-dir ./plugins/abbey
```

Use `/mcp` to check the `abbey` connection. The MCP server launches as a child process and exposes exactly `abbey_status`, `abbey_claims`, and `abbey_platform`. It needs no daemon, token, network listener, Node runtime, or function-hook API. Doctor and memory use Claude Code's normal shell permissions. Only status is eligible for automatic skill invocation; the other workflows require an explicit invocation.

Run operations require an already configured, running Abbey daemon and its existing authentication environment. See the root README's daemon setup. The plugin does not provision credentials or start that daemon. The owner-only Unix daemon transport is unavailable on Windows. CLI memory search can expose local record content; it is separate from the read-only MCP registry, which provides no memory access.

Examples:

```text
/abbey:status
/abbey:doctor
/abbey:memory terminal appearance
/abbey:runs submit abi summarize this task
/abbey:runs status <returned-run-id>
/abbey:runs events <returned-run-id>
/abbey:runs cancel <returned-run-id>
```

## Validate

```sh
claude plugin validate ./plugins/abbey --strict
```

The skills describe existing CLI contracts; they add no new daemon or MCP authority. Claims describe capability boundaries, not proof that a live provider is ready. This plugin requires a Claude Code release supporting plugin skills and MCP servers.
