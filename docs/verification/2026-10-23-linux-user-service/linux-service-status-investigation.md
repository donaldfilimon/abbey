# Read-only status-version investigation

Confirmed actual response mismatch, 2026-10-23. No canonical edits or tests,
Cargo, daemon/service operations, agents, profiles or commits performed.

The captured external `linux-service-actual-status.json` is a ready `status`
event for `standard`, with `protocol_version:2`, `schema_version:3`, nonempty
Foundation Models run_routes and runtime capabilities. This is a valid current
CLI response; the service controller's hardcoded1/2 check incorrectly rejects it.

Independent source verification:
- src/app_core/contracts.rs:11-17 defines APP_PROTOCOL_V1=1, APP_SCHEMA_V1=2,
  APP_PROTOCOL_VERSION=2, APP_SCHEMA_VERSION=3.
- src/daemon/client.rs:43-53 sends current protocol first. Only read-only
  commands may downgrade once, and only after explicit unsupported_version.
- src/daemon/client.rs:494-513 binds status protocol/schema to exact negotiated
  version: (1,2) or (2,3), rejecting mismatch. It then validates capabilities.
- src/app_core/context.rs:20-34 creates the read-only v1 status, while lines43-44
  create the current v2 runtime status.
- Existing tests completion_legacy_v1_claim_schema_is_rejected and
  completion_unaffected_v2_schema_three_still_round_trips independently encode
  the same boundary (src/daemon/client/tests.rs:657 onward).

Recommended bounded correction: keep exact event=status, edition, ready-state
checks; replace independent protocol==1/schema==2 predicates with membership
of the exact integer pair in {(1,2),(2,3)}. Preserve current CLI invocation,
transport authentication, bounded capture/timeouts and explicit readiness stop
behavior. Do not permit protocol3, arbitrary schema>=2, cross-pairs (1,3)/(2,2),
or unknown versions. Python JSON booleans compare equal to integers, so require
`type(protocol) is int` and `type(schema) is int` before tuple membership if
strict output validation is intended. Actual typed CLI already validates
negotiated status; the controller adds readiness and edition confirmation, not
an alternative wire negotiation implementation.

Regression recommendation before correction:
1. Fixture status returns each exact allowed pair for both editions. Current
   implementation must fail on (2,3); retain existing (1,2) behavior.
2. Cross-pairs (1,3),(2,2), legacy(1,1), unknown(2,4),(3,3),(0,2), missing fields,
   string versions and booleans refuse. Keep wrong edition and nonready tests.
3. Include one captured actual response shape (sanitized JSON above) in a fixture
   result or construct its v2 run_routes/capabilities fields, establishing that
   extra valid v2 fields do not lead to accidental v1 assumptions. Typed CLI
   owns full capability validation, so avoid duplicating that Rust schema here.
4. Controller's short-/tmp scratch actual daemon proof should pass after fix;
   no Linux/systemd acceptance follows from that macOS Unix proof.

Docs currently say readiness validates protocol/schema versions without listing
pairs; clarify exact supported (1,2)/(2,3) pairs and that current CLI prefersv2
with explicit readonly compatibility downgrade. Earlier report sections asserting
only protocol1/schema2 must be explicitly superseded in the new fix receipt.

The initial long macOS TMPDIR daemon bind failure is a separate harness socket
path length issue, per controller evidence; this investigation performed no new
bind and does not conflate that diagnostic with the confirmed status mismatch.
