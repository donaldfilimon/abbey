# 2026-10-22 — `abbey train` live run against the sibling trainer

A verification receipt for the bounded self-training pipeline (`src/train/`):
what was run on this host, what that run proves, and what it does not prove.

The tree was **in flight** while this was written: `src/train/` is new and
uncommitted, and other work was landing in the same checkout. Every citation
below is a snapshot read at 2026-10-22 07:2x EDT. Re-read the cited lines if
the module is still being changed.

## The metric changed once, and the change is recorded here

The first live run (below, **superseded**) scored an artifact by comparing a
seeded greedy sample with the pair's **reference response**. That metric could
not be positive: the sibling's sampler continues the model's **own corpus** from
the first occurrence of the seed character (`../abi/crates/abi-nn/src/sample_inc.rs`,
`SampleState::start`), so the sample and the reference response are two
unrelated texts. The live score was exactly `0.0000`, and the promotion path
could never be exercised.

The metric is now **greedy self-reproduction fidelity of the artifact's own
training corpus, probed with seeds drawn from the held-out split**
(`src/train/evaluate.rs`):

- Abbey reconstructs the trainer's corpus text from the dataset file it wrote
  (`<run>/dataset.jsonl`), using the sibling's own
  `extract_corpus_from_jsonl` rule (`src/train/trainer.rs::extract_corpus`).
- For a probe seed character, the expectation is the corpus text the sampler
  was asked to continue — from the seed's first occurrence **plus the model's
  context window**, read from the artifact's own checkpoint (`seq_len`) and
  falling back to the sibling's documented default of 2.
- The probe score is `lcp(sample, expected) / expected.len()`; the artifact's
  score is the mean over the probes. The probes are the first characters of the
  held-out pairs' prompts, deduplicated and capped.
- The baseline is scored with the same probes against the same corpus text, so
  candidate and baseline are directly comparable.
- A **leak check** fails closed: if a held-out pair's text appears in the
  training corpus, the run is recorded as `validation_leak` and nothing is
  promoted.

This is a **training-fidelity** measure, not a held-out quality or
generalization measure. The delegated trainer's public surface exposes no loss
on unseen text, so a held-out quality measure is not available and is not
claimed. The CLI says so on every run: `metric: corpus reproduction (not
held-out quality)`.

## What was run

The pipeline is a curation-to-artifact path. It seals a corpus, delegates the
weight update across a process boundary to the sibling `abi` trainer, scores
the artifact it produced, and records every step in an append-only ledger.
Every run below used a **scratch state directory** under `target/`, so nothing
touched the user's real state.

