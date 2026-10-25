//! Native Windows actual-plugin extraction tests. No installer entries exist.
//! Each child has a private TEMP/TMP and cwd; every encoded escape target is
//! inside the disposable parent root. No extraction engine is copied here.
#[path = "fixtures/update-zip/api.rs"]
mod api;

use std::{
    fs,
    path::Path,
    process::Command,
    thread,
    time::{Duration, Instant},
};
#[cfg(windows)]
use tauri_plugin_updater::Error;

fn outside_snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut entries = fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            assert!(
                !entry.file_type().unwrap().is_symlink(),
                "external symlink created"
            );
            assert!(
                entry.file_type().unwrap().is_file(),
                "external directory/reparse point created"
            );
            (
                entry.file_name().to_string_lossy().into_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    entries.sort();
    entries
}

#[test]
#[cfg(windows)]
fn hostile_archives_refuse_in_bounded_isolated_children() {
    let root = match std::env::var_os("ABBEY_UPDATER_QUAL_ROOT") {
        Some(parent) => tempfile::tempdir_in(parent).unwrap(),
        None => tempfile::tempdir().unwrap(),
    };
    let root_path = root.path().canonicalize().unwrap();
    // Strip Windows extended-length syntax; Python zipfile must encode ordinary
    // drive-absolute names, not a UNC device namespace.
    let root_path =
        std::path::PathBuf::from(root_path.to_string_lossy().trim_start_matches(r"\\?\"));
    // Preserve native runner evidence even if an assertion below fails. A plain
    // cargo test still cleans its disposable parent root on scope exit.
    let _cleanup = if std::env::var_os("ABBEY_UPDATER_QUAL_ROOT").is_some() {
        eprintln!("native child evidence retained at {:?}", root.keep());
        None
    } else {
        Some(root)
    };
    let outside = root_path.join("outside");
    fs::create_dir(&outside).unwrap();
    for name in [
        "sentinel.txt",
        "parent.txt",
        "absolute.txt",
        "relative.txt",
        "symlink.txt",
        "chained.txt",
    ] {
        fs::write(outside.join(name), b"unchanged outside sentinel").unwrap();
    }
    let before = outside_snapshot(&outside);
    let probe = root_path.join("link-privilege-probe");
    std::os::windows::fs::symlink_dir(&outside, &probe).expect(
        "UNEXECUTED: symlink privilege/Developer Mode required; rerun natively with privilege",
    );
    fs::remove_dir(&probe).unwrap();
    let fixtures = root_path.join("fixtures");
    let generated =
        Command::new(std::env::var_os("ABBEY_ZIP_PYTHON").unwrap_or_else(|| "python".into()))
            .arg(api::fixture("generate.py"))
            .arg("--output")
            .arg(&fixtures)
            .arg("--sandbox-root")
            .arg(&root_path)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
    assert!(
        generated.status.success(),
        "fixture generator: {}",
        String::from_utf8_lossy(&generated.stderr)
    );
    for case in [
        "parent",
        "absolute",
        "drive-relative",
        "outward-link",
        "chained-link",
        "truncated",
    ] {
        let temp = root_path.join("temp");
        fs::create_dir(&temp).unwrap();
        let result_path = root_path.join(format!("{case}-result.json"));
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "windows_zip_child", "--ignored", "--nocapture"])
            .env("ABBEY_ZIP_CHILD_ROOT", &root_path)
            .env("ABBEY_ZIP_CHILD_CASE", case)
            .env("TEMP", &temp)
            .env("TMP", &temp)
            .current_dir(&outside)
            .stdin(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let status = bounded_child(&mut child, Duration::from_secs(30));
        assert_eq!(
            outside_snapshot(&outside),
            before,
            "{case}: outside output changed"
        );
        assert!(
            status.is_some_and(|s| s.success()),
            "{case}: bounded child failed; result {result_path:?}"
        );
        let result: serde_json::Value =
            serde_json::from_slice(&fs::read(&result_path).unwrap()).unwrap();
        assert_eq!(result["case"], case);
        assert_eq!(result["verified"], true);
        assert_eq!(result["extraction_refused"], true);
        eprintln!("native updater case {case}: {result}");
        fs::remove_dir_all(temp).unwrap();
    }
}

// Dispatcher for the parent's isolated subprocess, not independent coverage.
#[test]
#[ignore = "dispatcher runs only in a parent-owned child"]
#[cfg(windows)]
fn windows_zip_child() {
    let Some(root) = std::env::var_os("ABBEY_ZIP_CHILD_ROOT") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let case = std::env::var("ABBEY_ZIP_CHILD_CASE").unwrap();
    assert!(
        [
            "parent",
            "absolute",
            "drive-relative",
            "outward-link",
            "chained-link",
            "truncated"
        ]
        .contains(&case.as_str())
    );
    let actual_temp = std::env::temp_dir();
    let cwd = std::env::current_dir().expect("UNEXECUTED: resolve child working directory");
    let paths = confinement_paths(&root, &actual_temp, &cwd);
    let guarded = with_confined_layout(&root, &actual_temp, &cwd, || {
        let fixtures = root.join("fixtures");
        let bytes = fs::read(fixtures.join(format!("{case}.zip"))).unwrap();
        let signature = fs::read_to_string(fixtures.join(format!("{case}.zip.sig"))).unwrap();
        let key = fs::read_to_string(fixtures.join("updater.pubkey")).unwrap();
        let update = api::update(&key, &signature, "99.0.0", &root.join("unused-app/Abbey"));
        update
            .verify(&bytes)
            .expect("valid signature must reach extraction");
        let result = update.install(&bytes);
        let refused = matches!(result, Err(Error::Extract(_)));
        fs::write(root.join(format!("{case}-result.json")), serde_json::to_vec_pretty(&serde_json::json!({
            "case": case, "verified": true, "extraction_refused": refused, "result": format!("{result:?}"), "resolved_paths": paths
        })).unwrap()).unwrap();
        assert!(refused, "expected ZIP extraction refusal, got {result:?}");
    });
    if let Err(reason) = guarded {
        eprintln!(
            "{}",
            serde_json::json!({"case": case, "setup": "UNEXECUTED", "reason": reason, "resolved_paths": paths})
        );
        panic!("{reason}");
    }
}

fn bounded_child(
    child: &mut std::process::Child,
    timeout: Duration,
) -> Option<std::process::ExitStatus> {
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        if start.elapsed() > timeout {
            child.kill().unwrap();
            child.wait().unwrap();
            return None;
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn independent_fixture_generation_manifest_and_signatures() {
    use sha2::{Digest, Sha256};
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("fixtures");
    let python = std::env::var_os("ABBEY_ZIP_PYTHON").unwrap_or_else(|| {
        if cfg!(windows) {
            "python".into()
        } else {
            "python3".into()
        }
    });
    let mut child = Command::new(python)
        .arg(api::fixture("generate.py"))
        .arg("--output")
        .arg(&output)
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    assert!(bounded_child(&mut child, Duration::from_secs(30)).is_some_and(|s| s.success()));
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("manifest.json")).unwrap()).unwrap();
    for (name, expected) in manifest["files"].as_object().unwrap() {
        let digest = format!("{:x}", Sha256::digest(fs::read(output.join(name)).unwrap()));
        assert_eq!(digest, expected.as_str().unwrap());
    }
    let key = fs::read_to_string(output.join("updater.pubkey")).unwrap();
    for (name, case) in manifest["cases"].as_object().unwrap() {
        let bytes = fs::read(output.join(format!("{name}.zip"))).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), case["sha256"]);
        let signature = fs::read_to_string(output.join(format!("{name}.zip.sig"))).unwrap();
        api::update(
            &key,
            &signature,
            "99.0.0",
            &root.path().join("unused-app/Abbey"),
        )
        .verify(&bytes)
        .unwrap();
        if name != "truncated" {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            for i in 0..archive.len() {
                let entry = archive.by_index(i).unwrap();
                let name = entry.name().unwrap().to_ascii_lowercase();
                assert!(!name.ends_with(".exe") && !name.ends_with(".msi"));
            }
        }
    }
}

