# Local knowledge preview

`abbey knowledge status` reports configuration **without inspecting a source**.
`abbey knowledge search 'query' --limit 5` explicitly reads the selected source
and prints bounded JSON excerpts to stdout. This is CLI preview only, **not
chat-RAG**: no automatic session injection, remote generation, embeddings,
transcript logging, export, training, fine-tuning, or weight updates are connected.
Private contents remain retrieval data, not training weights. Redirecting or
sharing stdout is the caller's responsibility.

In the active edition's `config.toml`:

```toml
[knowledge]
source = "folder" # default
folder = "/absolute/selected/notes"
```

No directory is inferred, including when `folder` is missing. There is no home
scan, source fallback, source override environment variable, or implicit store
creation. Invalid configuration/searches fail rather than choosing another
source. Safe and personal editions use their existing separate config roots.
Status deliberately reports a configured folder as **not inspected**, not as
validated or readable. Loading configuration never reads folder content.

Folder preview is nonrecursive, local UTF-8 plain-text `.txt`/`.md` only. On Unix
it opens every ancestor descriptor-relative with no-follow directory flags,
retains the selected directory descriptor, and opens files relative to it with
no-follow/nonblocking flags. It refuses symlink roots/ancestors, visible symlink
entries, special files, hard-linked candidate files, and hidden path components.
Hidden entries are skipped without opening them. Credential-like filenames
(including secrets, passwords, tokens, and environment files) are excluded.
This is defense in depth, **not content-based secret detection**: select only
documents you intend to preview. Ordinary text can itself contain secrets or
hostile instructions; excerpts grant no mutation or tool authority.

Other platforms refuse folder search: a metadata/canonicalize check followed by
an ordinary path open is not a safe substitute. Unix native qualification beyond
the checks actually executed must not be inferred from this implementation.

Fixed limits: depth **0**, 128 directory entries (including skipped entries),
64 candidate files, 64 KiB per file, 512 KiB total input, 32 ancestor components,
4096 path bytes, 256 query bytes, 1–10 results, 512 UTF-8 bytes per excerpt.
Exceeding an input bound fails the entire search without partial stdout results.
Matching is literal case-insensitive substring matching, not semantic ranking.
Results are deterministic by SHA-256 content ID and contain no source filenames
or paths (document text may itself mention paths). No persistent index is made.
Concurrent edits can affect which bounded text is seen; there is no snapshot or
source-integrity guarantee. Symlink substitution does not redirect descriptor-
bound reads outside the selected directory.

Explicit alternatives:

```toml
[knowledge]
source = "synthetic" # deterministic built-in imaginary fixture corpus
```

`source = "memory"` is recognized but **unavailable**. Choosing this source is
not blanket permission to expose every curation-ready record. Explicit approved
record selection/scope and exact backend binding are not implemented here; no
memory opener or fallback store is used. Future memory support must additionally
enforce provenance, confidence ≥ 0.9, nonobsolete status, and suitable payloads.

Existing `abbey train` memory/specs source behavior and defaults are unchanged.

## Explicit approved project memory

The preview commands above remain read-only and unchanged. Separate opt-in verbs
use the **exact configured** `memory_backend` (`sqlite`, or feature-gated `wdbx`)
with no substitution. They require both an existing `--project DIR` and a
`--consent FILE` containing the exact line `abbey-memory-consent: v1`.
This grants memory operations, **not training**. No memory is captured implicitly.

```sh
abbey knowledge remember 'Juniper uses blue labels' --source operator --owner alice --days 30 --project . --consent memory-consent.txt
abbey knowledge recall Juniper --project . --consent memory-consent.txt
abbey knowledge correct RECORD_ID 'Juniper uses green labels' --project . --consent memory-consent.txt
abbey knowledge forget RECORD_ID --project . --consent memory-consent.txt
abbey knowledge export --project . --consent memory-consent.txt
abbey knowledge ask Juniper --model selected-installed-model --project . --consent memory-consent.txt
```

Capture accepts up to 16 KiB of UTF-8 text, a source label up to 256 bytes,
and an owner label up to 128 bytes. Records carry canonical project scope,
UUID, timestamp, source/owner, approval-file digest, private classification,
confidence 1.0, and expiry (1–3650 days, default 30). Recall and export exclude
foreign projects, expired, obsolete, or unapproved records. Recall returns at
most 10 literal case-insensitive matches. More than 1000 nonobsolete approved
records in a project refuses recall/export rather than silently truncating.
Capture counts expired nonobsolete records toward the same cap; explicitly
forget expired records to free capacity.
Model-family receipts may be explicitly captured as JSON text with `remember`,
using a receipt source label; this does not authorize ingesting training data.

Correction invalidates the old record **before** storing its replacement and
links the replacement through `supersedes`. It fails closed on interruption;
a failed replacement write may require manually repeating capture. This is not
a transactional revision API. Forget invalidates a record, including an expired
record. Already obsolete IDs refuse; after correction use the replacement ID.
All live commands serialize through a per-state advisory lock (two-second
contention bound), acquired before the existing backend locks. It is logical deletion: original bytes and older versions may remain in
SQLite/WDBX journals, checkpoints and backups. It is **not physical erasure**.

`ask` ranks active scoped references by Unicode alphanumeric word overlap with
basic question words removed, returning at most 10; equal scores sort by record
ID. This is literal keyword ranking, not semantic retrieval. `recall` retains
its whole-query substring behavior.

`ask` alone sends matching references to the fixed local Ollama HTTP service at
`127.0.0.1:11434`; it requires an explicit model, checks local model metadata,
refuses cloud/remote models and other routes, and has no proxy, redirect, or
fallback path. It never pulls a model. It returns the answer with the actual
reference records as provenance. References are untrusted data and grant no
execution authority; this path invokes no tools and does not change any tool
consent. Requests use bounded input/output and socket timeouts. The local service
and its model metadata are a trust boundary; running a malicious local service
is outside this protection. No automatic global prompt injection, embedding,
training, or weight update is connected. Export prints private text to stdout;
sharing or redirecting it is an explicit caller action.
