//! Unix descriptor-relative, nonrecursive reads. Every ancestor is opened with
//! O_DIRECTORY | O_NOFOLLOW; entries are opened relative to the retained root.
//! No canonicalization or check-then-path-open security boundary is used.

use anyhow::{Context, Result, ensure};
use nix::fcntl::{AtFlags, OFlag, open, openat};
use nix::sys::stat::{Mode, SFlag, fstat, fstatat};
use std::fs::File;
use std::io::Read;
use std::os::fd::{AsRawFd, IntoRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

use super::{MAX_FILE_BYTES, MAX_FILES, MAX_TOTAL_BYTES};

const MAX_ENTRIES: usize = 128;

// nix's directory iterator needs an additional Cargo feature. Keep the small
// POSIX directory-enumeration adapter here instead of changing dependencies.
struct Directory(std::ptr::NonNull<libc::DIR>);

impl Drop for Directory {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        // SAFETY: fdopendir created this uniquely owned DIR; it is closed once.
        unsafe {
            libc::closedir(self.0.as_ptr());
        }
    }
}

#[allow(unsafe_code)]
fn entry_names(root: &File) -> Result<Vec<Vec<u8>>> {
    let file = root.try_clone()?;
    // SAFETY: file owns a live directory descriptor. On failure it remains
    // owned by File; on success ownership is transferred exactly once below.
    let pointer = unsafe { libc::fdopendir(file.as_raw_fd()) };
    let pointer = std::ptr::NonNull::new(pointer).context("open knowledge directory iterator")?;
    let _ = file.into_raw_fd();
    let directory = Directory(pointer);
    let mut names = Vec::new();
    loop {
        nix::errno::Errno::clear();
        // SAFETY: directory owns a live DIR, used only on this thread. Copy
        // each name before the next readdir call invalidates its storage.
        let entry = unsafe { libc::readdir(directory.0.as_ptr()) };
        if entry.is_null() {
            ensure!(
                nix::errno::Errno::last() == nix::errno::Errno::UnknownErrno,
                "read knowledge directory entries failed"
            );
            break;
        }
        // SAFETY: successful readdir returns a valid dirent with a terminated
        // d_name, valid until the next call on this DIR (which is below).
        let bytes = unsafe { std::ffi::CStr::from_ptr((*entry).d_name.as_ptr()) }.to_bytes();
        if bytes == b"." || bytes == b".." {
            continue;
        }
        ensure!(
            names.len() < MAX_ENTRIES,
            "knowledge directory entry limit exceeded"
        );
        names.push(bytes.to_vec());
    }
    names.sort();
    Ok(names)
}

fn directory_flags() -> OFlag {
    OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC
}

fn open_root(path: &Path) -> Result<File> {
    ensure!(
        path.is_absolute() && path.as_os_str().len() <= 4096,
        "knowledge folder requires an explicit bounded absolute path"
    );
    let mut root = File::from(open(Path::new("/"), directory_flags(), Mode::empty())?);
    let mut depth = 0;
    for component in path.components() {
        let name = match component {
            Component::RootDir => continue,
            Component::Normal(name) => name,
            _ => anyhow::bail!("knowledge folder path must not contain parent/current components"),
        };
        depth += 1;
        ensure!(depth <= 32, "knowledge folder ancestor limit exceeded");
        ensure!(
            !name.as_bytes().starts_with(b".")
                && !name.as_bytes().iter().any(|b| *b < 32 || *b == 127),
            "hidden/control-character knowledge paths are forbidden"
        );
        root = File::from(
            openat(&root, Path::new(name), directory_flags(), Mode::empty()).context(
                "knowledge folder ancestor refused (requires real directories, no symlinks)",
            )?,
        );
    }
    ensure!(depth > 0, "filesystem root cannot be a knowledge folder");
    Ok(root)
}

fn permitted_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with('.') || lower.chars().any(char::is_control) {
        return false;
    }
    // Filename exclusion is defense in depth, not a secret detector. Users
    // must select a directory containing only material they intend to preview.
    if [
        "credential",
        "secret",
        "password",
        "token",
        "private_key",
        "private-key",
        "id_rsa",
        "id_ed25519",
        "env.",
    ]
    .iter()
    .any(|part| lower.contains(part))
    {
        return false;
    }
    matches!(
        Path::new(&lower).extension().and_then(|x| x.to_str()),
        Some("txt" | "md")
    )
}

