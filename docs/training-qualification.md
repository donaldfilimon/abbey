# Training qualification: supplied-result checks

Abbey's existing `abbey train` pipeline delegates to the ABI demo trainer.
Its corpus-reproduction score is not held-out assistant quality, and its
artifact is not an MLX adapter or an automatic update to the chat model.
The active acceptance requirements remain in
[the Completion Program](superpowers/plans/2026-10-22-abbey-completion.md#7-production-models-accelerators-adapters-and-neural-media).

## Offline report checker

Run the read-only checker against a supplied evaluation report:

```sh
python3 tools/check_training_acceptance.py --help
python3 tools/check_training_acceptance.py evaluation.json
```

Use `-` instead of a path to read JSON from standard input. The checker emits
JSON to standard output; it does not train, load a model, contact a provider,
write files, or promote an artifact. Exit codes are 0 for passing supplied
results, 1 for a valid report that fails gates, and 2 for invalid input.

The module docstring and `--help` describe the strict version-1 input schema.
Both runs must identify the same immutable base, dataset, test set, tokenizer,
task suite, scored token sequences, and scoring mask using SHA-256 digests.
Example ID sets must match. The candidate artifact digest must differ from
the base artifact digest. These are identity assertions from the report,
not independently verified file hashes or proof of changed model parameters.

The derived gates require:

- At least 95% valid candidate tool-call JSON.
- Candidate token-weighted held-out NLL no more than 1.02 times base NLL,
  with identical positive scored-token counts for each example.
- Every public-refusal case succeeds with no reported unauthorized effects.
- Nonempty coding, personality, and private-knowledge categories, each with
  candidate success count at least as high as the base count.

The checker rejects unknown/missing fields, duplicate JSON keys or example
IDs, mismatched identities, nonfinite/negative losses, booleans masquerading
as numbers, empty suites, and inputs exceeding its byte/record limits.
Threshold comparisons use exact rational arithmetic rather than rounded
display percentages. Category non-regression is only a minimum gate; equal
or uniformly poor results do not demonstrate improvement.

## Evidence still required

A passing report is **supplied-result bookkeeping**, not authenticated
evaluation evidence, promotion authority, or production qualification.
The checker does not independently run the task graders, validate generated
tool calls, observe side effects, authenticate provenance, or establish
statistical confidence or test-set adequacy.

Before any real adapter is qualified, independently establish dataset rights,
explicit file consent, redaction, deduplication and lineage-grouped
deterministic 80/10/10 splits; freeze the final test suite before tuning.
Record the trainer and dependency hashes, exact base/tokenizer identity,
generation settings, device, budgets, and actual adapter tensor changes.
Verify unchanged base weights, inference compatibility, cancellation, and
rollback. Compare the base and adapter through the same qualified route.
Native-platform and hardware acceptance remain separate obligations.

Gemma 12B inference qualification and separate E4B adapter training are distinct
targets: an E4B adapter cannot be assumed compatible with a 12B base. Do not
silently substitute a small engineering-pilot model for either target.

## Private knowledge

The selected policy is configurable knowledge sources with an explicitly
selected local folder as the default, retrieval rather than weight ingestion.
No folder has been authorized merely by selecting that default. Synthetic
fixtures are suitable for implementation tests; approved memory requires an
explicit approved scope, not wholesale conversation access.

Local retrieval is not proof of local generation. Private excerpts must not
be injected into a remote or unqualified route, silently carried through a
provider fallback, or included in training exports. The checker's
`private_knowledge` category describes supplied retrieval outcomes only.
Private fine-tuning requires a separate decision and consent.

## Checks

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tools/tests -p 'test_check_training_acceptance.py' -v
```

These tests use synthetic results and establish checker behavior, not model
quality. The authoritative repository merge gate remains `./check.sh`.
