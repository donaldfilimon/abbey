use super::*;
use std::io::{self, Read};

struct CountingReader {
    remaining: usize,
    consumed: std::rc::Rc<std::cell::Cell<usize>>,
}
impl Read for CountingReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = buffer.len().min(self.remaining).min(17);
        buffer[..count].fill(b'a');
        self.remaining -= count;
        self.consumed.set(self.consumed.get() + count);
        Ok(count)
    }
}
#[test]
fn growing_input_has_fixed_read_budget() {
    let consumed = std::rc::Rc::new(std::cell::Cell::new(0));
    let result = read_bearer(
        CountingReader {
            remaining: 8192,
            consumed: consumed.clone(),
        },
        Path::new("fixture"),
    );
    assert!(matches!(result, Err(ConfigError::BearerLength)));
    assert!(
        consumed.get() <= MAX_BEARER_LEN + 3,
        "read {} bytes",
        consumed.get()
    );
}
#[test]
fn exact_cap_and_line_endings() {
    for ending in [b"".as_slice(), b"\n", b"\r\n"] {
        let mut bytes = vec![b'a'; MAX_BEARER_LEN];
        bytes.extend_from_slice(ending);
        assert_eq!(
            read_bearer(bytes.as_slice(), Path::new("fixture"))
                .unwrap()
                .as_str()
                .len(),
            MAX_BEARER_LEN
        );
    }
}

#[test]
fn malformed_and_oversize_inputs_keep_typed_errors() {
    let path = Path::new("fixture");
    assert!(matches!(
        read_bearer(&b"short"[..], path),
        Err(ConfigError::BearerLength)
    ));
    let mut bytes = vec![b'a'; 32];
    bytes[0] = 0xff;
    assert!(matches!(
        read_bearer(bytes.as_slice(), path),
        Err(ConfigError::BearerNotUtf8)
    ));
    bytes[0] = b'\t';
    assert!(matches!(
        read_bearer(bytes.as_slice(), path),
        Err(ConfigError::BearerControlCharacter)
    ));
    assert!(matches!(
        read_bearer(vec![b'a'; MAX_BEARER_LEN + 1].as_slice(), path),
        Err(ConfigError::BearerLength)
    ));
}

#[test]
fn partial_read_failure_is_not_accepted_as_a_token() {
    struct Broken(bool);
    impl Read for Broken {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.0 {
                return Err(io::Error::other("fixture failure"));
            }
            self.0 = true;
            buffer[..32].fill(b'a');
            Ok(32)
        }
    }
    assert!(matches!(
        read_bearer(Broken(false), Path::new("fixture")),
        Err(ConfigError::ReadBearer { .. })
    ));
}

#[test]
fn filesystem_types_and_parent_symlink_compatibility() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = std::env::temp_dir().join(format!("abbey-bearer-types-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let file = root.join("token");
    std::fs::write(&file, vec![b'a'; 32]).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(load_bearer_file(&file).is_ok());
    symlink(&file, root.join("link")).unwrap();
    assert!(matches!(
        load_bearer_file(&root.join("link")),
        Err(ConfigError::BearerFileType(_))
    ));
    assert!(matches!(
        load_bearer_file(&root),
        Err(ConfigError::BearerFileType(_))
    ));
    symlink(&root, root.join("parent")).unwrap();
    assert!(load_bearer_file(&root.join("parent/token")).is_ok());
    let fifo = root.join("fifo");
    nix::unistd::mkfifo(
        &fifo,
        nix::sys::stat::Mode::S_IRUSR | nix::sys::stat::Mode::S_IWUSR,
    )
    .unwrap();
    assert!(matches!(
        load_bearer_file(&fifo),
        Err(ConfigError::BearerFileType(_))
    ));
    std::fs::remove_dir_all(root).unwrap();
}
