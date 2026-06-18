//! Thin binary shim for the unified `rusty-idd` executable. All logic lives in
//! the `rusty_idd_cli` library (so it is reusable and testable).

fn main() {
    std::process::exit(rusty_idd_cli::run());
}
