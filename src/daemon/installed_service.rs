//! Read-only discovery of the Linux bootstrap's retained configuration.
use super::config::{ConfigError, DaemonConfig};
#[cfg(any(target_os = "linux", test))]
use crate::edition::Edition;
#[cfg(any(target_os = "linux", test))]
use std::path::{Path, PathBuf};

/// Fixed bootstrap paths; neither XDG nor Abbey state overrides are consulted.
#[cfg(any(target_os = "linux", test))]
struct InstalledServicePaths {
    config: PathBuf,
    socket: PathBuf,
}
#[cfg(any(target_os = "linux", test))]
impl InstalledServicePaths {
    fn for_home(edition: Edition, home: &Path) -> Self {
        Self {
            config: home
                .join(".config")
                .join(format!("{}-user-service", edition.slug())),
            socket: edition.daemon_socket_path(&home.join(".local/state").join(edition.slug())),
        }
    }
}
impl DaemonConfig {
    /// Linux-only production entry point. Other hosts retain environment-only routing.
    pub fn discover_installed_service() -> Result<Option<Self>, ConfigError> {
        #[cfg(target_os = "linux")]
        {
            let user = nix::unistd::User::from_uid(nix::unistd::Uid::effective())
                .map_err(|_| ConfigError::InstalledServiceConfiguration)?
                .ok_or(ConfigError::InstalledServiceConfiguration)?;
            Self::from_installed_service(&InstalledServicePaths::for_home(
                crate::edition::ACTIVE,
                &user.dir,
            ))
        }
        #[cfg(not(target_os = "linux"))]
        Ok(None)
    }
    /// Inspect only the derived layout. Valid retained credentials do not prove installation or readiness.
    #[cfg(any(target_os = "linux", test))]
    fn from_installed_service(paths: &InstalledServicePaths) -> Result<Option<Self>, ConfigError> {
        #[cfg(all(unix, any(target_os = "linux", test)))]
        {
            inspect(paths).map_err(|_| ConfigError::InstalledServiceConfiguration)
        }
        #[cfg(not(unix))]
        {
            let _ = paths;
            Ok(None)
        }
    }
}
#[cfg(all(unix, any(target_os = "linux", test)))]
fn inspect(paths: &InstalledServicePaths) -> Result<Option<DaemonConfig>, ()> {
    use nix::fcntl::{OFlag, openat};
    use nix::sys::stat::Mode;
    use std::os::unix::fs::MetadataExt;
    // Absence is decided before inspecting unrelated home ancestry. A user who
    // never configured this edition must not be rejected for unrelated layout.
    match std::fs::symlink_metadata(&paths.config) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(()),
        Ok(_) => {}
    }
    let config = open_directory(&paths.config, &[&paths.config])?;
    let fd = openat(
        &config,
        "bearer",
        OFlag::O_RDONLY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK,
        Mode::empty(),
    )
    .map_err(|_| ())?;
    let file = std::fs::File::from(fd);
    let meta = file.metadata().map_err(|_| ())?;
    if !meta.is_file()
        || meta.uid() != nix::unistd::Uid::effective().as_raw()
        || meta.mode() & 0o077 != 0
    {
        return Err(());
    }
    let bearer = super::config::read_bearer(file, Path::new("retained-service-credential"))
        .map_err(|_| ())?;
    let daemon = paths.socket.parent().ok_or(())?;
    let state = daemon.parent().ok_or(())?;
    // Missing state directories and sockets mean configured/offline, not absent.
    if let Some(dir) = open_optional_directory(daemon, &[state, daemon])? {
        match nix::sys::stat::fstatat(
            &dir,
            paths.socket.file_name().ok_or(())?,
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        ) {
            Ok(stat) => {
                if stat.st_mode & libc::S_IFMT != libc::S_IFSOCK
                    || stat.st_uid != nix::unistd::Uid::effective().as_raw()
                    || stat.st_mode & 0o077 != 0
                {
                    return Err(());
                }
            }
            Err(nix::errno::Errno::ENOENT) => {}
            Err(_) => return Err(()),
        }
    }
    Ok(Some(DaemonConfig::local(paths.socket.clone(), bearer)))
}

#[cfg(all(unix, any(target_os = "linux", test)))]
fn open_directory(path: &Path, private: &[&Path]) -> Result<std::fs::File, ()> {
    open_optional_directory(path, private)?.ok_or(())
}

#[cfg(all(unix, any(target_os = "linux", test)))]
fn open_optional_directory(path: &Path, private: &[&Path]) -> Result<Option<std::fs::File>, ()> {
    use nix::fcntl::{OFlag, open, openat};
    use nix::sys::stat::Mode;
    use std::os::unix::fs::MetadataExt;
    use std::path::Component;
    if !path.is_absolute()
        || path
            .as_os_str()
            .as_encoded_bytes()
            .iter()
            .any(|b| *b < 32 || *b == 127)
    {
        return Err(());
    }
    let flags = OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_NOFOLLOW | OFlag::O_NONBLOCK;
    let mut directory =
        std::fs::File::from(open(Path::new("/"), flags, Mode::empty()).map_err(|_| ())?);
    let mut current = PathBuf::from("/");
    for component in path.components() {
        let name = match component {
            Component::RootDir => continue,
            Component::Normal(name) => name,
            _ => return Err(()),
        };
        current.push(name);
        let fd = match openat(&directory, Path::new(name), flags, Mode::empty()) {
            Ok(fd) => fd,
            Err(nix::errno::Errno::ENOENT) => return Ok(None),
            Err(_) => return Err(()),
        };
        directory = std::fs::File::from(fd);
        let meta = directory.metadata().map_err(|_| ())?;
        let uid = nix::unistd::Uid::effective().as_raw();
        if private.contains(&current.as_path()) {
            if meta.uid() != uid || meta.mode() & 0o7777 != 0o700 {
                return Err(());
            }
        } else if (meta.uid() != uid && meta.uid() != 0)
            || (meta.mode() & 0o022 != 0 && !(meta.uid() == 0 && meta.mode() & 0o1000 != 0))
        {
            return Err(());
        }
    }
    Ok(Some(directory))
}

#[cfg(all(test, unix))]
mod tests;
