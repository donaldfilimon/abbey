//! Owner-only, no-follow opening for the canonical runtime database.

use super::StoreError;
use rusqlite::{Connection, OpenFlags};
use std::fs::{self, OpenOptions};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::path::Path;

const DIRECTORY_MODE: u32 = 0o700;
const DATABASE_MODE: u32 = 0o600;

pub(super) fn open(path: &Path) -> Result<Connection, StoreError> {
    let parent = path.parent().ok_or(StoreError::InvalidInput(
        "private runtime database has no parent",
    ))?;
    ensure_private_directory(parent)?;
    let canonical_parent = parent.canonicalize()?;
    validate_directory(&fs::symlink_metadata(&canonical_parent)?)?;
    let file_name = path.file_name().ok_or(StoreError::InvalidInput(
        "private runtime database has no file name",
    ))?;
    let canonical_path = canonical_parent.join(file_name);
    ensure_private_database(&canonical_path)?;

    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    // macOS exposes `/var` as a symlink to `/private/var`. Opening the
    // validated canonical parent avoids both rejecting that safe platform
    // layout and allowing an intermediate symlink to retarget the database.
    let connection = Connection::open_with_flags(&canonical_path, flags)?;
    validate_database(&fs::symlink_metadata(&canonical_path)?)?;
    Ok(connection)
}

fn ensure_private_directory(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_directory(&metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(path)?;
            fs::set_permissions(path, fs::Permissions::from_mode(DIRECTORY_MODE))?;
            validate_directory(&fs::symlink_metadata(path)?)?;
            if let Some(parent) = path.parent() {
                fs::File::open(parent)?.sync_all()?;
            }
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn ensure_private_database(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => validate_database(&metadata),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(DATABASE_MODE)
                .open(path)
            {
                Ok(file) => file.sync_all()?,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    validate_database(&fs::symlink_metadata(path)?)?;
                    return Ok(());
                }
                Err(error) => return Err(error.into()),
            }
            validate_database(&fs::symlink_metadata(path)?)?;
            if let Some(parent) = path.parent() {
                fs::File::open(parent)?.sync_all()?;
            }
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn validate_directory(metadata: &fs::Metadata) -> Result<(), StoreError> {
    if !metadata.file_type().is_dir()
        || metadata.file_type().is_symlink()
        || metadata.uid() != nix::unistd::Uid::effective().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(StoreError::InvalidInput(
            "private runtime database directory is not owner-only",
        ));
    }
    Ok(())
}

fn validate_database(metadata: &fs::Metadata) -> Result<(), StoreError> {
    if !metadata.file_type().is_file()
        || metadata.file_type().is_symlink()
        || metadata.uid() != nix::unistd::Uid::effective().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(StoreError::InvalidInput(
            "private runtime database path is not owner-only",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn creates_owner_only_database_and_rejects_unsafe_paths() {
        let root = scratch("private-open");
        let daemon = root.join("daemon");
        let database = daemon.join("runtime.sqlite");
        drop(open(&database).unwrap());
        assert_eq!(
            fs::metadata(&daemon).unwrap().permissions().mode() & 0o777,
            DIRECTORY_MODE
        );
        assert_eq!(
            fs::metadata(&database).unwrap().permissions().mode() & 0o777,
            DATABASE_MODE
        );

        fs::set_permissions(&database, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(open(&database).is_err());
        fs::remove_file(&database).unwrap();
        let target = root.join("target.sqlite");
        fs::write(&target, []).unwrap();
        symlink(&target, &database).unwrap();
        assert!(open(&database).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    fn scratch(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "abbey-runtime-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&path).unwrap();
        path
    }
}
