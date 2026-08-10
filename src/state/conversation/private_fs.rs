use super::*;
pub(super) fn validate_layout(state: &AbbeyState) -> Result<()> {
    private_directory(&resolved_path(state, &state.state_dir)?)?;
    private_directory(&resolved_path(state, &state.cwd_dir)?)?;
    let lexical_chat = resolved_path(state, &state.chat_file)?;
    for path in [
        resolved_path(state, &state.active_chat_file())?,
        lexical_chat.clone(),
        lexical_chat.with_extension("export"),
        resolved_path(state, &state.history_file)?,
        resolved_path(state, &state.model_file)?,
    ] {
        ensure_not_runtime_path(state, &path)?;
    }
    let targets = configured_targets_for(state, &state.cwd, state.per_cwd)?;
    let mut paths = Vec::with_capacity(targets.len() + 1);
    for (_, path) in targets {
        ensure_not_runtime_path(state, &path)?;
        validate_target_parent(&path)?;
        ensure!(
            !paths.contains(&path),
            "conversation mirror targets must be pairwise distinct"
        );
        paths.push(path);
    }
    let model = resolved_mirror_path(state, &state.model_file)?;
    ensure_not_runtime_path(state, &model)?;
    validate_target_parent(&model)?;
    ensure!(
        !paths.contains(&model),
        "model and conversation mirror targets must be distinct"
    );
    Ok(())
}

pub(super) fn ensure_not_runtime_path(state: &AbbeyState, path: &Path) -> Result<()> {
    let lexical_runtime = resolved_path(state, &state.state_dir)?.join("daemon");
    let canonical_runtime =
        canonical_directory(&resolved_path(state, &state.state_dir)?)?.join("daemon");
    ensure!(
        !path.starts_with(&lexical_runtime) && !path.starts_with(&canonical_runtime),
        "conversation and model files cannot use the daemon runtime subtree"
    );
    Ok(())
}

pub(super) fn resolved_path(state: &AbbeyState, path: &Path) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        let base = if state.cwd.is_absolute() {
            state.cwd.clone()
        } else {
            std::env::current_dir()?.join(&state.cwd)
        };
        base.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(Path::new("/")),
            Component::CurDir => {}
            Component::Normal(part) => normalized.push(part),
            Component::ParentDir => {
                ensure!(
                    normalized.pop(),
                    "conversation mirror path escapes the filesystem root"
                );
            }
        }
    }
    ensure!(
        normalized.is_absolute(),
        "conversation mirror path could not be resolved"
    );
    Ok(normalized)
}

pub(super) fn resolved_mirror_path(state: &AbbeyState, path: &Path) -> Result<PathBuf> {
    canonical_target(&resolved_path(state, path)?)
}

pub(super) fn canonical_target(path: &Path) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .context("conversation mirror target has no file name")?;
    let parent = path
        .parent()
        .context("conversation mirror target has no parent directory")?;
    let parent = canonical_directory(parent)?;
    Ok(parent.join(file_name))
}

pub(super) fn canonical_directory(path: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path)?;
    validate_secure_directory(&canonical)?;
    Ok(canonical)
}

pub(super) fn validate_target_parent(path: &Path) -> Result<()> {
    ensure!(
        canonical_target(path)? == path,
        "conversation mirror target parent is not canonical"
    );
    Ok(())
}

pub(super) fn validate_secure_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.file_type().is_dir(),
        "conversation mirror parent is not a real directory"
    );
    ensure!(
        metadata.uid() == nix::unistd::Uid::effective().as_raw(),
        "conversation mirror parent is not owned by the current user"
    );
    ensure!(
        metadata.permissions().mode() & 0o022 == 0,
        "conversation mirror parent is writable by another user"
    );
    Ok(())
}

pub(super) fn posix_single_quote(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

pub(super) fn target_limit(role: TargetRole) -> usize {
    match role {
        TargetRole::History => MAX_HISTORY_BYTES,
        TargetRole::Active | TargetRole::Global | TargetRole::Export => MAX_ID_FILE_BYTES,
    }
}

pub(super) fn read_first_line_bounded(path: &Path) -> Result<Option<String>> {
    let Some(bytes) = read_optional_bounded(path, MAX_ID_FILE_BYTES)? else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&bytes).context("conversation mirror is not UTF-8")?;
    Ok(text
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned))
}

