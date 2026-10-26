//! EXACT ZIP 9.0.5 dependency contract, not updater installation proof.
//! The updater's public API does not expose a deterministic preseed seam.
#[cfg(windows)]
use std::process::Command;
use std::{fs, io::Cursor, path::Path};

fn preexisting_link_contract(junction: bool) {
    let root = tempfile::tempdir().unwrap();
    let extraction = root.path().join("extraction");
    let outside = root.path().join("outside");
    fs::create_dir(&extraction).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel.txt"), b"prior bytes").unwrap();
    let link = extraction.join("existing");
    #[cfg(windows)]
    if junction {
        let result = Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "UNEXECUTED junction setup: {}",
            String::from_utf8_lossy(&result.stderr)
        );
    } else {
        std::os::windows::fs::symlink_dir(&outside, &link)
            .expect("UNEXECUTED: native symlink privileges/Developer Mode required");
    }
    #[cfg(unix)]
    {
        assert!(!junction);
        std::os::unix::fs::symlink(&outside, &link).unwrap();
    }
    let bytes = fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/update-zip/preexisting-link.zip"),
    )
    .unwrap();
    let result = zip::ZipArchive::new(Cursor::new(bytes))
        .unwrap()
        .extract(&extraction);
    assert!(
        result.is_err(),
        "ZIP9 must refuse traversal through pre-existing reparse point"
    );
    assert_eq!(
        fs::read(outside.join("sentinel.txt")).unwrap(),
        b"prior bytes"
    );
    assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    #[cfg(windows)]
    fs::remove_dir(link).unwrap();
    #[cfg(unix)]
    fs::remove_file(link).unwrap();
}

#[test]
fn zip9_refuses_a_preexisting_outward_directory_symlink() {
    preexisting_link_contract(false);
}

#[test]
#[cfg(windows)]
fn zip9_refuses_a_preexisting_outward_junction() {
    preexisting_link_contract(true);
}