| Fact | Value |
| --- | --- |
| Host | macOS ARM64 (`aarch64-apple-darwin`), Rust 1.99.1 |
| Binary | `target/debug/abbey` (working tree, uncommitted) |
| Trainer | `/Users/donald/sources/repos/abi/target/release/abi` — the sibling's release build, resolved through `ABBEY_ABI_BIN` |
| Trainer grammar | `abi nn train --jsonl <path> --field text --out <path>` (`src/train/trainer.rs`; the sibling's side is `../abi/crates/abi-cli/src/nn.rs`) |
| Corpus source (cycle) | `memory` — 7 curated distill pairs, 6 train / 1 validation |
| Corpus source (leak) | `memory` — 2 curated distill pairs with **identical** text, one per split |
| Corpus source (spec run) | `specs` — 60 claims + 27 goals from `tasks/goals.md` → 87 pairs, 70 train / 17 validation |

## 1. `abbey train plan --source memory` — read-only preview

```
abbey train plan — read-only preview (no file is written)
source:        memory
corpus:        7 pair(s)
redacted:      0 pair(s)
split:         abbey.train.split.v1 · 80/20 train/validation
train pairs:   6
validation:    1
dataset:       sha256:9bed838d621fbb0f2941970485b2827e406a852ba7e4a0ed62e24ed570d448b5
skipped:       1 candidate(s) — not_a_prompt_response_pair
consent:       not supplied — no dataset will be written and no trainer will run
trainer:       resolved: /Users/donald/sources/repos/abi/target/release/abi
metric:        corpus reproduction, probed from the held-out split
```

The plan wrote nothing, and it refused to resolve a trainer until one was
named — the consent gate and the trainer resolution are both explicit. The one
skipped candidate is the `abbey learn train` record (free text, not a
prompt/response pair); see *The curation gap is closed* below.

## 2. The promotion cycle — run 1, no baseline

```
abbey train run — run 20261022T112219Z-8cfa2838
source:      memory
dataset:     sha256:9bed838d621fbb0f2941970485b2827e406a852ba7e4a0ed62e24ed570d448b5
consent:     sha256:58bd0c289c8fb09c6107d96f4d52a5ff5626f85418c1e5a0e81bb82ce0f6344c
artifact:    …/train/20261022T112219Z-8cfa2838/artifact.json
digest:      sha256:8524365c6268085e4a349913e34fb949131c2ac771607764b449cf638fb0473d
reproduction: 0.0156 (1 probe(s))
baseline:    none
metric:      corpus reproduction (not held-out quality)
leak:        none
decision:    promoted
current:     …/train/current.json
```

This is the real sibling trainer: `abi nn train` read the sealed JSONL and
wrote a demo character-level checkpoint (a `vocab`/weights JSON — the sibling's
`abi nn` output, not a production model format). The score is **positive**,
and the artifact became the current one.

## 3. The promotion cycle — run 2, with a baseline

```
abbey train run — run 20261022T112237Z-ff84f829
artifact:    …/train/20261022T112237Z-ff84f829/artifact.json
digest:      sha256:8524365c6268085e4a349913e34fb949131c2ac771607764b449cf638fb0473d
reproduction: 0.0156 (1 probe(s))
baseline:    0.0156
leak:        none
decision:    promoted
```

The candidate matched the baseline exactly, so the non-regression rule kept it
(no regression, and it cleared the floor).

## 4. The rollback cycle

```
abbey train rollback — restored run 20261022T112219Z-8cfa2838
artifact:    …/train/20261022T112219Z-8cfa2838/artifact.json
digest:      sha256:8524365c6268085e4a349913e34fb949131c2ac771607764b449cf638fb0473d
current:     …/train/current.json
note:        every artifact is retained, so this rollback is itself reversible
```

`abbey train status` afterwards:

```
rows:        3
digest:      0 row(s) whose digest does not match
operations:  prepare=0 run=2 rollback=1
current:     20261022T112219Z-8cfa2838
reproduction: 0.0156
artifacts:   2 retained run artifact(s)
```

## 5. `abbey train evaluate` — read-only re-score

```
abbey train evaluate — read-only (no file is written)
current:     20261022T112219Z-8cfa2838
artifact:    …/train/20261022T112219Z-8cfa2838/artifact.json
reproduction: 0.0156 (1 probe(s))
baseline:    none
metric:      corpus reproduction (not held-out quality)
leak:        none
decision:    promoted
```

## 6. The leak check fails closed, live

Two curated pairs with **identical** text, one landing in each split
(`memory:leak-0`, bucket 54 → train; `memory:leak-10`, bucket 82 →
validation):

```
abbey train run — run 20261022T120920Z-e8031a68
source:      memory
dataset:     sha256:1dfbb18e6cbd3cc950f5b8ab3369296e13d37cd2f0a91a54a4e84120a73b56b1
artifact:    …/train/20261022T120920Z-e8031a68/artifact.json
digest:      sha256:96d34d86db11b464f17f9e72563e94bbe5fbc05a8e49e848969462ddfdcb1d7e
reproduction: 0.0156 (1 probe(s))
baseline:    none
leak:        1 validation pair(s) present in the training corpus
decision:    validation leak
current:     unchanged (the previous artifact is kept)
```

The held-out pair's text was in the training corpus, so the run refused to
promote and said exactly how many pairs leaked.

## 7. The `specs` source, under the new metric

```
abbey train run — run 20261022T112050Z-bf0f2192
source:      specs
dataset:     sha256:13315e9c7d2646363d46f56345a6651321f8ca40a7a42d9cb7e5cf4a17775cca
artifact:    …/train/20261022T112050Z-bf0f2192/artifact.json
digest:      sha256:8b584f553816f9f868c0251cfd845a47bebae03af751e48e0a409955099f8bd4
reproduction: 0.0000 (1 probe(s))
baseline:    none
leak:        none
decision:    trained, not promoted
current:     unchanged (the previous artifact is kept)
```

The real corpus of Abbey's own claim and goal text is **not learnable by the
demo trainer**: the score is `0.0000` and the artifact is correctly not
promoted. This is the honest result for that pairing.

## The curation gap is closed

The first receipt recorded that the `memory` corpus source was empty by
construction: a record had to be a live `train_candidate`, carry provenance,
carry confidence ≥ 0.9, **and** deserialize into the `DistillPair` shape — and
only `abbey learn distill` writes that shape, at confidence `0.6`, below the
bar. `abbey learn train` writes a curated record at `0.95` but with free text.

The bar is now **one number**, declared once in the learn module
(`src/learn.rs::READY_CONFIDENCE`) and shared with the corpus builder
(`src/train/dataset.rs` re-exports it), and `abbey learn promote <id>` is the
human step that lifts a distilled pair over it:

```
$ abbey learn promote <id>
promoted <id> to confidence 0.90 (curated)
```

The promotion raises the confidence, adds a `promoted` tag, and appends the
promotion to the existing provenance, so the record still names its teacher.
The payload — the pair itself — is not rewritten, and nothing is deleted.
`abbey train --source memory` is therefore reachable on a non-empty corpus, and
the live cycle above ran on exactly that corpus.

## What this proves

- The pipeline runs end to end on this host: seal → external trainer → artifact
  → scoring → recorded decision → ledger.
- **A live promotion-and-rollback cycle happened against the real sibling
  trainer**: run 1 promoted, run 2 promoted against the baseline, the rollback
  restored the earlier run, and `abbey train evaluate` re-scored the current
  artifact read-only. The ledger holds all three operations and
  `abbey train status` re-verified every row digest (`0 row(s) whose digest
  does not match`).
- **The leak check refused, live**, and named the count.
- The weight update happened **in the sibling trainer**, across a process
  boundary, with a fixed argv grammar, a hard timeout, and a bounded captured
  output (`src/train/trainer.rs`, `src/train/process.rs`). Abbey owns no
  trainer, optimizer, loss, or weights.
- The consent gate is a real gate: without `--consent <path>` the plan writes
  nothing and `prepare`/`run` exit 2.
- The decision rule is honest in both directions: a `0.0000` score did **not**
  become the current artifact, a positive score **did**, and a leaked holdout
  did **not**.
- The ledger is append-only and self-digesting, and every artifact is
  retained, so a rollback is itself reversible.
- The curation bar is one number shared by the review surface and the corpus
  builder, and a human can lift a distilled pair over it.

## What this does **not** prove

- **No model quality is claimed.** The trainer is the sibling's demo-grade
  character-level model. The `0.0156` score is a *self-reproduction* score on
  a deliberately repetitive seven-pair corpus; it is not a quality,
  generalization, or held-out score, and it says nothing about a production
  model.
- **The `specs` corpus is not learnable by this trainer.** Abbey's own claim
  and goal text scores `0.0000` and is correctly not promoted. The delegated
  trainer has a two-character context window, so over prose it predicts
  spaces and reproduces nothing. Proving the pipeline on realistic text needs
  a delegated trainer with a longer context — the sibling's business, not
  Abbey's.
- **A held-out quality measure is not available.** The delegated trainer
  exposes no loss on unseen text, so the metric measures training fidelity
  against the corpus the artifact was trained on, with the held-out split
  supplying the probe seeds. It is not a generalization measure.
- **The baseline is scored against the current run's corpus**, so a baseline
  trained on an older corpus is penalised. Within one run the comparison is
  like-for-like; across runs it is not.
- **The leak check fails closed**, so two curated records with identical
  prompt/response text block a run. That is conservative and may prove common.
- **This is not the LoRA/fine-tuning pipeline.** No adapter is produced, no
  base model is bound, and the promotion bar is a character-level reproduction
  score. `abbey learn lora` still refuses.
- **The run wrote only into the scratch state directory** under `target/`. No
  user state, no config, and no credential was read or written.
- **No Quesar corpus.** Quesar has no code, crate, config, or edition in this
  repository, so nothing of Quesar's own is trained. The `specs` source is
  Abbey's own claim registry and goal ledger.

## Superseded evidence (kept)

The first live run, under the old response-comparison metric:

```
abbey train run — run 20261022T091255Z-2c09a07b
artifact:    …/train/20261022T091255Z-2c09a07b/artifact.json
digest:      sha256:46a9fb8994714117f7cddcf63e0c052f06cbe4bdcadeba8d2a60b035ba2a3ea5
score:       0.0000 (16 pair(s) evaluated)
baseline:    none
decision:    trained, not promoted
```

Its ledger rows read `"score":0.0` and carry no `metric` field. Old rows are
**not** retro-labelled: a row whose measurement was a different quantity now
reads `reproduction_score: null` rather than pretending to be a reproduction
score.

## Reproduce

The promotion-and-rollback cycle, on a learnable corpus:

```sh
export ABBEY_STATE_DIR="$PWD/target/train-live3/state"
export ABBEY_ABI_BIN=/Users/donald/sources/repos/abi/target/release/abi
export ABBEY_MEMORY_BACKEND=sqlite
printf 'abbey-train-consent: v1\n' > target/train-live3/consent.txt
# seed the store, then insert seven curated distill pairs (prompt/response
# payload, provenance, confidence 0.95) — see the pair list in the run above
./target/debug/abbey train plan    --source memory
./target/debug/abbey train run     --source memory --consent target/train-live3/consent.txt
./target/debug/abbey train run     --source memory --consent target/train-live3/consent.txt
./target/debug/abbey train rollback
./target/debug/abbey train status
./target/debug/abbey train evaluate
```

The leak refusal, on two pairs with identical text split across the two
halves:

```sh
export ABBEY_STATE_DIR="$PWD/target/train-live4/state"
# two records, ids memory:leak-0 (train) and memory:leak-10 (validation),
# with the same prompt/response payload
./target/debug/abbey train run --source memory --consent target/train-live4/consent.txt
```

The `specs` run, whose dataset digest is a pure function of the registry and
the goal ledger:

```sh
export ABBEY_STATE_DIR="$PWD/target/train-live2/state"
export ABBEY_ABI_BIN=/Users/donald/sources/repos/abi/target/release/abi
printf 'abbey-train-consent: v1\n' > target/train-live2/consent.txt
./target/debug/abbey train plan --source specs
./target/debug/abbey train run  --source specs --consent target/train-live2/consent.txt
```

The `specs` dataset digest is stable for this tree:
`sha256:13315e9c7d2646363d46f56345a6651321f8ca40a7a42d9cb7e5cf4a17775cca`.
It changes when a claim or a goal changes — that is the point of the digest.
