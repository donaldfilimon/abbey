# Corpus README command verification (correctness plan Task 9, `corpus-command`)

**Result: the documented command works, but only from the ABI repository root.** The command `python3 tools/abbey_contracts.py verify contracts/abbey` exits 0 when run from an ABI checkout at the vendored revision and at the CI-pinned revision. Run literally from the Abbey root, it exits 2 because Abbey has no `tools/abbey_contracts.py`. The ABI tool also fails (exit 1) when pointed at Abbey's `contracts/abbey`, because Abbey vendors the corpus one level down in `contracts/abbey/corpus/`. Pointed at `contracts/abbey/corpus`, it passes with the digest pinned in Abbey's lock file. This is plan Step 2: the defect is the wording of the vendored README, not a broken tool.

This record is evidence only. No source, vendored corpus byte, ledger or status changed. `../abi` was not written to: each revision was read with `git archive` into a session scratch directory, not from the sibling's working tree.

## Inputs

| Item | Value |
|---|---|
| Vendored revision (`contracts/abbey/abbey-contracts.lock.json` `source_revision`) | `63e6d6a79d0b8745a652803887d07665245ddb39` (ABI, 2026-08-24, "feat(abbey): define v1 federation contract kernel (#814)") |
| CI sibling pin (`.github/workflows/rust.yml:23` `ABI_REVISION`) | `63d2bc0a393c3c3837eadab0d18bc2fc1fff5c78` (ABI, 2026-10-26) |
| Lock `aggregate_digest` | `3ffd487bdc497b7ce54b8c29978a3686dcbffdb66a85957a0ee4f99ba576cdfd` |
| ABI working-tree HEAD (not used) | `555e18abbd0b68317239212fa29deecbb7986020` |
| Abbey base | `96a5dc3` (`main`) |
| Python | `Python 3.15.2` (Homebrew, macOS) |

`git diff --stat 63e6d6a 63d2bc0 -- tools/abbey_contracts.py contracts/abbey` is empty, so the tool and corpus are identical at both revisions. `diff -rq` between ABI `contracts/abbey` (either revision) and Abbey `contracts/abbey/corpus` printed no differences.

## Commands and results

Extraction, for each `REV` above:

```sh
mkdir -p "$SCRATCH/abi-$REV"
git -C ../abi archive "$REV" tools/abbey_contracts.py contracts/abbey | tar -x -C "$SCRATCH/abi-$REV"
```

| # | Command | Run from | Exit | Output |
|---|---|---|---|---|
| 1a | `PYTHONDONTWRITEBYTECODE=1 python3 -I tools/abbey_contracts.py verify contracts/abbey` | `abi-63e6d6a…` extract | 0 | stdout: `abbey-contracts: verified 113 artifacts (133200 bytes), digest=3ffd487b…76cdfd`; stderr empty |
| 1b | same | `abi-63d2bc0…` extract | 0 | identical to 1a |
| 2a | `PYTHONDONTWRITEBYTECODE=1 python3 -I "$SCRATCH/abi-63e6d6a…/tools/abbey_contracts.py" verify <abbey>/contracts/abbey` | scratch dir | 1 | stderr: `abbey-contracts: artifact_unreadable: manifest.json` |
| 2b | same, with the `63d2bc0…` tool | scratch dir | 1 | identical to 2a |
| 2c | the `63e6d6a…` tool, `verify <abbey>/contracts/abbey/corpus` | scratch dir | 0 | stdout as in 1a; same digest as the lock file |
| 2d | the `63d2bc0…` tool, `verify <abbey>/contracts/abbey/corpus` | scratch dir | 0 | stdout as in 1a |
| 3 | `python3 tools/abbey_contracts.py verify contracts/abbey` (the README text, literally) | Abbey root | 2 | stderr: `can't open file '…/tools/abbey_contracts.py': [Errno 2] No such file or directory` |

Rows 2a/2b fail because Abbey's `contracts/abbey/` holds only `abbey-contracts.lock.json` and `corpus/`; `manifest.json` lives at `contracts/abbey/corpus/manifest.json`. The README's path argument is also relative to the ABI layout, not to Abbey's.

## Conclusion

- Plan Step 2 applies (row 1 exits 0). The vendored `contracts/abbey/corpus/README.md:32-42` gives an ABI-root-relative command without saying so, and both its script path and its corpus path are wrong when read from Abbey.
- Sibling request (completion index Task 2 (b)): the upstream README should state that the command runs from the ABI repository root (and that a vendored copy is verified by pointing the ABI tool at the vendored corpus directory). That request is a Donald step and is not filed by this record.
- Abbey does not patch vendored bytes. The README is digest-covered, so a local edit would break `bundled_corpus_qualifies_exact_bytes_and_every_declared_fixture`. Any Abbey-side change waits for the re-vendor in correctness plan Task 11.
- `corpus-command` stays `open` in `tasks/completion.json` until Task 11 re-vendors a corpus whose README names an executable command. This record changes no ledger entry.
