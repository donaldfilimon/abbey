//! Voice I/O: macOS `say` / Speech.framework, plus portable host tools.
//!
//! - **TTS**: `say(1)` on macOS (Premium/Enhanced/Sol/Siri when installed);
//!   espeak-ng/piper on Unix; Windows SAPI when those tools exist
//! - **STT**: Apple Speech helper on macOS; Whisper.cpp tiny/base when a CLI
//!   and ggml weight are on the host
//! - No cloud TTS/STT subscription; no bundled Whisper/Piper weights
//!
//! Missing host tools refuse honestly. Local neural speech remains Proposed.

use crate::agent::AgentConfig;
use crate::state::AbbeyState;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
pub struct VoiceInfo {
    pub name: String,
    pub locale: String,
    /// 3 = Premium, 2 = Enhanced, 1 = compact/standard, 0 = novelty
    pub quality: u8,
}

const NOVELTY: &[&str] = &[
    "Bad News",
    "Bahh",
    "Bells",
    "Boing",
    "Bubbles",
    "Cellos",
    "Good News",
    "Jester",
    "Junior",
    "Kathy",
    "Organ",
    "Pipe Organ",
    "Trinoids",
    "Whisper",
    "Wobble",
    "Zarvox",
    "Albert",
    "Fred",
    "Ralph",
    "Superstar",
    "Eddy",
    "Flo",
    "Reed",
    "Rocko",
    "Sandy",
    "Shelley",
];

/// Known natural compact voices — used when Premium/Enhanced aren't installed.
const NATURAL: &[&str] = &[
    "Sol", "Samantha", "Ava", "Allison", "Evan", "Nathan", "Zoe", "Nicky", "Noelle", "Susan",
    "Victoria", "Karen", "Daniel", "Moira", "Fiona", "Tessa", "Veena", "Rishi", "Serena",
];

fn is_macos() -> bool {
    cfg!(target_os = "macos")
}

fn refuse_platform(feature: &str) -> Result<i32> {
    eprintln!(
        "abbey: `{feature}` has no backend on this host.\n\
         macOS: `say` + Speech.framework (Current).\n\
         Portable: espeak-ng TTS; Piper with ABBEY_PIPER_MODEL and a host player; whisper-cli + ggml-tiny.bin STT.\n\
         Bundled neural speech is Proposed — Abbey does not ship Whisper/Piper weights."
    );
    Ok(2)
}

/// Parse one `say -v '?'` line into a voice record.
pub fn parse_voice_line(line: &str) -> Option<VoiceInfo> {
    let left = line.split('#').next()?.trim();
    if left.is_empty() || left.starts_with("Hearing") {
        return None;
    }
    // Locale is typically the last xx_YY token.
    let mut locale = String::new();
    let mut name_end = left.len();
    for tok in left.split_whitespace().rev() {
        if tok.len() == 5 && tok.as_bytes().get(2) == Some(&b'_') {
            locale = tok.to_string();
            if let Some(pos) = left.rfind(tok) {
                name_end = pos;
            }
            break;
        }
    }
    let name = left[..name_end].trim().to_string();
    if name.is_empty() {
        return None;
    }
    let mut quality = voice_quality(&name);
    if quality > 0
        && (line.contains("Siri")
            || name.eq_ignore_ascii_case("Sol")
            || name.to_ascii_lowercase().starts_with("sol "))
    {
        quality = quality.max(if name.eq_ignore_ascii_case("Sol") {
            3
        } else {
            2
        });
    }
    Some(VoiceInfo {
        name,
        locale,
        quality,
    })
}

pub fn voice_quality(name: &str) -> u8 {
    let lower = name.to_ascii_lowercase();
    if NOVELTY
        .iter()
        .any(|n| name.eq_ignore_ascii_case(n) || lower.starts_with(&n.to_ascii_lowercase()))
    {
        return 0;
    }
    if lower.contains("premium") || lower == "sol" || lower.starts_with("sol ") {
        3
    } else if lower.contains("enhanced") {
        2
    } else {
        1
    }
}

