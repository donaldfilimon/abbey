use anyhow::Result;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SkillOrigin {
    ProjectShared,
    ProjectCursor,
    ProjectCodex,
    ProjectClaude,
    UserShared,
    Cursor,
    Codex,
    Claude,
    Grok,
    AbiMega,
    CodexPlugin,
    ClaudePlugin,
    #[cfg(test)]
    Custom,
}

impl SkillOrigin {
    pub fn label(self) -> &'static str {
        match self {
            Self::ProjectShared => "project/shared",
            Self::ProjectCursor => "project/cursor",
            Self::ProjectCodex => "project/codex",
            Self::ProjectClaude => "project/claude",
            Self::UserShared => "user/shared",
            Self::Cursor => "cursor",
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Grok => "grok",
            Self::AbiMega => "abi-mega",
            Self::CodexPlugin => "codex-plugin",
            Self::ClaudePlugin => "claude-plugin",
            #[cfg(test)]
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkillRoot {
    pub path: PathBuf,
    pub origin: SkillOrigin,
    /// Plugin caches contain `<marketplace>/<plugin>/<version>/skills/...`.
    pub recursive: bool,
}

impl SkillRoot {
    pub fn direct(path: impl Into<PathBuf>, origin: SkillOrigin) -> Self {
        Self {
            path: path.into(),
            origin,
            recursive: false,
        }
    }

    pub fn recursive(path: impl Into<PathBuf>, origin: SkillOrigin) -> Self {
        Self {
            path: path.into(),
            origin,
            recursive: true,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub name: String,
    /// Skill directory, retained for compatibility with the TUI.
    pub root: PathBuf,
    pub manifest: PathBuf,
    pub description: String,
    pub origin: SkillOrigin,
    /// Every manifest carrying this exact name/content. Exact mirrors are
    /// consolidated for display without losing their provenance.
    pub provenance: Vec<SkillProvenance>,
}

#[derive(Debug, Clone)]
pub struct SkillProvenance {
    pub origin: SkillOrigin,
    pub manifest: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SkillDiagnostic {
    pub name: String,
    pub paths: Vec<PathBuf>,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct SkillInventory {
    pub entries: Vec<SkillEntry>,
    pub diagnostics: Vec<SkillDiagnostic>,
}

pub fn default_skill_roots() -> Vec<SkillRoot> {
    let mut roots = Vec::new();
    if let Ok(cwd) = std::env::current_dir() {
        roots.extend([
            SkillRoot::direct(cwd.join(".agents/skills"), SkillOrigin::ProjectShared),
            SkillRoot::direct(cwd.join(".cursor/skills"), SkillOrigin::ProjectCursor),
            SkillRoot::direct(cwd.join(".codex/skills"), SkillOrigin::ProjectCodex),
            SkillRoot::direct(cwd.join(".claude/skills"), SkillOrigin::ProjectClaude),
        ]);
    }
    if let Some(home) = dirs::home_dir() {
        roots.extend([
            SkillRoot::direct(home.join(".agents/skills"), SkillOrigin::UserShared),
            SkillRoot::direct(home.join(".cursor/skills"), SkillOrigin::Cursor),
            SkillRoot::direct(home.join(".codex/skills"), SkillOrigin::Codex),
            SkillRoot::direct(home.join(".claude/skills"), SkillOrigin::Claude),
            SkillRoot::direct(home.join(".grok/skills"), SkillOrigin::Grok),
            SkillRoot::direct(home.join("plugins/abi-mega/skills"), SkillOrigin::AbiMega),
            SkillRoot::recursive(home.join(".codex/plugins/cache"), SkillOrigin::CodexPlugin),
            SkillRoot::recursive(
                home.join(".claude/plugins/cache"),
                SkillOrigin::ClaudePlugin,
            ),
        ]);
    }
    roots
}

pub fn list_skills() -> Result<Vec<SkillEntry>> {
    Ok(skill_inventory()?.entries)
}

pub(crate) fn skill_inventory() -> Result<SkillInventory> {
    Ok(scan_skills(&default_skill_roots()))
}

pub fn scan_skills(roots: &[SkillRoot]) -> SkillInventory {
    let mut inventory = SkillInventory::default();
    let mut seen_manifests = HashSet::new();
    let mut contents = BTreeMap::<String, Vec<(PathBuf, String)>>::new();
    let mut exact = BTreeMap::<(String, String), usize>::new();

    for root in roots {
        let manifests = if root.recursive {
            recursive_manifests(&root.path, 8)
        } else {
            direct_manifests(&root.path)
        };
        for manifest in manifests {
            let identity = fs::canonicalize(&manifest).unwrap_or_else(|_| manifest.clone());
            if !seen_manifests.insert(identity) {
                continue;
            }
            let Ok(text) = fs::read_to_string(&manifest) else {
                inventory.diagnostics.push(SkillDiagnostic {
                    name: manifest
                        .parent()
                        .and_then(Path::file_name)
                        .map(|name| name.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "unknown".into()),
                    paths: vec![manifest.clone()],
                    message: format!("could not read skill manifest {}", manifest.display()),
                });
                continue;
            };
            let dir = manifest.parent().unwrap_or(&root.path).to_path_buf();
            let parsed = parse_frontmatter(&text);
            let name = parsed.name.unwrap_or_else(|| {
                dir.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "unknown".into())
            });
            let description = parsed
                .description
                .unwrap_or_else(|| first_body_paragraph(&text));
            contents
                .entry(name.to_ascii_lowercase())
                .or_default()
                .push((manifest.clone(), text.clone()));
            let provenance = SkillProvenance {
                origin: root.origin,
                manifest: manifest.clone(),
            };
            let exact_key = (name.to_ascii_lowercase(), text);
            if let Some(existing) = exact.get(&exact_key).copied() {
                inventory.entries[existing].provenance.push(provenance);
                continue;
            }
            exact.insert(exact_key, inventory.entries.len());
            inventory.entries.push(SkillEntry {
                name,
                root: dir,
                manifest,
                description,
                origin: root.origin,
                provenance: vec![provenance],
            });
        }
    }

    for group in contents.values() {
        if group.len() < 2 || group.iter().all(|(_, text)| text == &group[0].1) {
            continue;
        }
        let name = inventory
            .entries
            .iter()
            .find(|entry| entry.manifest == group[0].0)
            .map(|entry| entry.name.clone())
            .unwrap_or_else(|| "unknown".into());
        let paths: Vec<_> = group.iter().map(|(path, _)| path.clone()).collect();
        inventory.diagnostics.push(SkillDiagnostic {
            name: name.clone(),
            paths: paths.clone(),
            message: format!(
                "divergent duplicate skill `{name}` is preserved from {} manifests",
                paths.len()
            ),
        });
    }
    inventory.entries.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
            .then_with(|| left.origin.cmp(&right.origin))
            .then_with(|| left.manifest.cmp(&right.manifest))
    });
    inventory
}

fn direct_manifests(root: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let manifest = path.join("SKILL.md");
            (path.is_dir() && manifest.is_file()).then_some(manifest)
        })
        .collect()
}

fn recursive_manifests(root: &Path, max_depth: usize) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = pending.pop() {
        let manifest = dir.join("SKILL.md");
        if manifest.is_file() {
            found.push(manifest);
            continue;
        }
        if depth >= max_depth {
            continue;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with('.'))
            {
                continue;
            }
            if fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_dir()) {
                pending.push((path, depth + 1));
            }
        }
    }
    found
}

