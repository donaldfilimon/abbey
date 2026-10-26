use super::*;
use std::io::Cursor;
struct Fragmented<R>(R);
impl<R: Read> Read for Fragmented<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = buf.len().min(1);
        self.0.read(&mut buf[..n])
    }
}
#[test]
fn schema3_prefix_boundary_fragmented_and_coalesced_frames() {
    let body = vec![b'x'; 65536];
    let mut wire = Vec::new();
    write_frame(&mut wire, FrameKind::Schema3, &body, 1024 * 1024).unwrap();
    assert_eq!(&wire[..8], &[0x41, 0x42, 0x53, 0x33, 0, 1, 0, 0]);
    write_frame(&mut wire, FrameKind::Legacy, b"old", 1024 * 1024).unwrap();
    let mut reader = Fragmented(Cursor::new(wire));
    assert_eq!(
        read_frame(&mut reader, 1024 * 1024).unwrap(),
        (FrameKind::Schema3, body)
    );
    assert_eq!(
        read_frame(&mut reader, 1024 * 1024).unwrap(),
        (FrameKind::Legacy, b"old".to_vec())
    );
}
#[test]
fn schema3_rejects_after_header_without_reading_or_allocating_body() {
    for length in [65537, u32::MAX] {
        let mut wire = Vec::from(SCHEMA3_MAGIC);
        wire.extend_from_slice(&length.to_be_bytes());
        let mut reader = Cursor::new(wire);
        let error = read_frame(&mut reader, 1024 * 1024).unwrap_err();
        assert_eq!(reader.position(), 8);
        assert_eq!(error.kind, FrameKind::Schema3);
        assert_eq!(error.failure, FrameFailure::Oversize);
    }
    let mut zero = Vec::from(SCHEMA3_MAGIC);
    zero.extend_from_slice(&0u32.to_be_bytes());
    assert_eq!(
        read_frame(&mut Cursor::new(zero), 1024 * 1024)
            .unwrap_err()
            .failure,
        FrameFailure::Empty
    );
}
#[test]
fn incomplete_prefix_length_and_body_never_become_requests() {
    for wire in [
        b"A".to_vec(),
        b"ABS".to_vec(),
        b"ABS3".to_vec(),
        b"ABS3\0\0".to_vec(),
        b"ABS3\0\0\0\x02x".to_vec(),
    ] {
        assert_eq!(
            read_frame(&mut Cursor::new(wire), 1024 * 1024)
                .unwrap_err()
                .failure,
            FrameFailure::Io
        );
    }
}
#[test]
fn legacy_boundary_and_literal_bytes_are_unchanged() {
    let mut wire = Vec::new();
    write_frame(&mut wire, FrameKind::Legacy, b"{}", 1024 * 1024).unwrap();
    assert_eq!(wire, b"\0\0\0\x02{}");
    let body = vec![0; 1024 * 1024];
    wire.clear();
    write_frame(&mut wire, FrameKind::Legacy, &body, 1024 * 1024).unwrap();
    assert_eq!(
        read_frame(&mut Cursor::new(wire), 1024 * 1024)
            .unwrap()
            .1
            .len(),
        1024 * 1024
    );
    assert!(write_frame(&mut Vec::new(), FrameKind::Schema3, &body, 1024 * 1024).is_err());
}
#[test]
fn serializer_cap_includes_escaping_and_exact_json_overhead() {
    assert_eq!(
        serialize_bounded(&"x".repeat(65534), 65536).unwrap().len(),
        65536
    );
    assert!(serialize_bounded(&"x".repeat(65535), 65536).is_err());
    assert!(serialize_bounded(&"\n".repeat(32768), 65536).is_err());
}

#[test]
fn shared_framing_fixture_matches_the_executing_codec() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/application-schema3/framing.json"
    ))
    .unwrap();
    let hex = |s: &str| -> Vec<u8> {
        assert!(s.len().is_multiple_of(2));
        s.as_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect()
    };
    assert_eq!(fixture["max_payload_bytes"], 65536);
    assert_eq!(hex(fixture["magic_hex"].as_str().unwrap()), SCHEMA3_MAGIC);
    for (kind, key) in [
        (FrameKind::Schema3, "empty_json_object_schema3_hex"),
        (FrameKind::Legacy, "empty_json_object_legacy_hex"),
    ] {
        let mut actual = Vec::new();
        write_frame(&mut actual, kind, b"{}", 1024 * 1024).unwrap();
        assert_eq!(actual, hex(fixture[key].as_str().unwrap()));
    }
}
