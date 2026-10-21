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
    /// A light palette, for a light terminal profile.
    ///
    /// The other three are all dark. A terminal in a light profile inherits
    /// that profile's background, and a dark palette on top of it is
    /// either illegible or a rectangle of black in a white window. This one
    /// is the palette for that case: a paper-white ground, ink text, and
    /// the same teal accent carried over from [`ThemeId::Ink`].
    Daylight,
}

impl ThemeId {
    /// Parse `ink`, `violet`, `mono`, or `daylight` (case-insensitive).
    /// Whitespace is trimmed.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "ink" => Some(Self::Ink),
            "violet" => Some(Self::Violet),
            "mono" => Some(Self::Mono),
            "daylight" => Some(Self::Daylight),
            _ => None,
        }
    }

    /// Stable lowercase name written to disk and shown in the UI.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ink => "ink",
            Self::Violet => "violet",
            Self::Mono => "mono",
            Self::Daylight => "daylight",
        }
    }

    /// Every theme, in cycle order.
    pub fn all() -> [Self; 4] {
        [Self::Ink, Self::Violet, Self::Mono, Self::Daylight]
    }

    /// The theme the user has *chosen*, if any.
    ///
    /// The env override and the persisted file are the user's choice, and
    /// they win outright. This is what [`Self::resolve`] consults before it lets
    /// the terminal's appearance decide.
    pub fn chosen(state_dir: &Path) -> Option<Self> {
        if let Ok(raw) = std::env::var(ENV_THEME)
            && let Some(id) = Self::parse(&raw)
        {
            return Some(id);
        }
        if let Ok(contents) = fs::read_to_string(theme_file_path(state_dir))
            && let Some(id) = Self::parse(&contents)
        {
            return Some(id);
        }
        None
    }

    /// Resolve the theme for this session.
    ///
    /// Order: the user's choice (env override, then the persisted file) >
    /// the terminal's reported appearance > [`ThemeId::Ink`].
    ///
    /// The choice wins outright, because a theme the user picked is theirs.
    /// Only when they have not picked one does the terminal's brightness
    /// decide, and a terminal that does not answer leaves the dark palette —
    /// a palette must never be chosen from a value the app could not read.
    pub fn resolve(state_dir: &Path, brightness: TerminalBrightness) -> Self {
        decide(Self::chosen(state_dir), brightness)
    }

    /// Persist the theme id to `{state_dir}/tui-theme`.
    pub fn save(state_dir: &Path, id: Self) -> std::io::Result<()> {
        fs::create_dir_all(state_dir)?;
        fs::write(theme_file_path(state_dir), format!("{}\n", id.as_str()))
    }

    /// Cycle ink → violet → mono → daylight → ink.
    pub fn cycle(self) -> Self {
        let all = Self::all();
        let index = all.iter().position(|id| *id == self).unwrap_or(0);
        all[(index + 1) % all.len()]
    }
}

/// What the terminal said about its own background.
///
/// A terminal can report its default foreground and background colours
/// (the `OSC 10` / `OSC 11` queries). That is the only way a TUI can learn
/// whether it is running in a light or a dark profile, and it is the answer
/// that matters: a dark palette drawn on a light terminal is either
/// illegible or a rectangle of black in a white window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalBrightness {
    /// The terminal reported a dark background, or did not answer.
    Dark,
    /// The terminal reported a light background.
    Light,
}

impl TerminalBrightness {
    /// Classify a terminal's reported background colour.
    ///
    /// Uses the same luminance rule as the palette check below, so the
    /// decision and the assertion agree: a colour is light when the mean of
    /// its channels is above the midpoint. A terminal that answers with
    /// something that is not a colour is treated as dark, because a palette
    /// must never be chosen from a value that could not be understood.
    pub fn from_background(background: Option<(u8, u8, u8)>) -> Self {
        match background {
            Some((r, g, b)) if mean(r, g, b) > 127 => Self::Light,
            _ => Self::Dark,
        }
    }

    /// The palette this brightness implies.
    ///
    /// The dark profiles keep their own palette, because the three dark
    /// themes are a *choice* — ink, violet, mono — and the terminal's
    /// brightness does not narrow that choice. Only the light profile has
    /// one obvious palette.
    pub fn default_theme(self) -> ThemeId {
        match self {
            Self::Dark => ThemeId::Ink,
            Self::Light => ThemeId::Daylight,
        }
    }
}

/// The mean of three channels.
fn mean(r: u8, g: u8, b: u8) -> u32 {
    (r as u32 + g as u32 + b as u32) / 3
}

