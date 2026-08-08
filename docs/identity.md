# Abbey identity (distilled)
<!-- abbey-claims-sha256: 6451afc47d15af34424f5885e18a540bb2d317fba24d1f6323d6fcac4831d485 -->

Canonical product spec: [ABI `abbey-core-identity.mdx` at the integrated revision](https://github.com/donaldfilimon/abi/blob/32e372d7f522f5a6c9c0ef92c5b9612b52cfea05/docs/spec/abbey-core-identity.mdx).
Executable contracts: [ABI `identity.rs` at the integrated revision](https://github.com/donaldfilimon/abi/blob/32e372d7f522f5a6c9c0ef92c5b9612b52cfea05/crates/abi-ai/src/identity.rs).
Architecture: [architecture.md](architecture.md) · Production: [production.md](production.md).

**Status key:** **Current** = shipped in Abbey CLI · **Partial** = some shipped surface with
stated gaps · **Proposed** = approved direction, not verified runtime · **Blocked** = needs
an external prerequisite/proof · **Aspirational** = product norm, not verified runtime.

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
comes from cursor-agent, **on-device** from Apple's Foundation Models CLI under
`ABBEY_BACKEND=fm` (**Current**, macOS 26+), or from the sibling `abi` CLI under
`ABBEY_BACKEND=abi` (**Current** — `abi complete`: local persona-template by default;
bare `claude-*` / `live` opt into abi's Anthropic transport with abi credentials).
Under `fm` and `abi`, Max and Gemma share one local generation surface, so the role
difference is prompt-only — a real narrowing worth knowing. Cursor role/thinking
bindings left in state do **not** silently select abi `--live`.

---

## Worker roles (**Current** bindings)

| Role | Use | Binding |
| --- | --- | --- |
| **Max** | Code, tools, math, implementation | cursor-agent model alias (default `fable`) |
| **Gemma** | Visual / conversational | cursor-agent model alias (default `composer`) |

These are **role bindings**, not local Qwen/Gemma weights (**Proposed, not implemented** — see
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
| Local neural image / video models | **Proposed, unavailable** (`abbey vision refuse` exits 2) |
| CoT transcript viewer (`abbey cot`) | **Current** — display/save of structured reason output |
| Abbey-owned CoT engine / interactive CoT UI | **Out of scope** |
| Tool responsibility matrix (`abbey runtime`) | **Current** — who executes what |
| Provider-neutral Abbey-owned tool runtime / MCP-ACP host | **Proposed, unavailable** |
| `--thinking <level>` · `/think` · `abbey reason` · `/reason` | **Current** — Cursor `*-thinking-*` + structured reasoning wrap (not Abbey CoT UI) |
| Tools during a run | **Current via cursor-agent** (`--print` has tool access; `--approve-mcps`) |
| `abbey mcp` / `/mcp` provider-aware inventory + explicit management | **Current** (not an MCP host runtime) |
| `abbey acp` peer inventory / `acp run` | **Current** — launches gemini/opencode ACP stdio; not an ACP host |
| skills/plugins inventory | **Current** — skill provenance/divergence + provider-explicit plugin states |
| Tools / generation under `ABBEY_BACKEND=fm` or `abi` | **N/A** — refuse with exit 2 |
| `abbey voice` / `speak` / `listen` / `ask` | **Current (macOS)** — Premium/Enhanced `say` TTS + on-device Speech STT |
| Local neural speech model | **Proposed, unavailable**; current voice uses platform TTS/STT |
| Bundled cloud TTS/STT subscriptions | **Out of scope** |
| Auto code highlighting (`-p`/print fences · `abbey highlight`) | **Current** — syntect ANSI on TTY; `NO_COLOR` / `ABBEY_HIGHLIGHT=0` off |
| Full markdown renderer / LSP semantic highlight | **Out of scope** |
| Multi-subagent fan-out (`abbey subagents` / `parallel`) | **Current** — named lanes + optional `--synthesize` |
| Local distributed peers (`--peers gemini,claude,…`) | **Current** — PATH CLIs on this host |
| ABI authenticated local multi-process proof (`abbey mesh local-demo`) | **Current on Unix** — one host, 3–9 processes; non-Unix fails before spawn |
| Authenticated local three-VM shared-compute proof on one Mac | **Proposed** (not established by local-demo) |
| Production separate-host / geographic-HA / multi-GPU mesh | **Proposed after the VM proof**; the VM proof will not establish this |
| Host platform matrix + thread/GPU/NPU/TPU detect | **Current** — `abbey platform` (not accelerator runtime) |
| GPU/NPU/TPU compilation/training/inference inside Abbey | **Proposed, unavailable** |
| Tauri 2 + React/TypeScript desktop GUI | **Proposed**; ratatui TUI remains Current |
| Shared application core + authenticated read-only `abbeyd` | **Current on Unix** — `abbey daemon status\|claims` queries typed Status/Claims only; not model/tool execution, durable jobs, or an MCP host |

---

## Memory & self-learn

| Layer | Status |
| --- | --- |
| SQLite STM/LTM/activity/train_candidate | **Current** (`src/memory/sqlite.rs`, default) |
| `abbey learn` correction/preference/routes/digest/export | **Current** |
| `abbey learn` review/stats (+ `learn-review`/`learn-stats`) | **Current** — curation only; LoRA is Proposed, unavailable |
| OS allowlist (`abbey os` / `abbey allowlist`) | **Current** — dry-run; execute `--confirm` only |
| Personal-unrestricted separate edition | **Proposed, unavailable**; does not weaken shipped-edition controls |
| WDBX DurableStore in-process | **Current behind `--features wdbx`** (off by default, `src/memory/wdbx.rs`; fs4 `flock`/`LockFileEx` guard) |
| 3-D memory map (topic × recency × consolidation) | **Current** — deterministic axes, both backends; TUI Memory tab preview |
| Lexical similarity search (`abbey memory similar`) | **Current** — `abi-ai` n-gram feature hash + cosine, computed per query, both backends |
| Project/source/reference/tag + inclusive timestamp filters | **Current** — identical backend-neutral semantics |
| Learned/semantic memory embedding | **Current, opt-in** — Apple NaturalLanguage or OpenAI-compatible provider; isolated persisted spaces on SQLite/WDBX; no fallback |
| Live paid OpenAI-compatible request | **Unverified in this release evidence** — implementation/mock contract are Current; no credential was supplied |
| Fine-tuning / LoRA | **Proposed, unavailable** (`abbey lora` · `abbey learn lora` refuse) |
| Production-capable local model weights | **Proposed, unavailable** (`abbey weights`) |
| Unrestricted shell / allowlist bypass in shipped Abbey | **Out of scope** (`abbey shell` · `abbey os refuse`) |
| OOS honesty pack (`abbey oos`) | **Current** — status + refuse only |
| Claims gate CLI | **Current** — `abbey claims` / `/claims` |
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
