# Abbey live learning and model family

Abbey's approved direction combines immediate, correctable project memory with
reviewed periodic adapter training. Memory personalizes current facts and
preferences; weight updates target durable coding and assistant skills. Private
facts remain retrieval data unless a separate private-training consent is given.
See [approved project memory](knowledge.md#explicit-approved-project-memory).

The optional tool is `python tools/abbey_model_family.py --help`. It sits beside
the existing `abbey train` demo pipeline; it does not change its artifact format,
trainer grammar or promotion behavior. The new CLI does not read personal memory
or run a schedule by itself. Run explicit reviewed batches at chosen intervals.

## What WDBX does

The existing WDBX backend stores approved observations and model-lineage receipts.
It does not turn arbitrary architectures into a common set of weights. Sharing
memory between local models preserves their own tokenizers and prompt templates.
Compatible base/adapter pairs can be fused into one model directory. Transfer
between different architectures uses approved teacher examples and student
training, not arithmetic averaging of incompatible tensors.

The implementation supports one exact-base LoRA adapter per training run, fused
exports, and 4/8-bit exports. An approved teacher's answers can enter the ordinary
curation format below with provenance and one lineage per related task; training
a smaller selected base on that dataset is a separate student run. Teacher
collection and multi-adapter merging are not automated. No new WDBX weight format
or universal compiler is introduced.

## Local runtime and baseline

The observed machine is an Apple M5 with 24 GiB unified memory. Free disk is
variable; inventory reports file presence, never sufficient training capacity.
The initial cache inspection found a small task-specific Qwen3 checkpoint and
an embedding checkpoint with weights/tokenizers; a GPT-2 config and a Llama cache
reference were incomplete. Neither cache names nor Abbey model aliases prove
assistant quality, checkpoint completeness, rights, or production availability.

The training worker is Apple/MLX-specific; memory retains the existing Rust
platform boundaries. The developer pipeline uses an isolated Python 3.12 env:

```sh
uv venv --python 3.12 .work/model-family-venv
uv pip install --python .work/model-family-venv/bin/python --require-hashes -r requirements/model-family.lock
python tools/abbey_model_family.py inventory
```

`inventory` accepts explicit checkpoint folders or Hugging Face cache roots.
It performs no downloads and never opens a memory store. Local checkpoint paths
in run evidence are runtime bindings, not tracked machine-specific dependencies.
Pin exact downloaded revisions, review license/usage conditions and establish
resource headroom before selecting one. Never silently substitute another base.

Research shortlist checked 2026-10-25, **not a measured ranking**:

- [Gemma 4 E4B](https://huggingface.co/google/gemma-4-E4B-it): retain the existing
  Abbey E4B training target; qualify its exact text runtime/tokenizer first.
- [Qwen3.5 4B](https://huggingface.co/Qwen/Qwen3.5-4B) and
  [Qwen3.5 9B](https://huggingface.co/Qwen/Qwen3.5-9B): reference candidates to
  compare through identical local evaluations and resource measurements.
- [Qwen3.5 0.8B](https://huggingface.co/Qwen/Qwen3.5-0.8B): a possible student
  candidate; a smaller parameter count does not establish sufficient quality.

These official cards currently identify Apache 2.0 licensing. A converted
checkpoint's own provenance, complete files, modalities and license still need
review. The pinned MLX-LM distribution contains Qwen3.5/Gemma4 model modules;
module presence alone does not qualify their actual checkpoints or training.
Existing Gemma 12B inference and E4B adapter targets remain separate contracts.
No large candidate was downloaded or ranked in this implementation run.

## Datasets, consent and frozen evaluation

Every writing/training operation requires a file with the exact line
`abbey-train-consent: v1`. Private examples additionally require
`abbey-private-training-consent: v1` and `prepare --private`. A memory-consent file
is a different capability; it cannot authorize training. This repository includes
no consent granting access to real personal records.

Input JSONL has exactly these fields:

```json
{"id":"example-1","lineage":"task-42","prompt":"Question","response":"Reviewed answer","provenance":"approved source and rights reference","approved":true,"private":false}
```

Only approved, nonblank, bounded examples are accepted. Deterministic secret/path
shape redaction runs before deduplication. It is defense in depth: human corpus
review and rights verification remain necessary, and URLs/unrecognized secret
shapes can contain sensitive data. Exact normalized duplicate content is refused.
Repeated normalized prompts across different lineages are refused. The curator
must group semantically related tasks, revisions and teacher variants under the
same lineage: semantic relatedness is not inferred automatically.

Lineage hashes assign 80/10/10 buckets to train/validation/test. Small datasets
need not have exact percentage counts; all three splits must be nonempty. Files,
counts and source identity are sealed in a versioned manifest. No existing output
is overwritten. Training uses a separate verified run snapshot, checks it and
its source again afterwards, and verifies the heldout copy against the test split.

```sh
python tools/abbey_model_family.py prepare --source .work/approved.jsonl --output .work/dataset-v1 --consent .work/train-consent.txt
python tools/abbey_model_family.py freeze-suite --source .work/reviewed-suite.jsonl --output .work/suite-v1.json --consent .work/train-consent.txt
```

The suite has `id`, `category`, `prompt`, `expected` strings. Required categories:
`coding`, `tool_use`, `everyday`, `factual`, `personality`, `privacy`,
`public_refusal`, `private_knowledge`. Tool expectations are JSON objects.
[The seed suite](model-family/suite.seed.jsonl) is an engineering example, not a
sufficient personal-assistant benchmark. Freeze an independently reviewed suite
before tuning; changing it requires a new run. Exact training/suite prompt
overlap is refused. The heldout test set is separate from the fixed task suite.

## Train, compare and export

```sh
python tools/abbey_model_family.py benchmark --base .work/selected-base --suite .work/suite-v1.json --heldout .work/dataset-v1/test.jsonl --output .work/base-benchmark --consent .work/train-consent.txt --python .work/model-family-venv/bin/python
python tools/abbey_model_family.py train --base .work/selected-base --dataset .work/dataset-v1 --suite .work/suite-v1.json --output .work/run-v1 --license REVIEWED_LICENSE_ID --iters 100 --consent .work/train-consent.txt --python .work/model-family-venv/bin/python
python tools/abbey_model_family.py evaluate --run .work/run-v1 --suite .work/suite-v1.json --consent .work/train-consent.txt --python .work/model-family-venv/bin/python
python tools/abbey_model_family.py export --run .work/run-v1 --output .work/abbey-fused --consent .work/train-consent.txt --python .work/model-family-venv/bin/python
python tools/abbey_model_family.py export --run .work/run-v1 --output .work/abbey-q4 --bits 4 --consent .work/train-consent.txt --python .work/model-family-venv/bin/python
```

The initial engineering recipe trains rank-8 LoRA on the final two layers,
batch size one, learning rate 1e-5, seed 17, and a 256-token sequence envelope.
It is deliberately a bounded pilot, not a qualified long-context coding recipe.
Workers run offline with remote tokenizer code disabled. Iterations and timeouts
are bounded; interruption kills the worker process group. Failure preserves
partial diagnostics but creates no successful promotable training receipt.
Record dependencies, worker hash, actual initial/final adapter tensor hashes,
finite changed tensors and full unchanged base/tokenizer identities. Verification
reads the real safetensor bytes rather than accepting a changed-count assertion.

Evaluation uses the same frozen base-tokenizer sequences and response-only mask
for both variants. Overlong heldout responses are refused, not silently removed.
Generation is greedy, max 128 tokens. No generated tool or code is executed.
Reports record token-weighted NLL, exact-answer category scores, elapsed generation
time, token throughput and MLX peak allocation (not whole-machine resident memory).
Observed runtime, quality and hardware qualification remain separate dimensions.

The existing [training acceptance checker](training-qualification.md) is reused:
95% valid tool JSON, candidate NLL <= 1.02x base, all public refusals and no tool
execution, and coding/personality/private-knowledge non-regression. The additional
family gate requires every category non-regression, all privacy cases and at
least one task improvement. Tool answers must also match their expected object.
Exact-answer grading measures only the chosen suite. It does not establish broad
human-assistance quality, robust code execution or private retrieval effectiveness.
The private-knowledge cases here test supplied contexts; qualify the real memory
path separately. Checksums are integrity bookkeeping, not authenticated attestations.

Exports are explicitly **unqualified** until benchmarked through their own runtime.
Use `benchmark --base .work/abbey-q4 ...` with the frozen suite to compare each
size's quality and performance. A fused export is one model directory, normally
containing weight/tokenizer/config files; it is not one executable binary.
Quantization reduces precision, not parameter count. A smaller student is its own
training/evaluation run. The CLI does not promote exports by inheriting the
adapter's score, and does not claim generic GGUF conversion.

## Release records, WDBX and rollback

```sh
python tools/abbey_model_family.py promote --run .work/run-v1 --registry .work/releases --consent .work/train-consent.txt
python tools/abbey_model_family.py rollback --registry .work/releases --consent .work/train-consent.txt
python tools/abbey_model_family.py record-receipt --run .work/run-v1 --abbey .work/live-models-target/debug/abbey --project . --owner operator --backend wdbx --consent .work/memory-consent.txt
```

Promotion rechecks base/tokenizer, adapter, tensors, dataset, suite, measurements
and grades before publishing an atomic local registry pointer. It retains all
versioned records and the previous pointer for rollback. Registry operations
serialize through an exclusive lock. A crash can leave a lock; inspect the PID
and operation state before manually recovering it. Rollback verifies retained
bytes and grades again. Neither operation changes Abbey's global chat model or
claims production qualification. Deployment/runtime selection is a separate step.

`record-receipt` explicitly records compact content hashes and status through
Abbey's exact WDBX backend, requiring a WDBX-enabled Abbey binary and separate
memory consent. It does not upload weights or ingest private memory into training.
The local run folder retains detailed tensor and evaluation evidence.

## Verification and remaining release work

```sh
python -m unittest discover -s tools/tests -p 'test_model_family.py' -v
ABBEY_TEST_MLX=1 .work/model-family-venv/bin/python -m unittest discover -s tools/tests -p 'test_model_family_mlx.py' -v
./check.sh
```

The opt-in MLX test constructs a tiny random synthetic Qwen2 model and tokenizer;
it establishes real tensor updates, fixed-base inference, evaluation refusal for
poor quality, fusion and loadable 4/8-bit output. It cannot substitute for the
Gemma E4B training or Gemma 12B inference targets. No trained Abbey reference
release or smaller student is qualified by that test.

Before release: approve and freeze representative coding/everyday/privacy suites
and a rights-cleared corpus; choose actual candidate revisions through local
benchmarks; establish hardware/time budgets; run the reference training; review
real assistant/coding outcomes; independently qualify exported variants and the
intended native platforms. Personal data, paid compute, unattended training,
physical memory erasure and product retirement remain outside this code change.