/// The pure decision: which theme should the TUI use?
///
/// `chosen` is what the user selected — the env override, the persisted
/// file, or the theme they last cycled to. `brightness` is what the terminal
/// reported. A user who has *chosen* a theme keeps it, whatever the
/// terminal says: the choice is theirs. Only when nothing has been chosen
/// does the terminal's own appearance decide, and then only between the
/// one obvious palette for that appearance.
///
/// The ordering matters. An appearance-following default that overrode an
/// explicit choice would make the choice meaningless.
pub fn decide(chosen: Option<ThemeId>, brightness: TerminalBrightness) -> ThemeId {
    chosen.unwrap_or_else(|| brightness.default_theme())
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
            ThemeId::Daylight => Self::daylight(),
        }
    }

    /// A light palette for a light terminal profile.
    ///
    /// The accent is the same teal the ink palette uses, so the two read as
    /// one design in two light conditions rather than two designs. The
    /// status colors are darkened, because the vivid versions are chosen
    /// for a dark ground and lose their contrast on a light one.
    fn daylight() -> Self {
        Self {
            bg: rgb(250, 249, 245),
            fg: rgb(28, 30, 34),
            fg_dim: rgb(108, 112, 120),
            accent: rgb(14, 118, 110),
            accent_dim: rgb(60, 132, 126),
            ok: rgb(24, 122, 84),
            warn: rgb(160, 100, 12),
            error: rgb(176, 42, 42),
            border: rgb(196, 198, 204),
            border_focus: rgb(14, 118, 110),
            chip_bg: rgb(232, 234, 238),
            chip_fg: rgb(28, 30, 34),
            selection_bg: rgb(206, 232, 228),
            selection_fg: rgb(18, 20, 24),
            prompt_border: rgb(14, 118, 110),
            header_pulse: rgb(20, 140, 130),
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
        assert_eq!(
            ThemeId::resolve(&dir, TerminalBrightness::Dark),
            ThemeId::Ink
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_reads_persisted_file_when_env_unset() {
        let _guard = env_test_guard();
        unsafe { std::env::remove_var(ENV_THEME) };
        let dir = temp_state("file");
        ThemeId::save(&dir, ThemeId::Violet).unwrap();
        assert_eq!(
            ThemeId::resolve(&dir, TerminalBrightness::Dark),
            ThemeId::Violet
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_env_beats_file() {
        let _guard = env_test_guard();
        let dir = temp_state("env-beats-file");
        ThemeId::save(&dir, ThemeId::Violet).unwrap();
        unsafe { std::env::set_var(ENV_THEME, "mono") };
        assert_eq!(
            ThemeId::resolve(&dir, TerminalBrightness::Dark),
            ThemeId::Mono
        );
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
        assert_eq!(ThemeId::Mono.cycle(), ThemeId::Daylight);
        assert_eq!(ThemeId::Daylight.cycle(), ThemeId::Ink);
    }

    #[test]
    fn the_cycle_visits_every_theme_exactly_once() {
        // A palette that cannot be reached by cycling is a palette nobody
        // finds: the theme is chosen by pressing a key, not by editing a
        // file.
        let mut seen = Vec::new();
        let mut id = ThemeId::Ink;
        for _ in 0..ThemeId::all().len() {
            assert!(!seen.contains(&id), "the cycle revisited {id:?}");
            seen.push(id);
            id = id.cycle();
        }
        assert_eq!(seen.len(), ThemeId::all().len());
        assert_eq!(
            id,
            ThemeId::Ink,
            "the cycle must return to where it started"
        );
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

    #[test]
    fn the_daylight_palette_is_readable_on_a_light_ground() {
        // The reason this palette exists: the other three are dark, and a
        // dark palette in a light terminal profile is illegible. A light
        // palette must have a light background, and every colour that
        // carries text must be *darker* than that background — the only
        // relation that holds on a light ground.
        let day = Theme::from_id(ThemeId::Daylight);
        assert!(is_light(day.bg), "the daylight background must be light");
        assert!(
            !is_light(day.fg),
            "the daylight text must be dark, not light"
        );
        assert!(
            is_darker(day.fg, day.bg),
            "text must be darker than its ground"
        );
        assert!(
            is_darker(day.fg_dim, day.bg),
            "dim text must be darker than its ground"
        );
        assert!(
            is_darker(day.accent, day.bg),
            "the accent must be darker than its ground"
        );
        assert!(
            is_darker(day.chip_fg, day.chip_bg),
            "chip text must be darker than its chip"
        );
    }

    #[test]
    fn the_daylight_palette_keeps_the_ink_accent_family() {
        // The light palette is the same design in another light condition,
        // so it carries the ink palette's teal rather than a new hue.
        let ink = Theme::from_id(ThemeId::Ink);
        let day = Theme::from_id(ThemeId::Daylight);
        let hue = |color: Color| match color {
            Color::Rgb(r, g, b) => (r, g, b),
            _ => panic!("the palettes are RGB"),
        };
        let (ir, ig, ib) = hue(ink.accent);
        let (dr, dg, db) = hue(day.accent);
        // The accent family is teal: green and blue dominate, red does not.
        for (r, g, b) in [(ir, ig, ib), (dr, dg, db)] {
            assert!(g > r, "teal keeps green above red: {r},{g},{b}");
            assert!(b >= r, "teal keeps blue at or above red: {r},{g},{b}");
        }
    }

    #[test]
    fn every_theme_names_itself_and_parses_back() {
        for id in ThemeId::all() {
            let name = id.as_str();
            assert_eq!(ThemeId::parse(name), Some(id), "{name} must parse back");
            assert_eq!(
                ThemeId::parse(&name.to_uppercase()),
                Some(id),
                "{name} must parse case-insensitively"
            );
        }
    }

    /// Whether a colour is on the light side of the range.
    fn is_light(color: Color) -> bool {
        match color {
            Color::Rgb(r, g, b) => (r as u32 + g as u32 + b as u32) / 3 > 127,
            _ => false,
        }
    }

    /// Whether `a` is darker than `b`.
    fn is_darker(a: Color, b: Color) -> bool {
        match (a, b) {
            (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) => {
                (ar as u32 + ag as u32 + ab as u32) < (br as u32 + bg as u32 + bb as u32)
            }
            _ => false,
        }
    }

    #[test]
    fn a_chosen_theme_beats_the_terminals_appearance() {
        // The choice is the user's. An appearance-following default that
        // overrode it would make the choice meaningless.
        for chosen in ThemeId::all() {
            for brightness in [TerminalBrightness::Dark, TerminalBrightness::Light] {
                assert_eq!(
                    decide(Some(chosen), brightness),
                    chosen,
                    "{chosen:?} must survive a {brightness:?} terminal"
                );
            }
        }
    }

    #[test]
    fn an_unchosen_theme_follows_the_terminals_appearance() {
        assert_eq!(decide(None, TerminalBrightness::Light), ThemeId::Daylight);
        assert_eq!(decide(None, TerminalBrightness::Dark), ThemeId::Ink);
    }

    #[test]
    fn the_light_profile_picks_the_light_palette() {
        // The three dark palettes stay a choice among themselves; only the
        // light profile has one obvious palette.
        assert_eq!(TerminalBrightness::Light.default_theme(), ThemeId::Daylight);
        assert_eq!(TerminalBrightness::Dark.default_theme(), ThemeId::Ink);
    }

    #[test]
    fn a_terminal_that_answers_light_resolves_to_the_light_palette() {
        // The end-to-end rule: a light terminal with no chosen theme gets the
        // light palette.
        let _guard = env_test_guard();
        unsafe { std::env::remove_var(ENV_THEME) };
        let dir = temp_state("light-terminal");
        assert_eq!(
            ThemeId::resolve(&dir, TerminalBrightness::Light),
            ThemeId::Daylight
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_chosen_theme_survives_a_light_terminal() {
        let _guard = env_test_guard();
        unsafe { std::env::remove_var(ENV_THEME) };
        let dir = temp_state("chosen-over-light");
        ThemeId::save(&dir, ThemeId::Violet).unwrap();
        assert_eq!(
            ThemeId::resolve(&dir, TerminalBrightness::Light),
            ThemeId::Violet
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_env_override_survives_a_light_terminal() {
        let _guard = env_test_guard();
        let dir = temp_state("env-over-light");
        unsafe { std::env::set_var(ENV_THEME, "mono") };
        assert_eq!(
            ThemeId::resolve(&dir, TerminalBrightness::Light),
            ThemeId::Mono
        );
        unsafe { std::env::remove_var(ENV_THEME) };
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_terminal_that_does_not_answer_keeps_the_dark_palette() {
        // A palette must never be chosen from a value the app could not read.
        let _guard = env_test_guard();
        unsafe { std::env::remove_var(ENV_THEME) };
        let dir = temp_state("silent-terminal");
        assert_eq!(
            ThemeId::resolve(&dir, TerminalBrightness::Dark),
            ThemeId::Ink
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
