//! Media path attach helpers for cursor-agent sessions.
//!
//! Abbey does **not** encode pixels or run a local vision model. It resolves
//! image/video paths, adds their parent dirs via `--add-dir`, and puts absolute
//! paths into the prompt so the backend (cursor-agent) can read them from the
//! workspace. Under `ABBEY_BACKEND=fm` this is still text-only.

use crate::agent::AgentConfig;
use anyhow::{Result, bail};
use std::path::{Path, PathBuf};

const IMAGE_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "svg",
];
const VIDEO_EXTS: &[&str] = &[
    "mp4", "mov", "mkv", "webm", "avi", "m4v", "gifv", "mpeg", "mpg",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    Image,
    Video,
}

#[derive(Debug, Clone, Default)]
pub struct MediaAttach {
    pub paths: Vec<(PathBuf, MediaKind)>,
}

impl MediaAttach {
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Merge CLI / slash / prompt-discovered paths (dedup by canonical path).
    pub fn extend_paths(&mut self, paths: impl IntoIterator<Item = (PathBuf, MediaKind)>) {
        for (p, kind) in paths {
            if self.paths.iter().any(|(e, _)| e == &p) {
                continue;
            }
            self.paths.push((p, kind));
        }
    }

    pub fn prompt_note(&self) -> String {
        if self.paths.is_empty() {
            return String::new();
        }
        let mut out = String::from(
            "Attached media paths (read these files from the workspace; Abbey does not \
             embed pixels — the agent backend must open them):\n",
        );
        for (p, kind) in &self.paths {
            let label = match kind {
                MediaKind::Image => "image",
                MediaKind::Video => "video",
            };
            out.push_str(&format!("- ({label}) {}\n", p.display()));
        }
        out.push('\n');
        out
    }

    /// Ensure each file's parent is on `--add-dir` so cursor-agent can see it.
    ///
    /// Attaching a file widens the agent's readable scope to that file's whole
    /// directory, which is easy to miss: `--image ~/.ssh/key.png` grants read
    /// access to `~/.ssh`. Each newly added directory is announced on stderr so
    /// the grant is never silent.
    pub fn apply_add_dirs(&self, cfg: &mut AgentConfig) {
        for (p, _) in &self.paths {
            if let Some(parent) = p.parent() {
                let parent = parent.to_path_buf();
                if parent.as_os_str().is_empty() {
                    continue;
                }
                if !cfg.add_dirs.iter().any(|d| d == &parent) {
                    eprintln!(
                        "abbey: media attach grants the agent read access to {}",
                        parent.display()
                    );
                    cfg.add_dirs.push(parent);
                }
            }
        }
    }
}

pub fn kind_for_path(path: &Path) -> Option<MediaKind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if IMAGE_EXTS.contains(&ext.as_str()) {
        Some(MediaKind::Image)
    } else if VIDEO_EXTS.contains(&ext.as_str()) {
        Some(MediaKind::Video)
    } else {
        None
    }
}

pub fn looks_like_media_token(token: &str) -> bool {
    let t = token.trim().trim_matches('"').trim_matches('\'');
    if t.is_empty() {
        return false;
    }
    kind_for_path(Path::new(t)).is_some()
}

/// Resolve user-supplied paths; require the file to exist.
pub fn resolve_media_path(raw: &str, forced: Option<MediaKind>) -> Result<(PathBuf, MediaKind)> {
    let trimmed = raw.trim().trim_matches('"').trim_matches('\'');
    if trimmed.is_empty() {
        bail!("media path is empty");
    }
    let path = PathBuf::from(trimmed);
    let abs = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()?.join(path)
    };
    let abs = abs.canonicalize().unwrap_or(abs);
    if !abs.is_file() {
        bail!("media file not found: {}", abs.display());
    }
    let kind = forced.or_else(|| kind_for_path(&abs)).ok_or_else(|| {
        anyhow::anyhow!(
            "unrecognized media extension for {} (image/video only)",
            abs.display()
        )
    })?;
    Ok((abs, kind))
}

pub fn collect(images: &[PathBuf], videos: &[PathBuf], media: &[PathBuf]) -> Result<MediaAttach> {
    let mut out = MediaAttach::default();
    for p in images {
        out.extend_paths([resolve_media_path(
            &p.display().to_string(),
            Some(MediaKind::Image),
        )?]);
    }
    for p in videos {
        out.extend_paths([resolve_media_path(
            &p.display().to_string(),
            Some(MediaKind::Video),
        )?]);
    }
    for p in media {
        out.extend_paths([resolve_media_path(&p.display().to_string(), None)?]);
    }
    Ok(out)
}

/// Pull existing media path tokens out of a prompt (does not require `--image`).
pub fn discover_in_prompt(words: &[String]) -> MediaAttach {
    let mut out = MediaAttach::default();
    for w in words {
        if !looks_like_media_token(w) {
            continue;
        }
        if let Ok(pair) = resolve_media_path(w, None) {
            out.extend_paths([pair]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn kind_detects_image_and_video() {
        assert_eq!(kind_for_path(Path::new("a.PNG")), Some(MediaKind::Image));
        assert_eq!(kind_for_path(Path::new("clip.mov")), Some(MediaKind::Video));
        assert_eq!(kind_for_path(Path::new("main.rs")), None);
    }

    #[test]
    fn resolve_and_note_include_absolute_path() {
        let dir = std::env::temp_dir().join(format!("abbey-media-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let img = dir.join("shot.png");
        fs::write(&img, b"fake").unwrap();
        let (abs, kind) = resolve_media_path(&img.display().to_string(), None).unwrap();
        assert_eq!(kind, MediaKind::Image);
        let mut attach = MediaAttach::default();
        attach.extend_paths([(abs.clone(), kind)]);
        let note = attach.prompt_note();
        assert!(note.contains("(image)"));
        assert!(note.contains(&abs.display().to_string()));
        let mut cfg = AgentConfig::default();
        attach.apply_add_dirs(&mut cfg);
        assert!(
            cfg.add_dirs
                .iter()
                .any(|d| d == dir.as_path() || d == abs.parent().unwrap())
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
