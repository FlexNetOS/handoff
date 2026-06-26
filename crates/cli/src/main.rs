// HFTASK-0082 (ADR-0019 D5 #3): the rusty-idd toolkit now enforces the same error-handling deny
// lints (unwrap/expect/panic) in PRODUCTION as the kernel; they are allowed only under test
// (tests assert). The toolkit's production code already propagated errors, so the hardening was
// a clean flip — no bare production unwrap remained.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! Thin binary shim for the unified `rusty-idd` executable. All logic lives in
//! the `rusty_idd_cli` library (so it is reusable and testable).

fn main() {
    std::process::exit(rusty_idd_cli::run());
}
