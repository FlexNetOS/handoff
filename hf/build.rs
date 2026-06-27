//! HFTASK-0085 (automation rung 0): stamp the hf binary with its build provenance so any
//! consumer (loop-init Phase 0, the SessionStart hook, a fleet member) can tell whether the
//! installed binary is BEHIND the kernel source — the root primitive every staleness
//! automation needs.
//!
//! Emits two compile-time env vars consumed via `option_env!` in main.rs:
//!   HF_BUILD_COMMIT — short git commit of the kernel source at build time
//!   HF_BUILD_DATE   — UTC build date (YYYY-MM-DD), best-effort
//!
//! Resolution order for the commit, fail-soft at every step (a .git-less release tarball or a
//! sandbox without `git` must still build — it just stamps "unknown"): an injected GITHUB_SHA
//! (CI) → `git rev-parse --short HEAD` → "unknown". No unwrap/expect/panic (workspace deny lints).

use std::process::Command;

fn git_short_commit() -> Option<String> {
    // CI injects the precise commit; prefer it so a tarball build with no .git is still stamped.
    if let Ok(sha) = std::env::var("GITHUB_SHA") {
        let short: String = sha.chars().take(12).collect();
        if !short.is_empty() {
            return Some(short);
        }
    }
    let out = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Best-effort UTC date (YYYY-MM-DD) from SOURCE_DATE_EPOCH (reproducible builds) or `date`.
fn build_date() -> String {
    if let Ok(epoch) = std::env::var("SOURCE_DATE_EPOCH")
        && let Ok(secs) = epoch.parse::<i64>()
    {
        return civil_date_from_unix(secs);
    }
    Command::new("date")
        .args(["-u", "+%Y-%m-%d"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string())
}

/// Convert a unix timestamp to a civil YYYY-MM-DD (UTC) without any date dependency.
/// Howard Hinnant's days-from-civil inverse; pure integer math, no panics.
fn civil_date_from_unix(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

fn main() {
    let commit = git_short_commit().unwrap_or_else(|| "unknown".to_string());
    println!("cargo:rustc-env=HF_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=HF_BUILD_DATE={}", build_date());
    // Re-stamp when HEAD moves (a new commit) or the build script changes.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    emit_git_rerun_watches();
}

/// Resolve the kernel checkout's real git directory, worktree-aware (HFTASK-0095).
/// build.rs runs with CWD = the crate dir (`hf/`), so the repo `.git` is typically at `../.git`.
/// A normal checkout's `.git` is a directory; a LINKED WORKTREE's `.git` is a FILE containing
/// `gitdir: <path>`. Returns the gitdir, or None for a `.git`-less build (release tarball /
/// sandbox) — in which case no rerun watch is emitted (graceful: the binary still stamps the
/// commit captured at build time). Fail-soft at every step; no unwrap/expect/panic.
fn resolve_gitdir() -> Option<std::path::PathBuf> {
    use std::path::{Path, PathBuf};
    for cand in ["../.git", ".git"] {
        let p = Path::new(cand);
        if p.is_dir() {
            return Some(p.to_path_buf());
        }
        if p.is_file() {
            // Worktree gitlink: "gitdir: <abs-or-rel path>". A parse failure on this candidate
            // falls through to the next rather than aborting the resolution.
            if let Ok(contents) = std::fs::read_to_string(p)
                && let Some(rest) = contents
                    .lines()
                    .next()
                    .and_then(|l| l.strip_prefix("gitdir:"))
            {
                let gd = PathBuf::from(rest.trim());
                let gd = if gd.is_absolute() {
                    gd
                } else {
                    p.parent().unwrap_or_else(|| Path::new(".")).join(gd)
                };
                if gd.exists() {
                    return Some(gd);
                }
            }
        }
    }
    None
}

/// Emit `cargo:rerun-if-changed` for the files that actually move on a commit, so the build
/// stamp never lags HEAD (HFTASK-0095). The old code watched only `.git/HEAD`, but a normal
/// `git commit` moves the branch ref (`.git/refs/heads/<branch>`) — NOT `.git/HEAD` — so cargo
/// never re-ran build.rs after a commit and `HF_BUILD_COMMIT` went stale, making the SessionStart
/// staleness check false-positive. The reflog (`logs/HEAD`) updates on EVERY commit/checkout/reset
/// in both normal repos and linked worktrees, so it is the robust catch-all; HEAD + the concrete
/// ref (gitdir-local and, for a worktree, the shared common dir) are belt-and-suspenders.
fn emit_git_rerun_watches() {
    let Some(gitdir) = resolve_gitdir() else {
        return;
    };
    let watch = |path: std::path::PathBuf| {
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    };
    watch(gitdir.join("HEAD"));
    watch(gitdir.join("logs/HEAD"));
    // The concrete ref HEAD points at (e.g. refs/heads/develop), so a loose-ref update with no
    // reflog still re-stamps. Resolve against the gitdir and, for a worktree, the common dir.
    if let Ok(head) = std::fs::read_to_string(gitdir.join("HEAD"))
        && let Some(refpath) = head.lines().next().and_then(|l| l.strip_prefix("ref:"))
    {
        let refpath = refpath.trim();
        watch(gitdir.join(refpath));
        if let Ok(common) = std::fs::read_to_string(gitdir.join("commondir")) {
            watch(gitdir.join(common.trim()).join(refpath));
        }
    }
}