pub(super) fn read_optional_bounded(path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
    let mut file = match open_private_file(path, false) {
        Ok(file) => file,
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    ensure!(
        usize::try_from(file.metadata()?.len()).unwrap_or(usize::MAX) <= limit,
        "conversation mirror exceeds its size bound"
    );
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "conversation mirror exceeds its size bound"
    );
    Ok(Some(bytes))
}

pub(super) fn private_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => ensure!(
            metadata.file_type().is_dir(),
            "conversation journal path is not a directory"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Err(create_error) = fs::create_dir(path)
                && create_error.kind() != std::io::ErrorKind::AlreadyExists
            {
                return Err(create_error.into());
            }
        }
        Err(error) => return Err(error.into()),
    }
    set_private_directory_permissions(path)?;
    validate_owner(path, true)
}

pub(super) fn open_private_file(path: &Path, create: bool) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(create).create(create);
    configure_private_open(&mut options);
    let file = options.open(path)?;
    validate_open_file(&file)?;
    if create {
        set_private_file_permissions(path)?;
    }
    Ok(file)
}

pub(super) fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("conversation mirror has no parent directory")?;
    if !parent.exists() {
        fs::create_dir_all(parent)?;
    }
    validate_secure_directory(parent)?;
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let _ = read_optional_bounded(path, bytes.len().max(MAX_ID_FILE_BYTES))?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    let temporary = parent.join(format!(".abbey-mirror-{}.tmp", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    configure_private_open(&mut options);
    let mut file = options.open(&temporary)?;
    set_private_file_permissions(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)?;
        sync_directory(parent)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

pub(super) fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    Ok(())
}

pub(super) fn configure_private_open(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt as _;
    options.mode(0o600);
    options.custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
}

pub(super) fn validate_open_file(file: &File) -> Result<()> {
    ensure!(
        file.metadata()?.file_type().is_file(),
        "conversation mirror is not a regular file"
    );
    validate_open_owner(file)?;
    make_open_file_private(file)
}

pub(super) fn validate_open_owner(file: &File) -> Result<()> {
    use std::os::unix::fs::MetadataExt as _;
    ensure!(
        file.metadata()?.uid() == nix::unistd::Uid::effective().as_raw(),
        "conversation mirror is not owned by the current user"
    );
    Ok(())
}

pub(super) fn make_open_file_private(file: &File) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(())
}

pub(super) fn validate_owner(path: &Path, directory: bool) -> Result<()> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.uid() == nix::unistd::Uid::effective().as_raw(),
        "conversation journal is not owned by the current user"
    );
    ensure!(
        if directory {
            metadata.file_type().is_dir()
        } else {
            metadata.file_type().is_file()
        },
        "conversation journal has the wrong file type"
    );
    ensure!(
        metadata.permissions().mode() & 0o077 == 0,
        "conversation journal permissions are not owner-only"
    );
    Ok(())
}

pub(super) fn set_private_directory_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub(super) fn set_private_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

pub(super) fn path_to_hex(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt as _;
    lower_hex(path.as_os_str().as_bytes())
}

pub(super) fn path_from_hex(value: &str) -> Result<PathBuf> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;
    Ok(PathBuf::from(OsString::from_vec(decode_hex(value)?)))
}

pub(super) fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    encoded
}

pub(super) fn decode_hex(value: &str) -> Result<Vec<u8>> {
    ensure!(
        value.len().is_multiple_of(2),
        "conversation journal contains invalid hex"
    );
    (0..value.len())
        .step_by(2)
        .map(|index| {
            let bytes = value.as_bytes();
            let high = hex_nibble(bytes[index])?;
            let low = hex_nibble(bytes[index + 1])?;
            Ok((high << 4) | low)
        })
        .collect()
}

pub(super) fn hex_nibble(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => bail!("conversation journal contains invalid hex"),
    }
}
