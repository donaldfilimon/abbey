//! Native evaluation of the corpus's independent byte and digest vectors.
use super::{CORPUS_DOMAIN, FixtureDisposition, FixtureWire, JCS_SAFE_INTEGER, validate_relative};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use std::collections::BTreeSet;

pub(super) fn validate(wire: &FixtureWire) -> Option<FixtureDisposition> {
    let valid = match wire.case_id.as_str() {
        "jcs_unicode_order_escape_negative_zero" => canonical_vector(&wire.document),
        "corpus_independent_digest_vector" => aggregate_vector(&wire.document),
        _ => return None,
    };
    Some(if valid {
        FixtureDisposition::Valid
    } else {
        FixtureDisposition::SchemaInvalid
    })
}
fn canonical_vector(document: &Value) -> bool {
    let Some(input) = document.get("input") else {
        return false;
    };
    let Some(expected) = document.get("expected_hex").and_then(Value::as_str) else {
        return false;
    };
    let mut bytes = b"abbey-jcs-v1:approval:1\0".to_vec();
    if canonical(input, &mut bytes).is_none() {
        return false;
    }
    let actual: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    actual == expected
}
fn canonical(value: &Value, out: &mut Vec<u8>) -> Option<()> {
    match value {
        Value::Null | Value::Bool(_) | Value::String(_) => {
            out.extend(serde_json::to_vec(value).ok()?)
        }
        Value::Number(number) => {
            let text = if let Some(n) = number.as_i64() {
                (n.unsigned_abs() <= JCS_SAFE_INTEGER).then(|| n.to_string())?
            } else if let Some(n) = number.as_u64() {
                (n <= JCS_SAFE_INTEGER).then(|| n.to_string())?
            } else {
                (number.as_f64()? == 0.0).then(|| "0".to_owned())?
            };
            out.extend(text.as_bytes());
        }
        Value::Array(items) => {
            out.push(b'[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                canonical(item, out)?;
            }
            out.push(b']');
        }
        Value::Object(map) => {
            out.push(b'{');
            let mut keys: Vec<_> = map.keys().collect();
            keys.sort_by_key(|key| key.encode_utf16().collect::<Vec<_>>());
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                out.extend(serde_json::to_vec(key).ok()?);
                out.push(b':');
                canonical(&map[key], out)?;
            }
            out.push(b'}');
        }
    }
    Some(())
}
fn aggregate_vector(document: &Value) -> bool {
    aggregate(document).is_some_and(|actual| {
        document.get("expected_digest").and_then(Value::as_str) == Some(actual.as_str())
    })
}
fn aggregate(document: &Value) -> Option<String> {
    let mut entries = Vec::new();
    let mut paths = BTreeSet::new();
    for entry in document.get("entries")?.as_array()? {
        let path = entry.get("path")?.as_str()?;
        // Path::components normalizes repeated separators, interior dots and
        // trailing separators. Commitment paths must already be canonical.
        if path.split('/').any(|part| matches!(part, "" | "." | "..")) {
            return None;
        }
        validate_relative(path).ok()?;
        if !paths.insert(path) {
            return None;
        }
        let bytes = entry.get("bytes")?.as_str()?;
        if bytes.parse::<u64>().ok()?.to_string() != bytes {
            return None;
        }
        let digest = entry.get("sha256")?.as_str()?;
        if digest.len() != 64
            || !digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        entries.push((path, bytes, digest));
    }
    entries.sort_by(|left, right| left.0.as_bytes().cmp(right.0.as_bytes()));
    let mut hasher = Sha256::new();
    hasher.update(CORPUS_DOMAIN);
    for (path, bytes, digest) in entries {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(bytes.as_bytes());
        hasher.update([0]);
        hasher.update(digest.as_bytes());
        hasher.update(b"\n");
    }
    Some(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_profile_rejects_nonzero_floats_and_unsafe_integers() {
        for input in ["1.5", "1.0", "9007199254740992", "-9007199254740992"] {
            let value: Value = serde_json::from_str(input).unwrap();
            assert!(canonical(&value, &mut Vec::new()).is_none(), "{input}");
        }
        let mut zero = Vec::new();
        canonical(&serde_json::from_str::<Value>("-0.0").unwrap(), &mut zero).unwrap();
        assert_eq!(zero, b"0");
    }
    #[test]
    fn aggregate_profile_rejects_normalized_path_aliases() {
        let vector: FixtureWire = serde_json::from_str(include_str!(
            "../../contracts/abbey/corpus/v1/fixtures/valid/corpus-digest-vector.json"
        ))
        .unwrap();
        let mut accepted = Vec::new();
        for alias in ["a//b.json", "a/./b.json", "a/b.json/"] {
            let mut document = vector.document.clone();
            document["entries"][0]["path"] = Value::String("a/b.json".into());
            document["entries"][1]["path"] = Value::String(alias.into());
            if aggregate(&document).is_some() {
                accepted.push(alias);
            }
        }
        assert!(accepted.is_empty(), "accepted path aliases: {accepted:?}");
    }

    #[test]
    fn aggregate_profile_refuses_ambiguous_entry_encodings() {
        let vector: FixtureWire = serde_json::from_str(include_str!(
            "../../contracts/abbey/corpus/v1/fixtures/valid/corpus-digest-vector.json"
        ))
        .unwrap();
        for (field, changed) in [("path", "../a.json"), ("bytes", "03"), ("sha256", "bad")] {
            let mut document = vector.document.clone();
            document["entries"][0][field] = Value::String(changed.into());
            assert!(aggregate(&document).is_none(), "{field}");
        }
    }
}
