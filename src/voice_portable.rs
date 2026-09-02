//! Portable STT/TTS adapters used when macOS `say` / Speech.framework are
//! unavailable, or when a smaller local Whisper model is on disk.
//!
//! These spawn host tools that the user already installed. Abbey does not
//! bundle Whisper/Piper/espeak weights. Missing tools refuse honestly.

use crate::host::which_bin;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

/// One discovered host voice tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortableTool {
    pub role: &'static str,
    pub name: &'static str,
    pub path: PathBuf,
    /// True when this is a small/local tool (tiny/base Whisper, espeak, SAPI).
    pub small: bool,
}

/// Ranked Whisper-weight basenames. Tiny/base first; large tags are last-resort.
pub const WHISPER_SMALL_NAMES: &[&str] = &[
    "ggml-tiny.en.bin",
    "ggml-tiny.bin",
    "ggml-base.en.bin",
    "ggml-base.bin",
    "tiny.en.bin",
    "tiny.bin",
    "base.en.bin",
    "base.bin",
];

const WHISPER_BINS: &[&str] = &["whisper-cli", "whisper-cpp"];
const TTS_UNIX: &[(&str, bool)] = &[("espeak-ng", true), ("espeak", true)];

/// Discover portable tools on this host. Pure inventory: no spawn besides
/// `which`-style path probes.
pub fn inventory() -> Vec<PortableTool> {
    let mut out = Vec::new();
    for name in WHISPER_BINS {
        if let Some(path) = which_bin(name) {
            out.push(PortableTool {
                role: "stt",
                name,
                path,
                small: true,
            });
            break;
        }
    }
    if cfg!(target_os = "macos") {
        if which_bin("say").is_some() || Path::new("/usr/bin/say").is_file() {
            out.push(PortableTool {
                role: "tts",
                name: "say",
                path: PathBuf::from("/usr/bin/say"),
                small: false,
            });
        }
    } else if cfg!(windows)
        && let Some(path) = which_bin("powershell").or_else(|| which_bin("powershell.exe"))
    {
        out.push(PortableTool {
            role: "tts",
            name: "sapi",
            path,
            small: true,
        });
    }
    if let Some(path) = ready_piper() {
        out.push(PortableTool {
            role: "tts",
            name: "piper",
            path,
            small: false,
        });
    }
    for (name, small) in TTS_UNIX {
        if let Some(path) = which_bin(name) {
            out.push(PortableTool {
                role: "tts",
                name,
                path,
                small: *small,
            });
        }
    }
    if which_bin("ffmpeg").is_some() {
        out.push(PortableTool {
            role: "capture",
            name: "ffmpeg",
            path: which_bin("ffmpeg").unwrap_or_else(|| PathBuf::from("ffmpeg")),
            small: true,
        });
    }
    out
}

pub fn has_portable_tts() -> bool {
    inventory().iter().any(|t| t.role == "tts")
}

pub fn has_portable_stt() -> bool {
    whisper_bin().is_some() && prefer_whisper_model().is_some()
}

fn whisper_bin() -> Option<PathBuf> {
    inventory()
        .into_iter()
        .find(|t| t.role == "stt")
        .map(|t| t.path)
}

/// First existing small Whisper weight, then any explicit env override.
pub fn prefer_whisper_model() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ABBEY_WHISPER_MODEL").or_else(|_| std::env::var("WHISPER_MODEL"))
    {
        let path = PathBuf::from(p);
        if path.is_file() {
            return Some(path);
        }
    }
    let mut dirs = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/whisper"));
        dirs.push(home.join("whisper.cpp/models"));
        dirs.push(home.join("models/whisper"));
    }
    if let Some(bin) = whisper_bin()
        && let Some(parent) = bin.parent()
    {
        dirs.push(parent.to_path_buf());
        dirs.push(parent.join("models"));
    }
    let mut found = Vec::new();
    for dir in dirs {
        for name in WHISPER_SMALL_NAMES {
            let p = dir.join(name);
            if p.is_file() {
                found.push(p);
            }
        }
    }
    rank_whisper_models(&found).into_iter().next()
}

/// Rank a set of model paths: tiny/base before small/medium/large.
pub fn rank_whisper_models(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut scored: Vec<(u8, PathBuf)> = paths
        .iter()
        .filter(|p| p.is_file() || !p.exists())
        .map(|p| {
            let n = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let score = WHISPER_SMALL_NAMES
                .iter()
                .position(|want| n.contains(&want.replace(".bin", "")))
                .unwrap_or(80) as u8;
            let score = if n.contains("tiny") {
                0
            } else if n.contains("base") {
                1
            } else if n.contains("small") {
                10
            } else if n.contains("medium") {
                20
            } else if n.contains("large") {
                30
            } else {
                score
            };
            (score, p.clone())
        })
        .collect();
    scored.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, p)| p).collect()
}

