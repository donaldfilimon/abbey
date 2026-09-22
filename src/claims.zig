//! The capability ledger. `docs/claims.md` is generated from this table by
//! `abbey-zig claims --markdown`; the gate regenerates it, diffs it, and
//! checks that every Current row names tests that exist (`test "..."` in
//! src/, or a `gate:` stage in tools/check.sh). A capability with no test
//! is Proposed, whatever the code looks like.
const std = @import("std");

pub const Status = enum {
    current,
    partial,
    proposed,
    out_of_scope,

    pub fn label(s: Status) []const u8 {
        return switch (s) {
            .current => "Current",
            .partial => "Partial",
            .proposed => "Proposed",
            .out_of_scope => "Out of scope",
        };
    }
};

pub const Claim = struct {
    id: []const u8,
    status: Status,
    capability: []const u8,
    evidence: []const u8,
    tests: []const []const u8 = &.{},
};

pub const all = [_]Claim{
    .{ .id = "cli-p1-parser-help", .status = .current, .capability = "hand-written clap-equivalent parser for the P1 verbs", .evidence = "globals before/after the subcommand, trailing prompt keeps dashes, `--flag=value`, aliases; help text pinned per command by tests/golden/help/*.txt", .tests = &.{ "globals before and after the subcommand, trailing prompt keeps dashes", "help, version, aliases, and errors", "help text matches every golden", "gate:help goldens" } },
    .{ .id = "edition-separation", .status = .current, .capability = "compile-time safe (default) vs personal edition", .evidence = "identity and namespaces only; NEW roots `~/.local/state/abbey-zig[-personal]` and `ABBEY_ZIG_[PERSONAL_]STATE_DIR` / `_CONFIG`; daemon socket/bearer variables `ABBEY_ZIG_[PERSONAL_]DAEMON_SOCKET_PATH` / `_BEARER_TOKEN[_FILE]`; the Rust `ABBEY_STATE_DIR` / `ABBEY_CONFIG` / `ABBEYD_*` are never read. Behavior variables stay shared: ABBEY_BACKEND, ABBEY_MODEL, ABBEY_PERSONA, ABBEY_ROLE, ABBEY_ABI_BIN, ABBEY_AGENT. No unrestricted runtime in either edition", .tests = &.{ "edition namespaces never reuse the Rust variables", "state root honors only the edition variable", "gate:build (personal edition)" } },
    .{ .id = "config-load", .status = .current, .capability = "config.toml subset parser + shared env overrides", .evidence = "port of Rust parse_toml_lite (flat keys + [roles]; [embeddings] skipped because learned embeddings are Proposed); ABBEY_ROLE/PERSONA/MEMORY_BACKEND/ABI_BIN override", .tests = &.{ "parse default shape activates nothing", "parse flat keys, roles table, quotes, comments, unknown tables", "no commented line masquerades as an assignment", "load applies shared env overrides over the edition config file" } },
    .{ .id = "backend-resolution", .status = .current, .capability = "executor backend precedence and binary resolution", .evidence = "ABBEY_BACKEND env > config backend > legacy ABBEY_AGENT > ollama when `ollama list` names gemma4:26b-mlx > grok, fm, abi, claude, cursor last; unknown ABBEY_BACKEND selects ollama and never falls through; abi resolves config abi_bin / ABBEY_ABI_BIN first and never falls through to cursor-agent", .tests = &.{ "default backend prefers ollama but never requires cursor", "configured backend outranks the legacy agent path; unknown env selects ollama", "abi resolution uses configured abi_bin and never falls through to cursor", "ollama probe requires the default model in ollama list", "backend aliases parse" } },
    .{ .id = "backend-argv-grammars", .status = .current, .capability = "isolated argv grammars for abi, ollama, claude, fm, grok/cursor-agent", .evidence = "each grammar built from scratch (no leaked flags); abi/ollama prompts always follow `--`; abi live transport only for bare claude-* or live/anthropic; Claude vocabulary clamp and session mint/resume; per-prompt 96 KiB UTF-8-safe clamp", .tests = &.{ "fm argv never leaks cursor flags", "abi argv never leaks cursor or fm flags and the prompt follows --", "abi transport is live only for explicit aliases", "claude argv clamps vocabulary and never leaks cursor flags", "claude session mints then resumes", "ollama argv and aliases", "cursor argv carries its own grammar", "utf8 tail and truncation respect boundaries and caps" } },
    .{ .id = "persona-routing-contracts", .status = .current, .capability = "Abbey/Aviva/Abi persona selection and frozen response contracts", .evidence = "ABBEY_PERSONA > explicit leading address > 29-keyword prefix router with f32 `score * 0.1` accumulation and a 0.40/0.30/0.30 prior, ported from ../abi abi-ai; contract bytes include U+2019", .tests = &.{ "neutral input defaults to abbey; weights normalize", "action favors aviva, orchestration favors abi", "suffix false positives do not shift routing; prefix stems match", "explicit selector rules", "wrap uses the frozen contract bytes" } },
    .{ .id = "role-routing", .status = .current, .capability = "Max/Gemma role decision with audit-only confidence/alternate/fallback", .evidence = "keyword task classes; override 0.95, ABBEY_ROLE 0.9, class 0.85, hybrid 0.7, other 0.55; never re-invokes on low confidence. Roles are model bindings, not bundled weights", .tests = &.{"classify and decide match the Rust heuristic"} },
    .{ .id = "canonical-run-path", .status = .current, .capability = "one execution path: actions.runAgent(RunSpec) -> session.hybridRun", .evidence = "persona x role wrap, standing-preference context, route record, activity memory, resilient resume/create; the ask prompt handed to abi is byte-identical to the Rust-built argv element", .tests = &.{ "run specs encode the surface contracts", "assembled ask prompt matches the Rust-built abi argv element", "print bypass leaves route.jsonl untouched where ask appends one record" } },
    .{ .id = "headless-bypasses", .status = .current, .capability = "print and commit headless bypasses through one capture.zig", .evidence = "capture resumes only the live backend's chat and writes no route record or activity memory; commit reads `git diff --cached` (3000-line cap) and prints the captured message without committing", .tests = &.{ "print bypass leaves route.jsonl untouched where ask appends one record", "commit bypass captures a staged diff prompt without a route record", "truncate diff caps lines" } },
    .{ .id = "route-log-rust-compatible", .status = .current, .capability = "append-only route.jsonl byte-compatible with the Rust reader", .evidence = "serde field order, `skip_serializing_if` optionals, serde_json string escaping and f32 formatting (checked against serde_json output); the gate feeds a Zig-written log to the Rust `abbey routes` reader when present", .tests = &.{ "record JSON is byte-identical to a Rust-written route.jsonl line", "append and read back, optional fields round-trip, bad lines skipped", "writeF32 matches serde_json f32 output", "writeString matches serde_json escaping", "gate:rust route reader" } },
    .{ .id = "local-transcript-continuity", .status = .current, .capability = "Abbey-side transcript continuity for abi/ollama", .evidence = "`### user/### abbey` turns under <state>/<backend>/<id>.transcript, 8 KiB context tail replayed after `--`, 1 MiB rollover keeps .transcript.prev, per-turn exclusive lock; nonzero exit is never retried for local backends", .tests = &.{ "abi resume carries bounded transcript context", "second ask carries bounded transcript context and a nonzero exit is not retried for abi", "local transcript rolls over and keeps the previous file", "failed retry keeps the original chat" } },
    .{ .id = "chat-state", .status = .current, .capability = "edition-scoped chat id, per-cwd mirror, model file, history log", .evidence = "chat-id + by-cwd/<key> owner-only files; CURSOR_AGENT_CHAT_ID adopted only by server-session backends; history.log `<ts ms>\\t<id>\\t<cwd>`. The Rust runtime.sqlite identity journal is not reproduced (see conversation-identity-journal)", .tests = &.{ "save then resolve, per-cwd mirror, and CURSOR_AGENT_CHAT_ID only for server backends", "cwd key sanitizes like Rust" } },
    .{ .id = "memory-jsonl-append-only", .status = .current, .capability = "Zig-native append-only JSONL STM/LTM/activity/train_candidate store", .evidence = "one Rust-MemoryRecord-shaped JSON object per line under <state>/memory/memory.jsonl; store/update/promote/invalidate append snapshots, readers fold last-wins by id; obsolete records are retained; train_candidate requires provenance; SQLite column mapping documented in src/memory/record.zig; `memory search` is a case-insensitive substring over live records", .tests = &.{ "append-only fold: last snapshot wins, obsolete is retained not deleted", "record JSON has serde field order, nulls, and round-trips", "reflect ignores route duplicates and requires payload equality", "new stm record carries id, timestamp, project, stm tag", "keyword search is case-insensitive over summary, payload, provenance" } },
    .{ .id = "memory-lexical-similarity", .status = .current, .capability = "lexical feature-hash similarity over memory", .evidence = "signed wyhash 1/2/3-gram hashing into 32 dims + cosine, ported from abi-ai embedding.rs; std.hash.Wyhash matches the abi-foundation golden vectors; surfaced as `memory similar`. Not a learned embedding", .tests = &.{ "wyhash matches the abi-foundation golden vectors", "embeddings are unit, deterministic, case-insensitive, empty is e0", "shared n-grams outrank unrelated text and a typo still matches" } },
    .{ .id = "learn-pipeline", .status = .current, .capability = "self-learn curation: correction, train, preference, routes, digest, export, review, stats, improve", .evidence = "idempotent route promotion (provenance `route.jsonl @ <ts>`), reflect-driven digest, train_candidate provenance gate, additive-only improve apply; `learn lora` exits 2", .tests = &.{ "learn routes keeps alternate/fallback and is idempotent over the same tail", "learn correction, train, preference, stats, review, export, status", "plan groups low-confidence routes and clusters duplicates" } },
    .{ .id = "doctor", .status = .current, .capability = "doctor: paths, backend source, model, chat, memory, config", .evidence = "honest when no executor is installed; reports the requested memory backend when it is not jsonl", .tests = &.{"doctor reports the env-selected abi backend and edition paths"} },
    .{ .id = "wdbx-subprocess-bridge", .status = .current, .capability = "`wdbx` bridge to `abi wdbx` with base-path translation", .evidence = "argv construction matches the Rust bridge (`query` gains --json and Abbey's <state>/wdbx/wdbx base path); the in-process `stats`/`checkpoint` verbs refuse because WDBX is never linked", .tests = &.{"wdbx bridge argv matches the Rust bridge"} },
    .{ .id = "bounded-subprocess-capture", .status = .current, .capability = "bounded executor capture", .evidence = "stdin closed, 4 MiB stdout/stderr ceilings, 30-minute deadline (2 s for the ollama probe), exit code or 1 on signal", .tests = &.{ "capture collects stdout, stderr, and the exit code", "capture enforces the stdout ceiling and the deadline" } },
    .{ .id = "abi-end-to-end", .status = .current, .capability = "doctor, ask, print, commit, learn status against a real abi binary", .evidence = "gate stage runs the binary with ABBEY_BACKEND=abi, a temp HOME (abi writes ~/.abi there, never Donald's) and a temp state dir, when ABBEY_ZIG_E2E_ABI names an abi built from ../abi; it SKIPs loudly otherwise", .tests = &.{"gate:e2e with a real abi binary"} },
    .{ .id = "contracts-corpus-vendored", .status = .current, .capability = "vendored Abbey contract corpus with lock file", .evidence = "contracts/abbey copied byte-identical from ../abbey; the gate runs ../abi's tools/abbey_contracts.py (copied as tools/abbey_contracts.py) `verify` and checks the lock digest", .tests = &.{"gate:contracts qualification"} },
    .{ .id = "surface-parity-rust-cli", .status = .partial, .capability = "command-surface parity with the Rust abbey CLI", .evidence = "ships ask, print, commit, doctor, learn, routes, config, claims, memory, wdbx, daemon (serve/status/claims/routes), edition, version, help; every other Rust verb is absent (not a passthrough), and unknown verbs are an error" },
    .{ .id = "daemon-protocol-v1", .status = .current, .capability = "abbeyd protocol v1: read-only Status, Claims, ReadRoutes", .evidence = "Rust wire shapes byte for byte (serde order, adjacently tagged commands/events, unknown fields refused) and the Rust server's check order: JSON, bearer (echoing the caller's version), post-auth rate limit 64/s, strict envelope, request_id grammar, version, command minimum version, payload bounds. Status advertises exactly read_status/read_claims/read_routes at protocol 1 / schema 1; claims come from this ledger (name = claim id, note = capability plus evidence, instead = null; the `contains` filter matches id, capability, and evidence separately, so a needle spanning the joined note does not match); served by `abbey-zig daemon serve` and `abbeyd-zig`", .tests = &.{ "protocol v1 status round trip is the exact Rust wire fixture", "wrong bearer echoes the caller's version; v2 gets unsupported_version at v2 with its id", "request ids, unknown fields, and payload bounds fail closed in the Rust order", "claims read the canonical ledger with typed filters", "only authenticated requests consume the bounded rate limit" } },
    .{ .id = "daemon-unix-socket-transport", .status = .current, .capability = "owner-only Unix socket transport with 1 MiB big-endian framing", .evidence = "parent directory created 0700 and refused unless a real directory owned by the effective user with no group/other bits; socket chmod 0600 after bind and unlinked on shutdown (SIGINT/SIGTERM); only a dead socket we own is replaced; u32 big-endian length, empty and >1 MiB frames answered with their error frames; one connection at a time under 5 s read/write deadlines (poll, not SO_RCVTIMEO); Darwin 103-byte path limit enforced; bearer from exactly one of the edition's inline or owner-only file variable, 32..4096 bytes, no controls", .tests = &.{ "the daemon serves an authenticated status request and removes its socket on stop", "over the socket: a wrong bearer, an empty frame, and an oversize frame all fail closed", "the socket directory must be owner-only, and a foreign path is a conflict", "bearer rules: length, line ending, controls, and exclusive sources", "bearer file must be owner-only and regular", "lstat reports kind, owner, and mode without following symlinks" } },
    .{ .id = "daemon-route-audit-sanitized", .status = .current, .capability = "sanitized route-audit tail over the daemon", .evidence = "port of app_core/routes.rs: cwd becomes ws-<12 hex> of SHA-256 over a NUL-terminated domain (pinned against Python), Unicode-whitespace tokens that look like paths or contain the daemon's HOME become [path], Cc code points stripped, 64/240-byte UTF-8-safe truncation, confidence quantized to whole percent with NaN/inf -> 0, at most 50 newest entries, records failing the consumer invariant dropped (never refilled); the client re-validates every page. The RFC 3339 check is narrower than chrono (no lowercase t/z, space separator, or leap second)", .tests = &.{ "the working directory becomes a pinned opaque digest and never a path", "free text is bounded, control-stripped, and path-redacted with Unicode whitespace", "confidence is quantized and clamps instead of wrapping", "validation rejects an unsanitized entry and a record that cannot be represented is dropped", "route audit page caps at the limit, keeps the newest, and skips malformed lines", "unicode whitespace and control predicates match Rust char semantics", "rfc3339 checker accepts chrono shapes and rejects the rest", "a sanitized route audit page crosses the socket and the client re-validates it" } },
    .{ .id = "daemon-rust-client-wire-compat", .status = .current, .capability = "the Rust `abbey daemon` client talks to the Zig daemon", .evidence = "gate runs the installed Rust client (temp HOME and state, ABBEYD_SOCKET_PATH/ABBEYD_BEARER_TOKEN at a temp socket) for status, claims, routes; a recording proxy observes its v2 attempt refused with unsupported_version and its v1 retry served; a hostile route.jsonl line passes the Rust page validator with no path or control character on the wire; a wrong bearer is refused; the socket is gone after SIGTERM; SKIPs loudly when the Rust binary is absent", .tests = &.{"gate:rust daemon client"} },
    .{ .id = "tui", .status = .proposed, .capability = "seven-tab terminal UI with live Ctrl-B backend switch (P3)", .evidence = "not implemented" },
    .{ .id = "mcp-server", .status = .proposed, .capability = "read-only MCP server over stdio and loopback HTTP (P4)", .evidence = "not implemented" },
    .{ .id = "daemon-protocol-v2-v3", .status = .proposed, .capability = "daemon protocol v2/v3 and abbey.v1 federation: run control, tool authority, model lifecycle, signed manifests", .evidence = "not implemented; the v1 daemon answers a v2 envelope `unsupported_version` (so the Rust client retries at v1) and a v2 run command at v1 `unsupported_command`; v3 and federation frames are never decoded and fall out as `malformed_request` or `unauthorized`" },
    .{ .id = "os-control-allowlist", .status = .proposed, .capability = "OS control allowlist with the never-without---confirm invariant", .evidence = "not implemented in P1; no OS-control verb exists, so nothing can run" },
    .{ .id = "voice", .status = .proposed, .capability = "voice I/O and the voice-ask headless bypass", .evidence = "not implemented (Decision 3)" },
    .{ .id = "learned-embeddings", .status = .proposed, .capability = "learned semantic embeddings and semantic search", .evidence = "not implemented; only lexical feature hashing is Current" },
    .{ .id = "conversation-identity-journal", .status = .proposed, .capability = "runtime.sqlite conversation identity journal and legacy migration", .evidence = "not implemented; chat-id files are authoritative in this rewrite" },
    .{ .id = "hybrid-loop-subagents-improve", .status = .proposed, .capability = "hybrid-loop, subagents, parallel lanes, goal-driven improve loop", .evidence = "not implemented" },
    .{ .id = "media-attach-highlight", .status = .proposed, .capability = "media path attach and syntax highlighting of printed output", .evidence = "not implemented; output is emitted verbatim" },
    .{ .id = "memory-time-filters", .status = .proposed, .capability = "RFC 3339 since/until memory filters", .evidence = "not implemented; equality filters only" },
    .{ .id = "cursor-account-passthrough", .status = .proposed, .capability = "cursor-agent account verbs, create-chat surfaces, and external passthrough", .evidence = "not implemented as verbs; server-session create-chat is used internally only" },
    .{ .id = "lora-training", .status = .proposed, .capability = "LoRA / fine-tune training", .evidence = "refused (`learn lora` exits 2); curation only" },
    .{ .id = "accel-desktop-mesh-windows", .status = .proposed, .capability = "accelerator verify, desktop app, multi-node mesh, Windows host", .evidence = "not implemented" },
    .{ .id = "linked-abi-wdbx", .status = .out_of_scope, .capability = "linking abi crates or an in-process WDBX", .evidence = "Decision 1: abi is consumed only as a subprocess (`abi complete`, `abi wdbx`)" },
    .{ .id = "unrestricted-runtime", .status = .out_of_scope, .capability = "unrestricted or autonomous OS runtime in any edition", .evidence = "editions separate identity only" },
    .{ .id = "rust-state-sharing", .status = .out_of_scope, .capability = "reading or writing the Rust tree's state, config, or SQLite stores", .evidence = "separate edition-scoped roots by design" },
};

