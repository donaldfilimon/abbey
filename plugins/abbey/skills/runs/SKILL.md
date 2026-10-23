---
name: runs
description: Submit, inspect, or cancel Abbey bounded daemon runs through its existing CLI.
disable-model-invocation: true
argument-hint: status <run-id> | events <run-id> | submit <backend> <prompt> | cancel <run-id>
---

Use the user's explicit operation with Abbey's authenticated daemon CLI:

- Status: `abbey daemon run status <run-id> --json`
- Events: `abbey daemon run events <run-id> --limit 20 --json`
- Submit: `abbey daemon run submit --backend <backend> --json -- <prompt>`
- Cancel: `abbey daemon run cancel <run-id> --json`

Check `abbey daemon run submit --help` for the installed binary's supported backend vocabulary before submitting. Require an explicit backend and prompt for submission, and an exact run ID for status, events, or cancellation. Ask for missing inputs. Pass every user value as a properly shell-quoted data argument; never execute invocation text as shell syntax. Submit or cancel only when that operation was requested. Send a submission once; an ambiguous timeout is not permission to resubmit.

Use the existing environment's daemon authentication without printing credentials or reading bearer contents. Report authentication, startup-route, and connection failures directly. Do not start or reconfigure the daemon, substitute a backend, or fall back to a chat invocation. Summarize the returned durable run ID and lifecycle state; successful admission does not mean successful execution. Events contain sanitized lifecycle evidence, not raw provider output.
