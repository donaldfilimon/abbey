# Goals

## Fully independent TUI + agent runtime
status: done
- Closed 2026-08-08 on the **abi-backed-independence** reading: Abbey runs CLI,
  slash, and the full TUI with no cursor-agent present (`ABBEY_BACKEND=abi` or
  config `backend = "abi"`), including Abbey-side conversation continuity. The
  windowed-GUI and Abbey-as-own-runtime readings are **not** closed — they are
  recorded as boundaries below and in `tasks/todo.md`, exactly like multi-node
  and semantic embeddings were for earlier goals. Every buildable slice under
  the accepted reading is shipped, gate-green, and live-verified.

Captured 2026-07-31: "fully independent tui and runtime similar to claude code or
codex" — executor slice in progress (see below); TUI/GUI modernization still open.

**This collides with the project's central architecture decision, stated in
CLAUDE.md and enforced throughout this session:** "Backend is `cursor-agent`, not
a reimplementation of Grok/Codex/Claude runtimes." That still holds for the
*default* path and for "Abbey as tool runtime / MCP host / ACP host" (OOS;
`abbey runtime` still refuses with exit 2). The 2026-08-08 reading does **not**
require deleting those refusals: `ABBEY_BACKEND=abi` is an alternate executor
(`abi complete`), not Abbey becoming a tool runtime. Closing the *whole* goal
still needs TUI/GUI modernization choices (richer ratatui vs windowed GUI) and
an explicit decision if cursor-agent should ever stop being the default.

The phrase is genuinely ambiguous between readings that differ by orders of
magnitude (see the question asked in-session): a richer standalone TUI surface
(mostly tractable — Abbey's ratatui TUI already never touches cursor-agent's
presentation) vs. Abbey calling model APIs directly with its own tool-execution
loop (a large rewrite, replacing cursor-agent as executor) vs. local model weights
(currently Out of scope in three separate gate rows). Not resolved without the
user picking one — no reading was assumed or started.

**Re-captured 2026-08-08 (`/goal`): "interactive cli and tui and gui/tui advanced
more modern rust implementations without requirement for any other backend but
abi."** This resolves part of the 2026-07-31 ambiguity: the chosen direction is
Abbey standing alone on the **sibling `abi` workspace as the sole backend** — no
cursor-agent requirement — with a modernized interactive CLI + TUI (and a GUI/TUI
surface). Same coarse intention as this section, so recorded here rather than as a
duplicate goal (ledger rule: one intention, one `##`).

**Executor slice (2026-08-08, Max):** `ABBEY_BACKEND=abi` → `abi complete` is now a
real AgentBackend (own argv grammar, `--` separator, account/MCP/gen refuse, no
stale-chat retry). Default transport is local persona-template; bare `claude-*` /
`live`|`anthropic` opt into abi's Anthropic transport; cursor role/thinking
bindings (e.g. Max→`claude-*-thinking-*` leftovers in state) stay local — no
silent `--live`. Binary resolution prefers `ABBEY_ABI_BIN`/`abi_bin` and never
falls through to cursor-agent candidate paths. Claims gate + AGENTS/docs aligned.
This names the generation backend the earlier note asked for (`abi complete`), but
does **not** close the whole goal: interactive CLI/TUI modernization and any
windowed GUI remain open, and cursor-agent stays the default backend.

**Verified 2026-08-08:** `./check.sh` green end-to-end (fmt · clippy · tests, both
feature sets: 150+5+7 default, 156+5+7 wdbx · file-size guard). Live against a
locally built `abi` release binary (`ABBEY_ABI_BIN`, `ABI_WDBX_PATH=:memory:` to
skip the 94 MB store recovery that otherwise makes every `abi complete` open take
minutes): `abbey -p` returns a persona-template completion with honest metadata
(exit 0); `login` and `imagine` refuse with exit 2; `models` lists the abi
vocabulary; a leading-dash prompt arrives as text (the `--` separator works);
`doctor` names the active backend + resolved binary; `route.jsonl` gains rows, so
the routing audit sees abi runs. Also caught live before commit: a stale binary
routed `ABBEY_BACKEND=abi` to cursor-agent because the bin target had a private-
module path error (`E0603`) that `cargo test`'s lib pass never compiled — the
`/bin/echo` argv probe is what exposed it. Argv echo confirmed:
`complete --model local -- <persona/role-wrapped prompt>`.