pub fn counts() [4]usize {
    var c: [4]usize = @splat(0);
    for (all) |x| c[@backingInt(x.status)] += 1;
    return c;
}

pub fn writeMarkdown(w: *std.Io.Writer) std.Io.Writer.Error!void {
    const c = counts();
    try w.writeAll("# abbey-zig claims\n\n<!-- Generated by `abbey-zig claims --markdown`; tools/check.sh fails on drift. Do not edit. -->\n\n");
    try w.print("**{d} Current, {d} Partial, {d} Proposed, {d} Out of scope.** A Current row names the tests that prove it (`gate:` rows are tools/check.sh stages); a capability without a test is Proposed.\n\n", .{ c[0], c[1], c[2], c[3] });
    try w.writeAll("| ID | Status | Capability | Evidence boundary | Tests |\n| --- | --- | --- | --- | --- |\n");
    for (all) |x| {
        try w.print("| `{s}` | {s} | {s} | {s} | ", .{ x.id, x.status.label(), x.capability, x.evidence });
        for (x.tests, 0..) |t, i| {
            if (i != 0) try w.writeAll("<br>");
            try w.print("`{s}`", .{t});
        }
        try w.writeAll(" |\n");
    }
}

pub fn writeTable(w: *std.Io.Writer) std.Io.Writer.Error!void {
    const c = counts();
    try w.print("claims: {d} Current, {d} Partial, {d} Proposed, {d} Out of scope\n\n", .{ c[0], c[1], c[2], c[3] });
    for (all) |x| try w.print("{s:<13} {s:<32} {s}\n", .{ x.status.label(), x.id, x.capability });
}

test "every Current claim names at least one test; ids are unique" {
    for (all, 0..) |x, i| {
        if (x.status == .current) try std.testing.expect(x.tests.len > 0);
        for (all[i + 1 ..]) |y| try std.testing.expect(!std.mem.eql(u8, x.id, y.id));
        try std.testing.expect(std.mem.findScalar(u8, x.evidence, '|') == null);
        try std.testing.expect(std.mem.find(u8, x.evidence, "\u{2014}") == null); // no em dashes
    }
}