#[test]
fn portable_parent_bounds_child_success_and_timeout() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("sentinel.txt"), b"unchanged").unwrap();
    let before = outside_snapshot(root.path());
    for (delay, timeout, completes) in [(0, 10000, true), (10000, 100, false)] {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "portable_child_dispatcher",
                "--ignored",
                "--nocapture",
            ])
            .env("ABBEY_ZIP_PORTABLE_CHILD_DELAY", delay.to_string())
            .env("TEMP", root.path())
            .env("TMP", root.path())
            .current_dir(root.path())
            .stdin(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let result = bounded_child(&mut child, Duration::from_millis(timeout));
        assert_eq!(result.is_some_and(|s| s.success()), completes);
        assert_eq!(outside_snapshot(root.path()), before);
    }
}

#[test]
#[ignore = "dispatcher runs only in a parent-owned child"]
fn portable_child_dispatcher() {
    let delay: u64 = std::env::var("ABBEY_ZIP_PORTABLE_CHILD_DELAY")
        .unwrap()
        .parse()
        .unwrap();
    thread::sleep(Duration::from_millis(delay));
}

// The callback is the sole entry to constructing the updater/installing bytes.
fn with_confined_layout<T>(
    root: &Path,
    actual_temp: &Path,
    cwd: &Path,
    callback: impl FnOnce() -> T,
) -> Result<T, String> {
    let validate = || -> Result<(), String> {
        if !root.is_absolute() || root.file_name().is_none() {
            return Err("sandbox root must be an absolute private directory".into());
        }
        #[cfg(windows)]
        if !matches!(root.components().next(), Some(std::path::Component::Prefix(prefix))
            if matches!(prefix.kind(), std::path::Prefix::Disk(_) | std::path::Prefix::VerbatimDisk(_)))
        {
            return Err("sandbox root must be on a local drive, not UNC/device storage".into());
        }
        // Check the lexical directories before canonicalization can hide links.
        // Windows junctions/other reparse tags must refuse as well as symlinks.
        for path in [
            root.to_path_buf(),
            root.join("temp"),
            root.join("outside"),
            actual_temp.to_path_buf(),
            cwd.to_path_buf(),
        ] {
            if !path.is_absolute()
                || path.components().any(|part| {
                    matches!(
                        part,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
            {
                return Err("sandbox paths must be absolute without relative components".into());
            }
            for ancestor in path.ancestors() {
                let metadata = fs::symlink_metadata(ancestor)
                    .map_err(|e| format!("cannot inspect sandbox directory: {e}"))?;
                let linked = metadata.file_type().is_symlink();
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    let linked = linked || metadata.file_attributes() & 0x400 != 0; // FILE_ATTRIBUTE_REPARSE_POINT
                    if linked {
                        return Err("sandbox layout contains a Windows reparse point".into());
                    }
                }
                if linked || !metadata.is_dir() {
                    return Err(
                        "sandbox layout contains a link/reparse point or non-directory".into(),
                    );
                }
            }
        }
        let root = root
            .canonicalize()
            .map_err(|e| format!("cannot resolve sandbox root: {e}"))?;
        let expected_temp = root
            .join("temp")
            .canonicalize()
            .map_err(|e| format!("cannot resolve private temp: {e}"))?;
        let expected_cwd = root
            .join("outside")
            .canonicalize()
            .map_err(|e| format!("cannot resolve private cwd: {e}"))?;
        if expected_temp.parent() != Some(root.as_path())
            || expected_cwd.parent() != Some(root.as_path())
        {
            return Err("private temp/cwd must be direct children of sandbox root".into());
        }
        let actual_temp = actual_temp
            .canonicalize()
            .map_err(|e| format!("cannot resolve OS temporary directory: {e}"))?;
        let cwd = cwd
            .canonicalize()
            .map_err(|e| format!("cannot resolve actual working directory: {e}"))?;
        if actual_temp != expected_temp {
            return Err("OS temporary directory differs from sandbox root/temp".into());
        }
        if cwd != expected_cwd {
            return Err("actual working directory differs from sandbox root/outside".into());
        }
        Ok(())
    };
    validate().map_err(|reason| format!("UNEXECUTED: {reason}"))?;
    Ok(callback())
}

fn confinement_paths(root: &Path, actual_temp: &Path, cwd: &Path) -> serde_json::Value {
    fn resolved(path: &Path) -> serde_json::Value {
        serde_json::json!({
            "requested": path,
            "canonical": path.canonicalize().map(|p| p.to_string_lossy().into_owned()).map_err(|e| e.to_string()),
        })
    }
    serde_json::json!({"root": resolved(root), "actual_temp": resolved(actual_temp), "cwd": resolved(cwd)})
}

fn private_layout() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("temp")).unwrap();
    fs::create_dir(root.path().join("outside")).unwrap();
    root
}