**TUI backend-visibility slice (2026-08-08, /goal continue):** the TUI now tells
the truth about which executor runs the next prompt — `backend` KPI chip on Home,
backend name in the status bar (warn-coloured whenever it is not the cursor
default), a Doctor `backend:` line naming the active executor and its transport
rules, and a Doctor roles line that stops claiming "cursor-agent bindings" under
`fm`/`abi` (prompt-only there). Models tab prefetches the static model list for
non-cursor backends so it never falls back to the cursor alias table under
`abi`/`fm`. Helpers are pure free functions with unit tests (152/158 with wdbx,
gate green). TUI *rendering* was not driven live (no TTY in this session) — the
claim is compile + unit-tested helpers, not a screenshot.

**In-TUI backend switcher slice (2026-08-08, /goal continue):** Ctrl-B and a
"Cycle backend" palette entry rotate cursor → grok → fm → abi *in place* —
`resolve_agent_for(backend)` (new; ignores the `ABBEY_AGENT` override, which
belongs only to the env-chosen backend) re-resolves the binary per candidate and
unresolvable backends are skipped with no state change; if nothing else resolves,
the current backend stays and the status line says so. Switching refreshes the
Doctor panel, clears/prefetches the Models list, and the status-bar/chip honesty
from the previous slice makes the active executor visible immediately. Cycle
order + wrap covered by a unit test; gate green (153+5+7 / 159+5+7). Same honesty
caveat: switch logic is unit-tested and compiled, not TTY-driven in this session.

**Home routes-audit pane slice (2026-08-08, /goal continue-all):** the Home tab
is now multi-pane — Session (left), recent chats (right-top), and a live
**Routes · audit** pane (right-bottom) showing the compact tail of
`route.jsonl` (clock · persona/role · model · confidence · stage) via a new
`compact_route_line` next to the shared `format_route_line`, unit-tested
including the odd-timestamp fallback. The pane refreshes with the doctor
refresh, which already re-runs after every agent run, so each prompt's routing
decision appears immediately — the same records `abbey routes` prints, audit
only. Gate green (154+5+7 / 160+5+7). This delivers the "richer multi-pane"
reading for Home; same TTY caveat as the other TUI slices.

**Continuity + default-backend slice (2026-08-08, final):** the last two
buildable residuals landed, so the goal closes on evidence rather than on a
green gate alone.

- **Abbey-side continuity under abi.** Turns append to
  `<state>/abi/<chat>.transcript`; a bounded 8 KiB tail rides into the next
  turn as a context element *after* the `--` separator. `abi complete` itself
  remains a stateless one-shot — the continuity is Abbey's, and the claims
  gate says exactly that. Verified live by argv probe: turn 1 (fresh chat) has
  no context element; turn 2 carries `Previous conversation …` with turn 1's
  text, and both turns share one transcript.
- **Default backend is now the user's config choice**, not a code decision:
  `backend = "cursor"|"grok"|"fm"|"abi"` in `config.toml`, precedence
  `ABBEY_BACKEND` env > config > cursor. A *set but unknown* env value still
  means cursor, so an env typo cannot silently activate a config backend.
  Verified live: config-only selection activates abi; env `cursor` overrides it.
- **Two defects found live by this slice, both fixed** (neither was reachable
  from the unit tests): (1) `read_chat` adopted `CURSOR_AGENT_CHAT_ID` under
  every backend — running `abbey -c` under abi *inside a cursor session*
  resumed the cursor id, so each turn wrote a fresh transcript and continuity
  silently never happened; it is now gated on `has_server_sessions()`.
  (2) `abbey doctor` printed `ABBEY_BACKEND=<label>` unconditionally, which
  lied whenever the backend came from the config key or the default; it now
  names the real source (`from config backend=abi` / `from default`).

**Boundaries — deliberately not built (decisions, not pending work).** Reopening
one means amending the claims gate in `AGENTS.md` first, not ticking a box:

