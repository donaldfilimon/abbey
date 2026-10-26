//! Explicit schema-3 framing: ASCII `ABS3`, then u32BE length and JSON bytes.
//! Legacy schemas retain their original u32BE-length frames. No sniffing/downgrade.
use crate::app_core::MAX_V4_CONTROL_BYTES;
use std::io::{self, Read, Write};

pub(crate) const SCHEMA3_MAGIC: [u8; 4] = *b"ABS3";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameKind {
    Legacy,
    Schema3,
}
impl FrameKind {
    pub(crate) fn limit(self, legacy: usize) -> usize {
        match self {
            Self::Legacy => legacy.min(crate::app_core::MAX_APP_FRAME_BYTES),
            Self::Schema3 => legacy.min(MAX_V4_CONTROL_BYTES),
        }
    }
    pub(crate) fn schema(self) -> u16 {
        match self {
            Self::Legacy => crate::app_core::APP_SCHEMA_V4_CONVERSATIONS,
            Self::Schema3 => crate::app_core::APP_SCHEMA_V4,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FrameFailure {
    Empty,
    Oversize,
    Io,
}
#[derive(Debug)]
pub(crate) struct FrameReadError {
    pub(crate) kind: FrameKind,
    pub(crate) failure: FrameFailure,
}
/// Reads at most eight bytes before refusing an oversized schema-3 allocation.
pub(crate) fn read_frame(
    reader: &mut impl Read,
    legacy_max: usize,
) -> Result<(FrameKind, Vec<u8>), FrameReadError> {
    let mut prefix = [0; 4];
    reader.read_exact(&mut prefix).map_err(|_| FrameReadError {
        kind: FrameKind::Legacy,
        failure: FrameFailure::Io,
    })?;
    let kind = if prefix == SCHEMA3_MAGIC {
        FrameKind::Schema3
    } else {
        FrameKind::Legacy
    };
    if kind == FrameKind::Schema3 {
        reader.read_exact(&mut prefix).map_err(|_| FrameReadError {
            kind,
            failure: FrameFailure::Io,
        })?;
    }
    let length = u32::from_be_bytes(prefix) as usize;
    if length == 0 {
        return Err(FrameReadError {
            kind,
            failure: FrameFailure::Empty,
        });
    }
    if length > kind.limit(legacy_max) {
        return Err(FrameReadError {
            kind,
            failure: FrameFailure::Oversize,
        });
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| FrameReadError {
            kind,
            failure: FrameFailure::Oversize,
        })?;
    bytes.resize(length, 0);
    reader.read_exact(&mut bytes).map_err(|_| FrameReadError {
        kind,
        failure: FrameFailure::Io,
    })?;
    Ok((kind, bytes))
}
pub(crate) fn write_frame(
    writer: &mut impl Write,
    kind: FrameKind,
    bytes: &[u8],
    legacy_max: usize,
) -> io::Result<()> {
    if bytes.is_empty() || bytes.len() > kind.limit(legacy_max) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid control frame length",
        ));
    }
    let length = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid control frame length"))?;
    if kind == FrameKind::Schema3 {
        writer.write_all(&SCHEMA3_MAGIC)?;
    }
    writer.write_all(&length.to_be_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()
}
/// Stops JSON expansion at the cap, including escaped strings and envelope overhead.
pub(crate) fn serialize_bounded(value: &impl serde::Serialize, max: usize) -> io::Result<Vec<u8>> {
    struct Bounded {
        bytes: Vec<u8>,
        max: usize,
    }
    impl Write for Bounded {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.max.saturating_sub(self.bytes.len()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "control frame too large",
                ));
            }
            let required = self.bytes.len() + bytes.len();
            if required > self.bytes.capacity() {
                let target = required
                    .max(self.bytes.capacity().saturating_mul(2).max(4096))
                    .min(self.max);
                self.bytes
                    .try_reserve_exact(target.saturating_sub(self.bytes.len()))
                    .map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::OutOfMemory,
                            "control frame allocation refused",
                        )
                    })?;
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Bounded {
        bytes: Vec::new(),
        max,
    };
    serde_json::to_writer(&mut writer, value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid control frame"))?;
    Ok(writer.bytes)
}
#[cfg(test)]
#[path = "framing_tests.rs"]
mod tests;
