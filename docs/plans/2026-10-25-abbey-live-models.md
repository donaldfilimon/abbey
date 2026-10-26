# Abbey Live Memory and Model Family Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Deliver explicit approved live memory and an offline-first real model training/release pipeline.

**Architecture:** Extend Abbey MemoryStore and knowledge dispatch for approved project-scoped memory. Use an isolated Python MLX process for local adapter training, immutable lineage-grouped datasets and release receipts; preserve the legacy demo trainer.

**Tech Stack:** Rust, existing SQLite/WDBX MemoryStore, Python standard library, optional pinned MLX-LM.

**Spec:** User-approved combined design in this chat, reproduced in docs/model-family.md.

## Global Constraints
- Preserve pre-existing dirty work; no personal data ingestion, paid compute, or automatic promotion.
- Private memory stays out of remote generation and training unless explicitly selected and consented.
- WDBX is storage/provenance; no arbitrary cross-architecture weight conversion claim.
- Use existing Rust 1.99.1 gates and exact backend selection. Never silently substitute a model.

## Review Focus
- Remote/fallback routes must not receive private memory.
- Superseded, forgotten, expired or foreign-project memories must not be recalled.
- Related training examples must never cross split boundaries.
- Tampered base/tokenizer/adapter artifacts must not be promoted.
- Invalid evaluation evidence must preserve the current model and rollback history.

### Task 1: Approved immediate memory
**Files:** src/knowledge.rs, src/knowledge/live.rs, src/cli.rs, tests/live_memory.rs; agent generation integration as needed.
**Interfaces:** Extend knowledge subcommands for explicit remember/correct/forget/export and local recall. Use existing MemoryStore and exact configured backend, no new competing store.
- [ ] Write restart, correction, deletion exclusion, project scope and remote-route refusal tests; observe failure.
- [ ] Implement bounded metadata-rich explicit memory and scoped recall with retention and consent boundaries.
- [ ] Connect an explicit local-only generation path with no fallback and provenance-bearing observations.
- [ ] Run focused default and WDBX tests; document logical deletion versus physical erasure.

### Task 2: Real model-family tooling
**Files:** tools/abbey_model_family.py, tools/model_family/, tools/tests/test_model_family.py, docs/model-family.md, requirements/model-family.txt.
**Interfaces:** CLI inventory, prepare, train, evaluate, export, promote, rollback; JSON reports and immutable release identity.
- [ ] Write negative tests for split leakage, provenance/consent, artifact mutation and release regression before implementation.
- [ ] Implement local-only inventory and immutable dataset snapshots with explicit lineage-grouped 80/10/10 splits.
- [ ] Implement pinned optional MLX-LM adapter runner with bounded process execution, base/adapter hashes and cancellation cleanup.
- [ ] Implement evaluation execution and separately gated versioned export, promotion and rollback with checksummed receipts.
- [ ] Run standard-library tests, actual available local model feasibility check and document measured qualification limits.

### Task 3: Qualification and delivery
- [ ] Independent review of each subsystem and integrated changes; fix actionable findings.
- [ ] Run focused Rust feature cohorts, Python suites, formatting and repository ./check.sh; record every unavailable gate.
- [ ] Record hardware/checkpoint baseline without private identifiers, and distinguish infrastructure from production trained-model evidence.

## Execution rulings
- Use isolated manual task worktree with the pre-existing dirty tree copied as baseline; preserve canonical checkout.
- No base-model download or long training until selection and resource checks pass; actual small local fixtures may qualify tensor plumbing only.
- Existing consent-file contract remains required; approval of this code work does not authorize ingestion of private memories.

## Approved scope amendment: entire runtime in Rust

User explicitly selected native Rust for the entire pipeline and numerical backend. Replace the newly added Python runtime tooling with a standalone `abbey-learning` Rust crate and `abbey train family` integration. Existing repository Python verification tools remain test-only independent oracles. Native numerical execution uses pinned Candle operators with explicit CPU/optional Metal device selection, deterministic LoRA, native tokenizer and Safetensors/GGUF export. Repository-learning sources cover approved Abbey/ABI/WDBX docs with private classification, section lineage and explicit source manifests; no personal memory databases or evaluation fixtures are implicitly ingested. Initial backend architecture support and reference-model qualification must be stated precisely. No Python runtime fallback is permitted.