pub fn list_voices() -> Result<Vec<VoiceInfo>> {
    if !is_macos() {
        bail!("voice listing requires macOS");
    }
    let out = Command::new("say")
        .args(["-v", "?"])
        .output()
        .context("run say -v '?'")?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut voices: Vec<VoiceInfo> = text.lines().filter_map(parse_voice_line).collect();
    voices.sort_by(|a, b| b.quality.cmp(&a.quality).then_with(|| a.name.cmp(&b.name)));
    Ok(voices)
}

/// Pick the highest-quality voice for a locale prefix (e.g. `en`).
pub fn best_voice(locale_prefix: &str, preferred: Option<&str>) -> Result<VoiceInfo> {
    let voices = list_voices()?;
    if let Some(want) = preferred {
        if let Some(v) = voices.iter().find(|v| v.name.eq_ignore_ascii_case(want)) {
            return Ok(v.clone());
        }
        // substring match: "Zoe" → "Zoe (Premium)"
        if let Some(v) = voices.iter().find(|v| {
            v.name
                .to_ascii_lowercase()
                .contains(&want.to_ascii_lowercase())
        }) {
            return Ok(v.clone());
        }
        bail!("voice not found: {want} (try `abbey voice voices`)");
    }
    let pref = locale_prefix.to_ascii_lowercase();
    let filtered: Vec<_> = voices
        .iter()
        .filter(|v| v.quality > 0)
        .filter(|v| {
            pref.is_empty()
                || v.locale.to_ascii_lowercase().starts_with(&pref)
                || (pref == "en" && v.locale.to_ascii_lowercase().starts_with("en"))
        })
        .cloned()
        .collect();
    filtered
        .into_iter()
        .max_by_key(|v| (locale_boost(&v.locale), v.quality, name_boost(&v.name)))
        .or_else(|| voices.into_iter().find(|v| v.quality > 0))
        .context("no usable voices found")
}

fn locale_boost(locale: &str) -> u8 {
    match locale {
        "en_US" => 3,
        "en_GB" | "en_AU" => 2,
        l if l.starts_with("en") => 1,
        _ => 0,
    }
}

fn name_boost(name: &str) -> u8 {
    if NATURAL
        .iter()
        .any(|n| name == *n || name.starts_with(&format!("{n} ")))
    {
        2
    } else {
        0
    }
}

fn configured_voice() -> Option<String> {
    std::env::var("ABBEY_VOICE").ok().filter(|s| !s.is_empty())
}

fn configured_rate() -> Option<u32> {
    std::env::var("ABBEY_VOICE_RATE")
        .ok()
        .and_then(|s| s.parse().ok())
}

fn configured_locale() -> String {
    std::env::var("ABBEY_VOICE_LOCALE").unwrap_or_else(|_| "en".into())
}

/// Speak text with the best available (or requested) voice.
pub fn speak(
    text: &str,
    voice: Option<&str>,
    rate: Option<u32>,
    out: Option<&Path>,
) -> Result<i32> {
    if !is_macos() {
        if crate::voice_portable::has_portable_tts() {
            return crate::voice_portable::speak_portable(text, voice, rate, out);
        }
        return refuse_platform("voice speak");
    }
    let text = text.trim();
    if text.is_empty() {
        bail!("usage: abbey voice speak [-v VOICE] [-r RATE] [-o FILE] <text…>");
    }
    let chosen = best_voice(
        &configured_locale(),
        voice.or(configured_voice().as_deref()),
    )?;
    let rate = rate.or(configured_rate()).unwrap_or(match chosen.quality {
        3 => 175, // Premium — slightly slower sounds more natural
        2 => 180,
        _ => 190,
    });

    let mut cmd = Command::new("say");
    cmd.arg("-v").arg(&chosen.name);
    cmd.arg("-r").arg(rate.to_string());
    if let Some(path) = out {
        cmd.arg("-o").arg(path);
        // Prefer AAC m4a when extension suggests it — higher-quality file than AIFF-C default.
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("m4a") || e.eq_ignore_ascii_case("aac"))
        {
            cmd.arg("--data-format=aac");
        }
        eprintln!(
            "abbey: voice speak → {} (quality={}, rate={rate}) → {}",
            chosen.name,
            quality_label(chosen.quality),
            path.display()
        );
    } else {
        eprintln!(
            "abbey: voice speak → {} (quality={}, rate={rate})",
            chosen.name,
            quality_label(chosen.quality)
        );
    }
    if chosen.quality < 2 {
        eprintln!(
            "abbey: tip — download Premium/Enhanced voices in\n\
             System Settings → Accessibility → Spoken Content → System voice ⓘ"
        );
    }
    cmd.arg(text);
    let st = cmd.status().context("say")?;
    Ok(st.code().unwrap_or(1))
}

