//! The shared fixtures of the training tests.
//!
//! The trainer and the sampler are exercised against a **fixture script**
//! written into a scratch directory — the same shape the TUI prediction
//! fixture uses — so the tests never depend on a real `abi` binary, a real
//! model, or the network.
//!
//! The fixture script speaks the exact argv grammar the adapter uses and
//! behaves the way the sibling trainer behaves: it reads the `text` field of
//! every JSON object in the dataset, joins the surviving values with one
//! newline, keeps that corpus in the artifact it writes, and — when asked to
//! sample — continues **its own corpus** from the first occurrence of the
//! seed character, which is what the sibling's continuation model does.
//!
//! The fixture reads the corpus with `awk`, not with Abbey's own
//! reconstruction, so the end-to-end tests compare two independent
//! implementations of the same rule.

use crate::distill::DistillPair;
use crate::memory::{MemoryRecord, MemoryStore, SqliteMemory};
use crate::state::AbbeyState;
use crate::train::dataset::Pair;
use crate::train::split::{TRAIN_PERCENT, assign};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

/// A scratch directory that removes itself.
pub struct Scratch {
    /// The directory itself.
    pub dir: PathBuf,
}

impl Scratch {
    /// A fresh scratch directory.
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "abbey-train-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A state whose only contents are the ones a test creates.
pub fn state_in(dir: &Path) -> AbbeyState {
    AbbeyState {
        chat_file: dir.join("chat-id"),
        model_file: dir.join("model"),
        history_file: dir.join("history.log"),
        cwd_dir: dir.join("by-cwd"),
        state_dir: dir.to_path_buf(),
        per_cwd: false,
        cwd: dir.to_path_buf(),
    }
}

/// A pair with a known prompt and response.
pub fn pair(id: &str, prompt: &str, response: &str) -> Pair {
    Pair {
        id: id.to_string(),
        prompt: prompt.to_string(),
        response: response.to_string(),
    }
}

/// A corpus of pairs.
pub fn corpus(pairs: Vec<Pair>) -> crate::train::dataset::Corpus {
    crate::train::dataset::Corpus {
        source: Some(crate::train::dataset::Source::Memory),
        pairs,
        redacted_pairs: 0,
        skipped: Vec::new(),
        specs: None,
    }
}

/// Write a consent file and return its path.
pub fn consent_file(scratch: &Scratch) -> PathBuf {
    let path = scratch.dir.join("consent.txt");
    fs::write(&path, format!("{}\n", crate::train::consent::CONSENT_LINE)).unwrap();
    path
}

/// The fixture trainer script.
///
/// It speaks the exact argv grammar the adapter uses:
///
/// * `nn train --jsonl <path> --field text --out <path>` reads the dataset,
///   builds the corpus the way the sibling builds it, and writes an artifact
///   that keeps the corpus and the model's context window;
/// * `nn sample --checkpoint <path> --seed <char> --n <k>` continues that
///   corpus from the first occurrence of the seed character, which is what a
///   faithful character model does.
///
/// The modes are:
///
/// * `ok` — the artifact reproduces its own corpus (the faithful case);
/// * `unrelated` — the artifact holds a corpus of a character the real
///   corpus never contains, so it reproduces nothing;
/// * `train-fail`, `sample-fail`, `hang` — the failure modes.
pub fn write_trainer(scratch: &Scratch, mode: &str) -> PathBuf {
    // One file per mode: a test that compares two artifacts needs two
    // different trainers, not one trainer rewritten underneath it.
    let path = scratch.dir.join(format!("abi-fixture-{mode}"));
    let script = format!(
        r#"#!/bin/sh
ROOT='{root}'
MODE='{mode}'
WORK="$ROOT/.fixture-work"
mkdir -p "$WORK"
case "$1 $2" in
"nn train")
  dataset=""
  out=""
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "--jsonl" ]; then dataset="$arg"; fi
    if [ "$prev" = "--out" ]; then out="$arg"; fi
    prev="$arg"
  done
  if [ "$MODE" = "train-fail" ]; then
    printf 'fixture: refusing to train\n' >&2
    exit 1
  fi
  if [ "$MODE" = "hang" ]; then
    exec sleep 30
  fi
  if [ "$MODE" = "unrelated" ]; then
    # A character the real corpus never contains: the sample can never
    # share a prefix with the expectation.
    printf '~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~' > "$WORK/corpus.bin"
  else
    # The sibling's corpus rule, implemented independently of Abbey's.
    awk '
    {{
      line = $0
      gsub(/^[ \t\r]+/, "", line)
      gsub(/[ \t\r]+$/, "", line)
      if (line == "") next
      p = index(line, "\"text\":\"")
      if (p == 0) next
      v = substr(line, p + 8)
      # The row ends with the closing quote and brace.
      v = substr(v, 1, length(v) - 2)
      if (v == "") next
      gsub(/\\"/, "\"", v)
      gsub(/\\n/, "\n", v)
      if (out != "") out = out "\n"
      out = out v
    }}
    END {{ printf "%s", out }}' "$dataset" > "$WORK/corpus.bin"
  fi
  b64=$(base64 < "$WORK/corpus.bin" | tr -d '\n')
  printf '{{"version":1,"seq_len":2,"corpus_b64":"%s"}}\n' "$b64" > "$out"
  printf 'nn train: initial_loss=1.0000 final_loss=0.5000 improved=true steps=10\n'
  printf 'nn checkpoint: wrote %s\n' "$out"
  exit 0 ;;
"nn sample")
  ckpt=""
  seed=""
  n=16
  prev=""
  for arg in "$@"; do
    if [ "$prev" = "--checkpoint" ]; then ckpt="$arg"; fi
    if [ "$prev" = "--seed" ]; then seed="$arg"; fi
    if [ "$prev" = "--n" ]; then n="$arg"; fi
    prev="$arg"
  done
  if [ "$MODE" = "sample-fail" ]; then
    printf 'fixture: refusing to sample\n' >&2
    exit 3
  fi
  b64=$(sed -n 's/.*"corpus_b64":"//p' "$ckpt" | tr -d '"}}\n')
  printf '%s' "$b64" | base64 -d > "$WORK/corpus.bin"
  # Newlines become a flat sentinel so the whole corpus is one record; the
  # record separator is a character the corpus never holds.
  tr '\n' '\001' < "$WORK/corpus.bin" > "$WORK/corpus.flat"
  out=$(awk -v seed="$seed" -v n="$n" -v window=2 '
  BEGIN {{ RS = "\002" }}
  {{
    s = $0
    if (s == "") exit 0
    p = index(s, seed)
    if (p == 0) p = 1
    start = (p - 1 + window) % length(s)
    out = ""
    for (i = 0; i < n; i++) {{ out = out substr(s, ((start + i) % length(s)) + 1, 1) }}
    printf "%s", out
    exit 0
  }}' "$WORK/corpus.flat")
  printf 'nn train: initial_loss=1.0000 final_loss=0.5000 improved=true steps=10\n'
  printf 'nn sample: %s\n' "$out" | tr '\001' '\n'
  exit 0 ;;
esac
printf 'unexpected invocation\n' >&2
exit 9
"#,
        root = scratch.dir.display(),
        mode = mode,
    );
    fs::write(&path, script).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

