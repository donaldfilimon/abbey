//! Hermetic Unix process fixtures: no microphone, model, service, or env changes.
use super::*;
use std::os::unix::fs::PermissionsExt;

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn whisper_script(dir: &Path, body: &str) -> PathBuf {
    script(
        dir,
        "fake whisper",
        &format!(
            r#"[ "$1" = '-m' ]; shift 2
[ "$1" = '-f' ]; shift 2
[ "$1" = '-nt' ]; shift
[ "$1" = '-otxt' ]; shift
[ "$1" = '-of' ]; shift
base=$1
{body}"#
        ),
    )
}

fn check_transcript(body: &str, expected: Option<&str>) {
    let fixture = VoiceWorkspace::new().unwrap();
    let wav = fixture.0.join("user audio.wav");
    let neighbor = wav.with_extension("txt");
    let appended = fixture.0.join("user audio.wav.txt");
    std::fs::write(&wav, b"user wav").unwrap();
    std::fs::write(&neighbor, b"user text").unwrap();
    std::fs::write(&appended, b"other user text").unwrap();
    let bin = whisper_script(&fixture.0, body);
    let workspace = VoiceWorkspace::new().unwrap();
    let output_dir = workspace.0.clone();
    let result = transcribe_whisper_in(&bin, Path::new("unused model"), &wav, workspace);
    match expected {
        Some(text) => assert_eq!(result.unwrap(), text),
        None => assert!(result.is_err()),
    }
    assert!(!output_dir.exists(), "owned transcripts must be removed");
    assert_eq!(std::fs::read(&wav).unwrap(), b"user wav");
    assert_eq!(std::fs::read(&neighbor).unwrap(), b"user text");
    assert_eq!(std::fs::read(&appended).unwrap(), b"other user text");
}

#[test]
fn stdout_and_file_cleanup_preserves_user_neighbors() {
    check_transcript(
        "printf 'file transcript' > \"$base.txt\"\nprintf ' stdout transcript \\n'",
        Some("stdout transcript"),
    );
}

#[test]
fn file_only_transcript_is_read_and_cleaned() {
    check_transcript(
        "printf ' file transcript \\n' > \"$base.txt\"",
        Some("file transcript"),
    );
}

#[test]
fn failed_whisper_cleans_partial_transcript() {
    check_transcript("printf 'partial' > \"$base.txt\"\nexit 7", None);
}

#[test]
fn empty_whisper_cleans_workspace() {
    check_transcript("printf '' > \"$base.txt\"", None);
}

#[test]
fn whisper_spawn_failure_cleans_workspace_and_preserves_input() {
    let fixture = VoiceWorkspace::new().unwrap();
    let wav = fixture.0.join("user.wav");
    std::fs::write(&wav, b"untouched").unwrap();
    let workspace = VoiceWorkspace::new().unwrap();
    let output_dir = workspace.0.clone();
    let result = transcribe_whisper_in(
        &fixture.0.join("missing executable"),
        Path::new("unused model"),
        &wav,
        workspace,
    );
    assert!(result.is_err());
    assert!(!output_dir.exists());
    assert_eq!(std::fs::read(wav).unwrap(), b"untouched");
}

#[test]
fn recordings_are_unique_and_cleaned_after_whisper_failure() {
    let fixture = VoiceWorkspace::new().unwrap();
    let recorder = script(
        &fixture.0,
        "fake ffmpeg",
        "for path do :; done\nprintf 'recording' > \"$path\"",
    );
    let first = record_wav_with(&recorder, 1.0).unwrap();
    let second = record_wav_with(&recorder, 1.0).unwrap();
    assert_ne!(first.path, second.path);
    let first_path = first.path.clone();
    let second_path = second.path.clone();
    assert!(
        transcribe_whisper(&fixture.0.join("missing"), Path::new("model"), &first.path).is_err()
    );
    drop(first);
    drop(second);
    for path in [first_path, second_path] {
        assert!(!path.parent().unwrap().exists());
    }
}

#[test]
fn failed_recorder_cleans_partial_recording() {
    let fixture = VoiceWorkspace::new().unwrap();
    let recorder = script(
        &fixture.0,
        "fake ffmpeg",
        r#"for path do :; done
printf '%s' "$path" > "$0.path"
printf 'partial' > "$path"
exit 9"#,
    );
    assert!(record_wav_with(&recorder, 1.0).is_err());
    let path = std::fs::read_to_string(fixture.0.join("fake ffmpeg.path")).unwrap();
    assert!(!Path::new(&path).parent().unwrap().exists());
    assert!(record_wav_with(&fixture.0.join("missing"), 1.0).is_err());
}
