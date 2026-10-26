use anyhow::{Context, Result, bail, ensure};
use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub(crate) const MAX_JSON: usize = 8 * 1024 * 1024;
struct Strict(Value);
impl<'de> Deserialize<'de> for Strict {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Strict;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> std::result::Result<Strict, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| Strict(Value::Number(n)))
                    .ok_or_else(|| E::custom("nonfinite JSON"))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_string<E: serde::de::Error>(
                self,
                v: String,
            ) -> std::result::Result<Strict, E> {
                Ok(Strict(v.into()))
            }
            fn visit_unit<E: serde::de::Error>(self) -> std::result::Result<Strict, E> {
                Ok(Strict(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut access: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut rows = Vec::new();
                while let Some(Strict(v)) = access.next_element()? {
                    rows.push(v);
                }
                Ok(Strict(rows.into()))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut access: A,
            ) -> std::result::Result<Strict, A::Error> {
                let mut map = serde_json::Map::new();
                while let Some((key, Strict(value))) = access.next_entry::<String, Strict>()? {
                    if map.insert(key, value).is_some() {
                        return Err(serde::de::Error::custom("duplicate JSON key"));
                    }
                }
                Ok(Strict(Value::Object(map)))
            }
        }
        deserializer.deserialize_any(V)
    }
}
pub(crate) fn parse_json(bytes: &[u8]) -> Result<Value> {
    Ok(serde_json::from_slice::<Strict>(bytes)?.0)
}
pub(crate) fn bounded_read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "input exceeds byte limit");
    Ok(bytes)
}
pub(crate) fn read_json(path: &Path) -> Result<Value> {
    parse_json(&bounded_read(path, MAX_JSON)?)
        .with_context(|| format!("read JSON {}", path.display()))
}
pub(crate) fn read_jsonl(path: &Path) -> Result<Vec<Value>> {
    let bytes = bounded_read(path, MAX_JSON)?;
    std::str::from_utf8(&bytes)?
        .lines()
        .filter(|s| !s.trim().is_empty())
        .map(|s| parse_json(s.as_bytes()))
        .collect()
}
pub(crate) fn canonical(value: &Value) -> Vec<u8> {
    fn ordered(v: &Value) -> Value {
        match v {
            Value::Object(map) => {
                let sorted: BTreeMap<_, _> =
                    map.iter().map(|(k, v)| (k.clone(), ordered(v))).collect();
                Value::Object(sorted.into_iter().collect())
            }
            Value::Array(rows) => rows.iter().map(ordered).collect(),
            _ => v.clone(),
        }
    }
    serde_json::to_vec(&ordered(value)).expect("JSON value is serializable")
}
pub(crate) fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub(crate) fn digest_value(value: &Value) -> String {
    digest_bytes(&canonical(value))
}
pub(crate) fn sha256_file(path: &Path) -> Result<String> {
    let mut hash = Sha256::new();
    let mut file = File::open(path)?;
    let mut buffer = [0u8; 65536];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
pub(crate) fn write_json_new(path: &Path, value: &Value) -> Result<()> {
    let mut bytes = canonical(value);
    bytes.push(b'\n');
    ensure!(bytes.len() <= MAX_JSON, "JSON output exceeds bound");
    write_new(path, &bytes)
}
pub(crate) fn create_dir_private(path: &Path, recursive: bool) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(recursive);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}
pub(crate) fn private_file_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
}
pub(crate) fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = private_file_options().open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
pub(crate) fn consent(path: &Path, private: bool) -> Result<()> {
    let bytes = bounded_read(path, 4096)?;
    let lines: Vec<_> = std::str::from_utf8(&bytes)?.lines().collect();
    ensure!(
        lines.contains(&"abbey-train-consent: v1"),
        "explicit training consent required"
    );
    ensure!(
        !private || lines.contains(&"abbey-private-training-consent: v1"),
        "private examples require separate private training consent"
    );
    Ok(())
}
pub(crate) fn checked_text(value: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= 65536 && !value.contains('\0'),
        "nonempty bounded text required"
    );
    Ok(())
}
pub(crate) fn absent(path: &Path) -> Result<()> {
    ensure!(
        fs::symlink_metadata(path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound),
        "output already exists or is inaccessible"
    );
    Ok(())
}
pub(crate) fn atomic_json(path: &Path, value: &Value) -> Result<()> {
    let temp = path.with_file_name(format!(".publish-{}", uuid::Uuid::new_v4()));
    write_json_new(&temp, value)?;
    let result = fs::rename(&temp, path);
    if temp.exists() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    Ok(())
}
pub(crate) fn tree_identity(path: &Path) -> Result<Value> {
    fn walk(
        base: &Path,
        path: &Path,
        files: &mut BTreeMap<String, Value>,
        depth: usize,
    ) -> Result<()> {
        ensure!(depth <= 32, "artifact directory depth exceeded");
        let mut entries = fs::read_dir(path)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let p = entry.path();
            let meta = fs::symlink_metadata(&p)?;
            if meta.is_dir() {
                walk(base, &p, files, depth + 1)?;
            } else if p.is_file() {
                let name = p
                    .strip_prefix(base)?
                    .to_str()
                    .context("UTF-8 artifact path required")?
                    .replace('\\', "/");
                if name == "abbey-export.json" {
                    continue;
                }
                ensure!(files.len() < 10000, "artifact file count exceeded");
                files.insert(
                    name,
                    json!({"sha256":sha256_file(&p)?,"bytes":fs::metadata(&p)?.len()}),
                );
            } else {
                bail!("unsupported artifact entry");
            }
        }
        Ok(())
    }
    ensure!(path.is_dir(), "local artifact directory required");
    let mut files = BTreeMap::new();
    walk(path, path, &mut files, 0)?;
    ensure!(!files.is_empty(), "empty artifact");
    let files = serde_json::to_value(files)?;
    Ok(json!({"sha256":digest_value(&files),"files":files}))
}
pub(crate) fn model_identity(path: &Path) -> Result<Value> {
    let identity = tree_identity(path)?;
    let files = identity["files"]
        .as_object()
        .context("model files required")?;
    ensure!(
        files.contains_key("config.json")
            && files
                .keys()
                .any(|s| s.ends_with(".safetensors") || s.ends_with(".gguf")),
        "actual config and model weights required"
    );
    ensure!(
        files.contains_key("tokenizer.json") || files.contains_key("tokenizer.model"),
        "local tokenizer required"
    );
    Ok(identity)
}
pub(crate) struct Stage {
    pub path: PathBuf,
}
impl Stage {
    pub fn new(output: &Path) -> Result<Self> {
        absent(output)?;
        let parent = output.parent().context("output parent required")?;
        create_dir_private(parent, true)?;
        let path = parent.join(format!(".learning-stage-{}", uuid::Uuid::new_v4()));
        create_dir_private(&path, false)?;
        Ok(Self { path })
    }
    pub fn publish(self, output: &Path) -> Result<()> {
        absent(output)?;
        fs::rename(&self.path, output)?;
        Ok(())
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        if self.path.exists() {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}
