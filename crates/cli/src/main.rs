// HFTASK-0082 (ADR-0019 D2): the rusty-idd toolkit is a SEPARATE co-located project, never on
// the kernel trust path. Its error-handling hardening (unwrap/expect/panic = deny) is the
// tracked follow-up HFTASK-0082; until then it opts out of the workspace deny lints so the
// kernel hardening (HFTASK-0080) is not blocked on the toolkit's ~577 sites.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! Thin binary shim for the unified `rusty-idd` executable. All logic lives in
//! the `rusty_idd_cli` library (so it is reusable and testable).

fn main() {
    std::process::exit(rusty_idd_cli::run());
}
