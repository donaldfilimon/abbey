//! Golden fixtures must exercise their commitments, not only their JSON shape.
use super::*;

fn wire(vector: &str) -> FixtureWire {
    serde_json::from_str(vector).unwrap()
}
const JCS: &str = include_str!("../../contracts/abbey/corpus/v1/fixtures/valid/jcs-vector.json");
const CORPUS: &str =
    include_str!("../../contracts/abbey/corpus/v1/fixtures/valid/corpus-digest-vector.json");
fn disposition(wire: &FixtureWire) -> FixtureDisposition {
    let corpus = ContractCorpus::qualified().unwrap();
    fixture_validation::validate_wire(wire, &corpus.schemas)
}

#[test]
fn corpus_golden_expected_canonical_bytes_are_computed() {
    let mut vector = wire(JCS);
    assert_eq!(disposition(&vector), FixtureDisposition::Valid);
    vector.document["expected_hex"] = Value::String("00".repeat(80));
    assert_ne!(disposition(&vector), FixtureDisposition::Valid);
}

#[test]
fn corpus_golden_expected_aggregate_digest_is_computed() {
    let mut vector = wire(CORPUS);
    assert_eq!(disposition(&vector), FixtureDisposition::Valid);
    vector.document["expected_digest"] = Value::String("0".repeat(64));
    assert_ne!(disposition(&vector), FixtureDisposition::Valid);
}

#[test]
fn corpus_golden_digest_orders_paths_and_rejects_duplicates() {
    let mut vector = wire(CORPUS);
    vector.document["entries"].as_array_mut().unwrap().reverse();
    assert_eq!(disposition(&vector), FixtureDisposition::Valid);
    vector.document["entries"][1]["path"] = vector.document["entries"][0]["path"].clone();
    assert_ne!(disposition(&vector), FixtureDisposition::Valid);
}

#[test]
fn corpus_golden_canonical_order_uses_utf16_and_normalizes_negative_zero() {
    let mut vector = wire(JCS);
    vector.document["input"] = serde_json::json!({"\u{e000}": "bmp", "𝄞": "music", "zero": -0.0});
    // This literal is independent of the canonicalizer. UTF-16 places the
    // non-BMP key before U+E000; Rust string/UTF-8 ordering does the reverse.
    let expected = "abbey-jcs-v1:approval:1\0{\"zero\":0,\"𝄞\":\"music\",\"\u{e000}\":\"bmp\"}";
    vector.document["expected_hex"] = Value::String(
        expected
            .as_bytes()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    );
    assert_eq!(disposition(&vector), FixtureDisposition::Valid);
}
