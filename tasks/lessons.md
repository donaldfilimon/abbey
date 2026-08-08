# Lessons

- Abbey `/init` should stay offline-first (filesystem scan); `--agent` is optional refinement only.
- Keep parity claims honest: cursor-agent is the default backend; `/cost` N/A; Max/Gemma = bindings.
- Goals live in `tasks/goals.md`; granular work in `tasks/todo.md`.
- **Max/Gemma worker roles are cursor-agent model bindings**, not local Qwen or Gemma checkpoints — naming a role after a model family does not imply in-process weights or LoRA; document and doctor output must say which hosted id each role maps to.
- Under `ABBEY_BACKEND=abi`, never let a cursor `claude-*-thinking-*` (or Max/Gemma leftover) silently select `--live` — normalize at the argv choke point.
- Route **confidence / alternate / fallback** are audit fields only — do not add a second agent execution path when confidence is low; keep the one canonical `run_agent` path.
- please-fix must summarize captures before argv handoff; bare `running `/`using `/`reading ` are not always noise (cargo output is useful).
- `learn review`/`stats` are provenance curation, not a trainer — LoRA stays Out of scope.
- Voice “super high quality” on macOS means **downloaded Premium/Enhanced `say` voices** + on-device Speech STT — not a cloud subscription and not Abbey-owned neural weights.
- **A process-cached value must not gate per-call behaviour that a runtime switch can change.** `AgentBackend::from_env()` is resolved once; the TUI's Ctrl-B switch mutates `AgentConfig::backend`. `read_chat` gated its `CURSOR_AGENT_CHAT_ID` guard on the cached value, so switching to `abi` mid-session silently resumed a cursor chat id and killed continuity — the very bug the guard had just been added to fix. Thread the live backend (`read_chat_for(cfg.backend)`).
- **Fixing a perf problem can reintroduce a correctness one.** The `OnceLock` on `from_env()` was added to stop per-frame `config.toml` reads in the TUI draw path; it was correct for that, and wrong for the guard that happened to share the call. Check every caller when you memoize something.
- **A caveat repeated across docs can be wrong in all of them.** README/architecture/production described the WDBX lock as `fs2` (and README as Unix-only) long after the code moved to `fs4` with `LockFileEx` on Windows. Grep the crate name in `Cargo.toml`, not the other docs.
