//! JSONL routing audit log for hybrid persona/role decisions.

use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRecord {
    pub ts: String,
    pub cwd: String,
    pub persona: String,
    pub role: String,
    pub model: String,
    pub reason: String,
    pub confidence: f32,
    #[serde(default)]
    pub tools: Vec<String>,
    /// Shared id linking the stages of one multi-stage run (e.g. the
    /// Gemma-interpret → Max-implement hybrid loop). `None` for single-shot runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation: Option<String>,
    /// Stage name within a correlated run (`interpret`, `implement`, …).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    /// Alternate role Abi considered (not auto-invoked).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alternate: Option<String>,
    /// Fallback note when confidence is low (audit only — no second agent path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback: Option<String>,
}

impl RouteRecord {
    pub fn new(
        cwd: impl Into<String>,
        persona: impl Into<String>,
        role: impl Into<String>,
        model: impl Into<String>,
        reason: impl Into<String>,
        confidence: f32,
    ) -> Self {
        Self {
            ts: Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            cwd: cwd.into(),
            persona: persona.into(),
            role: role.into(),
            model: model.into(),
            reason: reason.into(),
            confidence,
            tools: Vec::new(),
            correlation: None,
            stage: None,
            alternate: None,
            fallback: None,
        }
    }

    /// Tag this record as one stage of a correlated multi-stage run.
    pub fn in_stage(mut self, correlation: impl Into<String>, stage: impl Into<String>) -> Self {
        self.correlation = Some(correlation.into());
        self.stage = Some(stage.into());
        self
    }

    pub fn with_routing(mut self, alternate: Option<String>, fallback: Option<String>) -> Self {
        self.alternate = alternate;
        self.fallback = fallback;
        self
    }
}

/// All records sharing `correlation`, oldest first — the linked view of one
/// multi-stage run.
pub fn correlated_routes(state_dir: &Path, correlation: &str) -> Result<Vec<RouteRecord>> {
    let path = route_log_path(state_dir);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    // Intentionally full-history: the stages of a run need not be near EOF.
    let mut out = Vec::new();
    for line in BufReader::new(File::open(path)?).split(b'\n') {
        if let Ok(record) = serde_json::from_slice::<RouteRecord>(&line?)
            && record.correlation.as_deref() == Some(correlation)
        {
            out.push(record);
        }
    }
    Ok(out)
}

pub fn route_log_path(state_dir: &Path) -> std::path::PathBuf {
    state_dir.join("route.jsonl")
}

pub fn append_route_record(state_dir: &Path, rec: &RouteRecord) -> Result<()> {
    fs::create_dir_all(state_dir)?;
    let path = route_log_path(state_dir);
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("open {}", path.display()))?;
    serde_json::to_writer(&mut f, rec)?;
    f.write_all(b"\n")?;
    Ok(())
}

const RECENT_READ_BYTES: usize = 8 * 1024;
const MAX_RECENT_READS: usize = 32;
const MAX_RECENT_BYTES: usize = RECENT_READ_BYTES * MAX_RECENT_READS;
const MAX_RECENT_LINES: usize = 2048;
const MAX_RECENT_RECORD_BYTES: usize = 32 * 1024;

/// Recent valid records, oldest first, within a bounded reverse tail.
///
/// Reads at most 256 KiB with 32 reads and examines at most 2,048 line
/// boundaries. Records over 32 KiB, malformed JSON, and the partial leading
/// record at the byte boundary are skipped. Consequently this may return fewer
/// than `n` entries even when older history contains more valid records.
pub fn recent_routes(state_dir: &Path, n: usize) -> Result<Vec<RouteRecord>> {
    if n == 0 {
        return Ok(Vec::new());
    }
    let path = route_log_path(state_dir);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    read_recent_routes(&mut File::open(path)?, n)
}

