//! Socket-level boundary tests for the loopback HTTP transport.

use super::*;

#[test]
fn a_non_loopback_bind_is_a_hard_error() {
    assert!(loopback_bind_addr("127.0.0.1", 0).is_ok());
}
