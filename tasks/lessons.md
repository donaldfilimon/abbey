# Lessons
<!-- abbey-claims-sha256: 1cff4b9922dd6eb1a09eced94a8452478a0e071cb0835fdf788dd9cbec335282 -->

- Abbey `/init` should stay offline-first (filesystem scan); `--agent` is optional refinement only.
- Keep parity claims honest: cursor-agent is the default backend; `/cost` N/A; Max/Gemma = bindings.
- Goals live in `tasks/goals.md`; granular work in `tasks/todo.md`.
- **Max/Gemma worker roles are cursor-agent model bindings**, not local Qwen or Gemma checkpoints — naming a role after a model family does not imply in-process weights or LoRA; document and doctor output must say which hosted id each role maps to.
- Under `ABBEY_BACKEND=abi`, never let a cursor `claude-*-thinking-*` (or Max/Gemma leftover) silently select `--live` — normalize at the argv choke point.
- Route **confidence / alternate / fallback** are audit fields only — do not add a second agent execution path when confidence is low; keep the one canonical `run_agent` path.
- please-fix must summarize captures before argv handoff; bare `running `/`using `/`reading ` are not always noise (cargo output is useful).
- `learn review`/`stats` are provenance curation, not a trainer. LoRA is now an approved
  Proposed direction, but approval does not turn curation into weight updates; keep refusal
  exit 2 until training, evaluation, rollback, and artifact provenance are real.
- Voice “super high quality” on macOS means **downloaded Premium/Enhanced `say` voices** + on-device Speech STT — not a cloud subscription and not Abbey-owned neural weights.
- **A process-cached value must not gate per-call behaviour that a runtime switch can change.** `AgentBackend::from_env()` is resolved once; the TUI's Ctrl-B switch mutates `AgentConfig::backend`. `read_chat` gated its `CURSOR_AGENT_CHAT_ID` guard on the cached value, so switching to `abi` mid-session silently resumed a cursor chat id and killed continuity — the very bug the guard had just been added to fix. Thread the live backend (`read_chat_for(cfg.backend)`).
- **Fixing a perf problem can reintroduce a correctness one.** The `OnceLock` on `from_env()` was added to stop per-frame `config.toml` reads in the TUI draw path; it was correct for that, and wrong for the guard that happened to share the call. Check every caller when you memoize something.
- **A commented example must be a clean `key = "value"`, never prose that starts like one.** The scaffold's `abi_bin` note began `# backend = "abi" and for …`, so uncommenting the real `backend` line matched two lines and parsed prose as the value. Pinned by a test that walks `default_toml_text()`.
- **Silent fallback hides a config typo.** `backend = "abbi"` used to mean "cursor-agent, no explanation". Unknown values now warn — a user who names an executor should never be quietly given a different one.
- **A caveat repeated across docs can be wrong in all of them.** README/architecture/production described the WDBX lock as `fs2` (and README as Unix-only) long after the code moved to `fs4` with `LockFileEx` on Windows. Grep the crate name in `Cargo.toml`, not the other docs.
- **A scope decision changes the roadmap status, not the evidence status.** Moving GUI,
  Tauri 2 + React/TypeScript desktop, owned runtime/tool hosting, local weights, LoRA, accelerator execution, neural media,
  a separate personal-unrestricted edition, and multi-VM compute from Out of scope to
  Proposed must not remove fail-closed refusal paths or promote them to Current.
- **Runner registration and CI execution are different facts.** GitHub-hosted
  `startup_failure`, a registered macOS ARM64 self-hosted runner, an unprovisioned
  variable-gated Linux ARM64 job, and open Windows runtime proof must be reported
  separately. A merged ABI dependency revision removes one blocker; it does not prove
  another platform ran.
- **Owned runtime work should reuse ABI boundaries, not fork them.** Establish
  provider-neutral executor/tool/compute contracts first, then layer GUI, local models,
  accelerators, training, neural media, and multi-VM scheduling over those contracts.
  Vendor runtime reimplementation remains Out of scope.
- **Credential policy must name the intentional reader.** Executor credentials belong to
  their backend, while the opt-in OpenAI-compatible embedding provider intentionally reads
  an environment key. The security invariant is no persistence, logs, config storage, or
  argv exposure—not the impossible claim that Abbey never reads a credential.
- **A second Cargo binary makes implicit package commands ambiguous.** Once `abbeyd`
  joined `abbey`, the claims validator's bare `cargo run` stopped selecting a target.
  Set `default-run = "abbey"`, and make automation/CI name `--bin abbey` explicitly;
  human convenience and reproducible automation are separate concerns.
- **A shared contract is not an owned runtime.** The Current app-core/daemon slice
  advertises only Status and Claims. Defining approval or run identifiers does not
  grant execution capability, and a Unix socket does not prove durable jobs, model
  workers, MCP hosting, memory ownership, or Windows named-pipe support.