fn read_recent_routes(reader: &mut (impl Read + Seek), n: usize) -> Result<Vec<RouteRecord>> {
    let mut out = Vec::new();
    if n == 0 {
        return Ok(out);
    }
    // Snapshot EOF so concurrent appends cannot extend this read's budget.
    let mut position = reader.seek(SeekFrom::End(0))?;
    let mut block = [0; RECENT_READ_BYTES];
    let mut line = Vec::new();
    let mut oversized = false;
    let mut lines = 0;
    let mut bytes_left = MAX_RECENT_BYTES;
    let mut reads = 0;
    for _ in 0..MAX_RECENT_READS {
        let size = position.min(bytes_left as u64).min(block.len() as u64) as usize;
        if size == 0 {
            break;
        }
        position -= size as u64;
        reader.seek(SeekFrom::Start(position))?;
        let mut filled = 0;
        while filled < size {
            if reads == MAX_RECENT_READS {
                // Short reads consume the same operation budget. Do not parse
                // an incomplete block as if it reached the snapshotted EOF.
                out.reverse();
                return Ok(out);
            }
            reads += 1;
            let received = reader.read(&mut block[filled..size])?;
            if received == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::UnexpectedEof).into());
            }
            filled += received;
        }
        bytes_left -= size;
        for &byte in block[..size].iter().rev() {
            if byte == b'\n' {
                parse_reverse_record(&mut line, oversized, &mut out);
                oversized = false;
                lines += 1;
                if out.len() == n || lines == MAX_RECENT_LINES {
                    out.reverse();
                    return Ok(out);
                }
            } else if !oversized {
                if line.len() == MAX_RECENT_RECORD_BYTES {
                    line.clear();
                    oversized = true;
                } else {
                    line.push(byte);
                }
            }
        }
    }
    // Only BOF proves that the remaining fragment is an entire record.
    if position == 0 {
        parse_reverse_record(&mut line, oversized, &mut out);
    }
    out.reverse();
    Ok(out)
}

fn parse_reverse_record(line: &mut Vec<u8>, oversized: bool, out: &mut Vec<RouteRecord>) {
    if !oversized && !line.is_empty() {
        line.reverse();
        if let Ok(record) = serde_json::from_slice(line) {
            out.push(record);
        }
    }
    line.clear();
}

/// Compact one-liner for the TUI Routes pane — narrow columns, no reason text.
///
/// The full audit line stays [`format_route_line`]; this keeps only what fits
/// a half-width pane: clock time, persona/role, model, confidence, stage.
pub fn compact_route_line(r: &RouteRecord) -> String {
    // ts is RFC3339-ish ("2026-08-08T04:53:21…") — keep HH:MM when present.
    let clock = if r.ts.len() >= 16 && r.ts.is_char_boundary(11) && r.ts.is_char_boundary(16) {
        &r.ts[11..16]
    } else {
        r.ts.as_str()
    };
    let stage = r.stage.as_deref().unwrap_or("-");
    format!(
        "{clock} {}/{} {} {:.2} {stage}",
        r.persona, r.role, r.model, r.confidence
    )
}