| Reading | Status | Why it stays closed |
|---|---|---|
| Windowed GUI (egui/winit) | **Proposed** | A new dependency tree and a new claims-gate row. Abbey's surface is a terminal CLI/TUI; "advanced/modern" was satisfied in-terminal (multi-pane Home, routing audit, backend switcher). Needs an explicit gate amendment, not a `continue` |
| Abbey as her own agent runtime (own tool loop / model APIs direct) | **Out of scope** | Unchanged from 2026-07-31: contradicts "backend is cursor-agent, not a reimplementation" and would require deleting shipped refusals (`abbey runtime` exit 2, `src/surfaces.rs`, `src/claims.rs`). `ABBEY_BACKEND=abi` satisfies independence *without* that |
| cursor-agent stops being the shipped default | **Decision, not built** | Now moot as a code question — the default is a config key, so each user picks. Changing the *shipped* default would silently re-point existing installs |
| Semantic recall from abi continuity | **Proposed** | The context prefix is delivered; whether the model uses it is the model's business. abi's local transport is a deterministic persona template (it echoes, it does not answer), so recall is only as good as the selected transport |

Found live 2026-07-31 while verifying `improve status`: `abbey memory map | head -2`
printed a Rust panic (`failed printing to stdout: Broken pipe`) instead of exiting
quietly like `cat`/`ls`. Rust's std sets `SIGPIPE` to `SIG_IGN` before `main`, so a
closed downstream reader becomes an `EPIPE` error that `println!` panics on. Only
fires once output exceeds the pipe buffer, which is why short commands looked fine
and it went unnoticed.

**Current:** `restore_sigpipe()` resets `SIG_DFL` as the first statement of `main` —
before `Cli::parse()`, which itself prints for `--help`/`--version`. `libc` added as
a `cfg(unix)` dep only (already in the tree transitively, so no extra build cost);
Windows gets a no-op arm since it has no `SIGPIPE`. Verified live on `memory map`,
`--help`, and `claims` piped to `head` — all clean stderr — and unpiped runs still
exit 0. Deliberately **no test**: reproducing `EPIPE` needs a real pipe with a
closing reader, and a flaky test would be worse than none.

## Production structure + spec
status: done
- Decomposed `main.rs` (1074→86) into session/commands/slash_dispatch/doctor/prompts
- Spec expanded: docs/architecture.md, docs/production.md, identity claims Current
- `./check.sh` production gate (fmt + clippy -D + test + file-size)
- v2.6.0 modular layout under 1k-line rule

## Hybrid routing and memory
status: done
- Personas, Max/Gemma, SQLite learn, TUI 7 tabs, OS control, parallel, inventory, build stamp
- Re-opened 2026-07-30: was marked `done` while `tasks/todo.md` Phase 3 (WDBX bridge) and
  Phase 4 (Gemma→Max hybrid loop) were still unchecked — named subsystems, not cosmetic leftovers
- Phase 3 closed: in-process `WdbxMemory` over `abi-wdbx` `DurableStore` behind the
  off-by-default `wdbx` feature; `memory::open_backend` dispatches every call site;
  `check.sh` now gates both feature sets. Verified live: put/search/get/promote/export
  across separate processes, WAL recovery on reopen, `wdbx stats`/`checkpoint`
- Phase 3 hardening: concurrent `abbey` processes corrupted the WDBX WAL beyond recovery
  (20 writers → store permanently unreadable). Fixed with `flock(2)` per store; 40/40
  concurrent writes now land. Found by testing the case rather than assuming it
- Phase 3 bridge verified live against a locally built `abi-cli` (`cargo build -p abi-cli`
  in `../abi`), including a cross-check that `abi wdbx query` reads the same records the
  in-process backend wrote. Also fixed a path-convention trap: `abi` takes base paths,
  Abbey opens a directory
- Phase 4 closed: `abbey hybrid-loop` (Gemma interpret → Max implement) links both stages
  by correlation id; `abbey routes --correlation <id>` reads them back. Verified live
- Strategy brief re-captured 2026-07-30 as **Hybrid model-routing architecture strategy**
  for Proposed residuals (richer routing, semantic memory, train curation) — not a reopen

