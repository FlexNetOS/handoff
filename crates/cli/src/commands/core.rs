//! Core-verb delegation: reconstruct the argv that
//! [`rusty_idd_core::cli::run`] expects and forward it verbatim.
//!
//! `cli::run` parses `argv[0]` as the program name and `argv[1]` as the verb,
//! so we hand it `["idd", <verb>, <passthrough...>]`. This is the *same* code
//! path the legacy `idd` binary uses, which is what guarantees parity.

/// Delegate a core verb to the idd core CLI, mapping its `Result<(), String>`
/// to a process exit code (0 on success, 1 on error with the message printed to
/// stderr — matching `idd`'s own `main`).
pub fn delegate(verb: &str, passthrough: &[String]) -> i32 {
    let mut argv: Vec<String> = Vec::with_capacity(passthrough.len() + 2);
    argv.push("idd".to_string());
    argv.push(verb.to_string());
    argv.extend(passthrough.iter().cloned());

    match rusty_idd_core::cli::run(argv) {
        Ok(()) => 0,
        Err(err) => {
            eprintln!("idd: {err}");
            1
        }
    }
}
