# Design: "Train Abbey to train itself" — the Abbey self-training workstream

> **This is a PLAN (a design), not evidence.** It changes no capability claim and
> grants no capability: every slice, verb, table, and acceptance test below is
> *proposed work*, not a shipped fact, and no weight update exists in this repo.
> The authoritative sources for what Abbey can actually do are the capability
> ledger (`src/claims/registry.rs`, re-exported by `src/claims.rs`) and the workflow
> ledger (`tasks/goals.md` / `tasks/todo.md`); where this design and those ledgers
> disagree, the ledgers win.
>
> **How to read it / status.** Every factual statement carries a `file:line`
> citation against the working tree at HEAD `ea2ae07` (dirty); sibling-repo paths
> are prefixed `abi/` (`../abi`), `wdbx/` (`../wdbx`), or `lylx/` (a *different*
> repository — see §3.4). Line numbers drift, so re-check any citation you rely on.
> Authored 2026-10-22 by a planning subagent; installed into `docs/` on 2026-10-22
> with citation corrections and this front-matter note only — no source, claim,
> ledger, or generated file was changed.
>
> **Re-check with:** `./check.sh` (the merge bar) · `python3 tools/check_claims_sync.py`
> (claims ledger + generated regions) · `python3 tools/check_instructions.py` (the
> AGENTS.md/CLAUDE.md split) · `python3 tools/check_p3_readonly.py` (Program 3
> read-only boundary) · `tools/ci/verify-pinned-siblings.sh` (the pinned-sibling
> check §3.2 and slice 3 depend on).

**Status:** design only. No code is proposed to be written into the repo by this document.
**Date:** 2026-10-22.
**Repo:** `/Users/donald/sources/repos/abbey`, branch `main`, HEAD `ea2ae07`, working tree dirty (pre-existing in-flight work; this document changed no source, claim, or ledger file there).
**Host:** Apple M5 MacBook Pro, 24 GB RAM, macOS ARM64 (`uname -m` → `arm64`; `sysctl hw.memsize` → 25769803776).

**Reading rule.** Every factual claim below carries a `file:line` citation. Citations without a repo prefix are in this repo; `abi/…` is `/Users/donald/sources/repos/abi`; `wdbx/…` is `/Users/donald/sources/repos/wdbx`; `lylx/…` is `/Users/donald/sources/repos/donald/lylx` (a **different** repository — see §3.4). Where I could not verify a fact, it is marked **unverified**.

**The honesty contract this design is written against** (`AGENTS.md` *Invariants*, `src/claims/registry.rs`, `tools/check_claims_sync.py:768-805`):