#[test]
fn confinement_guard_refuses_temp_cwd_and_layout_mismatches_before_callback() {
    let root = private_layout();
    let root = root.path().canonicalize().unwrap();
    let temp = root.join("temp");
    let cwd = root.join("outside");
    let unexpected = root.join("unexpected");
    fs::create_dir(&unexpected).unwrap();
    for (label, proposed_root, actual_temp, actual_cwd) in [
        (
            "OS temp outside private temp",
            root.as_path(),
            unexpected.as_path(),
            cwd.as_path(),
        ),
        (
            "cwd outside sentinels",
            root.as_path(),
            temp.as_path(),
            unexpected.as_path(),
        ),
        (
            "missing expected layout",
            unexpected.as_path(),
            temp.as_path(),
            cwd.as_path(),
        ),
        (
            "relative root",
            Path::new("."),
            temp.as_path(),
            cwd.as_path(),
        ),
    ] {
        let mut invoked = false;
        let result =
            with_confined_layout(proposed_root, actual_temp, actual_cwd, || invoked = true);
        assert!(result.is_err(), "{label}: must refuse setup");
        assert!(!invoked, "{label}: updater callback must not run");
    }
}

#[test]
fn confinement_guard_allows_only_the_actual_private_directories() {
    let root = private_layout();
    let root = root.path().canonicalize().unwrap();
    let mut invoked = false;
    with_confined_layout(&root, &root.join("temp"), &root.join("outside"), || {
        invoked = true
    })
    .unwrap();
    assert!(invoked);
    let paths = confinement_paths(&root, &root.join("temp"), &root.join("outside"));
    assert_eq!(
        paths["actual_temp"]["canonical"]["Ok"],
        root.join("temp").to_string_lossy().as_ref()
    );
}

