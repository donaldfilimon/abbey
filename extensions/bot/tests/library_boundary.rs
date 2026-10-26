//! A downstream client can link the actual Bot library without running startup.

#[test]
fn library_links_domain_state_and_explicit_process_entry() {
    let _entry: unsafe fn() -> Result<(), abbey_bot::Error> = abbey_bot::standalone::run;
    assert!(std::mem::size_of::<abbey_bot::Data>() > 0);
}