fn quality_label(q: u8) -> &'static str {
    match q {
        3 => "Premium",
        2 => "Enhanced",
        1 => "standard",
        _ => "novelty",
    }
}

fn stt_source() -> PathBuf {
    // Prefer repo script when developing; fall back to install location next to binary.
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/abbey-stt.swift");
    if here.is_file() {
        return here;
    }
    if let Ok(exe) = std::env::current_exe() {
        let sibling = exe
            .parent()
            .unwrap_or(Path::new("."))
            .join("../share/abbey/abbey-stt.swift");
        if sibling.is_file() {
            return sibling;
        }
    }
    here
}

fn ensure_stt_binary(state: &AbbeyState) -> Result<PathBuf> {
    let bin_dir = state.state_dir.join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let bin = bin_dir.join("abbey-stt");
    let src = stt_source();
    if !src.is_file() {
        bail!(
            "missing STT helper source at {}\n\
             expected scripts/abbey-stt.swift in the abbey checkout",
            src.display()
        );
    }
    let needs_build = match (bin.metadata(), src.metadata()) {
        (Ok(b), Ok(s)) => b.modified().ok() < s.modified().ok(),
        _ => true,
    };
    if needs_build {
        eprintln!("abbey: building on-device STT helper → {}", bin.display());
        let st = Command::new("swiftc")
            .args([
                "-O",
                "-o",
                bin.to_str().unwrap_or("abbey-stt"),
                src.to_str().unwrap_or("abbey-stt.swift"),
                "-framework",
                "Speech",
                "-framework",
                "AVFoundation",
                "-framework",
                "Foundation",
            ])
            .status()
            .context("swiftc (Xcode CLT required for voice listen)")?;
        if !st.success() {
            bail!("swiftc failed building abbey-stt (install Xcode CLT)");
        }
    }
    Ok(bin)
}

/// Listen on the mic and return recognized text (on-device when available).
pub fn listen(state: &AbbeyState, seconds: f64, locale: Option<&str>) -> Result<String> {
    if !is_macos() {
        return crate::voice_portable::listen_whisper(seconds, None);
    }
    let bin = ensure_stt_binary(state)?;
    let locale = locale.map(|s| s.to_string()).unwrap_or_else(|| {
        let loc = configured_locale();
        if loc.contains('-') || loc.contains('_') {
            loc.replace('_', "-")
        } else if loc == "en" {
            "en-US".into()
        } else {
            format!("{loc}-US")
        }
    });
    let secs = seconds.clamp(1.0, 60.0);
    let out = Command::new(&bin)
        .args(["--seconds", &format!("{secs:.0}"), "--locale", &locale])
        .output()
        .context("run abbey-stt")?;
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        bail!(
            "voice listen failed (exit {})",
            out.status.code().unwrap_or(1)
        );
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        bail!("no speech recognized");
    }
    Ok(text)
}

pub fn print_voices(locale_filter: Option<&str>) -> Result<i32> {
    if !is_macos() {
        println!("voice backends (portable — no bundled weights):");
        for t in crate::voice_portable::inventory() {
            println!(
                "  {:<8} {:<12} {}{}",
                t.role,
                t.name,
                t.path.display(),
                if t.small { "  [small]" } else { "" }
            );
        }
        if let Some(m) = crate::voice_portable::prefer_whisper_model() {
            println!("whisper model: {} (tiny/base preferred)", m.display());
        } else {
            println!(
                "whisper model: none — set ABBEY_WHISPER_MODEL to ggml-tiny.bin \
                 (preferred) or ggml-base.bin"
            );
        }
        return Ok(0);
    }
    let voices = list_voices()?;
    let filter = locale_filter.unwrap_or("").to_ascii_lowercase();
    println!("{:<8} {:<8} name", "quality", "locale");
    let mut premium = 0usize;
    let mut enhanced = 0usize;
    for v in &voices {
        if !filter.is_empty() && !v.locale.to_ascii_lowercase().starts_with(&filter) {
            continue;
        }
        match v.quality {
            3 => premium += 1,
            2 => enhanced += 1,
            _ => {}
        }
        if v.quality == 0 {
            continue; // hide novelty noise from the default list
        }
        println!(
            "{:<8} {:<8} {}",
            quality_label(v.quality),
            if v.locale.is_empty() { "-" } else { &v.locale },
            v.name
        );
    }
    println!();
    if let Ok(best) = best_voice(if filter.is_empty() { "en" } else { &filter }, None) {
        println!(
            "default pick: {} ({})",
            best.name,
            quality_label(best.quality)
        );
    }
    if premium + enhanced == 0 {
        println!(
            "note: no Premium/Enhanced voices installed yet.\n\
             System Settings → Accessibility → Spoken Content → System voice ⓘ → Download"
        );
    } else {
        println!("installed high-quality: {premium} Premium, {enhanced} Enhanced");
    }
    Ok(0)
}