pub fn speak_portable(
    text: &str,
    voice: Option<&str>,
    rate: Option<u32>,
    out: Option<&Path>,
) -> Result<i32> {
    let text = text.trim();
    if text.is_empty() {
        bail!("usage: abbey voice speak [-v VOICE] [-r RATE] [-o FILE] <text>");
    }
    if text.len() > 32 * 1024 {
        bail!("voice text exceeds the 32768-byte portable TTS limit");
    }
    if cfg!(windows)
        && let Some(powershell) = which_bin("powershell").or_else(|| which_bin("powershell.exe"))
    {
        return speak_sapi(&powershell, text, voice, rate, out);
    }
    if which_bin("piper").is_some()
        && piper_model().is_some()
        && (out.is_some() || audio_player().is_some())
    {
        return speak_piper(text, rate, out);
    }
    for (name, _) in TTS_UNIX {
        if let Some(bin) = which_bin(name) {
            let mut cmd = Command::new(&bin);
            if let Some(v) = voice {
                cmd.args(["-v", v]);
            }
            if let Some(rate) = rate {
                cmd.args(["-s", &rate.to_string()]);
            }
            if let Some(path) = out {
                cmd.arg("-w").arg(path);
            }
            cmd.arg(text);
            let st = cmd
                .status()
                .with_context(|| format!("exec {}", bin.display()))?;
            eprintln!("abbey: voice speak → {name} (portable, small)");
            return Ok(st.code().unwrap_or(1));
        }
    }
    bail!(
        "no portable TTS tool found (install espeak-ng, or set ABBEY_PIPER_MODEL with Piper and an audio player).\n\
         Abbey does not bundle neural speech weights."
    );
}

fn piper_model() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os("ABBEY_PIPER_MODEL")?);
    path.is_file().then_some(path)
}

fn audio_player() -> Option<(PathBuf, &'static [&'static str])> {
    for (name, args) in [
        ("afplay", &[][..]),
        ("aplay", &[][..]),
        ("paplay", &[][..]),
        (
            "ffplay",
            &["-nodisp", "-autoexit", "-loglevel", "error"][..],
        ),
    ] {
        if let Some(path) = which_bin(name) {
            return Some((path, args));
        }
    }
    None
}

fn ready_piper() -> Option<PathBuf> {
    let binary = which_bin("piper")?;
    piper_model()?;
    audio_player()?;
    Some(binary)
}