pub(super) fn read_documents(path: &Path) -> Result<Vec<String>> {
    let root = open_root(path)?;
    let names = entry_names(&root)?;
    let mut documents = Vec::new();
    let mut total = 0;
    for bytes in names {
        // Hidden entries are never opened or inspected, including .env.
        if bytes.starts_with(b".") {
            continue;
        }
        let name = std::str::from_utf8(&bytes).context("knowledge filename must be UTF-8")?;
        let path = Path::new(name);
        let stat = fstatat(&root, path, AtFlags::AT_SYMLINK_NOFOLLOW)
            .context("inspect knowledge entry")?;
        let kind = SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT;
        ensure!(
            kind == SFlag::S_IFREG || kind == SFlag::S_IFDIR,
            "knowledge symlink or special entry refused"
        );
        if kind == SFlag::S_IFDIR || !permitted_name(name) {
            continue;
        }
        ensure!(documents.len() < MAX_FILES, "knowledge file limit exceeded");
        ensure!(stat.st_nlink == 1, "knowledge hard-linked file refused");
        ensure!(
            stat.st_size >= 0 && stat.st_size as u64 <= MAX_FILE_BYTES as u64,
            "knowledge per-file byte limit exceeded"
        );
        let file = File::from(
            openat(
                &root,
                path,
                OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK | OFlag::O_CLOEXEC,
                Mode::empty(),
            )
            .context("open knowledge file without symlinks")?,
        );
        let meta = file.metadata()?;
        let opened = fstat(&file)?;
        ensure!(
            meta.is_file()
                && meta.nlink() == 1
                && opened.st_dev == stat.st_dev
                && opened.st_ino == stat.st_ino,
            "knowledge entry changed or is not a regular single-link file"
        );
        ensure!(
            meta.len() <= MAX_FILE_BYTES as u64,
            "knowledge per-file byte limit exceeded"
        );
        ensure!(
            total + meta.len() as usize <= MAX_TOTAL_BYTES,
            "knowledge total byte limit exceeded"
        );
        let mut bytes = Vec::new();
        let remaining = MAX_FILE_BYTES.min(MAX_TOTAL_BYTES - total);
        file.take((remaining + 1) as u64).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() <= remaining,
            "knowledge file grew beyond per-file or total byte limit"
        );
        total += bytes.len();
        ensure!(
            total <= MAX_TOTAL_BYTES,
            "knowledge total byte limit exceeded"
        );
        let document =
            String::from_utf8(bytes).context("knowledge files require plain UTF-8 text")?;
        ensure!(
            !document
                .chars()
                .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t')),
            "knowledge file is not plain text"
        );
        documents.push(document);
    }
    Ok(documents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("abbey-knowledge-fd-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir(&path).unwrap();
            Self(path.canonicalize().unwrap())
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn descriptor_stays_bound_when_root_is_replaced_by_symlink() {
        let scratch = Scratch::new();
        let selected = scratch.0.join("selected");
        let outside = scratch.0.join("outside");
        std::fs::create_dir(&selected).unwrap();
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(selected.join("note.txt"), "selected fixture").unwrap();
        std::fs::write(outside.join("note.txt"), "FORBIDDEN fixture").unwrap();
        let root = open_root(&selected).unwrap();
        std::fs::rename(&selected, scratch.0.join("retained")).unwrap();
        symlink(&outside, &selected).unwrap();
        assert!(open_root(&selected).is_err());
        let mut file = File::from(
            openat(
                &root,
                Path::new("note.txt"),
                OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK,
                Mode::empty(),
            )
            .unwrap(),
        );
        let mut text = String::new();
        file.read_to_string(&mut text).unwrap();
        assert_eq!(text, "selected fixture");
    }

    #[test]
    fn nofollow_refuses_entry_swapped_after_metadata_check() {
        let scratch = Scratch::new();
        let note = scratch.0.join("note.txt");
        let outside = scratch.0.join("outside.txt");
        std::fs::write(&note, "selected fixture").unwrap();
        std::fs::write(&outside, "FORBIDDEN fixture").unwrap();
        let root = open_root(&scratch.0).unwrap();
        let stat = fstatat(&root, Path::new("note.txt"), AtFlags::AT_SYMLINK_NOFOLLOW).unwrap();
        assert_eq!(
            SFlag::from_bits_truncate(stat.st_mode) & SFlag::S_IFMT,
            SFlag::S_IFREG
        );
        std::fs::remove_file(&note).unwrap();
        symlink(&outside, &note).unwrap();
        assert!(
            openat(
                &root,
                Path::new("note.txt"),
                OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK,
                Mode::empty()
            )
            .is_err()
        );
        assert!(read_documents(&scratch.0).is_err());
    }
}