## On-device backend, spatial memory, and capability reporting
status: done
- `ABBEY_BACKEND=fm` on-device executor (no cursor-agent/network); argv isolation + account
  verb refusals verified
- Interpretable 3-D memory map (topic × recency × consolidation) on both backends;
  TUI Memory tab shows the map preview; `nearest_to` shared (no dead spatial dual-write)
- Deferred by construction remain Out of scope / Proposed: multi-node training, NPU/TPU
  learning, autonomous OS, semantic embedder

## Broader polish
status: done

**Review 2026-07-30:** `./check.sh` was **not runnable in a default shell** — Homebrew's
rustc/cargo 1.97.1 shadows rustup's nightly 1.99.0 on PATH (no shims in `~/.cargo/bin`),
and the sibling `../abi` crates now require `rust-version = 1.99`. The code was fine:
the full gate passes under nightly 1.99 (101+7 and 107+7 tests). Added a toolchain
guard to `check.sh` that turns cargo's bare "not supported by the following packages"
into the actual remedy (`brew unlink rust` / `rustup default nightly`), verified in
both the broken and working configurations.

## Hybrid model-routing architecture strategy
status: done

Captured 2026-07-30 from the Hybrid Model Routing and Memory Architecture Strategy
brief. Coarse intention: formalize model specialization as a first-class principle
(Abi routes/moderates, Abbey integrates, Aviva as precise expert mode; Max =
changeable technical worker role; Gemma = visual/conversational), keep memory
backend-replaceable with provenance, delay LoRA/fine-tuning until curated data
exists, treat activity+corrections as future training substrate.

**Closed as Current (buildable slices):** richer `route_decision` confidence /
alternate / fallback on the route log (audit only — no second execution path);
`abbey learn review` + `stats` for train_candidate provenance curation; please-fix
capture summarizer + argv clamp. Already-shipped Max/Gemma/personas/memory/hybrid-loop/
fm/3-D map remain as before.

**Residuals stay Proposed / Out of scope (not laundered):** semantic embedding
space; multi-node / NPU-TPU learning; autonomous OS; local weights / LoRA;
CouchDB/Python second memory stack. Claims gate in AGENTS.md stays binding.

**Review 2026-07-30 (`/goal /review`) — gap found, recorded not built:** the brief's
memory interface names *filter by source*, *by timestamp*, and *by project/domain*
(plus recency weighting and confidence thresholds in the retrieval layer).
`MemoryStore::filter` only takes `(retention, tag, limit)`. Unlike semantic search
these need no embedder, so they were neither shipped nor listed as residual — an
honesty hole in this goal's close. Now **Proposed** in the claims gate. Goal stays
`done`: this is a named future slice, not a re-opening.

**Polish (2026-07-30):** CLI/slash route line parity via `format_route_line`;
hybrid-loop stages record paired alt/fb; learn routes keep routing fields;
docs/doctor/TUI honesty; please-fix keeps cargo/rustc signal.

## Media, thinking, and tools surface
status: done

Path-attach media (`--image`/`--video`/`/image`/`/video`), thinking aliases
(`--thinking`/`/think` → Cursor model ids), and MCP/tool passthrough
(`abbey mcp`, `--approve-mcps`). No local vision weights; no Abbey-owned CoT UI;
tools remain cursor-agent's during a run.

## Image/video generation and reasoning
status: done

Agent-orchestrated `imagine` / `generate video` (cursor-agent or MCP tools write
files; Abbey has no local gen model) and `reason` (thinking model + structured
wrap). Video is best-effort and fails honestly without a video tool. `fm`
refuses generation.

## High-quality voice input/output
status: done

macOS `say` TTS with automatic Premium/Enhanced preference, on-device Apple
Speech STT (`abbey-stt`), and `voice ask` (listen → agent → speak). Cloud TTS/STT
SaaS and in-process neural voice weights stay Out of scope — download Apple
Premium voices in System Settings for super-high quality.

## MCP and ACP server surfaces
status: done