fn speak_piper(text: &str, rate: Option<u32>, out: Option<&Path>) -> Result<i32> {
    let binary = which_bin("piper").context("piper")?;
    let model = piper_model().context("ABBEY_PIPER_MODEL must name an existing Piper model")?;
    let owned_wav;
    let (wav, keep) = if let Some(path) = out {
        (path, true)
    } else {
        owned_wav = std::env::temp_dir().join(format!(
            "abbey-piper-{}-{}.wav",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        (owned_wav.as_path(), false)
    };
    let result = (|| -> Result<i32> {
        let mut cmd = Command::new(&binary);
        cmd.args(["--model", &model.display().to_string(), "--output_file"])
            .arg(wav);
        if let Some(rate) = rate.filter(|rate| *rate > 0) {
            cmd.args(["--length_scale", &format!("{:.3}", 175.0 / f64::from(rate))]);
        }
        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .spawn()
            .with_context(|| format!("exec {}", binary.display()))?;
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write as _;
            stdin.write_all(text.as_bytes())?;
            stdin.write_all(b"\n")?;
        }
        let status = child.wait()?;
        if !status.success() {
            return Ok(status.code().unwrap_or(1));
        }
        if keep {
            eprintln!(
                "abbey: voice speak → piper (external model) → {}",
                wav.display()
            );
            return Ok(0);
        }
        let (player, player_args) =
            audio_player().context("Piper needs afplay, aplay, paplay, or ffplay")?;
        let status = Command::new(&player)
            .args(player_args)
            .arg(wav)
            .status()
            .with_context(|| format!("exec {}", player.display()))?;
        eprintln!("abbey: voice speak → piper (external model + host player)");
        Ok(status.code().unwrap_or(1))
    })();
    if !keep {
        let _ = std::fs::remove_file(wav);
    }
    result
}

fn speak_sapi(
    powershell: &Path,
    text: &str,
    voice: Option<&str>,
    rate: Option<u32>,
    out: Option<&Path>,
) -> Result<i32> {
    let dir = std::env::temp_dir();
    let text_path = dir.join(format!("abbey-tts-{}.txt", std::process::id()));
    let script_path = dir.join(format!("abbey-tts-{}.ps1", std::process::id()));
    std::fs::write(&text_path, text)?;
    let script = r#"param([Parameter(Mandatory=$true)][string]$TextPath,[string]$Voice,[int]$Rate=0,[string]$Out)
$t = Get-Content -Raw -Encoding UTF8 $TextPath
Add-Type -AssemblyName System.Speech
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
if ($Voice) { $s.SelectVoice($Voice) }
if ($Rate -gt 0) { $s.Rate = [Math]::Max(-10, [Math]::Min(10, [int](($Rate - 175) / 17))) }
if ($Out) { $s.SetOutputToWaveFile($Out) }
$s.Speak($t)
if ($Out) { $s.Dispose() }
"#;
    std::fs::write(&script_path, script)?;
    let mut cmd = Command::new(powershell);
    cmd.args([
        "-NoProfile",
        "-NonInteractive",
        "-File",
        &script_path.display().to_string(),
        "-TextPath",
        &text_path.display().to_string(),
    ]);
    if let Some(v) = voice {
        cmd.args(["-Voice", v]);
    }
    if let Some(rate) = rate {
        cmd.args(["-Rate", &rate.to_string()]);
    }
    if let Some(path) = out {
        cmd.args(["-Out", &path.display().to_string()]);
    }
    let st = cmd.status().context("powershell SAPI")?;
    let _ = std::fs::remove_file(&text_path);
    let _ = std::fs::remove_file(&script_path);
    eprintln!("abbey: voice speak → Windows SAPI (portable, small)");
    Ok(st.code().unwrap_or(1))
}

pub fn listen_whisper(seconds: f64, wav: Option<&Path>) -> Result<String> {
    let bin = whisper_bin().context(
        "no whisper.cpp CLI on PATH (whisper-cli or whisper-cpp).\n\
         A generic `whisper` binary is ignored because it is not whisper.cpp argv.\n\
         Install whisper.cpp and a tiny/base ggml weight; Abbey does not bundle models.",
    )?;
    let model = prefer_whisper_model().context(
        "no small Whisper weight found. Set ABBEY_WHISPER_MODEL to a ggml-tiny.bin \
         (preferred) or ggml-base.bin path.",
    )?;
    let wav_path = if let Some(p) = wav {
        p.to_path_buf()
    } else {
        record_wav(seconds)?
    };
    let out = Command::new(&bin)
        .args([
            "-m",
            &model.display().to_string(),
            "-f",
            &wav_path.display().to_string(),
            "-nt",
            "-otxt",
        ])
        .output()
        .with_context(|| format!("exec {}", bin.display()))?;
    if wav.is_none() {
        let _ = std::fs::remove_file(&wav_path);
    }
    if !out.status.success() {
        bail!(
            "whisper failed (exit {}): {}",
            out.status.code().unwrap_or(1),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !stdout.is_empty() {
        return Ok(stdout);
    }
    let txt = wav_path.with_extension("txt");
    if let Ok(s) = std::fs::read_to_string(&txt) {
        let _ = std::fs::remove_file(&txt);
        let t = s.trim().to_string();
        if !t.is_empty() {
            return Ok(t);
        }
    }
    bail!("whisper produced no transcript");
}

fn record_wav(seconds: f64) -> Result<PathBuf> {
    let ffmpeg = which_bin("ffmpeg")
        .context("ffmpeg is required to capture microphone audio for Whisper on this host")?;
    let path = std::env::temp_dir().join(format!("abbey-stt-{}.wav", std::process::id()));
    let secs = seconds.clamp(1.0, 60.0);
    let mut cmd = Command::new(&ffmpeg);
    cmd.args([
        "-y",
        "-t",
        &format!("{secs:.0}"),
        "-ac",
        "1",
        "-ar",
        "16000",
    ]);
    if cfg!(target_os = "macos") {
        cmd.args(["-f", "avfoundation", "-i", ":0"]);
    } else if cfg!(windows) {
        cmd.args(["-f", "dshow", "-i", "audio=default"]);
    } else {
        cmd.args(["-f", "pulse", "-i", "default"]);
    }
    cmd.arg(&path);
    let st = cmd.status().context("ffmpeg record")?;
    if !st.success() {
        bail!(
            "ffmpeg microphone capture failed (exit {})",
            st.code().unwrap_or(1)
        );
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_weights_rank_ahead_of_large() {
        let paths = [
            PathBuf::from("/m/ggml-large-v3.bin"),
            PathBuf::from("/m/ggml-tiny.en.bin"),
            PathBuf::from("/m/ggml-base.bin"),
            PathBuf::from("/m/ggml-medium.bin"),
        ];
        let ranked = rank_whisper_models(&paths);
        assert!(
            ranked[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("tiny"),
            "{ranked:?}"
        );
        assert!(
            ranked[1]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("base"),
            "{ranked:?}"
        );
    }

    #[test]
    fn inventory_does_not_panic() {
        let _ = inventory();
    }

    #[test]
    fn whisper_discovery_rejects_generic_whisper_name() {
        assert_eq!(WHISPER_BINS, ["whisper-cli", "whisper-cpp"]);
        assert!(!WHISPER_BINS.contains(&"whisper"));
    }
}
