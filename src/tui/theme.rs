//! Abbey TUI color palettes and persistence.

use ratatui::style::Color;
use std::fs;
use std::path::{Path, PathBuf};

const ENV_THEME: &str = "ABBEY_TUI_THEME";
const THEME_FILE: &str = "tui-theme";

/// Named TUI palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    Ink,
    Violet,
    Mono,
}

impl ThemeId {
    /// Parse `ink`, `violet`, or `mono` (case-insensitive). Whitespace is trimmed.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ink" => Some(Self::Ink),
            "violet" => Some(Self::Violet),
            "mono" => Some(Self::Mono),
            _ => None,
        }
    }

    /// Stable lowercase name written to disk and shown in the UI.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ink => "ink",
            Self::Violet => "violet",
            Self::Mono => "mono",
        }
    }

    /// Resolve order: `ABBEY_TUI_THEME` env > `{state_dir}/tui-theme` file > [`Ink`].
    pub fn resolve(state_dir: &Path) -> Self {
        if let Ok(raw) = std::env::var(ENV_THEME) {
            if let Some(id) = Self::parse(&raw) {
                return id;
            }
        }
        if let Ok(contents) = fs::read_to_string(theme_file_path(state_dir)) {
            if let Some(id) = Self::parse(&contents) {
                return id;
            }
        }
        Self::Ink
    }

    /// Persist the theme id to `{state_dir}/tui-theme`.
    pub fn save(state_dir: &Path, id: Self) -> std::io::Result<()> {
        fs::create_dir_all(state_dir)?;
        fs::write(theme_file_path(state_dir), format!("{}\n", id.as_str()))
    }

    /// Cycle ink → violet → mono → ink.
    pub fn cycle(self) -> Self {
        match self {
            Self::Ink => Self::Violet,
            Self::Violet => Self::Mono,
            Self::Mono => Self::Ink,
        }
    }
}

fn theme_file_path(state_dir: &Path) -> PathBuf {
    state_dir.join(THEME_FILE)
}

/// Full RGB palette for ratatui drawing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub fg_dim: Color,
    pub accent: Color,
    pub accent_dim: Color,
    pub ok: Color,
    pub warn: Color,
    pub error: Color,
    pub border: Color,
    pub border_focus: Color,
    pub chip_bg: Color,
    pub chip_fg: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub prompt_border: Color,
    pub header_pulse: Color,
}

impl Theme {
    pub fn from_id(id: ThemeId) -> Self {
        match id {
            ThemeId::Ink => Self::ink(),
            ThemeId::Violet => Self::violet(),
            ThemeId::Mono => Self::mono(),
        }
    }

    /// Deep ink background, teal accent, warm parchment text.
    fn ink() -> Self {
        Self {
            bg: rgb(14, 18, 26),
            fg: rgb(235, 225, 200),
            fg_dim: rgb(150, 140, 118),
            accent: rgb(72, 188, 176),
            accent_dim: rgb(48, 128, 120),
            ok: rgb(96, 196, 140),
            warn: rgb(224, 176, 88),
            error: rgb(224, 96, 96),
            border: rgb(56, 72, 84),
            border_focus: rgb(72, 188, 176),
            chip_bg: rgb(24, 34, 44),
            chip_fg: rgb(235, 225, 200),
            selection_bg: rgb(28, 52, 58),
            selection_fg: rgb(245, 240, 228),
            prompt_border: rgb(72, 188, 176),
            header_pulse: rgb(96, 210, 196),
        }
    }

    /// Refined Abbey violet — legacy TUI feel.
    fn violet() -> Self {
        Self {
            bg: rgb(18, 12, 28),
            fg: rgb(230, 228, 240),
            fg_dim: rgb(120, 120, 140),
            accent: rgb(180, 120, 255),
            accent_dim: rgb(120, 80, 180),
            ok: rgb(120, 220, 160),
            warn: rgb(240, 180, 80),
            error: rgb(255, 100, 120),
            border: rgb(80, 60, 100),
            border_focus: rgb(180, 120, 255),
            chip_bg: rgb(40, 30, 60),
            chip_fg: rgb(240, 236, 248),
            selection_bg: rgb(40, 30, 60),
            selection_fg: rgb(255, 255, 255),
            prompt_border: rgb(180, 120, 255),
            header_pulse: rgb(200, 150, 255),
        }
    }

