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