#[test]
#[cfg(unix)]
fn confinement_guard_refuses_real_directory_links_before_callback() {
    let root = private_layout();
    let parent = tempfile::tempdir().unwrap();
    let root_path = root.path().canonicalize().unwrap();
    let linked_root = parent.path().join("linked-root");
    std::os::unix::fs::symlink(&root_path, &linked_root).unwrap();
    let mut invoked = false;
    assert!(
        with_confined_layout(
            &linked_root,
            &root_path.join("temp"),
            &root_path.join("outside"),
            || invoked = true
        )
        .is_err()
    );
    assert!(!invoked);
    fs::remove_dir(root_path.join("temp")).unwrap();
    std::os::unix::fs::symlink(parent.path(), root_path.join("temp")).unwrap();
    assert!(
        with_confined_layout(
            &root_path,
            &root_path.join("temp"),
            &root_path.join("outside"),
            || invoked = true
        )
        .is_err()
    );
    assert!(!invoked);
    fs::remove_file(root_path.join("temp")).unwrap();
    fs::create_dir(root_path.join("temp")).unwrap();
    fs::remove_dir(root_path.join("outside")).unwrap();
    std::os::unix::fs::symlink(parent.path(), root_path.join("outside")).unwrap();
    assert!(
        with_confined_layout(
            &root_path,
            &root_path.join("temp"),
            &root_path.join("outside"),
            || invoked = true
        )
        .is_err()
    );
    assert!(!invoked);
}