- A claim is not evidence. The registry records what the code does.
- `unsafe_code` is denied at the manifest level (`Cargo.toml:44-45`); every `unsafe` needs an explicit `#[allow(unsafe_code)]` + SAFETY comment.
- The shipped edition keeps the `os_control` allowlist + `--confirm` in both editions.
- No fabricated evidence, no fabricated weights, no embedded credentials.
- Claims regions are generated: edit `src/claims/registry.rs`, then `python3 tools/check_claims_sync.py --write`.
- A new schema migration is a new numbered `.sql` file + constant + `MIGRATIONS` entry + `CURRENT_SCHEMA_VERSION` bump — never inline SQL (`AGENTS.md` *Generated code*).
- File size: `main.rs` hard-fails > 200 lines; other `src/**/*.rs` warn > 800 and hard-fail > 1000 (`check.sh`).
- A trainer living in a sibling must be pinned by revision for CI reproducibility (`AGENTS.md` *Three things that bite* #2; `tools/ci/verify-pinned-siblings.sh`).

---

## 1. Current-state map of the training pipeline

Each stage below is what exists **today**, what it produces, and exactly where it stops.

| # | Stage | Exists at | Produces | Stops because |
|---|---|---|---|---|
| 0 | Prompt source | `src/distill.rs:226-278` (`parse_args`) | In-memory `Vec<String>` of prompts; `--file` reads one prompt per line, `#` comments and blanks dropped (`src/distill.rs:249-256`); batch cap `MAX_BATCH = 256` (`src/distill.rs:22`) | Nothing durable. Prompts are never stored, never redacted, never split. |
| 1 | Teacher selection | `src/distill.rs:41` (`LOCAL_FIRST = [FmSystem, Ollama, FmPcc]`), `:44-53` (`Teacher::chain`), `:74-77` (`on_device()`), `:105-134` (`ask_chain`) | An ordered teacher chain; first non-empty answer wins unless `--all` (`src/distill.rs:118-121`) | Chain is fixed to 3 teachers. There is no teacher registry, no teacher qualification record, no teacher capability declaration beyond `on_device: bool`. |
| 2 | Ollama preflight | `src/distill.rs:143-161` | `ollama list` must already contain `gemma4:26b-mlx` (`src/models.rs:8`), else the turn fails | Deliberate: **Abbey never auto-pulls** (`src/agent/argv.rs`, `tests/ollama_no_pull.rs`). A teacher may not download. |
| 3 | Headless capture (bypass) | `src/capture.rs:33-42` (`capture_oneshot`: `cfg.no_resume = true` at `:35`, `run_capture(None, …)` at `:36`); called at `src/distill.rs:164` | `(ExitStatus, stdout, stderr)` (`src/capture.rs:9-14`) | **No conversation resume, no persona wrap, no `route.jsonl` row** (`src/distill.rs:11-13`). No transcript, no chat id, no turn identity, no cost/token accounting. |
| 4 | Teacher answer → memory row | `src/distill.rs:173-199` (`candidate_record`), stored at `src/distill.rs:361` | One `train_candidate` row: `payload` = JSON `DistillPair{prompt,response,teacher,on_device}` (`:81-87`, `:181`); `origin="teacher"` (`:182`); `source_type="distill"` (`:183`); `source_ref="<backend>:<model>"` e.g. `fm:system` (`:184`); `retention="train_candidate"` (`:185`); `tags=[train_candidate, distill, teacher:<label>, on-device\|off-device]` (`:186-195`); `confidence = 0.6` (`:196`, const `TEACHER_CONFIDENCE` at `:26`); `provenance="teacher <label> @ <ts>"` (`:197`); `summary` = prompt truncated to 120 chars (`:180`, const at `:24`) | SQLite `memory` table (`src/memory/sqlite.rs:31-48`) or WDBX. **No persona, no role, no dataset, no split, no license/consent field, no redaction state.** |
| 5 | Human review | `src/learn.rs:328-386` (`review_train`) | Per-row line `id \t conf \t prov \t ready \t <120-char payload preview>` (`:361-372`) | `ready` is computed, never settable: `prov_ok && confidence >= 0.9` (`src/learn.rs:357`, mirrored at `:243-246`). Teacher pairs are pinned at 0.6 → **every distilled pair is `ready = no` forever**. |
| 6 | Human invalidation | `src/cli/args.rs:81-83` (`MemoryCmd::Invalidate`), `src/memory/mod.rs:382` (trait), `src/memory/sqlite.rs:182` (impl) | `obsolete = true`; the row is **never deleted** (`src/memory/mod.rs:427`) | This is the only curation verb. There is **no promote-to-ready verb** anywhere in `src/learn.rs` (`:672-758` dispatch has `status|correction|train|preference|routes|digest|export|review|stats|improve|distill|sft|lora`). |
| 7 | Counts | `src/learn.rs:390-440` (`train_stats`), `:236-255` (`collect_train_curation`), `:444-464` (`curation_summary` for `doctor`/TUI) | `total / with_provenance / missing_provenance / high_confidence(≥0.9) / curation_ready / ready_pct` | `ready_pct` is therefore permanently 0 % for distilled pairs. Honest but useless as a gate. |
| 8 | SFT export | `src/distill.rs:203-218` (`sft_line`), `:384-393` (`export_sft`) | One JSONL line per live distill pair: `{"messages":[{"role":"user",…},{"role":"assistant",…}],"teacher":…,"on_device":…,"id":…}` | **No header, no dataset manifest, no digest, no split, no dedup, no redaction, no license/consent record, no held-out set.** Skips `source_type != "distill"` and `obsolete` rows only (`:204`). |
| 9 | Raw layer export | `src/learn.rs:724-734` (`export [layer]`) | One raw JSON `MemoryRecord` per row | Not a dataset. No manifest. |
| 10 | Training | **Nothing** | — | `src/learn.rs:751` → `crate::claims::refuse("lora")` → `src/claims.rs:583-586`: *"Fine-tuning / LoRA is Proposed but not implemented. train_candidate is curation only."* → **exit 2**. `src/learn.rs:308-325` prints *"LoRA / fine-tune is Proposed but unavailable — curation only"*. |
| 11 | Protocol surface (declared) | `src/app_core/v3.rs:40-41` (`ReadTraining`, `ManageTraining`); `:305-321` (`V3TrainingStart{dataset_id, base_model_id, base_revision, operation_id, seed}`); `:350-356` (six commands `TrainingDatasetStatus`/`StartTraining`/`TrainingStatus`/`CancelTraining`/`TrainingMetrics`/`PromoteAdapter`/`RollbackAdapter`); `:394-400` (grant mapping); `:697-700` (four events) | Type shapes only | `src/daemon/runtime_v3.rs:266` swallows all six into `_ => Err(capability_denied_failure())`. `src/daemon/runtime_v3.rs:7` says it outright: *"Training, worker, and polling grants remain absent."* `src/daemon/client.rs:644-646` carries event **name strings only**; `src/daemon/client/v3.rs` has **no training method**. |
| 12 | Durable store | `src/runtime/migrations.rs:6` (`CURRENT_SCHEMA_VERSION = 10`); `src/runtime/migration_7_model_operations.sql` (download/load/unload ledger); `src/runtime/store/model_operation.rs` (357 lines; `MAX_MODEL_OPERATIONS = 4_096` at `:8`) | Model lifecycle operation state machine | **No training table.** Next free migration number is **11**. |
| 13 | Desktop surface | `desktop/src/surfaces.ts:103-116` — the `training` surface is a stub with `unavailable.reason = "capability_not_implemented"` and the text *"A training view would be a picture of something that does not run."*; listed as advanced in `desktop/src/App.tsx:34` | An honest refusal | `desktop/src/ipc/generated.ts:921-922` already carries `"read_training"` / `"manage_training"` (generated from `src/app_core/v3.rs:40-41`); `desktop/src-tauri/src/v3_ipc.rs:271,294` mirrors them. Nothing consumes them. |
| 14 | Claims ledger | `src/claims/registry.rs:684-695` — `training-lora-pipeline`, `Status::Proposed`, `impl: []`, `tests: []`, `local: EvidenceState::Required(&["implement a bounded training pipeline and verify a weight update"])`, `next: "design dataset provenance, evaluation, rollback, and artifact controls before training"` | The machine-enforced boundary | Proposed claims must have empty impl/tests and a Required local evidence state (`tools/check_claims_sync.py:788-793`). |

### 1.1 The four honest facts this map establishes

1. **Stage 4 stores a *teacher identity* but not a *teacher qualification*.** `source_ref = "fm:system"` (`src/distill.rs:184`) and `teacher:fm:system` tag (`:189`) name a model; they say nothing about which weights, which tokenizer, which generation configuration, or whether that teacher was ever qualified to produce training data.
2. **Stage 5/6 make `ready` unreachable.** `ready` needs `confidence ≥ 0.9` (`src/learn.rs:245`); teacher rows are hard-pinned at `0.6` (`src/distill.rs:196`); no verb raises it. The curation surface therefore reports a permanent 0 %, which is *honest* but *not a gate*.
3. **Stage 8 produces a JSONL file with no identity.** Nothing in `sft_line`'s output (`src/distill.rs:208-216`) lets a consumer determine the dataset's provenance, digest, split, persona axis, or consent basis.
4. **Stage 10 does not exist and is correctly refused.** `src/learn.rs:751` → `src/claims.rs:583-586` → exit 2. That refusal must stay until §4's slices land.

---

## 2. Gap analysis: what "Abbey trains itself" can honestly mean

The constraints are: local-first teachers only, no weight update today, human curation, no cloud-only training, no data upload off-device, one macOS ARM64 host with 24 GB RAM, and a claim ledger that refuses to let "we ran a trainer" stand in for "the model improved."

Three concretely different definitions follow. They are **not** stages of one plan — they are different claims with different evidence.

### (a) Self-distillation dataset generation with automated quality gates

**Definition.** Abbey generates its own supervised corpus from local-first teachers, applies deterministic quality gates (dedup, redaction, length, refusal/empty detection, teacher-agreement, split assignment), and produces a *sealed, digest-bound dataset manifest*. No weights change. The deliverable is a **dataset**, not a model.

**What it may honestly claim.** "Abbey produced a bounded, provenance-bound, reproducible local training dataset from its own local-first teachers, with a published manifest, an 80/10/10 split, and a held-out evaluation set that no training split can see." It may claim *dataset quality metrics it actually measures* (dedup rate, redaction rate, teacher-agreement rate, split sizes) — as long as those metrics are computed from the dataset itself and not asserted.

**What it must NOT claim.** Any statement about model quality, accuracy, helpfulness, or "improvement". Any statement that the dataset is legally usable for training (see §7 Q4). Any statement that a teacher "verified" the answer. Any claim that this is "self-training" in the sense that the model improved — it is *self-data-generation*, not *self-improvement*.

**Honesty rank.** **Highest.** Nothing about it is unfalsifiable: the manifest digest is reproducible, the gates are deterministic, and a reviewer can re-run and diff.

**Feasibility on this host.** **Trivial.** Pure Rust over the existing SQLite store. Minutes. No new dependency. No new model.

**Effort.** Low. Slices 0 + 1 of §4. This is the only option that can honestly reach `Partial` in this workstream.

### (b) Self-evaluation-driven dataset refinement

**Definition.** A second pass over the corpus where Abbey *scores* candidate pairs against a rubric and refines, filters, or re-ranks them — e.g. an LLM judge over on-device `fm --model system` with a held-out rubric, or a deterministic rubric scorer, and then a second round of distillation conditioned on the refinement.

**What it may honestly claim.** "Abbey re-ranked / filtered its own corpus using rubric X, and the resulting dataset differs from the input dataset by digest D2 vs D1." If a judge model is used, it may claim *"filtered by an on-device judge model at revision R"* — and *must* say the judge is the same family as the teacher unless it is not (self-evaluation by an identical teacher is weaker evidence and must be labeled as such).

**What it must NOT claim.** That the refinement improved anything. A judge scoring its own teacher's output is **not** independent evidence; a rubric score is not a quality measurement; agreement between two local teachers from the same family is not consensus. Any claim of "better" requires the held-out eval in (a) to actually show it — which is a separate, expensive, and easily-faked step.

**Honesty rank.** **Medium.** The mechanism is real, but the natural reading ("it improved the data") is only as good as the eval, and the eval is the hard part.

**Feasibility on this host.** **Feasible but slow.** `fm --model system` on-device judging is bounded by Foundation Models' own context limits; a rubric pass over a few hundred pairs is minutes. Ollama `gemma4:26b-mlx` judging is feasible on 24 GB but slow and must never auto-pull (`src/distill.rs:154-161`).

**Effort.** Medium. Slices 0 + 1 plus a judge surface and a rubric contract.

### (c) A bounded adapter-training loop over a local base model

**Definition.** Abbey actually updates weights: LoRA/AdaLoRA on a locally-held base model, over a dataset from (a), with a deterministic seed, adapter-only writes, a held-out eval bar, an adapter registry, promotion/rollback, and signed artifact provenance. The base weights are never mutated.

**What it may honestly claim.** "On host H, with base model M at revision R and dataset D at digest X, a bounded adapter training run with seed S produced adapter A at digest Y, and A met the pre-registered bar (metric names + values) on the held-out split of D." It may claim *that a weight update happened* (that is literally what `training-lora-pipeline`'s Required evidence asks: *"implement a bounded training pipeline and verify a weight update"*).

**What it must NOT claim.** That the model is "better" in general (only that it moved a *pre-registered* metric on a *specific* held-out set, with the comparison to the base recorded). That the adapter generalizes. That training was cheap or fast. That the adapter is a "Quesar" or an "Abbey" — an adapter over a vendor base model is a **delta on that vendor's model**, and calling it a persona identity would be a false claim (see §3.4).

**Honesty rank.** **Lowest of the three**, but it is the *only* one that can satisfy the existing Required evidence string for `training-lora-pipeline`. It is also the only one that carries real risk of accidental mis-claiming (a "before/after" anecdote is not an eval).

**Feasibility on this host.** **Feasible at small scale, not at 12B.** 24 GB unified memory (measured above) is the hard constraint:
- A 4B QAT/4-bit base is comfortable; a 12B 4-bit base is marginal on a 24 GB host *with an optimizer state and activations in the same 24 GB*.
- The LYLX repo already pins a candidate for a *different* product: `Qwen/Qwen3.5-4B` at revision `851bf6e806efd8d0a36b00ddf55e13ccb7b8cd0a` (`lylx/docs/models/pretrained-candidate-research.md:10,47`; `lylx/docs/models/local-backend-audit.md:27`), and that repo's own audit records *"trainer masking/resume defects"* in that installed backend (`lylx/README.md:9`, `lylx/docs/models/local-backend-audit.md`). Those defects are a warning, not a base for a claim.
- Abbey's own default local model is `gemma4:26b-mlx` (`src/models.rs:8`), a 26B-class MLX build — **not** a viable LoRA training base on 24 GB.

**Effort.** **Very high.** Slices 0–4 of §4. It also requires a *dependency decision* (§7 Q5) and a *license decision* (§7 Q1) before any of it can be honest.

### 2.1 Ranking

| Definition | Honesty | Feasibility on macOS ARM64 / 24 GB | Effort | Can it satisfy `training-lora-pipeline`'s Required evidence? |
|---|---|---|---|---|
| **(a) self-distillation + automated quality gates** | **Highest** — deterministic, digest-reproducible, falsifiable | **Trivial** — pure Rust over the existing store; minutes | **Low** (~2 of 6 slices) | **No** (no weight update) |
| **(b) self-evaluation-driven refinement** | Medium — real mechanism, weak inference without a held-out eval | **Feasible** — on-device `fm` judging; minutes per few hundred pairs | Medium (2.5 slices) | **No** |
| **(c) bounded adapter-training loop** | Lowest of the three — highest mis-claiming risk | **Feasible small-scale only** (≤4B class); 12B marginal on 24 GB | **Very high** (4–5 slices + dependency/license decisions) | **Yes** — this is the only one |

### 2.2 The recommended posture

**Ship (a) first, honestly and completely; build (b) on top of it; attempt (c) only after §7 Q1/Q2/Q3/Q4/Q5 are answered.**

The critical honesty hazard is this: a "self-training" workstream that stops at (a) is easy to *over-claim* as "Abbey trains itself". It must not be. The correct phrasing for (a) is **"Abbey curates its own corpus"**. The claim ledger has exactly one row for this (`training-lora-pipeline`, Proposed) and it stays Proposed until (c) has a verified weight update. The recommended design therefore:

- keeps `training-lora-pipeline` **Proposed** through slices 0–2,
- adds **one new claim row** for (a) — e.g. `training-self-distilled-dataset` — so that the delivered capability has its own honest, non-over-reaching home,
- and treats (c) as the only path to flipping `training-lora-pipeline` to Partial.

---

## 3. The persona/spec dimension

### 3.1 What a persona is in this repo today

| Concept | Where | What it actually is |
|---|---|---|
| **Persona** Abbey / Aviva / Abi | `src/persona.rs:8-15` (`parse_persona`), `:19-29` (`select_persona`), `:32-40` (`wrap_prompt`); contracts come from the sibling `abi-ai` crate (`abi/crates/abi-ai/src/identity.rs:43-83` `profile_contract`) | A **frozen text contract**: `response_prefix` + `response_suffix` + a `description`. `abi/crates/abi-ai/src/identity.rs:62-83` gives Abbey `"Abbey: "`, Aviva `"Aviva direct expert: "`, Abi `"ABI orchestration review: "`. `src/persona.rs:43-58` renders them as doctor lines. |
| **Role** Max / Gemma | `src/roles.rs:5-31` (`WorkerRole`), `:150-206` (`route_decision`), `:210-215` (`default_model_for_role`), `:217-230` (`role_system_note`) | A **model-alias binding**. `src/roles.rs:208-209` says so verbatim: *"These are **cursor-agent model aliases**, not in-process Qwen/Gemma checkpoints."* `src/roles.rs:236`: *"Max/Gemma are roles, not bundled Abbey weights."* |
| **Routing prior** | `abi/crates/abi-ai/src/identity.rs:85-92` | Three fixed weights (Abbey 0.40, Aviva 0.30, Abi 0.30) summing to 1.0. Not learned in this repo. |
| **Persona in the sibling's training contract** | `abi/crates/abi-ai/src/training_contracts.rs:17-27` | `TrainingProfile::{Abbey, Aviva, Abi, Quesar}` — a **closed contract value**. `abi/crates/abi-ai/src/training.rs:60-62` `TrainingConfig.profile` is a persona profile label. |

**Consequence.** A "persona" in Abbey today is *a text prefix*. There is no persona-parameterised model. Therefore:

- **A per-persona corpus is meaningful** — the training target of a pair can differ per persona, and the prefix/suffix are the persona spec.
- **A per-persona adapter is a fiction unless a base model exists** — you cannot fine-tune "the Abbey persona" when there is no Abbey weights; you can only train an adapter over *some base model* and then *apply* the Abbey contract as a system prefix on top. That composition is a **prompt**, not a trained artifact, and must be described that way.

### 3.2 What a distilled pair must record

Today a pair records only `DistillPair{prompt, response, teacher, on_device}` (`src/distill.rs:81-87`). For (a)/(b)/(c) to be honest, a pair needs at least:

| Field | Why | Where it should live |
|---|---|---|
| `persona` | Which persona contract the target is written for (`abbey`/`aviva`/`abi`/`quesar`/`none`) | New field on the row (see §3.3) |
| `persona_contract_digest` | SHA-256 over the exact `ProfileContract` prefix/suffix/description used to produce the target — otherwise "the same prompt" is not reproducible across ABI revisions | New field |
| `role` | Which `WorkerRole` the target was produced under (`max`/`gemma`/`auto`) | New field |
| `role_binding` | The model alias that role resolved to at capture time (`src/roles.rs:210-215`) — a *binding*, not weights | New field |
| `spec_digest` | Digest over the persona+role+prompt contract bundle, so a dataset can be re-derived and diffed | New field |
| `teacher_base_digest` / `teacher_adapter_digest` / `teacher_tokenizer_digest` / `teacher_generation_configuration_digest` / `teacher_qualification_digest` | The **teacher identity** the sibling contract already defines (`abi/crates/abi-ai/src/training_contracts.rs:62-68` `TeacherIdentity`) | New field, or a sibling-side teacher registry row |
| `on_device` | Exists (`src/distill.rs:86`, `:177`) | Keep |
| `consent` / `license_basis` | Whether the teacher's terms permit training on its output — currently explicitly **not claimed** (`src/claims/registry.rs` `learn-distill-local-first-teachers` note) | New field; a closed enum, e.g. `unknown` / `denied` / `granted` |

**Important design choice.** The sibling already owns the exact vocabulary for all of this. `abi/crates/abi-ai/src/training_contracts.rs` defines:

- `ApprovedRecord { id, profile, family_id, input, target, sources, permission, approval, teacher }` (`:69`)
- `SnapshotPolicy { family_splits, permitted_specification_digests }` (`:70`)
- `SnapshotRecord { id, family_id, profile, digest, split }` (`:71-77`)
- `SnapshotManifest { version, records, specification_digests, root_digest }` (`:78`)
- `TeacherIdentity { base_digest, adapter_digest, tokenizer_digest, generation_configuration_digest, qualification_digest }` (`:62-68`)
- `Approval { approver_identity, decision_id, evidence: ApprovalKind }` with `ApprovalKind::Human` / `ApprovalKind::QualifiedEvaluator` (`:47-59`)
- `Split::{Train, Evaluation}` (`:29-33`)
- `TrainingProfile::{Abbey, Aviva, Abi, Quesar}` (`:17-27`)
- `canonical_record_bytes` + `sha256_hex` (`:203-241`) — canonical, sorted, versioned bytes
- `build_snapshot(records, policy)` (`:243-…`) — deterministic, refuses duplicate ids with differing bytes, refuses a teacher-authored record in the `Evaluation` split (`ContractError::TeacherInEvaluation`), and requires every source digest to be in `permitted_specification_digests`
- `validate_record` (`:135-201`) — requires sources, a permission decision id, an approval, and refuses **teacher self-approval** (`ContractError::TeacherSelfApproval`, `:192-195`)

**Recommendation:** Abbey's slice 1 must **reuse `abi_ai::training_contracts` verbatim** rather than invent a parallel schema. `abi-ai` is already a direct path dependency (`Cargo.toml:73`), already pinned in CI (`.github/workflows/rust.yml` `ABI_REVISION`), so the "proof that only holds against the sibling's working tree" trap does not apply — *provided the ABI revision that carries `training_contracts` is the pinned one*. **Verify before building**: `abi/crates/abi-ai/src/training_contracts.rs` exists in the local working tree; whether it exists at the **pinned** ABI revision is **unverified** and must be checked with `tools/ci/verify-pinned-siblings.sh` before slice 1 depends on it. If it is not at the pin, slice 1 must land in ABI first (a sibling PR), then Abbey may consume it after the pin moves.

**The `family_id` axis is the natural persona axis.** `SnapshotPolicy.family_splits` (`abi/crates/abi-ai/src/training_contracts.rs:70`) maps *family* → split, and `ApprovedRecord` carries `profile` *and* `family_id` (`:69`). So the clean mapping is:

- `profile` = `TrainingProfile` — the persona the target was written for.
- `family_id` = the *specification family* the record came from — e.g. `persona/abbey`, `persona/aviva`, `persona/abi`, `role/max`, `role/gemma`, `freeform`.
- `sources[].document_digest` = the digest of the persona/role contract document the pair was derived from. This is exactly what `permitted_specification_digests` gates (`:263-269`).

### 3.3 Schema changes needed

**(i) `MemoryRecord` — new optional fields** (`src/memory/mod.rs:31-53`)

Add, all `#[serde(default)]` so old rows and old stores keep loading:

```
persona: String            // "" | "abbey" | "aviva" | "abi" | "quesar"
role: String               // "" | "max" | "gemma" | "auto"
spec_digest: String        // sha256 over the persona+role+prompt contract bundle
teacher_identity: String   // JSON TeacherIdentity, "" when unknown
consent: String            // "unknown" | "denied" | "granted"
```

**(ii) `memory` SQLite table — `src/memory/sqlite.rs:29-52`**

The existing precedent for adding a column is `project` (`src/memory/sqlite.rs:53-66`): an idempotent `PRAGMA table_info(memory)` probe followed by `ALTER TABLE memory ADD COLUMN …`. Follow it exactly for the five new columns. The WDBX backend (`src/memory/wdbx.rs`) stores the whole record — verify its field mapping picks up the new fields (`src/memory/wdbx.rs` `row_to_rec` equivalent) or the two backends will disagree.

**(iii) `runtime.sqlite` — new migration `migration_11_training_operations.sql`**

`CURRENT_SCHEMA_VERSION` is `10` (`src/runtime/migrations.rs:6`); the next free number is **11**. Per `AGENTS.md`, that means a new file `src/runtime/migration_11_training_operations.sql`, a `const MIGRATION_11: &str = include_str!("migration_11_training_operations.sql");`, an entry `(11, MIGRATION_11)` in `MIGRATIONS` (`src/runtime/migrations.rs:34-52`), and a bump of `CURRENT_SCHEMA_VERSION` to `11`.

Proposed tables (design, not implemented):

- `training_datasets` — `dataset_id PK`, `root_digest`, `manifest_json`, `profile`, `family_id`, `record_count`, `train_count`, `evaluation_count`, `created_at_ms`, `retired_at_ms`. One row per sealed dataset.
- `training_operations` — mirror of `model_operations` (`src/runtime/migration_7_model_operations.sql`): `operation_id PK`, `dataset_id`, `base_model_id`, `base_revision`, `kind CHECK IN ('train','promote','rollback')`, `state CHECK IN ('queued','running','succeeded','failed','cancelled')`, `progress_basis_points`, `seed`, `adapter_digest`, `result_digest`, `created_at_ms`, `updated_at_ms`, `CHECK (progress_basis_points BETWEEN 0 AND 10000)`.
- `training_operation_events` — append-only transitions (the `tool_approval_events` pattern, `src/runtime/store/tool_approval.rs`).
- `training_adapters` — `adapter_id PK`, `dataset_id`, `base_model_id`, `base_revision`, `digest`, `state CHECK IN ('candidate','promoted','rolled_back')`, `promoted_at_ms`, `rolled_back_at_ms`.

Cap the operation ledger at 4,096 rows like the model one (`src/runtime/store/model_operation.rs:8`).

**(iv) `src/runtime/store/` — a new `training_operation.rs`**

Model it on `src/runtime/store/model_operation.rs` (357 lines) — globally single-use operation ids, monotonic state transitions, `recover_incomplete_on` on reopen (`src/runtime/store.rs:190-191`) turning `queued`/`running` into `failed` rather than replaying an ambiguous effect — and on `src/runtime/store/tool_execution.rs` (338 lines) for the *prepared-intent* pattern: write the intent, perform the effect, then write the terminal digest; on reopen an unfinished prepared intent becomes `interrupted` and never fabricates a result digest.

### 3.4 Quesar: what it actually is, and what "train Quesar" could honestly mean

**Finding.** Quesar is **not in this repository**, and **not in either sibling** (`../abi`, `../wdbx`). A repo-wide search of `/Users/donald/sources/repos` for `quesar` (case-insensitive) returns hits in exactly two places:

1. **Abbey itself, as a brand exclusion only.** `docs/brand.md:13`: *"Quesar keeps **Private AI operations** and does not inherit this tagline."* `docs/identity.md:88`: *"Not a Quesar tagline."* `docs/architecture.md:277`. `AGENTS.md` *What this is*. **No code, no data, no persona, no model.**
2. **The `donald` repository's `lylx/` workspace** — `/Users/donald/sources/repos/donald/lylx`. This is a *separate git repository* with its own `AGENTS.md`, its own `rust-toolchain.toml`, and its own gates. It is **not** a path dependency of Abbey (`Cargo.toml:72-86` lists only `../abi` and `../wdbx`), and Abbey has no dependency on it in any form.

**What Quesar is in LYLX.** From `lylx/README.md:3` and `lylx/AGENTS.md`:

- LYLX is *"The Donald Company's ground-up stable Rust reference platform for Quesar, Abbey, Aviva, ABI and WDBX"* and *"is not fully qualified LYLX, trained Quesar, trained personalities, or historical WDBX compatibility."*
- `lylx/crates/quesar/` is *"an independent bounded CPU f64 reference"* — *"It is **not the designated largest Quesar**, a trained language model, an accelerator backend, or evidence of production model qualification. No designated checkpoint, actual tokenizer or training rights were supplied to this crate. No designated-model weight updates were performed."* (`lylx/crates/quesar/IMPLEMENTATION.md:1-3`).
- It contains real, bounded mathematics: scalar reverse-mode autodiff `Tape` and `AdamW` (`lylx/crates/quesar/src/optim.rs`, 204 lines); a `Decoder::loss_and_gradients` reverse pass over the whole f64 decoder with central-difference oracles (`lylx/crates/quesar/src/model/backward.rs`, 552 lines); `masked_next_token_loss`, `WarmupCosine`, `resource_estimate`, `dense_training_flops`, `training_preflight` (`lylx/crates/quesar/src/training.rs`, 158 lines); seeded sampling (`lylx/crates/quesar/src/sampling.rs`, 339 lines).
- Its own `training_preflight` *"rejects missing checkpoint/tokenizer verification, rights and context evidence"* and *"requires a real owned 0700 Unix root"* for corpus privacy (`lylx/crates/quesar/IMPLEMENTATION.md`; `abi/crates/abi-sea/src/training_registry.rs:1-13` is the analogous ABI-side registry).
- `lylx/crates/lylx-data` has a `train` subcommand that *"selects a proposed configuration and stops at blocked preflight, without writing weights"* (`lylx/crates/lylx-data/src/main.rs:34-36` → `preflight::training_release`, `lylx/crates/lylx-data/src/preflight.rs:190`).
- **The persona files are the closest existing artifact to "the personas of Abbey".** `lylx/personalities/abbey.json` and `lylx/personalities/aviva.json`:
  ```json
  { "schema_version": 1, "id": "abbey", "revision": "reference-policy-v1",
    "default": true, "status": "specified_behavior_not_trained",
    "foundation": "shared_quesar_designated_artifact_unavailable",
    "style": ["warm","collaborative","candid","concrete","calibrated_uncertainty"],
    "facts_contract": "shared-facts-v1",
    "permissions_contract": "abi-independent-of-persona-v1",
    "authorization_effect": "none",
    "trained_adapter": null,
    "evaluation": "simulated authority parity only; no model behavior qualification" }
  ```
  `trained_adapter: null` and `status: "specified_behavior_not_trained"` are the honest current state.

**What "train Quesar" could honestly mean — three readings, only one of which is in scope for Abbey.**

| Reading | Meaning | Where it lives | Honest? |
|---|---|---|---|
| **Q1 (in scope).** "Train the *Quesar profile* of Abbey's corpus" | Use `TrainingProfile::Quesar` (`abi/crates/abi-ai/src/training_contracts.rs:17-27`) as the persona label on a subset of Abbey's corpus, so the corpus can later be used to train the Quesar-flavored model in LYLX. | Abbey's `train_candidate` rows + the sibling's `ApprovedRecord.profile` | **Yes**, as *corpus preparation*. It may be claimed as *"a Quesar-labelled corpus subset exists and is digest-bound"*. It may **not** be claimed as *"Quesar was trained"*. |
| **Q2 (out of scope for Abbey).** Actually train the Quesar model | Run a training run against the LYLX `quesar` crate / the pinned Qwen candidate | `donald/lylx` — a different repo, different gates, different ownership. Abbey cannot reach it without a cross-repo contract. | Possible, but **not an Abbey workstream**. Doing it in Abbey would (a) break the "keep new implementation within this project; do not depend on or modify older parent ABI/WDBX/Abbey systems" rule (`lylx/AGENTS.md` *Implementation boundaries*) in the other direction, and (b) create a CI-unpinnable dependency. |
| **Q3 (false).** "Train the Abbey persona so it becomes Quesar" | — | — | **False.** There are no Abbey weights to train. Abbey's personas are text contracts (`src/persona.rs:32-40`). An adapter over a vendor base model plus an Abbey prefix is a *composition*, and calling it "Quesar" would fabricate an identity. |

**Recommendation.** In this workstream, "train Quesar" means **Q1 only**: the corpus carries `TrainingProfile::Quesar` as one of its persona labels, using the sibling's own enum, so the corpus is *consumable* by the LYLX pipeline later — and the claim made is about the **corpus**, never about a Quesar model. Abbey's registry row for this must say *"a Quesar-labelled corpus subset exists"* and must **not** create any claim that Quesar is trained. If the owner wants actual Quesar training, that is a separate workstream in the `donald`/`lylx` repository with its own gates, and it must not be reported as an Abbey capability.

**One cross-repo consistency note.** LYLX's `personalities/abbey.json` uses `"style"` and `"facts_contract"`; Abbey's persona contract uses `response_prefix`/`response_suffix`/`description` (`abi/crates/abi-ai/src/identity.rs:43-83`). The two descriptions of "the Abbey persona" are **not** the same document, and neither is a superset of the other. Any persona `spec_digest` (§3.2) must digest **one named source document**, and the source must be recorded, or "the same persona" will silently mean two different things across repositories.

---

## 4. Target architecture — six independently shippable, independently gated slices

Each slice is independently shippable: it can be merged, gated, and claimed on its own, and the next slice is not required for it to be honest.

**Slice-0 → (a) partially · Slice-0+1 → (a) complete · Slice-2 → protocol surface · Slice-3 → (c) begins · Slice-4 → (c) complete · Slice-5 → presentation.**

---

### Slice 0 — The honesty / UX fix (no weights)

**Purpose.** Make the existing curation surface truthful and usable: a human can actually mark a pair as reviewed/ready, confidence semantics stop conflating *machine confidence* with *human review*, the SFT export gains a provenance header, and a dataset manifest is emitted.

**Files touched.**

| File | Change |
|---|---|
| `src/learn.rs` (796 lines, warn >800 — **split first**) | New `promote` / `review-approve` verb; new confidence semantics; `sft` header. **Note: `src/learn.rs` is already 796 lines against an 800-line warn threshold — split into `src/learn/` submodules before adding anything.** |
| `src/learn/args.rs` (49 lines) | Argument validation for the new verbs. |
| `src/distill.rs` (686 lines) | `DistillPair` gains fields; `sft_line` gains a manifest line. |
| `src/memory/mod.rs` (31-53) | New optional record fields (§3.3 (i)). |
| `src/memory/sqlite.rs` (29-66) | Idempotent `ALTER TABLE` for the new columns, following the `project` precedent. |
| `src/memory/wdbx.rs` | Ensure the WDBX backend round-trips the new fields. |
| `src/claims/registry.rs` | Edit the registry (never the generated regions). |
| `src/claims.rs:583-586` | Keep the `lora` refusal text, but it should now point at the new honest path. |

**New verbs (exact names — these become the acceptance-test names too).**

- `abbey learn promote <id> [--note <text>]` — mark a `train_candidate` row **human-reviewed**: sets `confidence` to `0.95` (matching the user-correction value at `src/learn.rs:63`), appends `reviewed` to `tags`, and rewrites `provenance` to `"human review @ <ts> (by <note>)"` **without deleting the original provenance** (append, never replace — provenance is evidence).
- `abbey learn demote <id>` — the exact inverse, restoring the prior confidence.
- `abbey learn sft [--manifest <path>]` — writes a **header line** before the JSONL: `{"abbey_dataset_manifest":{…}}` carrying the dataset id, root digest, per-split counts, per-profile counts, the teacher set, and the schema version. When stdout is a pipe the header is the first line; it is *not* a trainable example and consumers must skip it (`sft_line` continues to emit only real pairs at `src/distill.rs:208-216`).

**Honest confidence semantics (this is the core of the slice).**

Today one `f32` conflates three different things (`src/memory/mod.rs:42`). Split them:

- `confidence` keeps its meaning: **machine confidence in the record** (teacher output = `0.6`, `src/distill.rs:196`; user correction = `0.95`, `src/learn.rs:63`).
- **Human review** becomes an explicit, separate fact — a new tag `human_reviewed` plus a `reviewed_at` timestamp — and `ready` becomes `provenance present AND human_reviewed AND not obsolete`, **not** a confidence threshold.
- `src/learn.rs:243-246` (`collect_train_curation`) and `src/learn.rs:357` (`review_train`) both change to the new predicate. `src/learn.rs:423` (`curation_ready`) and `:403-407` (`ready_pct`) keep their labels but change their definition, and the label text changes from `conf>=0.9` to `reviewed`.

This is strictly more honest: the old `ready` predicate implied *"a 0.9 machine-confidence record is ready"*, which is false — a teacher never emits 0.9, and a hand-written row can carry 0.95 without ever being reviewed (`src/learn.rs:63`).

**Manifest (minimal form, slice 0).**

```
{
  "schema": "abbey.dataset.v1",
  "dataset_id": "<sha256 over the canonical record digests>",
  "created_at": "<RFC3339>",
  "records": {"train": N, "evaluation": M},
  "profiles": {"abbey": a, "aviva": b, "abi": c, "quesar": d},
  "teachers": ["fm:system", "ollama:gemma4:26b-mlx", "fm:pcc"],
  "root_digest": "<sha256>"
}
```
The digest must be computed over **canonical, sorted, versioned bytes** — reuse `abi_ai::training_contracts::canonical_record_bytes` / `sha256_hex` (`abi/crates/abi-ai/src/training_contracts.rs:203-241`) rather than inventing a second canonicalisation. **This depends on the pinned-ABI check in §3.2.**

**Claims affected.**

- `training-lora-pipeline` (`src/claims/registry.rs:684-695`): **stays `Proposed`**. Its Required evidence string — *"implement a bounded training pipeline and verify a weight update"* — is not met, and a Proposed claim may carry **no** impl/tests refs (`tools/check_claims_sync.py:788-793`).
- `learn-distill-local-first-teachers` (registry, `Status::Current`): **its note must be updated** — it currently says *"sft output is unreviewed unless a human curated it"* and *"there is NO promote-to-ready verb"*. After slice 0 that sentence becomes false and the note must change. A `Current` claim may carry no Required evidence (`tools/check_claims_sync.py:785`), so the note must describe the *new* honest boundary, not an outstanding proof.
- **New row recommended:** `training-self-distilled-dataset` — `Status::Partial` once slice 1 lands, `Status::Current` once it does. Impl/tests refs non-empty (required for `Partial` and `Current` — `tools/check_claims_sync.py:782-783`), local evidence `Required` for `Partial` (`:787`).
- `memory-sqlite-self-learn` (registry, `Current`): impl refs gain the new store surface.

**Acceptance tests (exact names).**

- `src/learn.rs::tests::promote_marks_a_pair_human_reviewed_and_keeps_the_original_provenance`
- `src/learn.rs::tests::promote_is_refused_for_a_non_candidate_record`
- `src/learn.rs::tests::demote_restores_the_previous_confidence`
- `src/learn.rs::tests::ready_predicate_is_review_based_not_confidence_based`
- `src/learn.rs::tests::review_lists_the_reviewed_flag`
- `src/distill.rs::tests::sft_export_writes_a_manifest_header_before_the_pairs`
- `src/distill.rs::tests::manifest_root_digest_is_stable_across_reordering`
- `src/distill.rs::tests::manifest_counts_exclude_obsolete_records`
- `tests/cli_surface.rs::learn_promote_marks_a_pair_ready`
- `tests/cli_surface.rs::learn_lora_still_refuses_with_exit_two`
- `src/memory/sqlite_tests.rs::new_record_columns_survive_a_reopen`
- `src/memory/sqlite_tests.rs::an_old_memory_database_gains_the_new_columns_idempotently`
- `src/memory/wdbx.rs` (feature cohort): `wdbx_round_trips_the_new_record_fields`

**Gate command.** `./check.sh` (the whole gate). The new WDBX test also runs in the `wdbx` cohort (`check.sh:48-52`).

**Size estimate.** `src/learn.rs` 796 → must be split to `src/learn/` (e.g. `review.rs`, `promote.rs`, `stats.rs`, `improve.rs`) before adding ~200 lines. `src/distill.rs` 686 → ~750 after the manifest. `src/memory/sqlite.rs` and `src/memory/wdbx.rs` each +~40. New `src/learn/manifest.rs` ~180. Net new ≈ **700–900 lines** across ~8 files, 2 of which are new.

**Dependencies.** None external. Slice 0 depends on the pinned-ABI check in §3.2 only for the canonical digest helper; if that fails, use a local canonicalisation identical in shape (`sha2` is already a dependency, `Cargo.toml:60`).

**False claims to avoid.**
- ❌ "Abbey now trains itself." Slice 0 touches no weights.
- ❌ "Pairs are now 90 % ready." The predicate changed; the *number of human reviews* did not.
- ❌ "The dataset is licensed for training." Slice 0 records a `consent` **enum value**, not a legal determination.
- ❌ "The manifest proves the data is good." A manifest proves *identity and composition*, not quality.

---

### Slice 1 — The durable dataset + evaluation harness

**Purpose.** Turn `train_candidate` rows into a **sealed, reproducible, split-assigned dataset** with a durable registry, and add a **held-out evaluation fixture set with a pre-registered pass/fail bar**.

**Files touched.**

| File | Change |
|---|---|
| `src/runtime/migration_11_training_operations.sql` | **new** — `training_datasets`, `training_operations`, `training_operation_events`, `training_adapters`. |
| `src/runtime/migrations.rs` | `const MIGRATION_11`, `MIGRATIONS` entry, `CURRENT_SCHEMA_VERSION` 10 → 11. |
| `src/runtime/store/training_operation.rs` | **new** — durable dataset + operation ledger. |
| `src/runtime/store.rs` | Wire the new store module; add `recovered_training_operations`. |
| `src/learn/dataset.rs` | **new** — build/seal a dataset from `train_candidate` rows: dedup, redaction, split, manifest. |
| `src/learn/eval.rs` | **new** — held-out evaluation fixtures + the pre-registered bar. |
| `src/distill.rs` | `DistillPair` gains the §3.2 fields; teacher identity recorded. |
| `src/app_core/v3.rs` | *(optional, forward-compatible)* the `V3TrainingStart.dataset_id` must resolve to a sealed dataset. |
| `src/claims/registry.rs` | New/updated rows. |

**Dataset construction pipeline (deterministic, order-stable).**

1. **Select.** `train_candidate` rows with `retention = "train_candidate"`, `source_type = "distill"`, `obsolete = false` (matching `src/distill.rs:204`), and `human_reviewed` (slice 0) — **or**, if the owner wants an un-reviewed path, that must be a distinct, explicitly-named dataset kind, never silently mixed.
2. **Redact.** Strip control characters, cap lengths, and redact anything matching an absolute-path or secret-like pattern. The rule set must be **versioned** and its version recorded in the manifest, so a dataset digest is reproducible only against a named redaction revision.
3. **Dedup.** Canonicalise whitespace + case for the *prompt*; exact-duplicate prompts with differing responses are a **conflict**, not a duplicate — refuse or quarantine, never silently keep one.
4. **Split.** 80/10/10 train/validation/evaluation, assigned **by a stable hash of the record id**, not by position — so adding one record does not reshuffle the split. The sibling already refuses a teacher-authored record in the evaluation split (`abi/crates/abi-ai/src/training_contracts.rs` `ContractError::TeacherInEvaluation`); Abbey must adopt that rule too, and it forces a real decision: **evaluation rows must be human-authored or human-approved, not raw teacher output.**
5. **Seal.** Write `training_datasets` with the manifest and `root_digest`. The digest covers the canonical record bytes, the split assignment, the redaction revision, and the teacher set.

**The evaluation harness (the part that makes (a) honest).**

- **Held-out fixtures**: a checked-in, small, hand-authored set of prompt→expected-property assertions per persona (e.g. "Aviva answers lead with the concrete answer"). These are *fixtures*, not a benchmark; they measure contract conformance, not quality.
- **A pre-registered bar**, written **before** the numbers are known: which fixture must pass, how many, and what the failure text is. The bar's digest and the fixture set's digest go in the manifest, so a later bar change is detectable.
- **The bar must not be "the model got better."** It must be "this specific adapter, on this specific held-out set, produced these specific fixture outcomes, compared against the base model's outcomes on the same set." Comparing against the base is what makes it evidence rather than anecdote.

**Claims affected.**

- New `training-self-distilled-dataset`: `Partial` with `impl`/`tests` non-empty and `local: Required(&[…])` naming what remains (e.g. *"a sealed dataset exists and its held-out fixtures are defined; a live comparison against a base model is still open"*). `tools/check_claims_sync.py:787` requires a Partial claim to identify missing proof in local or external.
- `training-lora-pipeline`: **stays `Proposed`**. Still no weight update.
- `memory-sqlite-self-learn`: gains the dataset-store impl ref.

**Acceptance tests (exact names).**

- `src/learn/dataset.rs::tests::a_sealed_dataset_records_a_stable_root_digest`
- `src/learn/dataset.rs::tests::the_split_is_stable_when_an_unrelated_record_is_added`
- `src/learn/dataset.rs::tests::redaction_removes_absolute_paths_and_control_characters`
- `src/learn/dataset.rs::tests::the_redaction_revision_is_recorded_in_the_manifest`
- `src/learn/dataset.rs::tests::duplicate_prompts_with_differing_responses_are_refused`
- `src/learn/dataset.rs::tests::a_teacher_authored_record_cannot_enter_the_evaluation_split`
- `src/learn/dataset.rs::tests::obsolete_records_never_enter_a_dataset`
- `src/learn/eval.rs::tests::the_held_out_fixture_set_has_a_stable_digest`
- `src/learn/eval.rs::tests::the_bar_is_rejected_when_it_does_not_match_the_fixture_set`
- `src/runtime/store/training_operation.rs::tests::a_dataset_operation_id_is_globally_single_use`
- `src/runtime/store/training_operation.rs::tests::an_unfinished_operation_reopens_as_failed_not_replayed`
- `src/runtime/store/training_operation.rs::tests::the_operation_ledger_is_capped`
- `src/runtime/migrations.rs::tests::schema_v11_adds_the_training_tables`
- `tests/cli_surface.rs::learn_dataset_seals_a_digest_bound_dataset`

**Gate command.** `./check.sh`. The migration test also runs in every cohort.

**Size estimate.** New `src/learn/dataset.rs` ~400, `src/learn/eval.rs` ~250, `src/runtime/store/training_operation.rs` ~450, migration SQL ~60. Net new ≈ **1,200 lines** across 4 new + 6 touched files.

**Dependencies.** Slice 0. Plus the pinned-ABI question (§3.2) for `training_contracts` reuse.

**False claims to avoid.**
- ❌ "The dataset improves the model." No model was touched.
- ❌ "Quality-gated." Gates are structural (redaction, dedup, split, manifest) unless a quality metric is actually computed — and then it must be named and reproducible.
- ❌ "Held-out." The held-out set must genuinely be excluded from the train split and must not contain teacher output (per the sibling's own rule).
- ❌ "80/10/10 is a statistical guarantee." It is a *ratio assignment*, not a power calculation.

---

### Slice 2 — The protocol surface (wire the declared v3 training commands)

**Purpose.** Turn the **already-declared** protocol-v3 training family into a real, grant-gated, crash-safe daemon surface with typed client methods.

**This slice ships no weights.** It ships the *control plane* for a trainer that does not exist yet. That is honest: a durable, single-use, append-only, digest-bound operation ledger that can queue, report, cancel, and audit a training operation — and a daemon that **fails closed** because no trainer is bound at startup. The capability claimed is *"a training operation can be durably queued, observed, and cancelled over authenticated protocol v3"*, not *"Abbey trains"*.

**Files touched.**

| File | Change |
|---|---|
| `src/app_core/v3.rs` (already declares everything) | Add `V3TrainingDataset`, `V3TrainingMetrics` payload types if the existing `V3EntityRecord`/`V3MetricPage` are insufficient (`:467-479`, `:271-282`). **Append-only to `V3Capability` ordering** (`:28-46`) — never reorder; the desktop mirror test at `desktop/src-tauri/src/v3_ipc.rs:260-300` asserts declaration order exactly. |
| `src/daemon/runtime_v3.rs` | Replace the catch-all at `:266` with explicit arms for the six commands. `:7`'s doc comment must be updated. |
| `src/daemon/runtime_v3/training.rs` | **new** — the training authority, mirroring `src/daemon/runtime_v3/model_lifecycle.rs`. |
| `src/daemon/client/v3.rs` (already 750+ lines — **split first**) | Typed methods: `training_dataset_status`, `start_training`, `training_status`, `cancel_training`, `training_metrics`, `promote_adapter`, `rollback_adapter`. |
| `src/daemon/client.rs:644-646` | Event names already exist. |
| `src/runtime/store/training_operation.rs` | From slice 1. |
| `src/app_core/` (protocol v4) | **Unchanged.** v4 grants no execution authority and conversations are not training. |

**Grant semantics (the part that must be right).**

- `ReadTraining` → `TrainingDatasetStatus`, `TrainingStatus`, `TrainingMetrics` (`src/app_core/v3.rs:394-396`).
- `ManageTraining` → `StartTraining`, `CancelTraining`, `PromoteAdapter`, `RollbackAdapter` (`:397-400`).
- The daemon grants `ReadTraining` **only** when a sealed dataset exists in the startup-selected store. It grants `ManageTraining` **only** when a trainer is bound at startup — and until slice 3 exists, **it is never granted**. A request for an ungranted capability must fail at *negotiation*, not at dispatch (`src/daemon/runtime_v3.rs:277` `authorizes`).
- **The command may not select its own inputs.** `V3TrainingStart` already binds `dataset_id`, `base_model_id`, `base_revision`, `operation_id`, `seed` (`src/app_core/v3.rs:305-321`). A request must not be able to name an executable, an argv, an environment, a workspace, a URL, or a substitute base model — the same rule the model lifecycle already follows (`src/claims/registry.rs` `daemon-protocol-v3-model-lifecycle`).
- **Single-use operation ids** globally, exactly like `model_operations` (`src/runtime/store/model_operation.rs`).
- **Crash-safe states**: `queued → running → succeeded | failed | cancelled`; `queued`/`running` reopens as `failed` (never replayed); `cancel` may only terminate `queued`/`running`.
- **One request, one send.** `V3DaemonSession` sends each request once; there is no automatic retry of a mutating command (the memory-mutation pattern at `src/runtime/store/tool_execution.rs`).

**Claims affected.**

- New row `daemon-protocol-v3-training-control` — `Status::Partial` (impl/tests non-empty; local `Required` naming the missing trainer), because the control plane ships before the trainer exists. **Must not be `Current`** — `Current` may carry no Required evidence (`tools/check_claims_sync.py:785`) and the trainer is missing.
- `daemon-protocol-v3-abi-model-inventory` (registry) — its note enumerates what is *absent*; if the training grants change, that note must change. Read it before editing.
- `training-lora-pipeline`: **stays `Proposed`** — no weight update yet.

**Acceptance tests (exact names).**

- `src/daemon/runtime_v3.rs::tests::training_dataset_status_is_refused_without_the_read_training_grant`
- `src/daemon/runtime_v3.rs::tests::manage_training_is_never_granted_without_a_bound_trainer`
- `src/daemon/runtime_v3.rs::tests::start_training_rejects_a_dataset_that_does_not_exist`
- `src/daemon/runtime_v3.rs::tests::start_training_rejects_a_reused_operation_id`
- `src/daemon/runtime_v3.rs::tests::start_training_rejects_a_base_model_the_startup_route_did_not_select`
- `src/daemon/runtime_v3.rs::tests::cancel_training_only_terminates_queued_or_running_operations`
- `src/daemon/runtime_v3.rs::tests::an_unfinished_training_operation_reopens_as_failed`
- `src/daemon/runtime_v3.rs::tests::training_metrics_never_expose_prompt_or_output_text`
- `src/daemon/client/v3.rs::tests::start_training_correlates_one_response_to_one_operation_id`
- `src/daemon/client/v3.rs::tests::a_refused_grant_stops_before_dispatch`
- `src/daemon/runtime_v3.rs::tests::protocol_v1_and_v2_are_unchanged_by_the_training_arms`
- `tests/daemon_cli.rs::the_daemon_refuses_a_training_request_with_no_grant`

**Gate command.** `./check.sh`.

**Size estimate.** `src/daemon/runtime_v3/training.rs` ~450; `src/daemon/client/v3.rs` split into `src/daemon/client/v3/` (currently 750+ lines) with `training.rs` ~250; `src/app_core/v3.rs` +~120. Net new ≈ **800 lines**, plus ~120 lines moved.

**Dependencies.** Slice 1.

**False claims to avoid.**
- ❌ "The daemon can train." It can queue and observe a training operation. Nothing trains.
- ❌ "Training is available over the daemon." `ManageTraining` is never granted until a trainer exists.
- ❌ "The desktop can train." Slice 5 is read-only.
- ❌ "Protocol v4 changed." It did not, and must not.

---

### Slice 3 — The bounded trainer

**Purpose.** Actually run a **bounded** trainer that produces an **adapter**, with deterministic seeds, CPU-first, adapter-only writes, and a pre-registered bar.

**Where it runs — three candidates, one recommendation.**

| Option | Pros | Cons | Verdict |
|---|---|---|---|
| **(i) In-process Rust trainer in Abbey** | No sibling pin; total control; can use `abi-model-runtime`'s Candle dependency (`abi/crates/abi-model-runtime/src/provider.rs:1` — *"Candle-backed local provider"*; `candle-core` is already in Abbey's **dev**-dependencies at `Cargo.toml:91`) | A trainer is ~2–4 kLoC of numerics; `unsafe_code` is denied (fine, Candle is a dependency); adds a heavy dep to the default build | ❌ Too much numerics to own |
| **(ii) A new crate in the ABI sibling** (`abi/crates/abi-trainer`) | ABI already owns `abi-model-runtime` (Candle), `abi-models` (signed manifests, license ledger, storage root, `abi/crates/abi-models/src/{signed,license,registry,manifest,storage}.rs`), `abi-sea`'s `TrainingRegistry` (`:179` `new`, `:308`/`:329`/`:366` `register_*`, `:511` `admit`, `:538` `register_snapshot`, `:591` `revoke`, `:611` `eligibility`, `:660` `training_records`), and `abi-ai`'s `training_contracts` receipts (`abi/crates/abi-ai/src/training_contracts/receipts.rs`: `RunState`, `RunInputs{seed, configuration_digest, initial_tensor_digest, trainer_principal, classification}`, `TensorChange{before_digest, after_digest, changed_tensors}`, `RunReceipt`, `EvaluationReceipt`) | **Must be pinned by revision** for CI reproducibility; ABI has its own gate | ✅ **Recommended** |
| **(iii) An external local process** (e.g. `mlx_lm`, `peft`, `python -m …`) | Fastest path to a real LoRA; real ecosystem | Unpinnable behavior; an external process would need the `os_control` allowlist + `--confirm` in the shipped edition (an invariant we must not relax); a black box Abbey cannot audit | ❌ Only as a *manual, owner-driven* step outside the shipped product |

**Recommendation: (ii).** ABI already has the exact vocabulary — `RunReceipt`, `EvaluationReceipt`, `ModelClassification::ProductionLlm | Fixture` (`abi/crates/abi-ai/src/training_contracts/receipts.rs`), `TrainingRegistry`, signed manifests with a license acceptance ledger — that a trainer needs. Putting the trainer there keeps Abbey's `unsafe_code`-denied, thin CLI/daemon shape and puts the numerics next to the Candle dependency that already exists.

**The pin discipline (non-negotiable).** A trainer in ABI must be at a **pinned** ABI revision before any Abbey claim depends on it. Per `AGENTS.md`: *"A fix must not depend on an uncommitted sibling change: CI pins a revision, so a proof that only holds against the sibling's working tree is not reproducible."* Concretely: land the trainer in ABI → ABI's own gate passes → the pin in `.github/workflows/rust.yml` (`ABI_REVISION`) moves → Abbey consumes it → `tools/ci/verify-pinned-siblings.sh` proves the local checkout matches. **Until then, no Abbey claim may cite it.**

**Trainer contract (what it must guarantee).**

| Requirement | Why | Where it echoes an existing precedent |
|---|---|---|
| Deterministic seed, recorded | Reproducibility | `abi/crates/abi-ai/src/training_contracts/receipts.rs` `RunInputs.seed` |
| Base model identity bound by digest | The adapter is meaningless without its base | `TeacherIdentity.base_digest` (`abi/crates/abi-ai/src/training_contracts.rs:62-68`); `RunInputs.base`/`tokenizer` `ArtifactRef` |
| **Adapter-only writes; base weights never mutated** | The shipped edition must not be able to corrupt a vendor base | `abi/crates/abi-sea/src/training_registry.rs:1-4` — *"this library enforces configured authority, not authentication… the registrar must verify weights and evaluation outside this API"* |
| Configuration digest recorded | Reproduce the run | `RunInputs.configuration_digest` |
| Initial tensor digest + `TensorChange{before_digest, after_digest, changed_tensors}` | This is the *proof of a weight update* the claim demands | `abi/crates/abi-ai/src/training_contracts/receipts.rs` `TensorChange` |
| **A held-out evaluation against the base**, recorded as `EvaluationReceipt` | The bar, not an anecdote | `EvaluationReceipt`; `abi/crates/abi-sea/src/training_registry.rs:366` `register_approval` with `ApprovalKind::QualifiedEvaluator` |
| **No raw prompt/output in any receipt** | The v3 boundary forbids it | `src/daemon/runtime_v3.rs` memory-reads redaction; `src/runtime/migration_10_assistant.sql` keeps assistant content in a separate table from lifecycle metadata |
| Bounded: fixed step count, fixed wall-clock ceiling, fixed token budget | A training run must not be unbounded | `src/daemon/runtime_v3.rs` 1-second cooperative deadline; `src/claims/registry.rs` v3 run budgets |

**CPU-first, Metal only if a bar is met.** CPU is the default and the *only* device that may be claimed until a Metal run has its own recorded native-operation evidence. The existing precedent is exact: `accelerator-kernel-execution-metal` records *"Native execution is read from the adapter's `CapabilityState` evidence ladder with a fresh adapter per kernel, never inferred from the return value"* (`src/claims/registry.rs`). A Metal training claim would need the same: device evidence read from the adapter, not inferred from a speedup. Note `abi-model-runtime` already has a `metal` feature (`Cargo.toml:126` gates `accel = [… , "abi-model-runtime/metal"]`).

**Model-size reality on this host (24 GB).** A 4B-class base is the honest ceiling; a 12B 4-bit base is marginal *with* optimizer state and activations in the same 24 GB. `src/models.rs:8`'s default `gemma4:26b-mlx` is not a training candidate here. The sibling-pinned candidate `Qwen/Qwen3.5-4B` @ `851bf6e806efd8d0a36b00ddf55e13ccb7b8cd0a` (`lylx/docs/models/pretrained-candidate-research.md:47`) is a *research record in another repository*, not an Abbey base model — reusing it would import another repo's license decision (see §7 Q1).

**Claims affected.**

- `training-lora-pipeline`: `Proposed → Partial`. The machine rules: `Partial` requires non-empty `impl` **and** `tests` (`tools/check_claims_sync.py:782-783`) and requires `Required` in local **or** external (`:787`).
- `models-production-local-weights` (registry, `Proposed`): **stays Proposed** — an adapter over a vendor base is not "production-capable local model weights". Its note may be extended to say *an adapter exists over base M*; it may not be promoted.
- `accelerator-execution-runtime` (registry, `Proposed`): if a Metal path is exercised, the note must name it as narrowly as `daemon-protocol-v3-exact-model-inference` does — no general training claim.
- New row `training-bounded-adapter-run` (or fold into `training-lora-pipeline`): `Partial` until the eval bar has been met **and re-run end to end from a cold start**.

**Acceptance tests (exact names).**

- `abi/crates/abi-trainer/src/lib.rs::tests::a_run_with_a_fixed_seed_is_bit_identical_across_runs` *(runs in the ABI repo's gate; Abbey's claim cites it as a sibling test path)*
- `src/daemon/runtime_v3/training.rs::tests::a_training_run_writes_an_adapter_and_never_touches_the_base`
- `src/daemon/runtime_v3/training.rs::tests::the_run_receipt_records_the_before_and_after_tensor_digest`
- `src/daemon/runtime_v3/training.rs::tests::a_run_without_a_bound_base_model_is_refused`
- `src/daemon/runtime_v3/training.rs::tests::a_run_that_misses_the_bar_is_recorded_as_failed_not_promoted`
- `src/daemon/runtime_v3/training.rs::tests::a_cancelled_run_writes_no_adapter`
- `src/daemon/runtime_v3/training.rs::tests::training_metrics_expose_no_prompt_or_output_text`
- `src/daemon/runtime_v3/training.rs::tests::the_device_recorded_is_the_device_that_actually_ran`
- `tests/daemon_cli.rs::a_bounded_training_run_promotes_an_adapter_over_a_fixture_base`

**Gate command.** `./check.sh` **and** the ABI sibling's own gate (`abi/tools/check.sh`) — a claim that depends on a sibling must be proved in the sibling too.

**Size estimate.** ABI side: `abi/crates/abi-trainer` ~1,500–3,000 (numerics). Abbey side: `src/daemon/runtime_v3/training.rs` +~400, `src/daemon/client/v3/training.rs` +~200. Net new ≈ **2,000–3,600 lines** across 2 repos.

**Dependencies.** Slices 0–2; the ABI pin move; §7 Q1/Q2/Q3/Q5 answered.

**False claims to avoid.**
- ❌ "Abbey trained a model." It trained an **adapter over a named base at a named revision**. If the base is a vendor model, the result is a delta on that vendor's model, and saying "Abbey trained" hides who owns the base.
- ❌ "It got better." Only: "it moved fixture F from P to Q on held-out split S of dataset D, compared against the same fixtures run on the base."
- ❌ "LoRA" if the method is AdaLoRA or DoRA. Name the method.
- ❌ "On Metal." Only with adapter-reported native-operation evidence.
- ❌ "For the personas." An adapter is not a persona; the persona remains a text contract (`src/persona.rs:32-40`).

---

### Slice 4 — Rollback + signed artifact provenance

**Purpose.** Make an adapter *reversible* and *verifiable*: promotion and rollback are durable, single-use, digest-bound operations, and every adapter artifact is signed and independently verifiable.

**Files touched.**

| File | Change |
|---|---|
| `src/runtime/migration_11_training_operations.sql` | `training_adapters` rows get `state`, `promoted_at_ms`, `rolled_back_at_ms`; a `training_adapter_events` append-only table |
| `src/runtime/store/training_operation.rs` | `promote`/`rollback` transitions, single-use decision ids |
| `src/daemon/runtime_v3/training.rs` | `PromoteAdapter` / `RollbackAdapter` arms |
| `src/daemon/client/v3/training.rs` | typed methods |
| ABI side | `abi/crates/abi-models/src/signed.rs` already implements Ed25519-signed manifests (`abi/crates/abi-models/src/`, and `src/daemon/runtime_v3/model_lifecycle.rs` verifies publisher signatures against configured public keys) — reuse it for adapter manifests rather than inventing a second signature scheme |

**Design.**

- **Promotion** = "this adapter is now the one that the trainer's inference path may use". It must be: bound to an exact `adapter_id` + `digest` + `base_model_id` + `base_revision`; a **globally single-use decision id**; append-only; and it must **never** install a global default or silently reroute protocol v2 — the exact boundary the existing model lifecycle holds (`src/claims/registry.rs` `daemon-protocol-v3-model-lifecycle`: *"A loaded provider is not installed as a global default or silently routed into protocol v2"*). Same rule, same reason.
- **Rollback** = "the previous promoted adapter is restored". It must restore the *exact* prior state, be single-use, and be itself auditable. Rollback must never delete an artifact; artifacts are retained with a `rolled_back` state.
- **Signed artifact provenance.** An adapter artifact carries a manifest — `{adapter_id, digest, byte_length, base_model_id, base_revision, dataset_root_digest, seed, configuration_digest, created_at, training_run_id}` — signed with the same publisher-key model `abi-models` already uses. The digest proves *integrity*, **not** authenticity (the sibling says exactly this: *"The digest provides corruption detection, not authenticity or authorization"* — `lylx/crates/quesar/IMPLEMENTATION.md`, same discipline as ABI's). Authenticity comes from the signature and the configured trust key.
- **Audit rows** must contain bounded ids, revision, operation kind, progress, requested device, and **no** registry URL, storage path, principal, license text, weights, tokenizer bytes, or credentials — the exact exclusion list the model-lifecycle row already states.

**Claims affected.**

- `daemon-protocol-v3-model-lifecycle` (registry) — note update if adapter promotion is folded into the same authority.
- New row `training-adapter-promotion-rollback` — `Partial` (impl/tests non-empty; local `Required` naming what remains, e.g. *"a cold-start rollback has not been re-proved"*).
- `training-lora-pipeline`: may move `Partial → Current` **only if** the local evidence is `Verified` (a `Current` claim may carry no `Required` evidence — `tools/check_claims_sync.py:785`).

**Acceptance tests (exact names).**

- `src/daemon/runtime_v3/training.rs::tests::promote_adapter_binds_the_exact_adapter_and_base`
- `src/daemon/runtime_v3/training.rs::tests::promote_adapter_refuses_a_reused_decision_id`
- `src/daemon/runtime_v3/training.rs::tests::rollback_restores_the_exact_previous_promoted_adapter`
- `src/daemon/runtime_v3/training.rs::tests::rollback_never_deletes_an_artifact`
- `src/daemon/runtime_v3/training.rs::tests::a_promoted_adapter_does_not_become_a_global_default`
- `src/daemon/runtime_v3/training.rs::tests::an_adapter_manifest_with_a_tampered_digest_is_refused`
- `src/daemon/runtime_v3/training.rs::tests::an_adapter_signed_by_an_untrusted_key_is_refused`
- `src/daemon/runtime_v3/training.rs::tests::the_audit_row_contains_no_path_no_weights_and_no_license_text`
- `src/runtime/store/training_operation.rs::tests::an_unfinished_promotion_reopens_as_failed_not_replayed`

**Gate command.** `./check.sh`.

**Size estimate.** `src/runtime/store/training_operation.rs` +~250; `src/daemon/runtime_v3/training.rs` +~250; ABI side (if the signed adapter manifest lives there) +~300. Net new ≈ **600–800 lines**.

**Dependencies.** Slice 3.

**False claims to avoid.**
- ❌ "Signed artifacts are authentic by default." A signature is only as good as the configured trust key.
- ❌ "Rollback restores the model." It restores the *promoted adapter selection*; the base model is unchanged throughout.
- ❌ "The adapter is installed." It is promoted within the trainer's scope and is not a global default.

---

### Slice 5 — The desktop Training view

**Purpose.** Replace the honest stub at `desktop/src/surfaces.ts:103-116` with a **read-only** view backed by slice 2's `ReadTraining` grant.

**Deliberately read-only.** The view must not offer Start / Cancel / Promote / Rollback in slice 5. The desktop's existing boundary is read-only by design (`src/claims/registry.rs` `daemon-protocol-v3-abi-model-inventory`: *"The desktop consumes that same typed client through enumerated read-only Tauri commands that request ReadMemory, ReadModels, and ReadClaimsById"*). A mutating desktop surface is a *different* slice with a different threat model.

**Files touched.**

| File | Change |
|---|---|
| `desktop/src/surfaces.ts:103-116` | `requires: "read_training"`; drop `unavailable`. |
| `desktop/src/views/TrainingView.tsx` | **new** — dataset status, operation status, metrics. |
| `desktop/src/App.tsx:34` | keep `"training"` in `ADVANCED`. |
| `desktop/src-tauri/src/v3_ipc.rs:271,294` | Add `ReadTraining` to the requested-grant set for the training read session. |
| `desktop/src-tauri/src/commands.rs` | Add the enumerated read-only Tauri commands (`app_training_dataset_status`, `app_training_status`, `app_training_metrics`) — read-only, exactly like the `ReadModels`/`ReadClaimsById` pattern (`desktop/src-tauri/src/commands.rs:175,196`). |
| `desktop/src/ipc/generated.ts` | **Regenerated**, not hand-edited: `(cd desktop && bun run codegen)`; the root gate checks drift via `cargo run --locked --quiet -p abbey-desktop-codegen -- --check` (`check.sh:113`). |
| `desktop/src-tauri/src/main.rs` | The `generate_handler!` block gains the new commands. **It must stay comment-free** — `verify-bundle.mjs` parses it by splitting on commas (`AGENTS.md` *Desktop Tauri entrypoints*). |

**Claims affected.**

- `desktop-tauri-react` (registry, `Proposed`): the note already enumerates what is *not* done. Adding a read-only Training view does **not** promote it to `Current` (it still lacks notarized distribution, governed tasks, Windows packaging). Its note gains one sentence naming the new read-only surface — and that sentence must be true.
- `training-lora-pipeline`: **unchanged** — a view is not a trainer.

**Acceptance tests (exact names).**

- `desktop/src-tauri/src/main_tests.rs::the_training_commands_are_registered_in_the_tauri_handler`
- `desktop/src-tauri/src/v3_ipc.rs::tests::the_training_read_session_requests_only_read_training`
- `desktop/src-tauri/src/main_tests.rs::the_training_view_refuses_without_the_read_training_grant`
- `desktop/src/views/TrainingView.tsx` (bun test): `the_training_view_shows_no_start_or_cancel_control`
- `desktop/src-tauri/src/main_tests.rs::the_training_surface_is_not_shown_when_the_grant_is_absent`

**Gate command.** `desktop/check.sh` (bun typecheck + tests + build + bundle security + a linked binary + a live scratch `abbeyd` read) **plus** `./check.sh` for the codegen-drift step. The desktop's own sources are **not** measured by the root file-size guard (`AGENTS.md` *Invariants*).

**Size estimate.** `desktop/src/views/TrainingView.tsx` ~250; `desktop/src-tauri/src/commands.rs` +~60; `desktop/src-tauri/src/v3_ipc.rs` +~25; `desktop/src/surfaces.ts` ±10. Net new ≈ **350 lines**.

**Dependencies.** Slice 2 (the grant and the typed client). The view shows nothing until the daemon grants `ReadTraining`, which per slice 2 requires a sealed dataset.

**False claims to avoid.**
- ❌ "The desktop can train." It can *observe* a training operation.
- ❌ "The Training view shows the model's quality." It shows operation state and metrics.
- ❌ "The desktop is now training-capable." It is read-only, by design.

---

### 4.1 Slice dependency graph and gating

```
Slice 0 (honesty/UX, no weights)
   └─> Slice 1 (durable dataset + eval bar)
         └─> Slice 2 (protocol-v3 control plane; no weights) ──> Slice 5 (desktop, read-only)
               └─> Slice 3 (bounded trainer, adapter-only) ──> Slice 4 (rollback + signed artifacts)
```

Each arrow is a **hard** dependency for honesty, not merely for convenience: shipping slice 3 without slices 0–2 would produce a trainer whose dataset has no manifest, no split, no eval, and no durable operation record — i.e. a weight update with no evidence chain.

---

## 5. The exact evidence bar for `training-lora-pipeline`

### 5.1 The machine rules this must satisfy

From `tools/check_claims_sync.py:768-805` (`validate_claim_semantics`) and `src/claims/registry/types.rs`:

| Status | `impl` refs | `tests` refs | `local_live` | `external_required` |
|---|---|---|---|---|
| `Proposed` | **must be empty** | **must be empty** | **must be `Required`** | any |
| `Partial` | must be non-empty | must be non-empty | **must be `Required`** in local *or* external | any |
| `Current` | must be non-empty | must be non-empty | **must not be `Required`** | **must not be `Required`** |

Also enforced:
- `automated_test_refs` entries must resolve: `path::…::name` requires `path` to be a real file and `name` to be a `fn <name>`/`def <name>` declared somewhere under `src/`, `tests/`, `desktop/src-tauri/src/`, or `tools/tests/` (`tools/check_claims_sync.py:89-155`); a bare path must exist; prose containing whitespace is left unresolved deliberately.
- A `Current` claim may carry **no** Required evidence anywhere.
- Only `Blocked` claims may carry a `blocker_owner` (`:798-801`).
- The digest of the registry is embedded in the generated summary; `python3 tools/check_claims_sync.py --write` rewrites every generated region. **Never hand-edit `AGENTS.md`, `CLAUDE.md`, `README.md`, `docs/claims.md`, `docs/architecture.md`, `docs/identity.md`, `docs/production.md`, `tasks/goals.md`, `tasks/todo.md`, `tasks/lessons.md`.**

### 5.2 Proposed → Partial (the bar at the end of slice 3)

**Status:** `Status::Partial`.

**`impl` refs (non-empty; each must be a real file):**

```
"src/learn/dataset.rs",
"src/learn/eval.rs",
"src/runtime/store/training_operation.rs",
"src/daemon/runtime_v3/training.rs",
"src/daemon/client/v3/training.rs",
"src/distill.rs",
```

**`tests` refs (non-empty; each must resolve to a declared test fn):**

```
"src/learn/dataset.rs::tests::a_sealed_dataset_records_a_stable_root_digest",
"src/learn/dataset.rs::tests::the_split_is_stable_when_an_unrelated_record_is_added",
"src/learn/dataset.rs::tests::a_teacher_authored_record_cannot_enter_the_evaluation_split",
"src/learn/eval.rs::tests::the_held_out_fixture_set_has_a_stable_digest",
"src/runtime/store/training_operation.rs::tests::a_dataset_operation_id_is_globally_single_use",
"src/runtime/store/training_operation.rs::tests::an_unfinished_operation_reopens_as_failed_not_replayed",
"src/daemon/runtime_v3/training.rs::tests::the_run_receipt_records_the_before_and_after_tensor_digest",
"src/daemon/runtime_v3/training.rs::tests::a_run_without_a_bound_base_model_is_refused",
"src/daemon/runtime_v3/training.rs::tests::a_run_that_misses_the_bar_is_recorded_as_failed_not_promoted",
"src/daemon/runtime_v3/training.rs::tests::training_metrics_expose_no_prompt_or_output_text",
"tests/daemon_cli.rs::a_bounded_training_run_promotes_an_adapter_over_a_fixture_base",
```

**`local_live` (must be `Required`, naming exactly what is still missing):**

```
EvidenceState::Required(&[
    "prove one bounded adapter training run end to end on this host from a cold start: sealed dataset digest X, base model M at revision R, seed S, adapter digest Y, and the held-out fixture outcomes compared against the base",
    "the run must be reproducible from the pinned sibling revision, not from the sibling working tree",
]),
```

**`external_required`:** `NO_EXTERNAL_PROOF` — *"the claim is bounded to local product behavior"* (`src/claims/registry.rs:43-44`).

**`next_action`:** *"re-run the bounded adapter run from a cold start and record the held-out comparison, then promote to Current only if the bar is met"*.

**`blocker_owner`:** `None` (only `Blocked` claims may carry one).

**`note` (the human-facing evidence boundary):** must say what the code does — e.g. *"a bounded adapter-training path now runs over a sealed dataset and writes an adapter artifact; base weights are never mutated; the adapter is not promoted until a held-out bar is met; no persona is trained, since Abbey's personas are text contracts"*.

### 5.3 Partial → Current (the bar at the end of slice 4)

**Status:** `Status::Current`.

**`impl` refs:** the §5.2 list **plus**:

```
"src/runtime/store/training_operation.rs",
"src/daemon/runtime_v3/training.rs",
```

(already present — keep them; add the promotion/rollback impl refs if the code lives in a separate module, e.g. `"src/daemon/runtime_v3/training/promotion.rs"`).

**`tests` refs:** the §5.2 list **plus**:

```
"src/daemon/runtime_v3/training.rs::tests::promote_adapter_binds_the_exact_adapter_and_base",
"src/daemon/runtime_v3/training.rs::tests::promote_adapter_refuses_a_reused_decision_id",
"src/daemon/runtime_v3/training.rs::tests::rollback_restores_the_exact_previous_promoted_adapter",
"src/daemon/runtime_v3/training.rs::tests::rollback_never_deletes_an_artifact",
"src/daemon/runtime_v3/training.rs::tests::a_promoted_adapter_does_not_become_a_global_default",
"src/daemon/runtime_v3/training.rs::tests::an_adapter_manifest_with_a_tampered_digest_is_refused",
"src/daemon/runtime_v3/training.rs::tests::the_audit_row_contains_no_path_no_weights_and_no_license_text",
```

**`local_live` — must NOT be `Required`.** Use:

```
EvidenceState::Verified(&[
    "one bounded adapter training run recorded end to end on a macOS ARM64 host: sealed dataset root digest, base model id and revision, seed, adapter digest, before/after tensor digest, and the held-out fixture outcomes compared against the base",
    "adapter promotion and rollback are durable, single-use, digest-bound, and survive a crash reopen without fabricating a result",
    "base model weights are never mutated by the trainer",
]),
```

**`external_required`:** `NO_EXTERNAL_PROOF`.

**`next_action`:** *"keep the adapter path bounded to one base revision at a time; re-qualify the claim when the trainer, the bar, or the base changes"*.

**`blocker_owner`:** `None`.

**The `Current` note must state the boundary explicitly**, e.g.: *"a bounded adapter-training path over one named local base model, with a sealed dataset, a held-out bar, and reversible promotion; it is an adapter over a vendor base model, not an Abbey-trained persona, and no claim is made about general model quality"*.

### 5.4 The parallel claim for (a) — required for honesty

`training-lora-pipeline` alone cannot carry the work done in slices 0–1, because it explicitly requires a weight update. Add a **separate** row so the delivered dataset capability has its own honest home:

```
id:       "training-self-distilled-dataset"
name:     "self-distilled, manifest-bound local training dataset"
status:   Status::Partial   (slice 1) → Status::Current (once the bar is met)
note:     "abbey learn distill asks local-first teachers and abbey learn promote records human
           review; abbey learn dataset seals a deterministic, digest-bound dataset with a
           versioned redaction revision, a stable 80/10/10 split, a held-out evaluation set that
           excludes teacher output, and a published manifest; this is dataset generation and
           curation only — no weights are updated and no model quality is claimed"
instead:  None
impl:     ["src/learn/dataset.rs", "src/learn/eval.rs", "src/distill.rs", "src/learn.rs"]
tests:    [... the dataset/eval test names above ...]
local:    Partial → Required(&["re-run the seal from a cold start and record the held-out
           fixture outcomes"]); Current → Verified(&[...])
external: NO_EXTERNAL_PROOF
next:     "keep the manifest digest and the split stable as records are added"
owner:    None
```

**And `learn-distill-local-first-teachers` (currently `Current`) must have its note rewritten** at slice 0, because it currently states that there is no promote-to-ready mechanism and that sft output is unreviewed — both become false after slice 0. A `Current` claim may carry no `Required` evidence (`tools/check_claims_sync.py:785`), so the note must describe the new boundary, not an outstanding proof.

### 5.5 The exact edit procedure

1. Edit `src/claims/registry.rs` **only** (never a generated region).
2. `python3 tools/check_claims_sync.py --write` — regenerates the ledger and every generated region in `AGENTS.md`, `CLAUDE.md`, `README.md`, `docs/claims.md`, `docs/architecture.md`, `docs/identity.md`, `docs/production.md`, `tasks/goals.md`, `tasks/todo.md`, `tasks/lessons.md`. It builds and runs the `abbey` binary under the qualified toolchain, so it takes minutes; `ABBEY_CLAIMS_SYNC_TIMEOUT` (default 900 s) widens the budget.
3. `python3 tools/check_claims_sync.py` — verify mode (the default).
4. `python3 tools/check_instructions.py` — the AGENTS.md/CLAUDE.md split.
5. `./check.sh` — the whole gate.
6. **Never** reintroduce the legacy strings `| Claim | Status | Evidence boundary |`, `(source: `src/claims.rs``, or a `## Gotchas` heading into `AGENTS.md` — the sync tool's legacy branch matches that combination and silently rewrites the whole file around it (`AGENTS.md` *Generated code*).

---

## 6. What must stay refused, and why

| Refusal | Where | Why it must stay |
|---|---|---|
| `abbey learn lora` / `finetune` / `fine-tune` / `fine_tune` → **exit 2** | `src/learn.rs:751` → `src/claims.rs:573-586` | Until slice 3 lands and the bar is met, there is no trainer. The refusal *is* the claim: `training-lora-pipeline` is `Proposed` with `Required` local evidence (`src/claims/registry.rs:684-695`). **It must not be softened to a warning, and it must not become exit 0 with a "coming soon" message.** If slice 3 lands, the refusal text changes to point at the *new* path — and `abbey learn lora` may then *route* to the real pipeline, but only after the `Current` bar is met. |
| `abbey claims refuse lora` | `src/claims.rs:583-586` | Same reason; it is the machine-readable refusal surface that `abbey oos` and the desktop ledger filter use (`desktop/src/surfaces.ts:114` `ledgerFilter: "lora"`). |
| `oos-fake-cost-accounting` | registry, `Status::OutOfScope` | `/cost` is N/A for cursor-agent. A training pipeline adds token/step accounting — if any of it were surfaced as a *cost*, that would be a fabricated number. **A training run may report steps, tokens, wall-clock, and adapter bytes — never a monetary cost**, unless a real billing source exists. |
| Cloud-only training | — | The shipped edition's whole posture is local-first (`src/distill.rs:41` `LOCAL_FIRST` puts the on-device `fm` first and the off-device `fm:pcc` last, and `on_device()` is false only for PCC — `src/distill.rs:74-77`). A trainer that *requires* a cloud GPU would invert the product's privacy stance and could not run on this host. |
| Uploading data off-device | — | The corpus contains user prompts and answers. `fm:pcc` already leaves the device and is **tagged** `off-device` (`src/distill.rs:190-194`) — the honest precedent is to tag the leak, not to prevent it silently. A trainer must never upload the dataset anywhere; the PCC path may only be used as a *teacher*, never as a *trainer*, and a PCC-authored pair must remain labelled `off-device` in the dataset manifest. |
| `os_control` allowlist + `--confirm` | `src/os_control.rs`, both editions | An external-process trainer (§4 option (iii)) would be an OS execution. The shipped edition keeps the allowlist and `--confirm`; a trainer must not be a way around it. This is why slice 3 recommends an in-repo sibling crate over an external binary. |
| `unsafe_code` deny at the manifest level | `Cargo.toml:44-45` | A trainer is the most numerics-heavy code Abbey would ever link. It belongs in ABI (which also denies `unsafe_code` workspace-wide, `abi/Cargo.toml`) rather than in a crate that must not grow `unsafe`. |
| Fabricated weights | — | Any adapter artifact must be produced by an actual run with a recorded `TensorChange{before_digest, after_digest, changed_tensors}` (`abi/crates/abi-ai/src/training_contracts/receipts.rs`). A hand-written or copied adapter file is a fabricated weight and must never be committed or shipped. |
| "Trained Quesar" / "trained persona" | — | See §3.4. Abbey's personas are text contracts (`src/persona.rs:32-40`); Quesar is a separate product in a separate repository (`lylx/`). The corpus may carry a Quesar label; the *model* is never trained here. |

---

## 7. Open questions for the owner

Each has a blocking dependency marked, a default recommendation, and the honest fallback.

**Q1 — Model choice and licensing. [Blocks slice 3]**
Which base model may Abbey train an adapter over, and under what licence? Candidates seen in this checkout: the local default `gemma4:26b-mlx` (`src/models.rs:8`) — 26B-class, **not** viable for LoRA on 24 GB; a 4B-class Qwen/Qwen3.5-4B at revision `851bf6e806efd8d0a36b00ddf55e13ccb7b8cd0a` (`lylx/docs/models/pretrained-candidate-research.md:47`) — researched in *another* repository, with that repo's own audit recording *"trainer masking/resume defects"* in the installed backend (`lylx/docs/models/local-backend-audit.md`); or another base entirely.
*Recommendation:* a 4B-class base with a permissive licence and a verified safetensors + tokenizer pair, reviewed by a human, with the licence decision recorded **as an approval** in the dataset/manifest (the sibling's `ApprovalKind::Human` / `Permission{approved_use_decision_id}` machinery already exists — `abi/crates/abi-ai/src/training_contracts.rs:47-59,69`).
*Fallback:* if no licence decision is made, slices 0–2 still ship honestly and `training-lora-pipeline` stays `Proposed`.

**Q2 — Where do the base weights live? [Blocks slice 3]**
`abi-models` already has a storage root, a license-acceptance ledger, and signed manifests (`abi/crates/abi-models/`), and Abbey's daemon already serves an edition-scoped signed model registry (`src/claims/registry.rs` `daemon-protocol-v3-model-lifecycle`). Should the training base live in that same registry (reusing the existing download/verify/license path), or in a separate training-specific root?
*Recommendation:* the **same** registry — one base-model identity, one license ledger, one digest vocabulary, one place where "the base is not downloaded" is answerable.
*Fallback:* a separate root means two license ledgers and two answers to "is the base present?".

**Q3 — Does a 12B QAT model fit in 24 GB? [Blocks slice 3 scope]**
Measured host: Apple M5, 24 GB unified memory. A 12B model at 4-bit is ~6–7 GB of weights; with gradients (adapter-only, so small), optimizer state, activations, and the OS, it *may* fit — but "may fit" is not evidence, and the LYLX repo already records *trainer masking/resume defects* for its pinned candidate's installed backend. A 4B base is comfortably safe and makes a bounded run actually reachable in a working session.
*Recommendation:* **4B-class for the first bounded run**; treat 12B as a later, separately-evidenced target.
*Fallback:* if 12B is required, it needs its own memory-budget evidence (`lylx/crates/quesar/src/training.rs:113` `resource_estimate` is a reference implementation of exactly this arithmetic) before any claim.

**Q4 — May the teacher's output be trained on at all? [Blocks slice 0's `consent` field semantics, and slice 1's `permitted_specification_digests`]**
`fm --model system` (on-device Foundation Models) and `fm --model pcc` (Private Cloud Compute) are Apple services with terms; `ollama:gemma4:26b-mlx` is a locally-pulled model with its own upstream licence; and a human-authored `abbey learn train` row is the user's own text. The current claim explicitly does not claim that any model's terms permit training on its output (`src/claims/registry.rs`, `learn-distill-local-first-teachers`).
*Recommendation:* a per-teacher `consent` enum (`unknown` / `denied` / `granted`), defaulting to `unknown`; rows with `consent != "granted"` are **excluded from the train split** and admitted only to a clearly-labelled `unlicensed` shard that the trainer refuses. This makes the legal uncertainty *structural* rather than a footnote.
*Fallback:* if the answer is "unknown for all teachers", the entire pipeline still ships; it just produces a dataset no trainer may consume — which is an honest, if useless, state, and the honest thing to do until it is answered.

**Q5 — AdaLoRA / PEFT dependency decision. [Blocks slice 3]**
Options: (a) implement adapter training in the ABI sibling with **no new external dependency**, using Candle directly (`abi/crates/abi-model-runtime` already depends on Candle — `abi/crates/abi-model-runtime/Cargo.toml`); (b) depend on a PEFT crate (AdaLoRA/DoRA) — but PEFT is Python-first; the Rust equivalents are young, and adding one means pinning a git revision and absorbing its MSRV (Rust 1.99.1, edition 2024 — `rust-toolchain.toml`) and its `unsafe` posture into a workspace that denies `unsafe_code`; (c) use the LYLX `quesar` crate's mathematics (`lylx/crates/quesar/src/optim.rs`, `src/model/backward.rs`) — but that is a **f64 reference with hard admission limits** (width ≤ 32, layers ≤ 2, vocab ≤ 512, ≤ 64 prediction positions, `BACKWARD_MAX_WORK = 16_000_000`), i.e. it cannot train a production-scale base model, and it lives in a different repository.
*Recommendation:* **(a)** — implement the bounded adapter update in the ABI sibling on Candle, with the PEFT method named exactly (e.g. "LoRA rank r on the attention projections"), a deterministic seed, and a numerical oracle (finite differences) proving the gradient — the LYLX crate demonstrates exactly this discipline (`lylx/crates/quesar/IMPLEMENTATION.md`: "Persistent integration oracles now check every coordinate … against public-forward central differences").
*Fallback:* (c) as a *numerical oracle only*, never as the production trainer.

**Q6 — Where does the Quesar-labelled corpus live? [Blocks §3.4 Q1 interpretation]**
Should `TrainingProfile::Quesar` rows be produced inside Abbey at all, or should Abbey only emit the contracts and let the `donald`/`lylx` repository label them?
*Recommendation:* Abbey emits them under the sibling's own `TrainingProfile` enum (`abi/crates/abi-ai/src/training_contracts.rs:17-27`) and claims only *"a Quesar-labelled corpus subset exists"*. The LYLX repository keeps its own gates.
*Fallback:* if the owner wants strict repo separation, Abbey emits only `Abbey`/`Aviva`/`Abi` profiles and the Quesar labelling happens downstream — at the cost of the corpus not being directly consumable.

**Q7 — Is `abbey learn promote` a mutating CLI verb, and does it need `--confirm`? [Blocks slice 0]**
`abbey learn promote <id>` changes a stored record's confidence. The `os_control` confirm gate is about **OS execution**, not memory mutation, so `--confirm` is not required by the existing invariant. But `abbey memory invalidate` (`src/cli/args.rs:81-83`) is a comparable memory mutation and takes no confirm flag — so the consistent choice is **no `--confirm`**, and the mutation is recorded in the audit trail.
*Recommendation:* no `--confirm`; match `abbey memory invalidate`; record the promotion in the memory row's own provenance (append-only) and in the daemon audit if the daemon ever gains the verb.
*Fallback:* if the owner wants a confirm gate on all curation mutations, apply it to `invalidate` too, for consistency.

**Q8 — Does the trainer run in the daemon process or as a supervised child? [Blocks slice 3]**
A long CPU training run inside `abbeyd` risks the 1-second cooperative deadline and the bounded run budget that `src/claims/registry.rs` records for protocol v3. A supervised child process would need the `os_control` allowlist in the shipped edition.
*Recommendation:* run the trainer **in the daemon process, but with its own bounded budget** (fixed step count, fixed wall-clock ceiling) and with cancellation that is checked at step boundaries — a separate budget from the 1-second *tool-execution* deadline, which does not apply to a training operation. The daemon already owns cancellation (`abi_agent_runtime::CancellationToken` is already imported at `src/daemon/runtime_v3.rs:14`).
*Fallback:* a supervised child, but then it must go through the allowlist, which weakens the product's default posture — avoid.

**Q9 — What is the pre-registered evaluation bar? [Blocks slice 1's completeness and slice 3's `Current` promotion]**
The bar must be written **before** the numbers exist, and must be a fixture-outcome comparison against the base model on the held-out split — not an absolute quality number.
*Recommendation:* per persona, a small hand-authored fixture set (say 10–20 assertions on contract conformance: does the Abbey target start with `"Abbey: "`, does the Aviva target lead with the concrete answer, does the Abi target stay in orchestration register) plus one end-to-end behavioural fixture; the bar is "N of M fixtures change from `base_outcome` to `trained_outcome` in the expected direction, with no fixture regressing". Write it into `src/learn/eval.rs` with its own digest so a later change is detectable.
*Fallback:* if no bar is agreed, slice 1 ships the *dataset* honestly and slice 3 cannot promote to `Current` — a trainer whose output is never compared to its base is an anecdote, not evidence.

---

## Appendix A — Sibling-repo inventory (what already exists, and what it does *not*)

| Artifact | Where | What it is | Can Abbey use it? |
|---|---|---|---|
| `TrainingProfile{Abbey,Aviva,Abi,Quesar}` | `abi/crates/abi-ai/src/training_contracts.rs:17-27` | Closed persona enum, **including Quesar** | **Yes** — `abi-ai` is already a dependency (`Cargo.toml:73`). **Verify the pinned ABI revision carries it.** |
| `ApprovedRecord` / `SnapshotPolicy` / `SnapshotManifest` / `TeacherIdentity` / `Approval` / `Split` | `abi/crates/abi-ai/src/training_contracts.rs:47-78` | The complete admission + snapshot contract, with canonical bytes and a root digest | **Yes**, and strongly preferred over inventing a parallel schema |
| `build_snapshot` / `canonical_record_bytes` / `sha256_hex` | `abi/crates/abi-ai/src/training_contracts.rs:203-241+` | Deterministic snapshot construction; refuses duplicate ids with differing bytes; refuses teacher-authored evaluation records | **Yes** |
| `RunReceipt` / `RunInputs` / `RunState` / `TensorChange` / `EvaluationReceipt` / `ModelClassification` | `abi/crates/abi-ai/src/training_contracts/receipts.rs` | Run attestation: seed, configuration digest, initial tensor digest, before/after tensor digests, changed tensor count, evaluation id | **Yes** — this is the shape a "verified weight update" receipt should take |
| `TrainingRegistry` | `abi/crates/abi-sea/src/training_registry.rs:179+` | Private training registry over `abi_wdbx::durable::DurableStore`: artifact/snapshot/approval registration, `admit`, `revoke`, `eligibility`, `training_records` (which *refuses* a held-out snapshot rather than filtering under an unchanged committed root) | **Yes**, if the trainer lives in ABI (§4 option (ii)) |
| `abi-sea::learn_loop` | `abi/crates/abi-sea/src/learn_loop.rs:1-2` | *"SEA self-learning loop: evidence → augment → adaptive complete → save weights"* — persona-router weight adaptation, not LLM training | Reference only; not a training pipeline for a base model |
| `abi_ai::train_inspect` | `abi/crates/abi-ai/src/training.rs:286-313` | **Inspect-only.** Discloses `"; model weights unchanged"` (`:291-292`). `abi train` (`abi/crates/abi-cli/src/train.rs:11-47`) is this and nothing more | **No** — it is not a trainer. Citing it as one would be a false claim |
| `abi_nn` trainer | `abi/crates/abi-nn/src/lib.rs:1-5` | *"Miniature character-level neural-net demo trainer… not a production LLM"*; the crate's own description reads *"Demo-grade, not a production LLM."* (`abi/crates/abi-nn/Cargo.toml:8`). `abi nn` (`abi/crates/abi-cli/src/nn.rs:1-4`) is *"miniature character-level demo trainer… Demo-grade only"* | **No** — a char-level toy, not an LLM trainer |
| `abi-model-runtime` | `abi/crates/abi-model-runtime/src/provider.rs:1` | *"Candle-backed local provider and the first deterministic fixture architecture."* `FIXTURE_BIGRAM_ARCHITECTURE = "abi-bigram-v1"`. Has a `metal` feature (Abbey gates it at `Cargo.toml:126`) | **Yes** — the natural home for the trainer's numerics, and the existing precedent for exact-loaded-model execution |
| `abi-models` | `abi/crates/abi-models/src/{signed,license,registry,manifest,storage,verify}.rs` | Signed publisher manifests, license acceptance ledger, storage root, artifact verification | **Yes** — reuse for adapter manifests and base-model identity (slice 4) |
| `JobControlStore` / `MemoryControlStore` | `abi/crates/abi-worker/src/store.rs:54-61, 63-92` | Append-only job-admission/replay trait with an in-memory impl | Reference for slice 2's operation ledger shape; `src/runtime/store/model_operation.rs` is the closer local precedent |
| `quesar` crate | `lylx/crates/quesar/` (**different repo**) | f64 CPU reference: autodiff `Tape`, `AdamW`, `Decoder::loss_and_gradients`, `masked_next_token_loss`, `WarmupCosine`, `resource_estimate`, `training_preflight` | **Not directly.** Different repo, different gates, hard admission limits (width ≤ 32, layers ≤ 2, vocab ≤ 512). Use as a *numerical oracle reference* only |
| `personalities/*.json` | `lylx/personalities/{abbey,aviva}.json` | `status: "specified_behavior_not_trained"`, `foundation: "shared_quesar_designated_artifact_unavailable"`, `trained_adapter: null`, `evaluation: "simulated authority parity only; no model behavior qualification"` | Reference for the *shape* of a persona spec document. **Not** a persona Abbey can train |
| `lylx-data train` | `lylx/crates/lylx-data/src/main.rs:34-36` → `preflight::training_release` | Selects a proposed configuration and **stops at blocked preflight, without writing weights** | Reference for the *shape* of an honest training preflight |
| `wdbx` sibling | `../wdbx/crates/abi-wdbx`, `abi-compute` | Memory substrate + compute oracle; no trainer | Memory substrate only |

---

## Appendix B — Verification log

Verified directly in this session (commands run against the working tree at HEAD `ea2ae07`, dirty):

| Fact | How verified |
|---|---|
| `src/learn.rs` is 796 lines; `src/distill.rs` 686; `src/capture.rs` 127; `src/persona.rs` 85; `src/roles.rs` 306; `src/models.rs` 169 | `wc -l` |
| `src/learn.rs:751` maps `lora`/`finetune`/`fine-tune`/`fine_tune` to `crate::claims::refuse("lora")` | file read |
| `src/claims.rs:583-586` refusal text; `refuse` at `:573` | file read |
| `src/claims/registry.rs:684-695` `training-lora-pipeline` = Proposed, empty impl/tests, Required local | file read |
| `src/app_core/v3.rs` line numbers for `ReadTraining`/`ManageTraining` (40-41), `V3TrainingStart` (305), the six commands (350-356), grant mapping (394-400), the four events (697-700), `V3MetricQuery` (271), `V3EntityRecord` (467) | `grep -n` |
| `src/daemon/runtime_v3.rs:266` is the `_ => Err(capability_denied_failure())` catch-all; `:7` says training grants remain absent | `sed` + `grep -n` |
| `src/daemon/client.rs:644-646` has only event **name** strings for training | file read |
| `src/daemon/client/v3.rs` has **no** training method (grep of `pub fn` listing) | `grep -n "pub fn "` |
| `src/runtime/migrations.rs:6` `CURRENT_SCHEMA_VERSION = 10`; migrations 1..10 exist; next free = 11 | file read + `ls src/runtime/` |
| `src/runtime/migration_7_model_operations.sql` = the schema-v7 model operation ledger | file read |
| `src/runtime/store/model_operation.rs` = 357 lines, `MAX_MODEL_OPERATIONS = 4_096` at `:10` | `wc -l` + file read |
| `src/runtime/store/tool_approval.rs` (646) / `tool_execution.rs` (338) = the schema-v5/v6 crash-safe patterns | `wc -l` |
| `src/memory/mod.rs:31-53` `MemoryRecord`; `src/memory/sqlite.rs:29-52` the `memory` DDL; `:53-66` the `project` ALTER precedent; `:182` `invalidate` | file read + `grep -n` |
| `src/cli/args.rs:81-83` `MemoryCmd::Invalidate` | file read |
| `src/persona.rs:8-15,19-29,32-40`; `src/roles.rs:210-215,232-238` (aliases, not weights) | file read |
| `src/models.rs:8` `OLLAMA_DEFAULT_MODEL = "gemma4:26b-mlx"` | `grep -n` |
| `src/capture.rs:33-42` `capture_oneshot` (`no_resume = true` at `:35`) | file read |
| `src/distill.rs` line numbers for `TEACHER_CONFIDENCE` (26), `LOCAL_FIRST` (41), `DistillPair` (81-87), `ask_chain` (105), `ask_teacher` (137), `capture_oneshot` call (164), ollama preflight (143-161), `candidate_record` (173-199), `sft_line` (203-218), `export_sft` (384-393), `MAX_BATCH` (22) | file read |
| `src/claims/registry.rs` machine rules: Proposed ⇒ empty impl/tests + Required local; Current ⇒ no Required; Partial ⇒ Required present; refs must resolve to declared test fns | `tools/check_claims_sync.py:740-805, 89-155` |
| `desktop/src/surfaces.ts:103-116` the training stub with `capability_not_implemented`; `desktop/src/App.tsx:34`; `desktop/src/ipc/generated.ts:921-922`; `desktop/src-tauri/src/v3_ipc.rs:271,294` | file read + `grep -n` |
| `Cargo.toml:44-45` unsafe deny; `:72-86` sibling path deps; `:91` `candle-core` dev-dep; `:126` `accel` gates `abi-model-runtime/metal` | file read |
| `abi/crates/abi-ai/src/training_contracts.rs` — `TrainingProfile` (17-27), `Split` (29-33), `ApprovalKind` (47-59), `TeacherIdentity` (62-68), `ApprovedRecord`/`SnapshotPolicy`/`SnapshotRecord`/`SnapshotManifest` (69-78), `validate_record` (135-201), `canonical_record_bytes`/`sha256_hex` (203-241), `build_snapshot` (243+) | file read |
| `abi/crates/abi-ai/src/training_contracts/receipts.rs` — `RunState`, `RunInputs{seed, configuration_digest, initial_tensor_digest, trainer_principal, classification}`, `TensorChange`, `RunReceipt`, `EvaluationReceipt`, `ModelClassification` | file read |
| `abi/crates/abi-ai/src/training.rs:286-313` `train_inspect` discloses *"model weights unchanged"* (291-292); `:57-66` `TrainingConfig.profile` | file read |
| `abi/crates/abi-cli/src/train.rs:11-47` (`abi train` → inspect only); `abi/crates/abi-cli/src/nn.rs:1-4` (demo trainer) | file read |
| `abi/crates/abi-nn/src/train.rs:1-8` — *"Demo-grade, not a production LLM"* | file read |
| `abi/crates/abi-sea/src/training_registry.rs` — module doc (1-13), `new` (179), `register_permission` (308), `register_qualification` (329), `register_approval` (366), `admit` (511), `register_snapshot` (538), `revoke` (591), `eligibility` (611), `training_records` (660) | `grep -n` + file read |
| `abi/crates/abi-sea/src/learn_loop.rs:1-2` — *"evidence → augment → adaptive complete → save weights"* | file read |
| `abi/crates/abi-worker/src/store.rs:54-61` `JobControlStore` trait; `:70-92` `MemoryControlStore` | file read |
| `abi/crates/abi-model-runtime/src/provider.rs:1` — *"Candle-backed local provider"*; `FIXTURE_BIGRAM_ARCHITECTURE` | file read |
| `abi/crates/abi-ai/src/identity.rs:43-83` `ProfileContract` per persona; `:35` `PROFILE_LABELS`; `:85-92` routing weights | `grep -n` + file read |
| **Quesar is absent from Abbey except as a brand exclusion** (`docs/brand.md:13`, `docs/identity.md:88`, `docs/architecture.md:277`, `AGENTS.md:100`) and absent from `../abi` and `../wdbx`; it lives in the separate `donald/lylx` repository | repo-wide `grep -rn -i "quesar"` over `/Users/donald/sources/repos`, excluding `abbey/.claude/worktrees/` and `abbey/zig/` |
| `lylx/crates/quesar/IMPLEMENTATION.md:1-3` — *"not the designated largest Quesar … No designated-model weight updates were performed"* | file read |
| `lylx/crates/quesar/src/{optim.rs (204), model/backward.rs (552), training.rs (158), sampling.rs (339)}` | `wc -l` + `grep -n` |
| `lylx/personalities/{abbey,aviva}.json` — `status: "specified_behavior_not_trained"`, `trained_adapter: null` | file read |
| `lylx/crates/lylx-data/src/main.rs:34-36` → `preflight::training_release` (`lylx/crates/lylx-data/src/preflight.rs:190`) | file read |
| `lylx/docs/models/pretrained-candidate-research.md:10,47` and `lylx/docs/models/local-backend-audit.md:27` — pinned `Qwen/Qwen3.5-4B` @ `851bf6e806efd8d0a36b00ddf55e13ccb7b8cd0a`; *"trainer masking/resume defects"* | `grep -n` |
| Host: Apple M5 MacBook Pro, 24 GB RAM, macOS ARM64 | `system_profiler SPHardwareDataType`; `uname -m`; `sysctl hw.memsize` |

**Unverified / to verify before building:**

1. Whether `abi/crates/abi-ai/src/training_contracts.rs` exists **at the pinned ABI revision** that `.github/workflows/rust.yml` names. Must be checked with `tools/ci/verify-pinned-siblings.sh` before slice 1 depends on it. If it does not, slice 1 must land in ABI first and the pin must move before Abbey can cite it.
2. The exact `ABI_REVISION` / `WDBX_REVISION` values in `.github/workflows/rust.yml` as of the dirty working tree — read them before planning the sibling work.
3. Whether `abi-models` exposes a reusable *artifact* signing entry point for a non-model artifact (adapter) manifest, or whether that needs a small ABI-side addition.
4. The exact `qwen3_5` / MLX trainer defect list in `lylx/docs/models/local-backend-audit.md` — the summary line was read; the defect detail was not.
5. Whether any `WDBX` backend field-mapping code needs explicit updates for the new `MemoryRecord` fields beyond `src/memory/wdbx.rs`'s record round-trip (the WDBX record schema was not read in full).
6. `desktop/src-tauri/src/main.rs`'s current `generate_handler!` command count (37 per `AGENTS.md`) and the exact `verify-bundle.mjs` parsing constraint — read before editing.