Config inventory for MCP (`abbey mcp status` over standard mcp.json paths) plus
cursor-agent management passthrough; ACP peer discovery/launch for gemini and
opencode. Abbey remains a client-side CLI — not an MCP host or ACP host. Tool
execution during runs stays inside cursor-agent (`--approve-mcps`).

## Auto code highlighting
status: done

syntect ANSI colour for markdown fences on captured `-p`/print/commit/hybrid-loop
output when stdout is a TTY; `abbey highlight` / `/highlight` for files and stdin;
`abbey diff` highlighted. Off via `NO_COLOR` or `ABBEY_HIGHLIGHT=0`. Not a full
markdown renderer or LSP semantic highlight.

## Multi-subagent and local distributed peers
status: done

`abbey subagents` catalog + `run --lanes/--peers/--jobs/--synthesize`. Abbey lanes
(max/gemma/aviva/abbey/abi/reviewer/security/planner) via cursor-agent; peers
(gemini/opencode/claude/codex) as same-host PATH CLIs. `parallel` remains the
default-lane alias. Multi-node mesh stays Proposed.

## Proposed / OOS claims surface
status: done

`abbey claims` (+ `roadmap`/`scope`) prints the Current / Proposed / Out of scope
gate from `src/claims.rs`; `refuse embeddings|lora|multinode|…` and
`memory embed` / `learn lora` exit 2 with substitutes. No claim laundering —
embeddings and multi-node stay Proposed; LoRA/weights/accelerator-runtime stay OOS.

## Platform targets + compute inventory
status: done

`abbey platform` / `compute`: linux/macos/windows primary-target matrix for portable
surfaces; `available_parallelism` thread budget driving subagent `--jobs`; GPU/NPU/TPU
host detect (report-only). WDBX lock via `fs2` on Unix and Windows. Voice/fm remain
macOS-only; Abbey still does not run GPU/NPU/TPU kernels.

## Vision / CoT / tool-runtime honesty
status: done

`abbey vision` · `abbey cot` · `abbey runtime` surface Current substitutes (path attach,
agent gen, reason transcript viewer, responsibility matrix) while refusing local
vision/video weights, Abbey-owned CoT engine/UI, and Abbey-as-tool-runtime (exit 2).
`abbey reason` saves a CoT transcript for `cot show`.

## OOS honesty pack (LoRA · weights · NPU/TPU · unrestricted OS · MCP/ACP host)
status: done

`abbey oos` index plus `abbey lora|weights|accel|shell|host` (and slash peers) print
Current substitutes and refuse with exit 2. Does **not** implement LoRA runners, local
weights, accelerator kernels, unrestricted shell, or Abbey-as-MCP/ACP-host — those stay
Out of scope. Cross-links: `learn lora`, `os refuse`, `platform refuse`, `mcp|acp refuse`.

## learn review/stats + OS allowlist Current surfaces
status: done

Hardened Current substitutes the OOS pack points at: `learn` status embeds train_candidate
curation counts; `review`/`stats` show provenance + ready (prov+conf≥0.9); top-level
`learn-review`/`learn-stats` aliases. `abbey os` / `allowlist` print the policy panel by
default; execute still requires `--confirm`; off-list denied. Unrestricted shell stays OOS.

## Cross-platform host support
status: done

PATHEXT-aware `which_bin`, Windows agent/install candidate paths, platform-aware argv
clamp (CreateProcess-safe), Windows OS allowlist of real executables, `abbey platform
paths` + this-host matrix column, `install.ps1`, softer `check.sh` cross-target smoke when
targets are already installed. Voice/fm remain macOS-only; accelerator runtimes stay OOS.

## Lexical similarity search over memory
status: done

Captured 2026-07-31 while auditing the last unchecked todos. The "semantic memory
search" residual had been filed as Proposed on the premise that Abbey has no
embedder — but `abi-ai` (an **unconditional** dependency, not `--features wdbx`)
already ships `text_embedding`: a deterministic signed feature hash over character
n-grams. The gap was never a missing embedder, only unwired plumbing.

