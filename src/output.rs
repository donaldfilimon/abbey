//! Stdout helpers that tolerate broken pipes (`abbey doctor | head`).

use std::io::{self, Write};

/// Write a line to stdout; treat BrokenPipe as success (exit quietly).
pub fn println(s: impl AsRef<str>) -> io::Result<()> {
    let mut out = io::stdout().lock();
    match writeln!(out, "{}", s.as_ref()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e),
    }
}

/// Write without trailing newline; treat BrokenPipe as success.
pub fn print(s: impl AsRef<str>) -> io::Result<()> {
    let mut out = io::stdout().lock();
    match write!(out, "{}", s.as_ref()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        Err(e) => Err(e),
    }
}