/// Record ids with a known split, so a test always has both halves.
pub fn ids_with_splits(train: usize, validation: usize) -> Vec<String> {
    let mut out = Vec::new();
    let (mut seen_train, mut seen_validation) = (0usize, 0usize);
    for index in 0..10_000usize {
        let id = format!("memory:{index:04}");
        match assign(&id, TRAIN_PERCENT) {
            crate::train::split::Split::Train if seen_train < train => {
                seen_train += 1;
                out.push(id);
            }
            crate::train::split::Split::Validation if seen_validation < validation => {
                seen_validation += 1;
                out.push(id);
            }
            _ => {}
        }
        if seen_train == train && seen_validation == validation {
            break;
        }
    }
    out
}

/// Store one curated pair in a scratch state.
pub fn store_pair(state_dir: &Path, id: &str, prompt: &str, response: &str, confidence: f32) {
    let mem = SqliteMemory::open(&SqliteMemory::path_for_state_dir(state_dir)).unwrap();
    let mut record = MemoryRecord::new_stm(
        "candidate",
        serde_json::to_string(&DistillPair {
            prompt: prompt.to_string(),
            response: response.to_string(),
            teacher: "fm:system".into(),
            on_device: true,
        })
        .unwrap(),
    );
    record.id = id.to_string();
    record.retention = "train_candidate".into();
    record.provenance = "teacher fm:system @ 2026-10-22T00:00:00Z".into();
    record.confidence = confidence;
    mem.store(record).unwrap();
}

/// The prompts and responses the fixture corpus is built from.
///
/// Every prompt starts with its own character, so the held-out pairs supply
/// distinct probe seeds, and no two pairs share their text — so no
/// validation text can appear in the training corpus by accident.
pub const FIXTURE_PROMPTS: [&str; 5] = ["alpha", "bravo", "charlie", "delta", "echo"];
/// The held-out prompts.
pub const FIXTURE_VALIDATION_PROMPTS: [&str; 2] = ["foxtrot", "golf"];
/// The training responses.
pub const FIXTURE_RESPONSES: [&str; 5] = ["one", "two", "three", "four", "five"];
/// The held-out responses.
pub const FIXTURE_VALIDATION_RESPONSES: [&str; 2] = ["six", "seven"];

/// Store a corpus of `train` training pairs and `validation` held-out pairs.
///
/// The held-out pairs are stored too: their prompts are the probe seeds, and
/// their text must stay out of the training corpus.
pub fn store_corpus(state_dir: &Path, train: usize, validation: usize) {
    let ids = ids_with_splits(train, validation);
    let train_ids: Vec<&String> = ids
        .iter()
        .filter(|id| assign(id, TRAIN_PERCENT) == crate::train::split::Split::Train)
        .collect();
    let validation_ids: Vec<&String> = ids
        .iter()
        .filter(|id| assign(id, TRAIN_PERCENT) == crate::train::split::Split::Validation)
        .collect();
    for (index, id) in train_ids.iter().enumerate() {
        store_pair(
            state_dir,
            id,
            FIXTURE_PROMPTS[index % FIXTURE_PROMPTS.len()],
            FIXTURE_RESPONSES[index % FIXTURE_RESPONSES.len()],
            0.95,
        );
    }
    for (index, id) in validation_ids.iter().enumerate() {
        store_pair(
            state_dir,
            id,
            FIXTURE_VALIDATION_PROMPTS[index % FIXTURE_VALIDATION_PROMPTS.len()],
            FIXTURE_VALIDATION_RESPONSES[index % FIXTURE_VALIDATION_RESPONSES.len()],
            0.95,
        );
    }
}