#[derive(Default)]
struct Frontmatter {
    name: Option<String>,
    description: Option<String>,
}

fn parse_frontmatter(text: &str) -> Frontmatter {
    let lines: Vec<_> = text.lines().collect();
    if lines.first().map(|line| line.trim()) != Some("---") {
        return Frontmatter::default();
    }
    let end = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, line)| line.trim() == "---")
        .map(|(index, _)| index)
        .unwrap_or(lines.len());
    let mut parsed = Frontmatter::default();
    let mut index = 1;
    while index < end {
        let line = lines[index];
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let Some((key, value)) = trimmed.split_once(':') else {
            index += 1;
            continue;
        };
        let value = value.trim();
        if key == "name" {
            parsed.name = scalar(value);
        } else if key == "description" {
            if value == ">" || value.starts_with(">-") || value.starts_with(">+") {
                let (block, next) = yaml_block(&lines, index + 1, end, indent);
                parsed.description = Some(fold_yaml_block(&block));
                index = next;
                continue;
            }
            if value == "|" || value.starts_with("|-") || value.starts_with("|+") {
                let (block, next) = yaml_block(&lines, index + 1, end, indent);
                parsed.description = Some(block.join("\n").trim_end().to_string());
                index = next;
                continue;
            }
            parsed.description = scalar(value);
        }
        index += 1;
    }
    parsed
}