/// Listen → agent → speak the reply (high-quality local voice loop).
pub fn ask(
    cfg: &mut AgentConfig,
    state: &AbbeyState,
    seconds: f64,
    hint: &[String],
) -> Result<i32> {
    if !is_macos()
        && !(crate::voice_portable::has_portable_stt() && crate::voice_portable::has_portable_tts())
    {
        return refuse_platform("voice ask");
    }
    let heard = listen(state, seconds, None)?;
    println!("abbey: heard: {heard}");
    let mut prompt = String::new();
    if !hint.is_empty() {
        prompt.push_str(&hint.join(" "));
        prompt.push_str("\n\n");
    }
    prompt.push_str("Voice input transcript:\n");
    prompt.push_str(&heard);
    prompt.push_str(
        "\n\nReply in clear spoken prose (short paragraphs, no markdown tables). \
         This answer will be read aloud.",
    );
    let captured = crate::capture::capture_chat(cfg, state, &[prompt])?;
    eprint!("{}", captured.stderr);
    let reply = captured.stdout.trim();
    if !reply.is_empty() {
        let needs_nl = !reply.ends_with('\n');
        crate::highlight::emit_agent_stdout(reply);
        if needs_nl {
            println!();
        }
        let _ = speak(reply, None, None, None)?;
    }
    Ok(captured.status.code().unwrap_or(1))
}

pub fn dispatch(state: &AbbeyState, cfg: &mut AgentConfig, args: &[String]) -> Result<i32> {
    if args.is_empty() {
        return status(state);
    }
    match args[0].as_str() {
        "status" | "doctor" => status(state),
        "voices" | "list" => {
            let filter = args.get(1).map(|s| s.as_str());
            print_voices(filter)
        }
        "speak" | "say" | "tts" => {
            let mut voice = None;
            let mut rate = None;
            let mut out = None;
            let mut text: Vec<String> = Vec::new();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "-v" | "--voice" => {
                        i += 1;
                        voice = args.get(i).cloned();
                    }
                    "-r" | "--rate" => {
                        i += 1;
                        rate = args.get(i).and_then(|s| s.parse().ok());
                    }
                    "-o" | "--out" => {
                        i += 1;
                        out = args.get(i).map(PathBuf::from);
                    }
                    other => text.push(other.to_string()),
                }
                i += 1;
            }
            if text.is_empty() && !std::io::IsTerminal::is_terminal(&std::io::stdin()) {
                use std::io::Read;
                let mut buf = String::new();
                std::io::stdin().read_to_string(&mut buf)?;
                text.push(buf);
            }
            speak(&text.join(" "), voice.as_deref(), rate, out.as_deref())
        }
        "listen" | "stt" | "mic" => {
            let mut seconds = 5.0;
            let mut locale = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "-s" | "--seconds" => {
                        i += 1;
                        seconds = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(seconds);
                    }
                    "-l" | "--locale" => {
                        i += 1;
                        locale = args.get(i).map(|s| s.as_str());
                    }
                    _ => {}
                }
                i += 1;
            }
            let text = listen(state, seconds, locale)?;
            println!("{text}");
            Ok(0)
        }
        "ask" => {
            let mut seconds = 5.0;
            let mut hint = Vec::new();
            let mut i = 1;
            while i < args.len() {
                if args[i] == "-s" || args[i] == "--seconds" {
                    i += 1;
                    seconds = args.get(i).and_then(|s| s.parse().ok()).unwrap_or(seconds);
                } else {
                    hint.push(args[i].clone());
                }
                i += 1;
            }
            ask(cfg, state, seconds, &hint)
        }
        other => bail!(
            "unknown voice subcommand `{other}`\n\
             usage: abbey voice [status|voices|speak|listen|ask]"
        ),
    }
}

