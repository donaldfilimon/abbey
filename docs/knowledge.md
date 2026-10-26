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