fn yaml_block(
    lines: &[&str],
    mut index: usize,
    end: usize,
    parent_indent: usize,
) -> (Vec<String>, usize) {
    let mut raw = Vec::new();
    let mut block_indent = None;
    while index < end {
        let line = lines[index];
        if line.trim().is_empty() {
            raw.push(String::new());
            index += 1;
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent <= parent_indent {
            break;
        }
        let block_indent = *block_indent.get_or_insert(indent);
        raw.push(line.chars().skip(block_indent).collect());
        index += 1;
    }
    (raw, index)
}

fn fold_yaml_block(lines: &[String]) -> String {
    let mut out = String::new();
    let mut previous_blank = false;
    for line in lines {
        if line.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            previous_blank = true;
        } else {
            if !out.is_empty() && !previous_blank {
                out.push(' ');
            }
            out.push_str(line.trim_end());
            previous_blank = false;
        }
    }
    out.trim_end().to_string()
}

fn scalar(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }
    Some(
        value
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .or_else(|| {
                value
                    .strip_prefix('\'')
                    .and_then(|value| value.strip_suffix('\''))
            })
            .unwrap_or(value)
            .to_string(),
    )
}

fn first_body_paragraph(text: &str) -> String {
    let mut frontmatter = text.lines().next().is_some_and(|line| line.trim() == "---");
    let mut body = Vec::new();
    for line in text.lines().skip(usize::from(frontmatter)) {
        if frontmatter {
            if line.trim() == "---" {
                frontmatter = false;
            }
            continue;
        }
        let line = line.trim();
        if line.is_empty() {
            if !body.is_empty() {
                break;
            }
        } else if !line.starts_with('#') {
            body.push(line);
        }
    }
    body.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root(label: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("abbey-skills-{label}-{stamp}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn requires_manifest_and_parses_folded_and_literal_descriptions() {
        let root = temp_root("yaml");
        fs::create_dir(root.join("missing")).unwrap();
        fs::create_dir(root.join("folded")).unwrap();
        fs::write(
            root.join("folded/SKILL.md"),
            "---\nname: folded\ndescription: >\n  one line\n  two line\n\n  next\n---\n",
        )
        .unwrap();
        fs::create_dir(root.join("literal")).unwrap();
        fs::write(
            root.join("literal/SKILL.md"),
            "---\nname: literal\ndescription: |\n  one line\n  two line\n---\n",
        )
        .unwrap();

        let got = scan_skills(&[SkillRoot::direct(&root, SkillOrigin::Custom)]);
        assert_eq!(got.entries.len(), 2);
        assert_eq!(
            got.entries
                .iter()
                .find(|skill| skill.name == "folded")
                .unwrap()
                .description,
            "one line two line\nnext"
        );
        assert_eq!(
            got.entries
                .iter()
                .find(|skill| skill.name == "literal")
                .unwrap()
                .description,
            "one line\ntwo line"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn preserves_divergent_duplicates_with_provenance() {
        let left = temp_root("left");
        let right = temp_root("right");
        for (root, description) in [(&left, "left"), (&right, "right")] {
            fs::create_dir(root.join("same")).unwrap();
            fs::write(
                root.join("same/SKILL.md"),
                format!("---\nname: same\ndescription: {description}\n---\n"),
            )
            .unwrap();
        }
        let got = scan_skills(&[
            SkillRoot::direct(&left, SkillOrigin::Codex),
            SkillRoot::direct(&right, SkillOrigin::Claude),
        ]);
        assert_eq!(got.entries.len(), 2);
        assert_eq!(got.diagnostics.len(), 1);
        assert!(got.diagnostics[0].message.contains("divergent duplicate"));
        assert_eq!(got.entries[0].origin, SkillOrigin::Codex);
        assert_eq!(got.entries[1].origin, SkillOrigin::Claude);
        fs::remove_dir_all(left).unwrap();
        fs::remove_dir_all(right).unwrap();
    }

    #[test]
    fn consolidates_exact_mirrors_without_losing_provenance() {
        let left = temp_root("mirror-left");
        let right = temp_root("mirror-right");
        let manifest = "---\nname: mirror\ndescription: identical\n---\n";
        for root in [&left, &right] {
            fs::create_dir(root.join("mirror")).unwrap();
            fs::write(root.join("mirror/SKILL.md"), manifest).unwrap();
        }
        let got = scan_skills(&[
            SkillRoot::direct(&left, SkillOrigin::Codex),
            SkillRoot::direct(&right, SkillOrigin::Claude),
        ]);
        assert_eq!(got.entries.len(), 1);
        assert_eq!(got.entries[0].provenance.len(), 2);
        assert!(got.diagnostics.is_empty());
        fs::remove_dir_all(left).unwrap();
        fs::remove_dir_all(right).unwrap();
    }
}
