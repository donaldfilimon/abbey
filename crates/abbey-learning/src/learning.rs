use crate::io::*;
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
const MAX_FILE: usize = 256 * 1024;
const MAX_FILES: usize = 256;
const ALIASES: [&str; 3] = ["abbey", "abi", "wdbx"];
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema: String,
    private: bool,
    sources: BTreeMap<String, Vec<String>>,
}
struct Pattern {
    parts: Vec<Option<regex::Regex>>,
    prefix: Vec<String>,
}
fn patterns(values: &[String]) -> Result<Vec<Pattern>> {
    ensure!(
        !values.is_empty() && values.len() <= 64,
        "1..64 source glob patterns required"
    );
    values
        .iter()
        .map(|value| {
            ensure!(
                !value.is_empty()
                    && value.len() <= 1024
                    && !value.starts_with('/')
                    && !value.contains(['\\', ':'])
                    && !value.split('/').any(|p| matches!(p, "" | "." | "..")),
                "relative nonescaping glob required"
            );
            let mut parts = Vec::new();
            let mut prefix = Vec::new();
            let mut wildcard = false;
            for part in value.split('/') {
                ensure!(parts.len() < 64, "glob depth limit exceeded");
                if !wildcard && !part.contains(['*', '?', '[']) {
                    prefix.push(part.to_string());
                } else {
                    wildcard = true;
                }
                if part == "**" {
                    parts.push(None);
                    continue;
                }
                let mut expression = String::from("^");
                let mut chars = part.chars().peekable();
                while let Some(ch) = chars.next() {
                    match ch {
                        '*' => expression.push_str(".*"),
                        '?' => expression.push('.'),
                        '[' => {
                            let mut class = String::new();
                            for next in chars.by_ref() {
                                if next == ']' {
                                    break;
                                }
                                class.push(next);
                            }
                            ensure!(
                                !class.is_empty() && !class.contains('/'),
                                "invalid glob character class"
                            );
                            expression.push('[');
                            if let Some(rest) = class.strip_prefix('!') {
                                expression.push('^');
                                expression.push_str(rest);
                            } else {
                                expression.push_str(&class);
                            }
                            expression.push(']');
                        }
                        _ => expression.push_str(&regex::escape(&ch.to_string())),
                    }
                }
                expression.push('$');
                parts.push(Some(regex::Regex::new(&expression)?));
            }
            Ok(Pattern { parts, prefix })
        })
        .collect()
}
fn matches(parts: &[&str], pattern: &Pattern) -> bool {
    fn go(
        i: usize,
        j: usize,
        parts: &[&str],
        pattern: &Pattern,
        memo: &mut BTreeMap<(usize, usize), bool>,
    ) -> bool {
        if let Some(v) = memo.get(&(i, j)) {
            return *v;
        }
        let result = if j == pattern.parts.len() {
            i == parts.len()
        } else if let Some(expression) = &pattern.parts[j] {
            i < parts.len()
                && expression.is_match(parts[i])
                && go(i + 1, j + 1, parts, pattern, memo)
        } else {
            go(i, j + 1, parts, pattern, memo)
                || (i < parts.len() && go(i + 1, j, parts, pattern, memo))
        };
        memo.insert((i, j), result);
        result
    }
    go(0, 0, parts, pattern, &mut BTreeMap::new())
}
fn possible(parts: &[&str], patterns: &[Pattern]) -> bool {
    patterns.iter().any(|p| {
        let common = parts.len().min(p.prefix.len());
        parts[..common]
            .iter()
            .zip(&p.prefix[..common])
            .all(|(a, b)| a == b)
    })
}
fn ignored(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with('.')
        || [
            "tests",
            "fixtures",
            "contracts",
            "verification",
            "evidence",
            "target",
            "node_modules",
        ]
        .contains(&n.as_str())
        || [
            "credential",
            "secret",
            "password",
            "private_key",
            "private-key",
            "id_rsa",
            "id_ed25519",
        ]
        .iter()
        .any(|s| n.contains(s))
        || ["token.md", "tokens.md", "token.txt", "tokens.txt"].contains(&n.as_str())
}
struct Selected {
    alias: String,
    relative: String,
    file: std::fs::File,
}
#[cfg(unix)]
fn discover(
    root: &std::fs::File,
    alias: &str,
    relative: &str,
    patterns: &[Pattern],
    selected: &mut Vec<Selected>,
    visited: &mut usize,
) -> Result<()> {
    use nix::{
        dir::Dir,
        fcntl::{AtFlags, OFlag, openat},
        sys::stat::{Mode, SFlag, fstat, fstatat},
    };
    let mut directory = Dir::from_fd(root.try_clone()?.into())?;
    let mut names = Vec::new();
    for entry in directory.iter() {
        let entry = entry?;
        let name = entry
            .file_name()
            .to_str()
            .context("UTF-8 learning filename required")?;
        if name != "." && name != ".." {
            names.push(name.to_string());
        }
    }
    names.sort();
    *visited += names.len();
    ensure!(*visited <= 200000, "source tree entry bound exceeded");
    for name in names {
        if ignored(&name) {
            continue;
        }
        let rel = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        ensure!(
            rel.len() <= 4096 && rel.split('/').count() <= 64,
            "source path bound exceeded"
        );
        let parts: Vec<_> = rel.split('/').collect();
        let selected_name = patterns.iter().any(|p| matches(&parts, p));
        let stat = fstatat(root, Path::new(&name), AtFlags::AT_SYMLINK_NOFOLLOW)?;
        let kind = SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT;
        if kind == SFlag::S_IFLNK {
            ensure!(
                !selected_name && !possible(&parts, patterns),
                "symlink learning source refused"
            );
            continue;
        }
        if kind == SFlag::S_IFDIR {
            if !possible(&parts, patterns) {
                continue;
            }
            let child = std::fs::File::from(openat(
                root,
                Path::new(&name),
                OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC,
                Mode::empty(),
            )?);
            discover(&child, alias, &rel, patterns, selected, visited)?;
        } else if selected_name
            && Path::new(&name)
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| matches!(s.to_ascii_lowercase().as_str(), "md" | "txt"))
        {
            ensure!(
                kind == SFlag::S_IFREG && stat.st_nlink == 1,
                "regular single-link source required"
            );
            ensure!(
                stat.st_size >= 0 && stat.st_size <= MAX_FILE as i64 && selected.len() < MAX_FILES,
                "source size/count limit exceeded"
            );
            let file = std::fs::File::from(openat(
                root,
                Path::new(&name),
                OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
                Mode::empty(),
            )?);
            let opened = fstat(&file)?;
            ensure!(
                opened.st_dev == stat.st_dev
                    && opened.st_ino == stat.st_ino
                    && opened.st_nlink == 1
                    && SFlag::from_bits_truncate(opened.st_mode).contains(SFlag::S_IFREG),
                "source entry changed while opening"
            );
            selected.push(Selected {
                alias: alias.into(),
                relative: rel,
                file,
            });
        }
    }
    Ok(())
}
#[cfg(unix)]
fn root_handle(path: &Path) -> Result<std::fs::File> {
    use nix::{
        fcntl::{OFlag, open, openat},
        sys::stat::Mode,
    };
    use std::path::Component;
    let physical = path.canonicalize()?;
    ensure!(physical.is_dir(), "explicit repository root required");
    let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_CLOEXEC;
    let mut file = std::fs::File::from(open(Path::new("/"), flags, Mode::empty())?);
    for part in physical.components() {
        if let Component::Normal(part) = part {
            file = std::fs::File::from(openat(&file, Path::new(part), flags, Mode::empty())?);
        }
    }
    Ok(file)
}
fn sections(text: &str, markdown: bool) -> Result<Vec<(String, String)>> {
    if !markdown {
        return Ok(vec![(
            String::new(),
            text.split_whitespace().collect::<Vec<_>>().join(" "),
        )]);
    }
    let heading = regex::Regex::new(r"^ {0,3}(#{1,6})\s+(.+?)\s*#*\s*$")?;
    let fence = regex::Regex::new(r"^ {0,3}(`{3,}|~{3,})")?;
    let mut fenced: Option<(char, usize)> = None;
    let mut stack: Vec<(usize, String)> = Vec::new();
    let mut current = String::new();
    let mut body = Vec::new();
    let mut result = Vec::new();
    for line in text.lines() {
        if let Some(found) = fence.captures(line) {
            let marker = found[1].chars().next().context("fence marker")?;
            let count = found[1].len();
            if fenced.is_some_and(|(c, n)| c == marker && count >= n) {
                fenced = None;
            } else if fenced.is_none() {
                fenced = Some((marker, count));
            }
            body.push(line);
            continue;
        }
        if fenced.is_none()
            && let Some(found) = heading.captures(line)
        {
            if !body.is_empty() {
                let normalized = body
                    .join(" ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ");
                if !normalized.is_empty() {
                    result.push((current.clone(), normalized));
                }
            }
            let depth = found[1].len();
            stack.retain(|(n, _)| *n < depth);
            stack.push((
                depth,
                found[2].split_whitespace().collect::<Vec<_>>().join(" "),
            ));
            current = stack
                .iter()
                .map(|(_, s)| s.as_str())
                .collect::<Vec<_>>()
                .join(" / ");
            ensure!(current.len() <= 4096, "heading path bound exceeded");
            body = vec![line];
            continue;
        }
        body.push(line);
    }
    let normalized = body
        .join(" ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if !normalized.is_empty() {
        result.push((current, normalized));
    }
    Ok(result)
}
fn chunks(text: &str) -> Vec<String> {
    let mut rest = text;
    let mut result = Vec::new();
    while !rest.is_empty() {
        let mut end = rest
            .char_indices()
            .nth(400)
            .map(|(i, _)| i)
            .unwrap_or(rest.len());
        if end < rest.len() {
            let midpoint = rest.char_indices().nth(200).map(|(i, _)| i).unwrap_or(0);
            if let Some(index) = rest[..end].rfind(' ')
                && index >= midpoint
            {
                end = index;
            }
        }
        result.push(rest[..end].into());
        rest = rest[end..].trim_start();
    }
    result
}
pub(crate) fn collect(
    config_path: &Path,
    roots: &BTreeMap<String, PathBuf>,
    output: &Path,
    grant: &Path,
) -> Result<Value> {
    let config_bytes = bounded_read(config_path, MAX_JSON)?;
    let config: Config = serde_json::from_value(parse_json(&config_bytes)?)?;
    ensure!(
        config.schema == "abbey.learning.sources.v1"
            && config
                .sources
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>()
                == ALIASES.into()
            && roots.keys().map(String::as_str).collect::<BTreeSet<_>>() == ALIASES.into(),
        "explicit abbey/abi/wdbx config roots required"
    );
    consent(grant, config.private)?;
    absent(output)?;
    #[cfg(not(unix))]
    {
        let _ = (config, roots, output);
        anyhow::bail!("secure learning collection unavailable on this platform");
    }
    #[cfg(unix)]
    {
        let mut selected = Vec::new();
        let mut visited = 0;
        for alias in ALIASES {
            let before = selected.len();
            let pattern = patterns(&config.sources[alias])?;
            let root = root_handle(&roots[alias])?;
            discover(&root, alias, "", &pattern, &mut selected, &mut visited)?;
            ensure!(
                selected.len() > before,
                "every repository must contribute selected files"
            );
        }
        let mut docs = Vec::new();
        let mut total = 0;
        for source in &mut selected {
            let mut bytes = Vec::new();
            std::io::Read::by_ref(&mut source.file)
                .take(MAX_FILE as u64 + 1)
                .read_to_end(&mut bytes)?;
            ensure!(bytes.len() <= MAX_FILE, "source size limit exceeded");
            total += bytes.len();
            ensure!(total <= MAX_JSON, "source total byte limit exceeded");
            let text = std::str::from_utf8(&bytes)?.to_string();
            ensure!(
                !text.trim().is_empty() && !text.contains('\0'),
                "nonempty UTF-8 plain text required"
            );
            docs.push((source.alias.clone(), source.relative.clone(), bytes, text));
        }
        let mut rows = Vec::new();
        let mut files = Vec::new();
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        let mut ids = BTreeSet::new();
        let mut duplicates = 0;
        let mut repositories: BTreeMap<String, Value> = ALIASES
            .into_iter()
            .map(|s| (s.into(), json!({"file_count":0,"bytes":0,"pairs":0})))
            .collect();
        for (alias, relative, bytes, text) in docs {
            let sha = digest_bytes(&bytes);
            let mut item =
                json!({"repository":alias,"path":relative,"sha256":sha,"bytes":bytes.len()});
            let repo = repositories.get_mut(&alias).context("known repository")?;
            repo["file_count"] = json!(repo["file_count"].as_u64().unwrap_or(0) + 1);
            repo["bytes"] = json!(repo["bytes"].as_u64().unwrap_or(0) + bytes.len() as u64);
            let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
            if let Some(original) = seen.get(&normalized) {
                item["duplicate_of"] = Value::String(original.clone());
                duplicates += 1;
                files.push(item);
                continue;
            }
            seen.insert(normalized, format!("{alias}:{relative}"));
            files.push(item);
            for (section, body) in sections(&text, relative.to_ascii_lowercase().ends_with(".md"))?
            {
                let lineage = format!(
                    "{alias}:{relative}{}",
                    if section.is_empty() {
                        String::new()
                    } else {
                        format!("#section:{section}")
                    }
                );
                for (i, excerpt) in chunks(&body).into_iter().enumerate() {
                    let index = i + 1;
                    let id = digest_value(&json!([alias, relative, sha, section, index, excerpt]));
                    if !ids.insert(id.clone()) {
                        continue;
                    }
                    rows.push(json!({"id":id,"lineage":lineage,"prompt":format!("Source continuation from {alias}:{relative}; section {}; chunk {index}.",if section.is_empty(){"(document)"}else{&section}),"response":excerpt,"provenance":format!("{alias}:{relative} sha256:{sha}"),"approved":true,"private":config.private}));
                    repo["pairs"] = json!(repo["pairs"].as_u64().unwrap_or(0) + 1);
                }
            }
        }
        ensure!(!rows.is_empty(), "nonempty source examples required");
        let mut content = Vec::new();
        for row in &rows {
            content.extend(canonical(row));
            content.push(b'\n');
            ensure!(
                content.len() <= MAX_JSON,
                "generated JSONL byte limit exceeded"
            );
        }
        let mut manifest = json!({"schema":"abbey.learning.collection.v1","qualification":"source text continuation examples; not reviewed instruction-distillation/assistant quality","private":config.private,"config_sha256":digest_bytes(&config_bytes),"source_sha256":digest_bytes(&content),"file_count":files.len(),"total_bytes":total,"pair_count":rows.len(),"duplicate_documents":duplicates,"repositories":repositories,"files":files,"lineage":"repository:relative-path#section:heading-path; all section chunks grouped"});
        manifest["sha256"] = digest_value(&manifest).into();
        let stage = Stage::new(output)?;
        write_new(&stage.path.join("source.jsonl"), &content)?;
        write_json_new(&stage.path.join("sources.json"), &manifest)?;
        stage.publish(output)?;
        Ok(manifest)
    }
}
