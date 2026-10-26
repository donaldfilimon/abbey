# Application protocol v4, schema 3

Schema 3 adds validated administrative data contracts. Task/action/automation
execution remains **unserved** until the durable runtime and authority adapters
are implemented. A type, profile reference, artifact digest or advertised enum
never grants permission. Existing local safe/personal and local/remote boundaries
remain in force.

## Compatible transport and selection

The new framing identity is ASCII `ABS3` (`41 42 53 33`), followed by a four-byte
unsigned big-endian JSON payload length and that many JSON bytes. Length must be
1–65,536 inclusive. The decoder rejects a larger announcement after eight bytes,
before allocating a body. Replies, including authentication/malformed/limit
errors, retain the framing kind. Length includes escaped JSON and all envelope
fields; the eight-byte transport header is separate.

Schema 2 conversations retain the original four-byte-length framing, existing
payloads and supported legacy frame limit. Older v1/v2/v3 and federation framing
remain unchanged. `ABS3` exceeds the legacy maximum and is unambiguous. Framing
and declared schema must match: schema 3 cannot arrive in a legacy frame, nor
schema 2 in an `ABS3` frame. There is no partial-JSON version sniffing.

`DaemonClient::negotiate_v4` explicitly selects schema 3.
`DaemonClient::negotiate_v4_schema2` explicitly selects conversation compatibility.
Neither retries, downgrades or resends after a lost response. The server filters
requested capabilities to its actually served set, so administrative grants are
withheld in both editions. Schema 2 rejects administrative capabilities,
commands and responses during envelope decoding.

The generic framing codec supports fragmented reads and coalesced frames; the
current server handles one bounded request per connection. Windows v4 named-pipe
transport remains unavailable and returns its existing explicit error. This
contract and macOS socket tests do not qualify Windows or Linux runtime builds.

## Record boundaries

- Task creation binds a client operation identity, conversation, configured
  workspace, exact route/profile revision, finite token budget and read-only or
  governed execution kind. The host assigns task/attempt/run IDs and resolves
  authenticated principal/edition. No wire owner or local-personal default exists.
- Mutations require expected task revision and operation identity. Retry also
  names the previous attempt; it cannot authorize repeating an uncertain effect.
  Pause/resume data contracts do not imply the current manager can pause work.
- Task results join one current attempt/run with a closed lifecycle, ordered
  timestamps and monotonic revisions. Event pages bind task, exclusive `after`,
  fixed inclusive `through`, strictly increasing sequences, count and byte bounds.
  Event payloads are closed metadata, not arbitrary executable JSON or prompts.
- Profiles retain bounded configuration plus immutable revision, digest and
  revocation counter. Real authenticated namespace/edition resolution and any
  server-derived scope receipt belong to the later trusted adapter. Missing
  authority context must fail closed, never infer personal execution privileges.
- An action names exact task/attempt/run, operation and preview/input/target/binding
  digests. Decisions echo the exact preview and expected revision. Their receipt
  confirms a later revision; it does not report that an external effect occurred.
  Uncertain effect state is distinct from cancellation and failure.
- Automations use typed Once/Hourly/Daily/Weekly/Cron schedules validated by the
  existing calendar implementation, immutable profile revision, CAS and explicit
  enablement. Manual operation identities and scheduled UTC occurrences are
  distinct. A run-now receipt joins occurrence, task, attempt and run; a definition
  echo alone is insufficient execution evidence.
- Artifact references carry bounded identity, SHA-256, media type and byte count.
  They provide no path, URL, credentials or transfer authorization. Attachments,
  datasets and model bytes require a separately authenticated bounded stream;
  its runtime is outside this contract packet and is not simulated in frames.

Identifiers are at most 128 bytes. Goals/instructions are at most 1,024 bytes;
page count is at most 50. Administrative pages also cap their encoded content at
48 KiB, leaving envelope space; transport checks remain authoritative. Integer
revisions/sequences/timestamps stay within JavaScript's exact integer range,
with positive revisions. Public records redact private configuration and text
from Debug. Unknown fields, closed-enum violations, invalid relationships and
unrelated command/event pairs fail validation.

## Sources and fixtures

Rust declarations under `src/app_core/v4/` and `v4_protocol.rs` are canonical;
`desktop/codegen` derives TypeScript and actual Rust serialization examples.
The independently authored versioned corpus is under
`tests/fixtures/application-schema3/`: positive administrative records,
malformed requests, literal schema-2 envelopes and framing vectors. Regression
checks cover semantic bounds, exact correlation and real authenticated
RuntimeHandler/SQLite conversation compatibility. They do not qualify durable
administration, artifact streams, remote tenants or deployments.

Compatibility direction matters: existing schema-2 clients can use the new
server unchanged. Existing desktop conversation and route sessions explicitly
select schema 2, preserving legal large transcript items. New Rust consumers
may explicitly select schema 2 or schema 3. This is a fixed rollout profile,
not an automatic retry after an unknown outcome. No CLI v4 selection verb exists.

| Client selection | Schema-2-only daemon | Current daemon |
| --- | --- | --- |
| Existing schema-2 client and preserved desktop conversations | Supported | Supported |
| New Rust client explicitly calling `negotiate_v4_schema2` | Supported | Supported |
| Explicit schema-3 Rust selection | Refused; update daemon or explicitly select schema 2 through the Rust API | Supported within the schema-3 envelope/item limits |

Schema-3 page responses budget the complete encoded envelope, including escaping
and framing metadata. Only whole trailing items may be deferred, with
`next_after` naming the last retained item. Stored text, identities and existing
fixed watermarks are unchanged. Existing conversation/message/content pages have
live cursors rather than an explicit `through` field; this adapter preserves
those semantics and does not manufacture a snapshot watermark.

A legal individual text delta may encode to more than 64 KiB: 16,384 control
bytes can escape to 98,304 JSON text bytes. A legal user message can be larger
still. Schema 3 explicitly returns `ResponseTooLarge` for an indivisible item;
it never clips text, splits an event or reports missing content as complete.
Schema 2 retains those conversation payloads. An owned schema-3 chunk/artifact
read API and its frontend consumer remain **required follow-on implementation**
before desktop/browser conversations migrate. No lossless schema-3 chunk reader
or artifact-stream runtime is claimed here.

Generated `SUPPORTED_APPLICATION_SCHEMA` is 3; the preserved
`DESKTOP_CONVERSATION_SCHEMA` is 2. `V4DaemonSession::schema_version()` reports
the actually selected schema. Selection precedes effects; there is no automatic
fallback, reconnect/resend, remote authority or unavailable platform adapter.