fn status(state: &AbbeyState) -> Result<i32> {
    println!(
        "voice: {}",
        if is_macos() {
            "macOS say + Speech.framework"
        } else {
            "portable host tools only (no bundled neural speech)"
        }
    );
    println!("state: {}", state.state_dir.display());
    println!("portable tools:");
    let tools = crate::voice_portable::inventory();
    if tools.is_empty() {
        println!("  (none discovered)");
    } else {
        for t in &tools {
            println!(
                "  {:<8} {:<12} {}{}",
                t.role,
                t.name,
                t.path.display(),
                if t.small { "  [small]" } else { "" }
            );
        }
    }
    if let Some(m) = crate::voice_portable::prefer_whisper_model() {
        println!("whisper: {} (tiny/base preferred over large)", m.display());
    } else {
        println!("whisper: no ggml tiny/base weight (set ABBEY_WHISPER_MODEL)");
    }
    if !is_macos() {
        println!(
            "env:    ABBEY_VOICE · ABBEY_VOICE_RATE · ABBEY_VOICE_LOCALE · ABBEY_WHISPER_MODEL"
        );
        return Ok(0);
    }
    println!("macos:  say + on-device Speech.framework");
    match list_voices() {
        Ok(v) => {
            let premium = v.iter().filter(|x| x.quality == 3).count();
            let enhanced = v.iter().filter(|x| x.quality == 2).count();
            println!(
                "voices: {} total · {premium} Premium · {enhanced} Enhanced",
                v.len()
            );
            if let Ok(best) = best_voice(&configured_locale(), configured_voice().as_deref()) {
                println!(
                    "pick:   {} ({}) locale={}",
                    best.name,
                    quality_label(best.quality),
                    best.locale
                );
            }
            if premium + enhanced == 0 {
                println!(
                    "tip:    download Premium/Enhanced voices for super-high quality:\n\
                     System Settings → Accessibility → Spoken Content → System voice ⓘ"
                );
            }
        }
        Err(e) => println!("voices: error: {e}"),
    }
    let stt = state.state_dir.join("bin/abbey-stt");
    println!(
        "stt:    {} ({})",
        stt.display(),
        if stt.is_file() {
            "built"
        } else {
            "will build on first listen"
        }
    );
    println!("env:    ABBEY_VOICE · ABBEY_VOICE_RATE · ABBEY_VOICE_LOCALE");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_standard_and_premium_lines() {
        let a =
            parse_voice_line("Daniel              en_GB    # Hello! My name is Daniel.").unwrap();
        assert_eq!(a.name, "Daniel");
        assert_eq!(a.locale, "en_GB");
        assert_eq!(a.quality, 1);

        let b = parse_voice_line("Zoe (Premium)       en_US    # Hello! My name is Zoe.").unwrap();
        assert!(b.name.contains("Premium"));
        assert_eq!(b.quality, 3);

        let c =
            parse_voice_line("Samantha (Enhanced) en_US    # Hello! My name is Samantha.").unwrap();
        assert_eq!(c.quality, 2);
    }

    #[test]
    fn novelty_is_lowest_quality() {
        assert_eq!(voice_quality("Bells"), 0);
        assert_eq!(voice_quality("Zarvox"), 0);
        assert_eq!(voice_quality("Ava (Premium)"), 3);
        assert_eq!(voice_quality("Sol"), 3);
        assert_eq!(voice_quality("Whisper"), 0);
    }

    #[test]
    fn siri_comment_promotes_quality() {
        let v = parse_voice_line("Aman (English (India)) en_IN    # Hi, I’m Siri!").unwrap();
        assert_eq!(v.quality, 2);
        let sol =
            parse_voice_line("Sol                 en_US    # Hello! My name is Sol.").unwrap();
        assert_eq!(sol.quality, 3);
        assert_eq!(sol.name, "Sol");
    }
}
