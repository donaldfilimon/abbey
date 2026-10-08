# Executor stream fixtures

Live captures are optional. Decoder unit tests skip a missing fixture rather
than calling a vendor CLI during `cargo test`. When a capture exists, home
paths must be scrubbed to `/Users/USER` before commit. Record with the
commands in `docs/superpowers/plans/2026-09-29-tui-chat-parity.md` Task 3
Step 1, from a scratch directory.

These files pin observed wire shapes. Vendor formats may change; adjust the
decoder to the fixture, never the fixture to the decoder.
