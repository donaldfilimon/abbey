pub fn max_prompt_argv_bytes() -> usize {
    if cfg!(windows) { 24 * 1024 } else { 96 * 1024 }
}
#[test]
fn advertised_prompt_fits_supervisor_argument_ceiling() { assert!(max_prompt_argv_bytes() <= 32 * 1024); }