**Current:** `abbey memory similar <query>` / `--id <anchor>` over a new
`src/memory/similarity.rs` — cosine over `abi-ai` n-gram vectors, computed at query
time on both backends (mirrors how `map::nearest_to` recomputes coordinates). Six
unit tests plus live verification: `memory search "chekpoint"` (typo) returns
nothing where `memory similar "chekpoint"` ranks the intended record first at 0.44.

**Deliberately NOT closed — learned/semantic embedding stays Proposed.** A feature
hash has no trained semantics: it matches surface form, not meaning, so the same
idea in different words still misses (observed live — an unrelated record scored
0.47 against the wdbx anchor purely on shared trigrams). `abbey claims refuse
embeddings` still exits 2, and its wording now names the real substitute instead of
the now-false "Abbey has no embedder". Vectors are never persisted, so abi-wdbx's
`put_vector` storage stays honestly unwired.

## Smart improve loop (ledger + gate, bounded auto)
status: done

`abbey improve` — parse `tasks/goals.md` + `todo.md`, fan out local diagnose lanes
(reviewer/gemma), Max implement with one `--confirm` for the run, hard stop on
`./check.sh` green (or max-rounds / max-minutes). Same-host only; never auto-marks
goals.md done; OS allowlist execute unchanged.

**Closed 2026-07-31:** live-verified against the built binary — `abbey improve status`
correctly parses the real ledger (18 goals / 86 todos) and picks focus; `abbey improve
plan` renders the diagnose + implement prompts with no apply — both against a green
`./check.sh`. Also closed a gap found while verifying: `check.sh`'s file-size guard had
an unreachable hard-fail branch (`elif n > 800` shadowed `elif n > 1000`), and
`src/tui/app.rs` had already crossed that 1000-line cap under the dead branch. Fixed the
guard ordering and decomposed `app.rs` (1025→896: refresh methods split into
`src/tui/refresh.rs`) so the corrected guard passes for real, not by omission.
**Not exercised:** `improve run --confirm` — the Max force-apply path spends real
cursor-agent credits and edits the tree autonomously; left for a human-initiated run.

**Follow-up defect fixed 2026-07-31 (stays `done` — rule 4, not new scope):** `pick_work`
nominated unchecked boxes under "Deferred by construction" as the next slice, so
`improve run --confirm` would have dispatched Max to build a capability
`abbey claims refuse` exits 2 for — wasted spend plus direct laundering pressure.
`ledger::is_deferred_section` now excludes those sections (matched on the heading, so a
real todo like "document why X is out of scope" still counts as work), with two
regression tests. Verified live: `improve status` went 5 open todos → 0 and focus
`stabilize`; `improve run` takes the "already production-ready (gate green · no open
slice)" early return with zero lanes dispatched.

**Maintenance 2026-07-31 (stays `done`):** `src/improve/mod.rs` 842 → 735 lines —
`RunReport` + `report_path`/`write_report`/`latest_report_path` extracted to
`src/improve/report.rs` (a self-contained concern: owns its fields, reaches
`AbbeyState` only for `state_dir`). Addresses the file-size guard's **soft WARN**,
not a failure — the file was never near the 1000-line hard cap. Verified beyond the
gate: `improve status` still prints its `last report:` line, which `latest_report_path`
feeds and which no test would have caught. `src/tui/app.rs` (896) deliberately left
alone — it was already decomposed once this session (1025 → 896) and cutting it again
to chase a soft warning would be line-shaving, not decomposition.

**Dead-code audit 2026-07-31 (stays `done`):** `src/improve/` now carries **zero**
`#[allow(dead_code)]`. Three genuinely-unused items deleted — `gate::check_script_path`
(zero callers; its comment claimed "used only in tests / helpers", which was simply
false, and it duplicated `resolve_check_cmd`), `Ledger::all_goals_closed` (zero callers;
`pick_work` derives the same thing directly), and `GateReport::output` (never read;
`excerpt` is the used path and the full text is consumed locally). `Ledger::goals_path`/
`todo_path` were wired up instead of deleted: `improve status` now names the exact ledger
files with a `(missing)` marker, which is what tells you *which* path was empty when a
ledger looks unexpectedly bare — verified live in both the present and missing cases.
Most of these allows were added earlier the same session under gate pressure; the audit
was resolving that, not new scope.