/// One-line display for CLI / slash — keeps both surfaces in lockstep.
pub fn format_route_line(r: &RouteRecord) -> String {
    format!(
        "{}\t{}\t{}\t{}\t{:.2}\t{}\talt={}\tfb={}\t{}",
        r.ts,
        r.persona,
        r.role,
        r.model,
        r.confidence,
        r.stage.as_deref().unwrap_or("-"),
        r.alternate.as_deref().unwrap_or("-"),
        r.fallback.as_deref().unwrap_or("-"),
        r.reason
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    struct Meter<R> {
        inner: R,
        bytes: usize,
        reads: usize,
        seeks: usize,
    }

    impl<R> Meter<R> {
        fn new(inner: R) -> Self {
            Self {
                inner,
                bytes: 0,
                reads: 0,
                seeks: 0,
            }
        }

        fn assert_bounded(&self) {
            assert!(self.bytes <= MAX_RECENT_BYTES);
            assert!(self.reads <= MAX_RECENT_READS);
            assert!(self.seeks <= MAX_RECENT_READS + 1);
        }
    }

    impl<R: Read> Read for Meter<R> {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            self.reads += 1;
            let size = self.inner.read(buf)?;
            self.bytes += size;
            Ok(size)
        }
    }

    impl<R: Seek> Seek for Meter<R> {
        fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
            self.seeks += 1;
            self.inner.seek(from)
        }
    }

    fn encoded(reason: &str) -> Vec<u8> {
        serde_json::to_vec(&RouteRecord::new(".", "abbey", "max", "fable", reason, 0.8)).unwrap()
    }

    fn reasons(records: &[RouteRecord]) -> Vec<&str> {
        records
            .iter()
            .map(|record| record.reason.as_str())
            .collect()
    }

    #[test]
    fn recent_tail_orders_records_and_handles_final_json() {
        for ending in [b"\n".as_slice(), b"", b"\n{\"ts\":"] {
            let mut bytes = Vec::new();
            for reason in ["old", "middle", "new"] {
                bytes.extend(encoded(reason));
                bytes.push(b'\n');
            }
            bytes.pop();
            bytes.extend(ending);
            let got = read_recent_routes(&mut Cursor::new(bytes), 2).unwrap();
            assert_eq!(reasons(&got), ["middle", "new"]);
        }
        assert!(
            read_recent_routes(&mut Cursor::new(b""), 8)
                .unwrap()
                .is_empty()
        );
        let got = read_recent_routes(&mut Cursor::new(encoded("only")), 8).unwrap();
        assert_eq!(reasons(&got), ["only"]);
    }

    #[test]
    fn recent_tail_skips_malformed_and_oversized_records_across_blocks() {
        let mut bytes = encoded("old");
        bytes.extend(b"\nnot json\n\xff\xfe\n\n");
        bytes.extend(encoded(&"x".repeat(MAX_RECENT_RECORD_BYTES)));
        bytes.push(b'\n');
        // This valid record spans multiple reverse-read blocks.
        let long_reason = "y".repeat(RECENT_READ_BYTES + 17);
        bytes.extend(encoded(&long_reason));
        bytes.extend(b"\r\n");
        bytes.extend(encoded("new"));
        bytes.extend(b"\n{incomplete");
        let mut reader = Meter::new(Cursor::new(bytes));
        let got = read_recent_routes(&mut reader, usize::MAX).unwrap();
        assert_eq!(reasons(&got), ["old", long_reason.as_str(), "new"]);
        reader.assert_bounded();
    }

    #[test]
    fn recent_tail_discards_partial_leading_record_and_stops_at_byte_budget() {
        let mut bytes = vec![b'x'; MAX_RECENT_BYTES];
        // The window begins on apparently valid JSON, but the preceding byte
        // proves it is actually the suffix of a malformed oversized line.
        let mut tail = encoded("not a whole line");
        tail.extend(b"\n");
        tail.extend(encoded("new"));
        tail.push(b'\n');
        let padding = MAX_RECENT_BYTES - tail.len();
        tail.extend(std::iter::repeat_n(b' ', padding));
        bytes.extend(tail);
        let mut reader = Meter::new(Cursor::new(bytes));
        let got = read_recent_routes(&mut reader, usize::MAX).unwrap();
        assert_eq!(reasons(&got), ["new"]);
        assert_eq!(reader.bytes, MAX_RECENT_BYTES);
        assert_eq!(reader.reads, MAX_RECENT_READS);
        reader.assert_bounded();
    }

    #[test]
    fn recent_tail_bounds_scanned_lines_even_when_blank_or_malformed() {
        for line in [b"\n".as_slice(), b"bad\n"] {
            let mut bytes = encoded("outside line budget");
            bytes.push(b'\n');
            for _ in 0..MAX_RECENT_LINES {
                bytes.extend(line);
            }
            let mut reader = Meter::new(Cursor::new(bytes));
            assert!(
                read_recent_routes(&mut reader, usize::MAX)
                    .unwrap()
                    .is_empty()
            );
            reader.assert_bounded();
            assert_eq!(reader.reads, 1);
        }
    }

    #[test]
    fn recent_tail_zero_does_not_touch_reader_or_path() {
        let mut reader = Meter::new(Cursor::new(encoded("unused")));
        assert!(read_recent_routes(&mut reader, 0).unwrap().is_empty());
        assert_eq!((reader.bytes, reader.reads, reader.seeks), (0, 0, 0));
        assert!(
            recent_routes(Path::new("missing-route-state"), 0)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn recent_tail_bounds_actual_read_calls_even_with_short_reads() {
        struct ShortReads(Cursor<Vec<u8>>);

        impl Read for ShortReads {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let size = buf.len().min(1);
                self.0.read(&mut buf[..size])
            }
        }

        impl Seek for ShortReads {
            fn seek(&mut self, from: SeekFrom) -> std::io::Result<u64> {
                self.0.seek(from)
            }
        }

        let mut reader = Meter::new(ShortReads(Cursor::new(encoded("short reads"))));
        assert!(read_recent_routes(&mut reader, 8).unwrap().is_empty());
        assert_eq!(reader.reads, MAX_RECENT_READS);
        assert_eq!(reader.bytes, MAX_RECENT_READS);
        reader.assert_bounded();
    }

    #[test]
    fn recent_tail_reads_only_one_block_after_large_sparse_prefix() {
        let dir = std::env::temp_dir().join(format!("abbey-route-sparse-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = route_log_path(&dir);
        let mut file = File::create(&path).unwrap();
        file.set_len(1024 * 1024 * 1024).unwrap();
        file.seek(SeekFrom::End(0)).unwrap();
        file.write_all(b"\n").unwrap();
        for i in 0..8 {
            file.write_all(&encoded(&i.to_string())).unwrap();
            file.write_all(b"\n").unwrap();
        }
        drop(file);
        let mut reader = Meter::new(File::open(&path).unwrap());
        let got = read_recent_routes(&mut reader, 8).unwrap();
        assert_eq!(reasons(&got), ["0", "1", "2", "3", "4", "5", "6", "7"]);
        assert_eq!(reader.reads, 1);
        assert_eq!(reader.bytes, RECENT_READ_BYTES);
        reader.assert_bounded();
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn correlation_lookup_remains_full_history_and_ordered() {
        let dir = std::env::temp_dir().join(format!("abbey-route-history-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = route_log_path(&dir);
        fs::write(&path, b"").unwrap();
        let first = RouteRecord::new(".", "abbey", "max", "fable", "first", 0.8)
            .in_stage("linked", "interpret");
        append_route_record(&dir, &first).unwrap();
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        for _ in 0..MAX_RECENT_BYTES / 4 + 1 {
            file.write_all(b"bad\n").unwrap();
        }
        drop(file);
        let last = RouteRecord::new(".", "abbey", "max", "fable", "last", 0.8)
            .in_stage("linked", "implement");
        append_route_record(&dir, &last).unwrap();
        assert_eq!(reasons(&recent_routes(&dir, usize::MAX).unwrap()), ["last"]);
        assert_eq!(
            reasons(&correlated_routes(&dir, "linked").unwrap()),
            ["first", "last"]
        );
        assert!(correlated_routes(&dir, "other").unwrap().is_empty());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn append_and_read() {
        let dir = std::env::temp_dir().join(format!("abbey-route-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let rec = RouteRecord::new(".", "abbey", "max", "fable", "code heuristic", 0.8);
        append_route_record(&dir, &rec).unwrap();
        let got = recent_routes(&dir, 5).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].role, "max");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn alternate_and_fallback_survive_jsonl_roundtrip() {
        let dir = std::env::temp_dir().join(format!("abbey-route-alt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let rec = RouteRecord::new(".", "abbey", "max", "fable", "hybrid", 0.7)
            .with_routing(Some("gemma".into()), Some("prefer hybrid-loop".into()));
        append_route_record(&dir, &rec).unwrap();
        let got = recent_routes(&dir, 1).unwrap();
        assert_eq!(got[0].alternate.as_deref(), Some("gemma"));
        assert_eq!(got[0].fallback.as_deref(), Some("prefer hybrid-loop"));
        assert!(format_route_line(&got[0]).contains("alt=gemma"));
        assert!(format_route_line(&got[0]).contains("fb=prefer hybrid-loop"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn compact_line_keeps_route_fields_and_drops_reason() {
        let rec = RouteRecord::new(".", "abbey", "max", "fable", "a very long reason", 0.82);
        let line = compact_route_line(&rec);
        assert!(line.contains("abbey/max"), "{line}");
        assert!(line.contains("fable"), "{line}");
        assert!(line.contains("0.82"), "{line}");
        assert!(
            !line.contains("a very long reason"),
            "reason must not widen the pane: {line}"
        );
        // A short/odd timestamp must fall back rather than panic or slice
        // off a char boundary.
        let mut odd = RouteRecord::new(".", "abbey", "max", "fable", "r", 0.5);
        odd.ts = "now".into();
        assert!(compact_route_line(&odd).starts_with("now "));
    }
}
