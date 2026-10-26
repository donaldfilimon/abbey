//! Standalone compatibility executable for the retained Bot process contract.

fn main() -> Result<(), abbey_bot::Error> {
    // SAFETY: this is the process entry, before creating application threads.
    unsafe { abbey_bot::standalone::run() }
}
