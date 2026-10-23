---
name: status
description: Inspect Abbey runtime status, capability claims, and host support through its read-only MCP tools.
---

Use the Abbey MCP server's abbey_status and abbey_platform tools. If the user asks about a capability, query abbey_claims with the appropriate schema-supported filters.

Report the edition, build, host support, and relevant claim status. Preserve the distinction between Current, Partial, Proposed, Blocked, and Out of scope and include the evidence boundary. Do not infer live provider readiness from compiled capability claims.

If the server is unavailable, check whether `abbey` is on PATH and explain the connection error. Do not install binaries or launch a daemon automatically.
