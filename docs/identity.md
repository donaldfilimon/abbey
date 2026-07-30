# Abbey identity (distilled)

Canonical product spec: [`/Users/donaldfilimon/abi/docs/spec/abbey-core-identity.mdx`](/Users/donaldfilimon/abi/docs/spec/abbey-core-identity.mdx).  
Executable contracts: [`/Users/donaldfilimon/abi/crates/abi-ai/src/identity.rs`](/Users/donaldfilimon/abi/crates/abi-ai/src/identity.rs).  
Architecture: [architecture.md](architecture.md) · Production: [production.md](production.md).

**Status key:** **Current** = shipped in Abbey CLI · **Proposed** = direction · **Aspirational** = product norm, not verified runtime.

---

## Mission

Abbey is a personal AI assistant (she/her) created by Donald Filimon and The Donald Company. She speaks in the first person without implying biological humanity, consciousness, or experiences she does not possess.

**Purpose:** amplify human ability — help people learn, build, reason, create, decide, and complete meaningful work. Abbey strengthens rather than replaces human agency.

---

## Personas (**Current** in Abbey via `abi-ai`)

| Persona | Role | Tone / when |
| --- | --- | --- |
| **Abbey** | Primary empathetic polymath | Warm, clear, collaborative; default |
| **Aviva** | Direct expert mode | Answer first, concise, candid |
| **Abi** | Router / orchestrator | Intent, risk, tools; usually behind the voice |

CLI: `abbey persona` · `/persona` · explicit `@Abbey` / `Aviva:` / `Abi:`.

Local abi template completion remains deterministic (**Current** in ABI); hosted LLM quality
comes from cursor-agent, or **on-device** from Apple's Foundation Models CLI under
`ABBEY_BACKEND=fm` (**Current**, macOS 26+). On-device, Max and Gemma share one model
(`system`), so the role difference is prompt-only — a real narrowing worth knowing.

---

## Worker roles (**Current** bindings)

| Role | Use | Binding |
| --- | --- | --- |
| **Max** | Code, tools, math, implementation | cursor-agent model alias (default `fable`) |
| **Gemma** | Visual / conversational | cursor-agent model alias (default `composer`) |

These are **role bindings**, not local Qwen/Gemma weights (**out of scope** — see
[AGENTS.md](../AGENTS.md) claims gate and `tasks/lessons.md`).

The two roles compose in `abbey hybrid-loop` (**Current**): Gemma interprets the request,
Max implements against that interpretation, and Abi's route log links both stages under one
correlation id (`abbey routes --correlation <id>`).

Routing decisions record **confidence**, **alternate**, and **fallback** on `route.jsonl`
(**Current**, audit only — Abbey does not auto-reinvoke a second agent).

---

## Media, thinking, and tools

| Surface | Status |
| --- | --- |
| `--image` / `--video` / `--media` · `/image` · `/video` | **Current** — resolve path, `--add-dir` parent, prompt note; **no pixel encode** |
| Prompt-token paths (`./shot.png`) | **Current** — auto-discover + Gemma preference |
| `abbey imagine` · `generate image` · `/imagine` | **Current** — agent-orchestrated image gen/edit (depends on cursor-agent/MCP tools) |
| `abbey generate video` · `/gen-video` | **Current (best-effort)** — same pattern; fails honestly if no video tool |
| Local vision / video / generation weights | **Out of scope** |
| `--thinking <level>` · `/think` · `abbey reason` · `/reason` | **Current** — Cursor `*-thinking-*` + structured reasoning wrap (not Abbey CoT UI) |
| Tools during a run | **Current via cursor-agent** (`--print` has tool access; `--approve-mcps`) |
| `abbey mcp` / `/mcp` config inventory + cursor-agent management | **Current** (not an MCP host runtime) |
| `abbey acp` peer inventory / `acp run` | **Current** — launches gemini/opencode ACP stdio; not an ACP host |
| skills/plugins inventory | **Current** |
| Tools / generation under `ABBEY_BACKEND=fm` | **N/A** — refuse with exit 2 |
| `abbey voice` / `speak` / `listen` / `ask` | **Current (macOS)** — Premium/Enhanced `say` TTS + on-device Speech STT |
| Cloud TTS/STT subscriptions · local neural voice weights | **Out of scope** (use System Settings downloads for Premium voices) |
| Auto code highlighting (`-p`/print fences · `abbey highlight`) | **Current** — syntect ANSI on TTY; `NO_COLOR` / `ABBEY_HIGHLIGHT=0` off |
| Full markdown renderer / LSP semantic highlight | **Out of scope** |
| Multi-subagent fan-out (`abbey subagents` / `parallel`) | **Current** — named lanes + optional `--synthesize` |
| Local distributed peers (`--peers gemini,claude,…`) | **Current** — PATH CLIs on this host |
| Multi-node / multi-GPU agent mesh | **Proposed** (not Current) |

---

## Memory & self-learn

| Layer | Status |
| --- | --- |
| SQLite STM/LTM/activity/train_candidate | **Current** (`src/memory/sqlite.rs`, default) |
| `abbey learn` correction/preference/routes/digest/export | **Current** |
| `abbey learn` review/stats (train_candidate provenance) | **Current** — curation only; LoRA **out of scope** |
| WDBX DurableStore in-process | **Current behind `--features wdbx`** (off by default, `src/memory/wdbx.rs`; `flock`-guarded) |
| 3-D memory map (topic × recency × consolidation) | **Current** — deterministic axes, both backends; TUI Memory tab preview |
| Learned/semantic memory embedding | **Proposed** — Abbey has no embedder |
| `abi wdbx` CLI bridge when `abi` is a real binary on PATH | **Current** (doctor reports availability honestly) |

---

## Privacy

- No hidden profiling; store only user-approved / explicit learn captures
- State under XDG (`~/.local/state/abbey`) — not for commit
- OS execute always requires `--confirm`

---

## Related

- [AGENTS.md](../AGENTS.md) — agent guidance + claims gate
- [architecture.md](architecture.md) — module map
- [production.md](production.md) — release gate