    /// Near-monochrome chrome; status colors stay vivid.
    fn mono() -> Self {
        Self {
            bg: rgb(20, 20, 22),
            fg: rgb(200, 200, 200),
            fg_dim: rgb(120, 120, 120),
            accent: rgb(180, 180, 180),
            accent_dim: rgb(100, 100, 100),
            ok: rgb(120, 220, 160),
            warn: rgb(240, 180, 80),
            error: rgb(255, 90, 90),
            border: rgb(60, 60, 60),
            border_focus: rgb(140, 140, 140),
            chip_bg: rgb(35, 35, 38),
            chip_fg: rgb(200, 200, 200),
            selection_bg: rgb(50, 50, 55),
            selection_fg: rgb(255, 255, 255),
            prompt_border: rgb(100, 100, 100),
            header_pulse: rgb(160, 160, 160),
        }
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

#[cfg(test)]
// `std::env::{set_var, remove_var}` are unsafe in edition 2024; these tests
// drive the theme env override and serialise on `env_test_guard`. Test-only:
// the crate otherwise denies `unsafe_code`.
#[allow(unsafe_code)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_test_guard() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn temp_state(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("abbey-theme-{}-{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parse_accepts_known_ids_case_insensitive() {
        assert_eq!(ThemeId::parse("ink"), Some(ThemeId::Ink));
        assert_eq!(ThemeId::parse("INK"), Some(ThemeId::Ink));
        assert_eq!(ThemeId::parse("  Violet  "), Some(ThemeId::Violet));
        assert_eq!(ThemeId::parse("MONO"), Some(ThemeId::Mono));
        assert_eq!(ThemeId::parse("sepia"), None);
        assert_eq!(ThemeId::parse(""), None);
    }

    #[test]
    fn resolve_defaults_to_ink_without_env_or_file() {
        let _guard = env_test_guard();
        unsafe { std::env::remove_var(ENV_THEME) };
        let dir = temp_state("default");
        assert_eq!(ThemeId::resolve(&dir), ThemeId::Ink);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_reads_persisted_file_when_env_unset() {
        let _guard = env_test_guard();
        unsafe { std::env::remove_var(ENV_THEME) };
        let dir = temp_state("file");
        ThemeId::save(&dir, ThemeId::Violet).unwrap();
        assert_eq!(ThemeId::resolve(&dir), ThemeId::Violet);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_env_beats_file() {
        let _guard = env_test_guard();
        let dir = temp_state("env-beats-file");
        ThemeId::save(&dir, ThemeId::Violet).unwrap();
        unsafe { std::env::set_var(ENV_THEME, "mono") };
        assert_eq!(ThemeId::resolve(&dir), ThemeId::Mono);
        unsafe { std::env::remove_var(ENV_THEME) };
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_writes_theme_name() {
        let dir = temp_state("save");
        ThemeId::save(&dir, ThemeId::Mono).unwrap();
        let contents = fs::read_to_string(dir.join(THEME_FILE)).unwrap();
        assert_eq!(contents.trim(), "mono");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn cycle_rotates_ink_violet_mono() {
        assert_eq!(ThemeId::Ink.cycle(), ThemeId::Violet);
        assert_eq!(ThemeId::Violet.cycle(), ThemeId::Mono);
        assert_eq!(ThemeId::Mono.cycle(), ThemeId::Ink);
    }

    #[test]
    fn from_id_builds_distinct_palettes() {
        let ink = Theme::from_id(ThemeId::Ink);
        let violet = Theme::from_id(ThemeId::Violet);
        let mono = Theme::from_id(ThemeId::Mono);
        assert_ne!(ink.accent, violet.accent);
        assert_ne!(ink.bg, mono.bg);
        assert_eq!(mono.ok, rgb(120, 220, 160));
        assert_eq!(violet.accent, rgb(180, 120, 255));
    }
}
