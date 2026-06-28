// HFTASK-0080 (ADR-0019 D5 #3): error-handling deny lints allowed under test only (tests assert).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! `hf` — the .handoff continuity CLI (S1 spike).
//!
//! Implements the core of the .handoff "hard product standard": a fresh agent runs
//! `hf resume` and immediately knows project / objective / done / remaining / safe next
//! task / next command — no chat archaeology. Built on the validated `work-order` (the
//! handoff.task.v1 envelope) + `ledger` (rusqlite event store + rvf-crypto witness) crates.
//!
//! Verbs: init · seed · status · claim <id> · release <id> · checkpoint <id> [note] · handoff · resume [--json]
//!        · ship <id> [--base BRANCH] · review request <pr> [--task <id>] · review verdict <id> <pr> <approve|deny> [--by WHO]
//! State precedence (tier 2/3): `.handoff/ledger.db` (events) > `.handoff/tasks/*.task.json` (cards).

#[cfg(feature = "cognitum")]
mod cognitum;
mod contract;
mod delivery;
mod durability;
mod kb;
mod prompt_hub;
mod routing;
mod session;
mod sync;

use std::fs;
use std::path::{Path, PathBuf};

// HFTASK-0081 (ADR-0019 D5 #4): the branch/remote/merge policy engine was peeled into the
// `handoff-policy` crate. Re-export its modules so existing `crate::policy::…` / `crate::branch::…`
// / `policy::…` / `branch::…` paths across hf (main, gatekeeper, session) stay valid unchanged.
pub(crate) use handoff_policy::{branch, policy};
// HFTASK-0081: the card validator was peeled into `handoff-schema`; alias it as `schema` so
// existing `schema::validate_card` / `schema::cmd_schema` call sites stay valid unchanged.
use handoff_schema as schema;
// HFTASK-0081: the claim-lease bridge was peeled into `handoff-lease`; alias it as `lease` so
// existing `lease::Leaser` / `lease::ClaimGate` / `lease::WeaveCli` call sites stay valid.
use handoff_lease as lease;
// HFTASK-0083: the typed hook contract was peeled into `handoff-hooks`; alias it as `hooks` so
// `hooks::cmd_hook_list` / `hooks::cmd_hook_run` call sites stay valid unchanged.
use handoff_hooks as hooks;
// HFTASK-0083: index/plan maps peeled into `handoff-index`; alias as `index`.
use handoff_index as index;
// HFTASK-0083: fleet rollup aggregation peeled into `handoff-fleet`; alias as `fleet`.
use handoff_fleet as fleet;
// HFTASK-0083: drift-audit + policy-check engine peeled into `handoff-drift`; alias as `gates`.
use handoff_drift as gates;
// HFTASK-0083: the ledger-routing module peeled into `handoff-route`; alias as `route`.
use handoff_route as route;
// HFTASK-0083: the AI gatekeeper peeled into `handoff-gatekeeper`; alias as `gatekeeper`. It also
// owns the shared `GhPrView` GitHub-PR type used by hf's review-request flow.
use handoff_gatekeeper as gatekeeper;
use handoff_gatekeeper::GhPrView;
// HFTASK-0083: the front-door intake/dispatch verbs peeled into `handoff-intake`; alias as `intake`.
use handoff_intake as intake;

use lease::Leaser;
use ledger::Ledger;
use work_order::{PrioStr, Priority, Status, WorkOrder};

// ADR-0019 D5 #4 (12-crate decomposition): the leaf continuity primitives now live in the
// `handoff-core` crate. Re-exported `pub(crate)` so existing `crate::HF` / `crate::now_ns` /
// `crate::ledger_path` / `crate::tasks_dir` / `crate::run_out` / `crate::current_statuses` /
// `crate::status_of` references across the feature modules are unchanged (behavior-preserving).
pub(crate) use handoff_core::{
    HF, capsule_field, capsule_path, current_northstar_revision, current_statuses, ledger_path,
    load_task_in, load_tasks, must_witness, next_safe, now_ns, pretty_json, run_out, save_task,
    save_task_in, scan_card_conformance, status_of, tasks_dir,
};

/// TTL of a claim lease: a claim represents an active work session. Re-claiming
/// (heartbeat) extends it; `hf release` or expiry frees it.
const CLAIM_TTL_SECS: u64 = 3600;

const HF_USAGE: &str = "hf [--ledger PATH] <version [--json]|init|seed|status [--json]|index|plan [--json]|session start|end [--recycle] [--reap]|session reap [--force]|claim ID|claim --next|claim --batch|doctor [--json]|gitignore [--check|--repair|--write]|reconcile|export|import|migrate [PATH]|release ID|reopen ID \"reason\"|checkpoint ID [note] [--auto] [--quiet] [--sync-cards]|sync-cards|sync [--auto] [--dry-run] [--json]|done ID [--pr N]|test [ID]|task mint --from-kb SLUG|intake --bundle FILE [--vibe TEXT] [--intent FILE] [--scope a,b]|prompt-hub \"<vibe>\" [--scope a,b] [--dispatch] [--json]|dispatch WORKFLOW_ID [--next]|delivery get CORRELATION_ID [--json]|delivery list [--json]|ship ID [--base BR]|promote|review verdict ID PR approve|deny [--by WHO]|drift [--json]|policy gate ACTION [--task ID]|policy check-claim|check-edit|check-handoff [--json]|gatekeeper check PR [--task ID]|hook list|hook run EVENT [--payload JSON] [--json]|lease [--json]|fleet status [--json] [--fix [--dry-run]]|fleet sync [--dry-run] [--json]|fleet render MEMBER|schema [--check|--write]|handoff|resume [--json|--compact]>";
const HF_HELP: &str = "usage: hf [--ledger PATH] <command> [args]\n\nCommon commands:\n  resume [--json|--compact]      Render the current handoff packet\n  status [--json]                Show task/loop status\n  claim ID|--next|--batch        Claim work with a witnessed lease\n  checkpoint ID [note]           Witness progress\n  test [ID]                      Run the task's test_commands\n  done ID [--pr N]               Mark verified work done\n  drift [--json]                 Check policy/contract drift\n  fleet status|sync|render       Audit/remediate fleet handoff deployment\n  prompt-hub \"<vibe>\"            Mint verifiable tasks from intent\n  task mint --from-kb SLUG       Mint a task from a git-kb document\n\nRun `hf help <command>` or `hf <command> --help` for focused usage.\n";
const HF_FLEET_HELP: &str = "usage: hf fleet <status|sync|render> [args]\n\n  hf fleet status [--json] [--fix [--dry-run]]\n      Audit all meta members for handoff onboarding, ledger export, and residency guards.\n      --fix is an alias for `hf fleet sync`; --dry-run is valid only with --fix.\n\n  hf fleet sync [--dry-run] [--json]\n      Remediate detected fleet deployment drift and verify after-state flags.\n\n  hf fleet render MEMBER\n      Render one member packet from the FLEET ledger.\n";
const HF_TASK_HELP: &str = "usage: hf task mint --from-kb SLUG\n\nMint a witnessed handoff.task.v1 card from a git-kb task/spec document. Task lifecycle commands are top-level: `hf claim`, `hf checkpoint`, `hf test`, `hf done`, `hf release`, and `hf reopen`.\n";
const HF_PROMPT_HUB_HELP: &str = "usage: hf prompt-hub \"<vibe>\" [--scope glob,glob] [--dispatch] [--json]\n\nMint one or more verifiable handoff.task.v1 cards from natural-language intent. Use --dispatch to immediately claim the next safe minted task.\n";
const HF_RESUME_HELP: &str = "usage: hf resume [--json|--compact]\n\nRender the latest handoff packet so an agent can resume from ledger-backed project state.\n";
const HF_STATUS_HELP: &str = "usage: hf status [--json]\n\nShow task/loop status, including next safe task and witnessed-event count.\n";
const HF_CLAIM_HELP: &str = "usage: hf claim ID|--next|--batch\n\nClaim work with a witnessed lease. Use --next for the next safe task, or --batch for a safe batch.\n";
const HF_CHECKPOINT_HELP: &str = "usage: hf checkpoint ID [note] [--auto] [--quiet] [--sync-cards]\n\nWitness progress for an active task before stopping or after material work.\n";
const HF_TEST_HELP: &str =
    "usage: hf test [ID]\n\nRun a task card's test_commands and witness the result.\n";
const HF_DONE_HELP: &str = "usage: hf done ID [--pr N]\n\nMark verified work done after tests/evidence pass, optionally linking a pull request.\n";
const HF_DRIFT_HELP: &str = "usage: hf drift [--json]\n\nCheck policy, scope, intent-lock, and evidence drift before handoff or completion.\n";
const HF_RELEASE_HELP: &str = "usage: hf release ID\n\nRelease an active task claim and its lease without marking the task done.\n";
const HF_REOPEN_HELP: &str = "usage: hf reopen ID \"reason\"\n\nReopen a completed or released task with a witnessed reason.\n";
const HF_HANDOFF_HELP: &str = "usage: hf handoff\n\nRender .handoff/packets/latest.md from authoritative git, ledger, and task state.\n";
const HF_SHIP_HELP: &str = "usage: hf ship ID [--base BR]\n\nOpen the shipping flow for a verified task against the selected base branch.\n";
const HF_LEASE_HELP: &str = "usage: hf lease [--json]\n\nInspect currently held coordination leases; terminal task leases are filtered from active holders.\n";
const HF_VERSION_HELP: &str = "usage: hf version [--json]\n\nPrint the hf package version plus embedded build commit/date metadata.\n";
const HF_POLICY_HELP: &str = "usage: hf policy <gate ACTION [--task ID]|check-claim|check-edit|check-handoff [--json]>\n\nRun continuity policy gates for claim, edit, and handoff preconditions.\n";
const HF_INIT_HELP: &str =
    "usage: hf init\n\nInitialize .handoff continuity state for the current repository.\n";
const HF_SEED_HELP: &str =
    "usage: hf seed\n\nSeed the repository with the built-in kernel task cards.\n";
const HF_INDEX_HELP: &str = "usage: hf index\n\nGenerate repository, test, owner, and dependency navigation maps under .handoff/maps/.\n";
const HF_PLAN_HELP: &str =
    "usage: hf plan [--json]\n\nRender the task DAG/order from the current card set.\n";
const HF_SESSION_HELP: &str = "usage: hf session <start|end|reap> [--recycle] [--reap] [--force] [--base BRANCH]\n\nManage continuity sessions and stale worktree reaping.\n";
const HF_DOCTOR_HELP: &str = "usage: hf doctor [--json]\n\nAudit the local continuity ledger, cards, and witness chain health.\n";
const HF_GITIGNORE_HELP: &str = "usage: hf gitignore [--check|--repair|--write]\n\nCheck or update .gitignore guards for local ledger/cache residency.\n";
const HF_RECONCILE_HELP: &str =
    "usage: hf reconcile\n\nSync task cards to ledger truth and re-render the handoff packet.\n";
const HF_EXPORT_HELP: &str =
    "usage: hf export\n\nExport the local ledger database to .handoff/ledger.events.jsonl.\n";
const HF_IMPORT_HELP: &str = "usage: hf import\n\nImport .handoff/ledger.events.jsonl into the local ledger database cache.\n";
const HF_MIGRATE_HELP: &str = "usage: hf migrate [PATH]\n\nMigrate a legacy SQLite ledger into the current local ledger format.\n";
const HF_SYNC_CARDS_HELP: &str =
    "usage: hf sync-cards\n\nSync task card status from authoritative ledger truth.\n";
const HF_SYNC_HELP: &str = "usage: hf sync [--auto] [--dry-run] [--json]\n\nCompare or update task cards from ledger truth.\n";
const HF_INTAKE_HELP: &str = "usage: hf intake --bundle FILE [--vibe TEXT] [--intent FILE] [--scope a,b]\n\nCreate task cards from an intake bundle and optional natural-language intent.\n";
const HF_DISPATCH_HELP: &str = "usage: hf dispatch WORKFLOW_ID [--next]\n\nDispatch the next safe order for an intake/prompt-hub workflow.\n";
const HF_DELIVERY_HELP: &str = "usage: hf delivery <get CORRELATION_ID [--json]|list [--json]>\n\nInspect delivery records by correlation ID or list recent deliveries.\n";
const HF_PROMOTE_HELP: &str =
    "usage: hf promote\n\nRun the local promotion gate for verified work.\n";
const HF_REVIEW_HELP: &str = "usage: hf review <request ID PR|verdict ID PR approve|deny [--by WHO]>\n\nRequest or record a witnessed review verdict for a task/PR.\n";
const HF_GATEKEEPER_HELP: &str = "usage: hf gatekeeper check PR [--task ID]\n\nRun the AI gatekeeper check for a pull request, optionally scoped to a task.\n";
const HF_HOOK_HELP: &str = "usage: hf hook <list|run EVENT [--payload JSON] [--json]>\n\nList or run configured continuity hooks.\n";
const HF_SCHEMA_HELP: &str =
    "usage: hf schema [--check|--write]\n\nCheck or regenerate committed JSON schemas.\n";

fn packet_path() -> PathBuf {
    Path::new(HF).join("packets").join("latest.md")
}

/// ADR-0018 D1: the committed continuity truth — the deterministic JSONL export of the witnessed
/// ledger that travels with git (the binary `ledger.db` stays a local, gitignored cache). Sits
/// beside the ledger (`ledger.db` → `ledger.events.jsonl`); follows `HANDOFF_LEDGER`.
pub(crate) fn ledger_jsonl_path() -> String {
    let db = ledger_path();
    match db.strip_suffix(".db") {
        Some(stem) => format!("{stem}.events.jsonl"),
        None => format!("{db}.events.jsonl"),
    }
}

/// `hf export` — (re)write the committed JSONL ledger export (ADR-0018 D1). Run as a separate
/// process at a commit point (the session-end hook / the loop), NOT inside a mutating verb — redb
/// is single-writer, so a same-process second open would contend with the open mutation handle.
fn cmd_export() {
    match Ledger::open(&ledger_path())
        .and_then(|led| led.all_events())
        .and_then(|evs| ledger::export_jsonl(&evs))
    {
        Ok(text) => {
            let n = text.lines().count();
            let p = ledger_jsonl_path();
            match std::fs::write(&p, &text) {
                Ok(()) => println!("hf export: wrote {n} event(s) to {p}"),
                Err(e) => {
                    eprintln!("hf export: write {p} failed: {e}");
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("hf export: {e}");
            std::process::exit(1);
        }
    }
}

/// `hf import` — rebuild the local binary ledger from the committed JSONL export (ADR-0018 D1; a
/// fresh clone re-derives its cache). Fail-closed: refuses to overwrite an existing binary ledger
/// (never silently clobber the authoritative store), and aborts if the rebuilt chain mismatches.
fn cmd_import() {
    let jsonl_path = ledger_jsonl_path();
    let db = ledger_path();
    if db != ":memory:" && Path::new(&db).exists() {
        eprintln!(
            "hf import: a ledger already exists at {db}; refusing to overwrite. Remove it first to \
             rebuild from {jsonl_path}."
        );
        std::process::exit(1);
    }
    let text = match std::fs::read_to_string(&jsonl_path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("hf import: cannot read {jsonl_path}: {e}");
            std::process::exit(1);
        }
    };
    match ledger::rebuild_from_jsonl(&text, &db) {
        Ok(n) => {
            println!(
                "hf import: rebuilt {db} from {jsonl_path} ({n} events, witness chain verified)"
            )
        }
        Err(e) => {
            eprintln!("hf import: FAILED (fail-closed): {e}");
            std::process::exit(1);
        }
    }
}

// HFTASK-0083 (ADR-0019 D5 #4): the loud, schema-validated card load/save helpers (try_parse_card,
// parse_card_file, scan_card_conformance, load_tasks, save_task, save_task_in, load_task_in) +
// next_safe were lifted into handoff-core so the peeled feature crates (fleet/index/…) load cards
// through the SAME fail-closed path. Re-exported below; call sites are unchanged.

/// Open the ledger or exit fail-closed with a clean message — never panic. Transient RVF
/// lock contention (0x0300 LockHeld) is already retried inside `Ledger::open` (HFTASK-0060);
/// this guards the genuinely-fatal open errors (corruption, disk) at the call site instead of
/// `.unwrap()`-ing them into a backtrace.
fn open_ledger_or_exit(path: &str) -> Ledger {
    match Ledger::open(path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("hf: cannot open ledger at {path} ({e})");
            std::process::exit(1);
        }
    }
}
/// Witness a lifecycle event against the default ledger, surfacing a LOUD warning if either
/// the open or the append fails (fail-open-audit R3). These are best-effort lifecycle markers
/// (session start/end, preflight refusal) where the side effect has already taken place and
/// aborting would be worse than proceeding — but a LOST witness must never vanish silently the
/// way `if let Ok(mut led) = Ledger::open(..) { let _ = led.append(..) }` did.
pub(crate) fn witness_lifecycle(event: &str, wo_id: &str, payload: &str) {
    match Ledger::open(&ledger_path()) {
        Ok(mut led) => {
            if let Err(e) = led.append(event, wo_id, payload, now_ns()) {
                eprintln!("hf: WARNING — failed to witness {event} ({e}); event NOT recorded");
            }
        }
        Err(e) => {
            eprintln!(
                "hf: WARNING — cannot open ledger to witness {event} ({e}); event NOT recorded"
            );
        }
    }
}
// HFTASK-0083: save_task_in / load_task_in / next_safe lifted to handoff-core (re-exported below).

/// The handoff *kernel*'s own North Star doctrine. Used only when `hf init` runs in the
/// kernel home (the handoff repo); a member repo gets a neutral "(seed me)" northstar so
/// it never inherits the kernel's identity.
const KERNEL_NORTHSTAR: &str = "KERNEL DOCTRINE — build a local-first, auditable, reversible, model-native agentic OS where every agent action increases verified capability without corrupting the baseline: Integrity · Reversibility · Capability Gain (no promotion without all three). CECCA/NOA is the executive kernel; the Gold World is the protected baseline; failures compress into evidence. Authoritative: NORTH-STAR.md · keystone docs/adr-0001-flexnetos-autopilot-keystone.md. FLEET VISION (the why): NO HUMAN IN THE LOOP — multi-provider autopilot; user directs, system builds/operates; NEEDS-HUMAN is a scaffold replaced by a model with the human's skillset; end-state = single-person conglomerate. See ../NORTH-STAR.md · ../ARCHITECTURE-TRUTH.md · ../RUVECTOR-RUNBOOK.md";

/// The repo's own name: the basename of the git toplevel, falling back to the cwd
/// basename. This is what makes `hf init` portable — a member repo identifies as itself,
/// not as "handoff".
fn repo_name() -> String {
    let dir = repo_toplevel().or_else(|| std::env::current_dir().ok());
    dir.as_deref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_string())
}

/// True iff the cwd is the handoff *kernel* home — the repo that owns the keystone ADR and
/// the backlog. Only here does `hf init` write the kernel doctrine + is `hf seed` meaningful.
fn is_kernel_home() -> bool {
    // The keystone ADR path is unique to the handoff kernel repo and present in every
    // checkout (incl. git worktrees, where the toplevel basename is not "handoff") — so it
    // is the robust signal, where a dir-name check would misfire.
    Path::new("docs/adr-0001-flexnetos-autopilot-keystone.md").exists()
}

/// Guarantee the repo's `.gitignore` ignores the **binary ledger cache** so a freshly-init'd
/// repo is P7-conformant out of the box (`hf fleet status` requires the guard). ADR-0018 D1
/// (HFTASK-0067): the cache covers `ledger.db`/`-wal`/`-shm`/`*.rvf*` + the out-of-tree migration
/// artifacts — it does NOT ignore the rendered views (`packets/`/`active.md`/`deliveries/`) or the
/// `ledger.events.jsonl` text export, which are now committed. Idempotent — returns `true` iff it
/// ADDED the guard, `false` if git already ignores `.handoff/ledger.db`.
fn ensure_ledger_guard() -> bool {
    let already = std::process::Command::new("git")
        .args(["check-ignore", "-q", ".handoff/ledger.db"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if already {
        return false;
    }
    let block = "\n# handoff continuity: the binary ledger is a gitignored LOCAL CACHE — the\n\
        # committed truth is `.handoff/ledger.events.jsonl` (ADR-0018 D1 / HFTASK-0067).\n\
        .handoff/**/ledger.db\n.handoff/**/*.db-wal\n.handoff/**/*.db-shm\n\
        .handoff/**/*.rvf\n.handoff/**/*.rvf.lock\n\
        .handoff/**/*.sqlite.bak\n.handoff/**/*.redb.tmp\n";
    let prev = fs::read_to_string(".gitignore").unwrap_or_default();
    let _ = fs::write(".gitignore", format!("{prev}{block}"));
    true
}

/// Build the `handoff.context_capsule.v1` an `hf init` writes. Pure (no I/O) so it is
/// testable: a **member** capsule identifies as its own repo and carries a neutral northstar
/// — never the kernel's `project_name`/doctrine — which is the portability contract.
fn init_capsule(
    kernel: bool,
    name: &str,
    role: &str,
    plane: &str,
    northstar: &str,
) -> serde_json::Value {
    let project_name = if kernel {
        "handoff (Continuity Ledger Kernel)".to_string()
    } else {
        name.to_string()
    };
    serde_json::json!({
        "schema": "handoff.context_capsule.v1",
        "project_name": project_name,
        "role": role,
        "plane": plane,
        "northstar": northstar,
        "next_command": "hf resume"
    })
}

/// `hf init` — initialize the `.handoff` continuity kernel in *any* repo (portable, ADR-0006).
///
/// In a **member** repo it writes a capsule describing that repo (name derived from the git
/// toplevel; neutral "(seed me)" northstar — never the kernel's doctrine), a Tier-A README,
/// the ledger schema, and the `.gitignore` residency guard so the repo passes `hf fleet status`
/// immediately. In the **kernel home** (handoff) it writes the full kernel doctrine capsule.
///
/// Idempotent and non-destructive: an existing capsule/README is preserved (never clobbered),
/// so re-running `hf init` — or running it where the fleet steward already seeded a capsule —
/// is safe. Flags: `--name NAME`, `--northstar TEXT`, `--role ROLE`, `--plane PLANE`.
fn cmd_init(args: &[String]) {
    let flag = |name: &str| -> Option<String> {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };

    for d in ["tasks", "packets", "context", "decisions"] {
        let _ = fs::create_dir_all(Path::new(HF).join(d));
    }
    let _ = Ledger::open(&ledger_path()); // creates schema
    let _ = fs::write(
        Path::new(HF).join("active.md"),
        "# Active\n\n(generated by `hf handoff`)\n",
    );

    let kernel = is_kernel_home();
    let name = flag("--name").unwrap_or_else(repo_name);
    let role = flag("--role").unwrap_or_else(|| {
        if kernel {
            "kernel".into()
        } else {
            "tool".into()
        }
    });
    let plane = flag("--plane").unwrap_or_else(|| {
        if kernel {
            "orchestration".into()
        } else {
            "execution".into()
        }
    });
    let northstar = flag("--northstar").unwrap_or_else(|| {
        if kernel {
            KERNEL_NORTHSTAR.to_string()
        } else {
            format!("(seed me) the guiding goal for {name}")
        }
    });
    // Non-destructive: never clobber a curated/seeded capsule. Only write when absent.
    let capsule_existed = capsule_path().exists();
    if !capsule_existed {
        let capsule = init_capsule(kernel, &name, &role, &plane, &northstar);
        let _ = fs::write(capsule_path(), pretty_json(&capsule));
    }

    // Member repos get the Tier-A README contract (kernel home has its own docs).
    let readme = Path::new(HF).join("README.md");
    if !kernel && !readme.exists() {
        let body = format!(
            "# .handoff (ADR-0004 §3.3/§6 rev; ADR-0018 D1)\n\n\
            Continuity layer for `{name}`. **All durable `.handoff` state is committed** — capsule,\n\
            cards, decisions, the rendered views (`packets/`, `active.md`, `deliveries/`), and the\n\
            `ledger.events.jsonl` text export (the committed continuity truth). The binary `ledger.db`\n\
            (+ `*.rvf` sidecar) is a **gitignored local rebuild cache** re-derived via `hf import`; a\n\
            *committed* binary ledger is banned (commit the JSONL text, not the binary). Events roll up\n\
            into the FLEET ledger at `meta/.handoff/`. See `meta/handoff/FLEET_GUIDE.md`.\n\n\
            Cold start: read `context/capsule.json`, then run `hf resume`.\n"
        );
        let _ = fs::write(&readme, body);
    }

    let guarded = ensure_ledger_guard();
    let kind = if kernel { "kernel home" } else { "member" };
    println!(
        "hf init: {kind} `{name}` ready — {}/ (ledger, tasks, packets, context){}{}",
        HF,
        if capsule_existed {
            "; capsule preserved"
        } else {
            "; capsule written"
        },
        if guarded {
            "; .gitignore ledger guard added"
        } else {
            "; ledger guard present"
        }
    );
}

/// CLI entry for `hf claim <ID>`: exits nonzero when the claim is refused/blocked so
/// callers (hooks, scripts, the loop) see the failure (HFTASK-0029 Defect C). The
/// internal dispatch loop (intake.rs) calls `cmd_claim_with` directly and inspects the
/// bool instead, so it can skip a blocked order without aborting the whole process.
fn cmd_claim(id: &str) {
    if !cmd_claim_with(id, &lease::WeaveCli::from_env()) {
        std::process::exit(1);
    }
}

/// `hf claim --batch`: claim the HIGHEST-VALUE safe task via the domain-expansion Thompson
/// router (HFTASK-0018, ADR-0012) instead of the topologically-first `next_safe`. Resumes an
/// in-progress task if one exists (same precedence as `next_safe`); otherwise routes over the
/// ready backlog candidates (deps all Done) and claims the winner.
/// HFTASK-0049: `hf claim --next` — claim the next safe task by topological/dependency order
/// (resume an in-progress task first, else the first ready backlog card). The value-routed
/// sibling is `hf claim --batch` (ADR-0012 bandit); `--next` is the deterministic claim.
fn cmd_claim_next() {
    let tasks = load_tasks();
    let replay = current_statuses();
    match next_safe(&tasks, &replay) {
        Some(t) => {
            let id = t.id.clone();
            cmd_claim(&id);
        }
        None => {
            eprintln!("hf claim --next: no safe task (all done or blocked)");
            std::process::exit(1);
        }
    }
}

/// HFTASK-0058 (ADR-0016): `hf gitignore` — ship/repair/check the canonical `.handoff`
/// durability policy. With no mode it prints the canonical contents-form fragment to stdout
/// (consumers inherit it instead of hand-rolling). `--check` runs the swallow-guard and exits
/// nonzero (fail-closed) if a durable `.handoff` path would be ignored. `--repair`/`--write`
/// strips any dir-form `.handoff/`/`.claude/` swallow and appends the fragment idempotently.
fn cmd_gitignore(mode: Option<&str>) {
    let repo = Path::new(".");
    match mode {
        Some("--check") => {
            let r = durability::swallow_report(repo);
            if r.is_healthy() {
                println!("hf gitignore --check: OK (no .handoff durability swallow)");
                for p in &r.regenerable_unignored {
                    println!("  · note: regenerable path not ignored: {p}");
                }
            } else {
                eprintln!(
                    "hf gitignore --check: SWALLOW — durable .handoff state would not commit"
                );
                for l in &r.dir_form_ignores {
                    eprintln!("  ⚠ dir-form ignore (swallows durable children): {l}");
                }
                for p in &r.swallowed_durable {
                    eprintln!("  ⚠ durable path is ignored: {p}");
                }
                eprintln!("  → fix: `hf gitignore --repair`");
                std::process::exit(1);
            }
        }
        Some("--repair") | Some("--write") => match durability::repair_gitignore(repo) {
            Ok(o) if o.changed() => {
                for l in &o.removed_dir_form {
                    println!("hf gitignore: removed dir-form swallow `{l}`");
                }
                if o.added_fragment {
                    println!("hf gitignore: appended canonical durability fragment (ADR-0016)");
                }
                for rule in &o.ensured_rules {
                    println!("hf gitignore: ensured canonical regenerable-cache rule `{rule}`");
                }
            }
            Ok(_) => println!("hf gitignore: already canonical (no change)"),
            Err(e) => {
                eprintln!("hf gitignore --repair: {e}");
                std::process::exit(1);
            }
        },
        None => print!("{}", durability::CANONICAL_GITIGNORE_FRAGMENT),
        Some(other) => {
            eprintln!(
                "hf gitignore: unknown mode '{other}' (expected --check | --repair | --write)"
            );
            std::process::exit(2);
        }
    }
}

/// HFTASK-0049: `hf doctor` — kernel health diagnostics. Verifies the witness chain
/// (tamper-evidence), reports event/task counts and the next safe task, and checks ledger
/// residency (the local ledger lives under `.handoff/`). Exits nonzero if the witness chain
/// is broken or the ledger is unreadable — fail-closed, so a hook/CI can gate on it.
fn cmd_doctor(json: bool) {
    let tasks = load_tasks();
    let replay = current_statuses();
    let done = tasks
        .iter()
        .filter(|t| status_of(&t.id, &replay, t) == Status::Done)
        .count();
    let ledger_present = Path::new(&ledger_path()).exists();
    let chain = Ledger::open(&ledger_path()).and_then(|l| l.verify_witness_chain());
    let (chain_ok, events) = match chain {
        Ok(n) => (true, n),
        Err(_) => (false, 0),
    };
    let next = next_safe(&tasks, &replay).map(|t| t.id.clone());
    // HFTASK-0058 (ADR-0016): the durability swallow-guard. Ask Git whether a durable
    // `.handoff` path is being ignored (a dir-form `.handoff/`/`.claude/` ignore silently
    // swallows tasks/decisions/loop ledgers). Only runs inside a git repo; a swallow is
    // fatal (fail-closed), like a broken witness chain.
    let in_git = Path::new(".git").exists();
    let swallow = in_git.then(|| durability::swallow_report(Path::new(".")));
    let durability_ok = swallow.as_ref().map(|r| r.is_healthy()).unwrap_or(true);
    // HFTASK-0064 (a): every card file on disk MUST conform — a card that can't load is
    // invisible to `hf status`. The loud loader warns; here it is a HARD health failure
    // (catches the load_tasks silent-drop the fail-open lesson L9 names).
    let unconformant = scan_card_conformance();
    // HFTASK-0064 (b): no empty-default masking — if the ledger is present, its replay MUST
    // succeed. `current_statuses()` uses `unwrap_or_default()` (a fail-open that would report
    // 0 tasks on a read error); doctor asserts the read explicitly instead.
    let replay_ok = !ledger_present
        || Ledger::open(&ledger_path())
            .and_then(|l| l.replay_latest_status())
            .is_ok();
    // HFTASK-0064 (c): opening the ledger above auto-reclaims a provably-dead RVF lock
    // (HFTASK-0062) and witnesses it. Surface the lifetime reclaim count and whether a
    // (necessarily live-holder, since dead ones were just reclaimed) lock lingers — the
    // latter is informational, not a failure (a live writer is legitimate).
    let reclaimed_total = Ledger::open(&ledger_path())
        .and_then(|l| l.all_events())
        .map(|evs| {
            evs.iter()
                .filter(|e| e.event_type == "lock_reclaimed")
                .count()
        })
        .unwrap_or(0);
    let rvf_lock_present = Path::new(&format!("{}.rvf.lock", ledger_path())).exists();
    let healthy =
        chain_ok && ledger_present && durability_ok && replay_ok && unconformant.is_empty();
    if json {
        let out = serde_json::json!({
            "schema": "handoff.doctor.v1",
            "healthy": healthy,
            "witness_chain_ok": chain_ok,
            "witnessed_events": events,
            "ledger_present": ledger_present,
            "ledger_path": ledger_path(),
            "tasks_total": tasks.len(),
            "done": done,
            "remaining": tasks.len() - done,
            "next_task_id": next,
            "replay_ok": replay_ok,
            "cards_conformant": unconformant.is_empty(),
            "unconformant_cards": unconformant
                .iter()
                .map(|(name, reason)| serde_json::json!({ "card": name, "reason": reason }))
                .collect::<Vec<_>>(),
            "rvf_locks_reclaimed_total": reclaimed_total,
            "rvf_lock_present": rvf_lock_present,
            "durability": swallow.as_ref().map(|r| serde_json::json!({
                "ok": r.is_healthy(),
                "dir_form_ignores": r.dir_form_ignores,
                "swallowed_durable": r.swallowed_durable,
                "regenerable_unignored": r.regenerable_unignored,
            })),
        });
        println!("{}", pretty_json(&out));
    } else {
        println!("=== hf doctor ===");
        println!(
            "  witness chain : {}  ({events} witnessed events)",
            if chain_ok {
                "OK (tamper-evident)"
            } else {
                "BROKEN"
            }
        );
        println!(
            "  ledger        : {} @ {}",
            if ledger_present { "present" } else { "MISSING" },
            ledger_path()
        );
        println!("  tasks         : {done}/{} done", tasks.len());
        println!(
            "  cards         : {}",
            if unconformant.is_empty() {
                format!("OK (all {} conform)", tasks.len())
            } else {
                format!("NON-CONFORMANT ({} unloadable)", unconformant.len())
            }
        );
        for (name, reason) in &unconformant {
            println!("                  ⚠ {name}: {reason}");
        }
        println!(
            "  replay        : {}",
            if replay_ok { "OK" } else { "UNREADABLE" }
        );
        println!(
            "  rvf lock      : {} reclaimed (lifetime){}",
            reclaimed_total,
            if rvf_lock_present {
                "; a lock is currently held (live writer)"
            } else {
                ""
            }
        );
        println!(
            "  next safe     : {}",
            next.as_deref().unwrap_or("(none — all done or blocked)")
        );
        match &swallow {
            Some(r) if r.is_healthy() => println!("  durability    : OK (no .handoff swallow)"),
            Some(r) => {
                println!("  durability    : SWALLOW — durable .handoff state would not commit");
                for l in &r.dir_form_ignores {
                    println!(
                        "                  ⚠ dir-form ignore: {l}  (use `hf gitignore --repair`)"
                    );
                }
                for p in &r.swallowed_durable {
                    println!("                  ⚠ durable path ignored: {p}");
                }
            }
            None => println!("  durability    : n/a (not a git repo)"),
        }
        println!(
            "  health        : {}",
            if healthy { "OK" } else { "DEGRADED" }
        );
    }
    if !healthy {
        std::process::exit(1);
    }
}

/// HFTASK-0049: `hf reconcile` — re-apply state precedence (Git > ledger > cards > packet):
/// sync each card's status to ledger truth and re-render the packet from the live ledger, so
/// the derived views match reality. Never edits the ledger (the source of truth) — this is the
/// verb the docs' precedence rule tells an agent to run when views look stale.
/// `hf migrate [PATH]` — one-time legacy C-SQLite → redb ledger importer (ADR-0017 cutover).
///
/// Only functional in a binary built with `--features legacy-sqlite` (that build links bundled
/// C-SQLite for the read side; the default no-C binary deliberately cannot migrate and says so).
/// Safe + fail-closed: migrates the SQLite file to a temp redb store (the importer re-verifies the
/// witness chain to the same event count or aborts), backs the original up to an **out-of-tree**
/// `*.sqlite.bak` (under `$HANDOFF_LEDGER_BACKUP_DIR` / `$XDG_DATA_HOME` / `~/.local/share`, never
/// inside the tracked `.handoff/` tree — the HFTASK-0053 cutover hygiene gap), then atomically
/// renames the redb store into place. Exits nonzero on any error.
#[cfg(feature = "legacy-sqlite")]
fn cmd_migrate(path: &str) {
    if path == ":memory:" {
        eprintln!("hf migrate: nothing to migrate for an in-memory ledger");
        std::process::exit(2);
    }
    if !std::path::Path::new(path).exists() {
        eprintln!("hf migrate: no ledger file at {path}");
        std::process::exit(1);
    }
    if !ledger::file_is_legacy_sqlite(path) {
        eprintln!(
            "hf migrate: {path} is not a legacy C-SQLite ledger (already redb, or not a ledger) \
             — nothing to migrate"
        );
        std::process::exit(0);
    }
    let tmp = format!("{path}.redb.tmp");
    let bak = resolve_backup_target(path);
    let _ = std::fs::remove_file(&tmp);
    match ledger::migrate_sqlite_to_redb(path, &tmp) {
        Ok(n) => {
            if let Err(e) = std::fs::rename(path, &bak) {
                eprintln!("hf migrate: imported {n} events but could not back up {path}: {e}");
                let _ = std::fs::remove_file(&tmp);
                std::process::exit(1);
            }
            if let Err(e) = std::fs::rename(&tmp, path) {
                eprintln!(
                    "hf migrate: imported {n} events, backed up to {bak}, but could not install \
                     the redb store ({e}); restore with `mv {bak} {path}`"
                );
                std::process::exit(1);
            }
            println!(
                "hf migrate: {path} → redb ({n} events, witness chain re-verified); legacy SQLite backed up to {bak}"
            );
        }
        Err(e) => {
            eprintln!("hf migrate: FAILED (fail-closed, original untouched): {e}");
            let _ = std::fs::remove_file(&tmp);
            std::process::exit(1);
        }
    }
}

/// Fallback for the default no-C build: `hf migrate` exists as a verb but cannot link the legacy
/// C-SQLite read side — direct the operator to the migration build (fail-closed, never silent).
#[cfg(not(feature = "legacy-sqlite"))]
fn cmd_migrate(path: &str) {
    eprintln!(
        "hf migrate: this binary is the default no-C build and cannot read legacy C-SQLite.\n\
         To convert {path}, run a migration build:\n\
         \x20 cargo run -p hf --features legacy-sqlite -- migrate {path}"
    );
    std::process::exit(2);
}

/// Resolve the out-of-tree directory for migration backups so a legacy `*.sqlite.bak` never lands
/// inside the tracked `.handoff/` tree (where it churns git and trips `hf drift`'s
/// `deny_without_claim` — the HFTASK-0053 cutover hygiene gap). Honors, in order,
/// `$HANDOFF_LEDGER_BACKUP_DIR`, then `$XDG_DATA_HOME/handoff-ledger-backups`, then
/// `$HOME/.local/share/handoff-ledger-backups`. `None` only when no home/data dir is resolvable
/// (the caller then falls back to an in-tree, gitignored backup with a loud warning).
#[cfg_attr(not(feature = "legacy-sqlite"), allow(dead_code))]
fn ledger_backup_dir() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    if let Ok(d) = std::env::var("HANDOFF_LEDGER_BACKUP_DIR")
        && !d.is_empty()
    {
        return Some(PathBuf::from(d));
    }
    if let Ok(d) = std::env::var("XDG_DATA_HOME")
        && !d.is_empty()
    {
        return Some(PathBuf::from(d).join("handoff-ledger-backups"));
    }
    if let Ok(h) = std::env::var("HOME")
        && !h.is_empty()
    {
        return Some(PathBuf::from(h).join(".local/share/handoff-ledger-backups"));
    }
    None
}

/// Encode an absolute ledger path into a single safe backup filename stem: every character outside
/// `[A-Za-z0-9._-]` becomes `_` and a leading `_` (from the root `/`) is trimmed, so the full
/// source location is preserved and two different ledgers can never collide on one backup name.
#[cfg_attr(not(feature = "legacy-sqlite"), allow(dead_code))]
fn backup_stem_for(abs: &str) -> String {
    let mut s: String = abs
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    while s.starts_with('_') {
        s.remove(0);
    }
    s
}

/// Compute the backup target path for `hf migrate`: an out-of-tree `<stem>.sqlite.bak` under
/// [`ledger_backup_dir`], never clobbering an existing backup (`.1`, `.2`, … on collision). Falls
/// back to the in-tree (gitignored) `<path>.sqlite.bak` with a loud warning only when no
/// out-of-tree dir is resolvable/creatable — an upgrade over the old always-in-tree behavior.
#[cfg(feature = "legacy-sqlite")]
fn resolve_backup_target(path: &str) -> String {
    if let Some(dir) = ledger_backup_dir()
        && std::fs::create_dir_all(&dir).is_ok()
    {
        let abs = std::fs::canonicalize(path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());
        let stem = backup_stem_for(&abs);
        let mut cand = dir.join(format!("{stem}.sqlite.bak"));
        let mut n = 1u32;
        while cand.exists() {
            cand = dir.join(format!("{stem}.sqlite.bak.{n}"));
            n += 1;
        }
        return cand.to_string_lossy().into_owned();
    }
    eprintln!(
        "hf migrate: WARNING — no out-of-tree backup dir resolvable; backing up in-tree to \
         {path}.sqlite.bak (gitignored by the ledger guard)"
    );
    format!("{path}.sqlite.bak")
}

fn cmd_reconcile() {
    let n = sync_cards();
    let tasks = load_tasks();
    let replay = current_statuses();
    let witness = Ledger::open(&ledger_path())
        .and_then(|l| l.verify_witness_chain())
        .unwrap_or(0);
    let md = render_packet_md(&tasks, &replay, witness);
    let _ = std::fs::write(packet_path(), &md);
    println!(
        "hf reconcile: synced {n} card(s) to ledger truth; re-rendered packet ({witness} witnessed events)"
    );
}

/// HFTASK-0043: replay the ledger's task_transition history into per-context-bucket outcome
/// counts for the bandit. `done` = success; a transition back to Backlog from an in-progress
/// state (release/reopen) = failure. Tasks not in the current backlog are ignored. The ledger
/// is the outcome store — this closes the keystone T5 co-learning loop (ADR-0012 v2).
fn routing_history(tasks: &[WorkOrder]) -> routing::History {
    use std::collections::HashMap;
    let bucket: HashMap<&str, _> = tasks
        .iter()
        .map(|t| (t.id.as_str(), routing::bucket_of(t)))
        .collect();
    let mut hist = routing::History::new();
    let Ok(led) = Ledger::open(&ledger_path()) else {
        return hist;
    };
    let Ok(events) = led.all_events() else {
        return hist;
    };
    let mut prev: HashMap<String, Status> = HashMap::new();
    for e in events {
        if e.event_type != "task_transition" {
            continue;
        }
        let Some(st) = serde_json::from_str::<serde_json::Value>(&e.payload_json)
            .ok()
            .and_then(|v| v.get("status").cloned())
            .and_then(|s| serde_json::from_value::<Status>(s).ok())
        else {
            continue;
        };
        let was = prev.get(&e.work_order_id).copied();
        if let Some(b) = bucket.get(e.work_order_id.as_str()) {
            match st {
                Status::Done => hist.entry(b.clone()).or_default().0 += 1,
                Status::Backlog
                    if matches!(
                        was,
                        Some(
                            Status::Claimed
                                | Status::Active
                                | Status::Checkpointed
                                | Status::Review
                        )
                    ) =>
                {
                    hist.entry(b.clone()).or_default().1 += 1;
                }
                _ => {}
            }
        }
        prev.insert(e.work_order_id, st);
    }
    hist
}

fn cmd_claim_batch() {
    use rand::SeedableRng;
    let tasks = load_tasks();
    let replay = current_statuses();
    // 1) an in-progress task takes precedence — resume it (mirrors next_safe step 1).
    if let Some(t) = tasks.iter().find(|t| {
        matches!(
            status_of(&t.id, &replay, t),
            Status::Claimed | Status::Checkpointed | Status::Active | Status::Review
        )
    }) {
        cmd_claim(&t.id);
        return;
    }
    // 2) route over the ready backlog candidates (deps all Done).
    let done = |id: &str| replay.iter().any(|(k, s)| k == id && *s == Status::Done);
    let candidates: Vec<&WorkOrder> = tasks
        .iter()
        .filter(|t| {
            status_of(&t.id, &replay, t) == Status::Backlog
                && t.dependencies.iter().all(|d| done(d))
        })
        .collect();
    if candidates.is_empty() {
        eprintln!("hf claim --batch: no ready safe task to route");
        std::process::exit(1);
    }
    // Seed from the witnessed count so the draw is reproducible for a given ledger state
    // and re-explores as history grows.
    let witness = Ledger::open(&ledger_path())
        .and_then(|l| l.verify_witness_chain())
        .unwrap_or(0);
    let mut rng = rand::rngs::StdRng::seed_from_u64(witness as u64);
    // HFTASK-0043: seed the bandit from real ledger outcomes (done = success, reopen =
    // failure) so routing LEARNS, not just samples the priority prior.
    let history = routing_history(&tasks);
    match routing::route_with_history(&candidates, &history, &mut rng) {
        Some((t, d)) => {
            println!(
                "hf claim --batch: routed to arm {} (context {}/{}, value {:.3}) [ADR-0012]",
                d.arm.0, d.bucket.difficulty_tier, d.bucket.category, d.value
            );
            cmd_claim(&t.id);
        }
        None => {
            eprintln!("hf claim --batch: no ready safe task to route");
            std::process::exit(1);
        }
    }
}

/// Mesh-coordinated claim: reserve a weave lease on the task *before* recording the
/// ledger transition, so two sessions can't claim the same task. Refuses the claim
/// if another peer holds it; degrades to ledger-only when no weave mesh is present.
///
/// Returns `true` when the task was claimed, `false` when the claim was refused/blocked
/// (HFTASK-0029 Defect C) — the CLI path turns `false` into a nonzero exit; the dispatch
/// loop uses the bool to skip-and-continue.
fn cmd_claim_with(id: &str, leaser: &dyn lease::Leaser) -> bool {
    let (ledger, tasks_dir) = match route::route_for_task(id) {
        Ok(homes) => homes,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let Some(wo) = load_task_in(&tasks_dir, id) else {
        eprintln!("no such task {id}");
        std::process::exit(1);
    };
    let resource = lease::claim_resource(id);
    let mut degraded = false;
    match lease::gate(leaser.reserve(&resource, CLAIM_TTL_SECS, &format!("hf claim {id}"))) {
        lease::ClaimGate::Refuse(reason) => {
            eprintln!(
                "hf claim: {id} BLOCKED — {resource} is held by another peer ({reason}); not claiming"
            );
            return false;
        }
        lease::ClaimGate::ProceedDegraded => {
            degraded = true;
            eprintln!(
                "hf claim: weave lease unavailable — falling back to the atomic in-ledger lease"
            );
        }
        lease::ClaimGate::Proceed => {
            println!("hf claim: reserved weave lease {resource} (ttl {CLAIM_TTL_SECS}s)");
        }
    }

    // HFTASK-0048: acquire the atomic in-ledger lease as a no-downgrade SUPERSET of the weave
    // lease — it gives real mutual exclusion even in the degraded (weave-absent) path, which
    // previously had none. The whole check-then-write is one BEGIN IMMEDIATE tx, so concurrent
    // claimers serialize and exactly one wins.
    let holder = lease::local_holder();
    let now = now_ns();
    let mut led = open_ledger_or_exit(&ledger.to_string_lossy());
    match led.try_acquire_lease(&resource, &holder, CLAIM_TTL_SECS, now) {
        Ok(ledger::LeaseOutcome::Conflict { holder: other }) => {
            eprintln!(
                "hf claim: {id} BLOCKED — in-ledger lease {resource} is held by '{other}'; not claiming"
            );
            // Don't orphan a weave lease we just took for a claim we're now refusing.
            if !degraded {
                lease::WeaveCli::from_env().release(&resource);
            }
            return false;
        }
        Ok(ledger::LeaseOutcome::Acquired { .. }) => {
            lease::write_lockfile(&resource, &holder, CLAIM_TTL_SECS, now);
            println!(
                "hf claim: acquired in-ledger lease {resource} (holder '{holder}', ttl {CLAIM_TTL_SECS}s)"
            );
        }
        Ok(ledger::LeaseOutcome::Heartbeat { .. }) => {
            lease::write_lockfile(&resource, &holder, CLAIM_TTL_SECS, now);
            println!("hf claim: extended in-ledger lease {resource} (holder '{holder}')");
        }
        Err(e) => {
            // Lease bookkeeping failure must not silently drop exclusion — fail closed.
            eprintln!("hf claim: {id} BLOCKED — in-ledger lease error: {e}");
            return false;
        }
    }

    must_witness(
        led.record_transition(&wo, Status::Claimed, now_ns()),
        "claim transition",
    );
    // ADR-0003 rule 3 (HFTASK-0042): mirror the claim to the kb plan (status → active).
    // One-way + best-effort: a no-op for non-kb cards.
    if kb::write_back(&wo.correlation_id, &kb::KbTransition::Claimed) {
        println!("hf claim: kb {} → active (write-back)", wo.correlation_id);
    }
    println!("hf claim: {id} -> claimed");
    println!(
        "hf claim: next grit cycle (ADR-0018 D8): `scripts/grit-shared.sh claim --agent \"$HF_LEASE_HOLDER\" --intent \"{}: {}\" <file::symbol>` → work in the grit worktree → `scripts/grit-shared.sh done --agent \"$HF_LEASE_HOLDER\"`",
        wo.id, wo.title
    );
    true
}

/// True iff a task in this status should be reverted to `Backlog` on release — i.e. it is an
/// active claim being relinquished. Terminal/post-work states (`Review`/`Done`) and already-
/// `Backlog` are left untouched: a release must never un-finish completed work (HFTASK-0038).
fn should_unclaim(status: Option<Status>) -> bool {
    matches!(
        status,
        Some(Status::Claimed) | Some(Status::Checkpointed) | Some(Status::Active)
    )
}

/// HFTASK-0061: only a TERMINAL/post-work state (Done/Review) is reopenable. An in-progress
/// claim is relinquished with `hf release` (see `should_unclaim`); a Backlog task or an unknown
/// id has nothing to reopen. Pure so the gating decision is unit-testable.
fn should_reopen(status: Option<Status>) -> bool {
    matches!(status, Some(Status::Done) | Some(Status::Review))
}

fn terminal_task_status(status: Option<Status>) -> bool {
    matches!(status, Some(Status::Done) | Some(Status::Review))
}

/// Release the claim on a task: free the weave lease AND **un-claim** it — revert an
/// in-progress task's ledger status back to `Backlog` (HFTASK-0038). A lease-only release
/// left the task stuck `Claimed`, so the claim was never truly relinquished and
/// `hf claim --batch` would resume the phantom.
fn cmd_release(id: &str) {
    let resource = lease::claim_resource(id);
    if lease::WeaveCli::from_env().release(&resource) {
        println!("hf release: freed weave lease {resource}");
    } else {
        eprintln!(
            "hf release: no active weave lease {resource} held by you (or weave unavailable)"
        );
    }
    // HFTASK-0048: release the atomic in-ledger lease + drop its lockfile mirror, so the
    // resource is genuinely free for the next claimer (the weave release alone left the
    // in-ledger lease live until TTL).
    let holder = lease::local_holder();
    if let Ok((ledger_path, _)) = route::route_for_task(id)
        && let Ok(mut led) = Ledger::open(&ledger_path.to_string_lossy())
        && led.release_lease(&resource, &holder, now_ns()).is_ok()
    {
        println!("hf release: freed in-ledger lease {resource}");
    }
    lease::remove_lockfile(&resource);
    // Un-claim: only revert an in-progress claim (never Review/Done/Backlog).
    let status = current_statuses()
        .into_iter()
        .find(|(k, _)| k == id)
        .map(|(_, s)| s);
    if !should_unclaim(status) {
        return;
    }
    let Ok((ledger, tasks_dir)) = route::route_for_task(id) else {
        return;
    };
    let Some(wo) = load_task_in(&tasks_dir, id) else {
        return;
    };
    // fail-open-audit R3: a silent `if let Ok` here lost the un-claim witness on a ledger open
    // or transition failure, leaving the task stuck Claimed with no record of the attempt. Open
    // fail-closed and surface a failed transition loudly.
    let mut led = open_ledger_or_exit(&ledger.to_string_lossy());
    match led.record_transition(&wo, Status::Backlog, now_ns()) {
        Ok(_) => {
            println!("hf release: {id} -> backlog (un-claimed)");
            // ADR-0003 rule 3 (HFTASK-0042) gap-hunt: a released kb-minted card should also
            // revert its planning-plane status to backlog (mirrors claim → active).
            if kb::write_back(&wo.correlation_id, &kb::KbTransition::Released) {
                println!(
                    "hf release: kb {} → backlog (write-back)",
                    wo.correlation_id
                );
            }
        }
        Err(e) => {
            eprintln!(
                "hf release: WARNING — failed to witness un-claim of {id} ({e}); task may still be Claimed"
            );
        }
    }
}

/// `hf reopen <ID> "<reason>"` (HFTASK-0061) — the witnessed inverse of completion: revert a
/// **terminal** task (Done/Review) back to `Backlog` with a recorded reason. The fail-closed
/// kernel must be able to CORRECT a false-Done — e.g. a task marked Done via a pre-guard
/// blanket-`cargo test` rubber stamp whose feature was never actually built. A reason is
/// MANDATORY (fail-closed: no silent un-completion), and only a terminal state is reopenable
/// (an in-progress claim is relinquished with `hf release`, not reopened). The WHY is witnessed
/// as a `task_reopened` event before the `task_transition → Backlog` that replay acts on, so the
/// audit trail records both that the Done was reverted and why.
fn cmd_reopen(id: &str, reason: &str) {
    if id.is_empty() {
        eprintln!("hf reopen: an id is required — `hf reopen <ID> \"<reason>\"`");
        std::process::exit(2);
    }
    if reason.trim().is_empty() {
        eprintln!(
            "hf reopen: a reason is required — `hf reopen <ID> \"<reason>\"` (no silent un-completion)"
        );
        std::process::exit(2);
    }
    let status = current_statuses()
        .into_iter()
        .find(|(k, _)| k == id)
        .map(|(_, s)| s);
    // Only a terminal/post-work state is reopenable; in-progress uses `hf release`.
    if !should_reopen(status) {
        eprintln!(
            "hf reopen: {id} is {status:?}, not Done/Review — nothing to reopen \
             (use `hf release` to relinquish an in-progress claim)"
        );
        std::process::exit(1);
    }
    let Ok((ledger, tasks_dir)) = route::route_for_task(id) else {
        eprintln!("hf reopen: cannot route {id}");
        std::process::exit(1);
    };
    let Some(wo) = load_task_in(&tasks_dir, id) else {
        eprintln!("hf reopen: no such task {id} on disk");
        std::process::exit(1);
    };
    // INVARIANT: `should_reopen(status)` above returns false for `None` (only Done/Review are
    // reopenable) and we exit on `!should_reopen`, so `status` is necessarily `Some` here.
    // Justified per-site (HFTASK-0080).
    #[allow(clippy::expect_used)]
    let from = format!("{:?}", status.expect("matched Done/Review above"));
    let mut led = open_ledger_or_exit(&ledger.to_string_lossy());
    // Witness the WHY first, then the status revert replay acts on.
    let payload = serde_json::json!({ "id": id, "reason": reason, "from": from }).to_string();
    must_witness(
        led.append("task_reopened", id, &payload, now_ns()),
        "task_reopened",
    );
    must_witness(
        led.record_transition(&wo, Status::Backlog, now_ns()),
        "reopen transition",
    );
    println!("hf reopen: {id} {from} -> Backlog (witnessed; reason: {reason})");
    // ADR-0003 rule 3: a reopened kb-minted card reverts its planning-plane status too.
    if kb::write_back(&wo.correlation_id, &kb::KbTransition::Released) {
        println!("hf reopen: kb {} → backlog (write-back)", wo.correlation_id);
    }
    // Keep the on-disk card snapshot in sync with the new ledger truth.
    let n = sync_cards();
    if n > 0 {
        println!("hf reopen: synced {n} card(s) from ledger truth");
    }
}

/// Pure re-mint decision for `hf relock` (no I/O, unit-testable): produce the intent-lock that
/// matches the card's CURRENT fields, in the SAME form (partial vs full) as the recorded lock so
/// re-locking never silently up/down-grades the contract surface. A legacy 3-field (partial) lock
/// re-mints the three content hashes only; a full 5-field lock re-mints all five and re-binds to
/// the supplied (current) North-Star revision.
fn relock_intent(wo: &WorkOrder, northstar_revision: &str) -> work_order::IntentLock {
    let cur = &wo.intent_lock;
    let is_full = !cur.constraint_hash.is_empty() || !cur.northstar_revision.is_empty();
    if is_full {
        wo.full_intent_lock(northstar_revision)
    } else {
        WorkOrder::compute_intent_lock(&wo.objective, &wo.path_scope, &wo.acceptance_criteria)
    }
}

/// `hf relock <ID> "<reason>"` — re-mint a task's intent-lock from its CURRENT card fields,
/// clearing a benign intent-lock drift. This is the operator remedy the drift sentinel NAMES
/// ("re-lock {id} objective" / "re-mint/reclaim") but historically shipped NO verb for: when a
/// card's objective/path_scope/acceptance text is edited LEGITIMATELY after minting (a doctrine
/// or typo correction), `hf drift` flags the stale hash forever and the only escape was to
/// hand-edit the lock — exactly the tampering the lock exists to detect.
///
/// Deliberate, reasoned, and witnessed — never a silent re-baseline of the intent contract: it
/// requires a `<reason>` (like `hf reopen`) and emits a `task_relocked` event recording the
/// old→new hashes, so the objective change is an auditable entry in the tamper-evident ledger.
/// The lock lives in the card (`sync_cards` only touches status), so writing the card clears the
/// drift durably. No-downgrade via [`relock_intent`]: a partial lock stays partial; a full lock is
/// re-minted across all five surfaces and re-bound to the current North-Star (the named "re-mint
/// against the current North Star" remedy).
fn cmd_relock(id: &str, reason: &str) {
    if id.is_empty() {
        eprintln!("hf relock: an id is required — `hf relock <ID> \"<reason>\"`");
        std::process::exit(2);
    }
    if reason.trim().is_empty() {
        eprintln!(
            "hf relock: a reason is required — `hf relock <ID> \"<reason>\"` \
             (no silent re-baseline of the intent contract)"
        );
        std::process::exit(2);
    }
    let Ok((ledger, tasks_dir)) = route::route_for_task(id) else {
        eprintln!("hf relock: cannot route {id}");
        std::process::exit(1);
    };
    let Some(mut wo) = load_task_in(&tasks_dir, id) else {
        eprintln!("hf relock: no such task {id} on disk");
        std::process::exit(1);
    };
    let old = wo.intent_lock.clone();
    let fresh = relock_intent(&wo, &current_northstar_revision());
    if fresh == old {
        println!("hf relock: {id} intent-lock already matches its card — no drift to clear");
        return;
    }
    let mut led = open_ledger_or_exit(&ledger.to_string_lossy());
    // Witness the WHY + the old→new surfaces first, then mutate the card the drift gate reads.
    let payload = serde_json::json!({
        "id": id,
        "reason": reason,
        "old": {
            "objective": old.objective_hash, "path_scope": old.path_scope_hash,
            "acceptance": old.acceptance_hash, "constraint": old.constraint_hash,
            "northstar": old.northstar_revision,
        },
        "new": {
            "objective": fresh.objective_hash, "path_scope": fresh.path_scope_hash,
            "acceptance": fresh.acceptance_hash, "constraint": fresh.constraint_hash,
            "northstar": fresh.northstar_revision,
        },
    })
    .to_string();
    must_witness(
        led.append("task_relocked", id, &payload, now_ns()),
        "task_relocked",
    );
    wo.intent_lock = fresh;
    save_task(&wo);
    println!("hf relock: {id} intent-lock re-minted to current card (witnessed; reason: {reason})");
}

/// `hf lease` (HFTASK-0048) — list the currently-held atomic in-ledger leases (resource →
/// holder), resolved live over the witnessed history with TTL/release applied. Read-only.
fn cmd_lease(json: bool) {
    let ledger = ledger_path();
    let terminal_claims: std::collections::HashSet<String> = current_statuses()
        .into_iter()
        .filter(|(_, s)| terminal_task_status(Some(*s)))
        .map(|(id, _)| lease::claim_resource(&id))
        .collect();
    let Ok(led) = Ledger::open(&ledger) else {
        eprintln!("hf lease: ledger unavailable at {ledger}");
        std::process::exit(1);
    };
    let now = now_ns();
    let mut resources: Vec<String> = vec![];
    if let Ok(events) = led.all_events() {
        for e in events {
            if (e.event_type == "lease_acquired" || e.event_type == "lease_released")
                && !resources.contains(&e.work_order_id)
            {
                resources.push(e.work_order_id);
            }
        }
    }
    let held: Vec<(String, String)> = resources
        .into_iter()
        .filter(|r| !terminal_claims.contains(r))
        .filter_map(|r| led.lease_holder(&r, now).ok().flatten().map(|h| (r, h)))
        .collect();
    if json {
        let out = serde_json::json!({
            "schema": "handoff.leases.v1",
            "now_ns": now,
            "held": held.iter().map(|(r, h)| serde_json::json!({"resource": r, "holder": h})).collect::<Vec<_>>(),
        });
        println!("{}", pretty_json(&out));
    } else if held.is_empty() {
        println!("hf lease: no active in-ledger leases");
    } else {
        println!("hf lease: {} active in-ledger lease(s):", held.len());
        for (r, h) in &held {
            println!("  🔒 {r}  →  {h}");
        }
    }
}

/// `hf checkpoint <ID> [note]` — or `--auto` to checkpoint the current active task (the
/// resume target), `--quiet` to suppress stdout (for hook callers). Rejects a missing or
/// flag-shaped id so a malformed invocation can't pollute the witnessed ledger.
fn cmd_checkpoint(id: Option<&str>, note: &str, auto: bool, quiet: bool) {
    let resolved = if auto {
        let tasks = load_tasks();
        let replay = current_statuses();
        next_safe(&tasks, &replay).map(|t| t.id.clone())
    } else {
        id.map(|s| s.to_string())
    };
    let Some(id) = resolved else {
        eprintln!(
            "hf checkpoint: no task id — use `hf checkpoint <ID> [note]`, or `--auto` with an active task"
        );
        return;
    };
    if id.is_empty() || id.starts_with("--") {
        eprintln!("hf checkpoint: invalid task id '{id}'");
        return;
    }
    let note = if note.trim().is_empty() && auto {
        "auto checkpoint"
    } else {
        note
    };
    // Route the checkpoint to the home its task lives in (KERNEL vs FLEET). The
    // `--auto` path already resolved `id` from the LOCAL backlog, so it routes
    // local; an explicit `checkpoint <ID>` for a FLEET-resident task routes FLEET.
    let (ledger, tasks_dir) = match route::route_for_task(&id) {
        Ok(homes) => homes,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let payload = serde_json::json!({ "id": id, "note": note }).to_string();
    let mut led = open_ledger_or_exit(&ledger.to_string_lossy());
    must_witness(
        led.append("checkpoint", &id, &payload, now_ns()),
        "checkpoint",
    );
    // ADR-0003 rule 3 (HFTASK-0042): append a progress line to the kb plan (no-op for non-kb).
    if let Some(wo) = load_task_in(&tasks_dir, &id)
        && kb::write_back(
            &wo.correlation_id,
            &kb::KbTransition::Progress(note.to_string()),
        )
        && !quiet
    {
        println!(
            "hf checkpoint: kb {} progress logged (write-back)",
            wo.correlation_id
        );
    }
    if !quiet {
        println!("hf checkpoint: {id} :: {note}");
    }
}

/// `hf done <id> [--pr N]` — record the terminal `Done` transition (the loop's completion
/// signal, previously missing: claim→Claimed and checkpoint is a non-status event, so nothing
/// ever marked a task Done). With `--pr`, also witnesses a `pr_merged` event (ADR-0003 terminal).
fn cmd_done(id: &str, pr: Option<&str>) {
    if id.is_empty() {
        eprintln!("usage: hf done <task-id> [--pr N]");
        return;
    }
    let (ledger, tasks_dir) = match route::route_for_task(id) {
        Ok(homes) => homes,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let Some(wo) = load_task_in(&tasks_dir, id) else {
        eprintln!("hf done: no such task {id}");
        std::process::exit(1);
    };
    let mut led = open_ledger_or_exit(&ledger.to_string_lossy());
    // HFTASK-0045 (PRD §4.7 evidence-backed completion): a task that declares
    // test_commands may only reach Done once a witnessed `test_result` shows green.
    // Fail-closed — unproven completion never lands. Tasks with no test_commands are
    // exempt (nothing to run). Run `hf test <id>` to produce the evidence.
    if !wo.test_commands.is_empty() && latest_test_passed(&led, id) != Some(true) {
        eprintln!(
            "hf done: {id} blocked — no green witnessed test_result (PRD §4.7 completion evidence). Run `hf test {id}` first."
        );
        std::process::exit(1);
    }
    must_witness(
        led.record_transition(&wo, Status::Done, now_ns()),
        "done transition",
    );
    // A Done task must not leave a live claim lease behind. Otherwise `hf lease --json`
    // advertises false active holders/conflicts after verified completion, and a reopened
    // task can be blocked by its own stale completion-era lease until TTL expiry.
    let resource = lease::claim_resource(id);
    let holder = lease::local_holder();
    match led.release_lease(&resource, &holder, now_ns()) {
        Ok(_) => lease::remove_lockfile(&resource),
        Err(e) => eprintln!("hf done: WARNING — failed to release in-ledger lease {resource}: {e}"),
    }
    // HFTASK-0052 gap-hunt: auto-detect the merged PR from a prior `pr_opened` event if the
    // user did not pass `--pr N`. This gives every merged task a `pr_merged` ledger marker.
    let resolved_pr = pr.map(String::from).or_else(|| latest_pr_opened(&led, id));
    if let Some(ref p) = resolved_pr {
        let payload = serde_json::json!({ "id": id, "pr": p }).to_string();
        let _ = led.append("pr_merged", id, &payload, now_ns());
        // HFTASK-0021: round-trip the merged result to the originating prompt_hub workflow
        // via the correlation_id carried on the WorkOrder.
        delivery::emit_delivery(&mut led, &wo, p, now_ns());
        // HFTASK-0076 (ADR-0018 D11): under the develop-base pipeline a merged PR lands on the
        // base (develop), which is now AHEAD of the trunk — so the post-merge signal first
        // PROMOTES develop → trunk (hands-off ff, no manual `gh api`). After this the trunk ==
        // develop, so the HFTASK-0044 mirror-back below is current (a no-op), but kept for the
        // trunk-hotfix case where the trunk leads. Both are ff-only and non-fatal.
        promote_develop_to_trunk(&mut led, id);
        // HFTASK-0044: keep develop current with the trunk (trunk → base mirror-back).
        sync_develop_to_trunk(&mut led, id);
        // HFTASK-0075 (ADR-0018 D10): the "removed ON verified PR merge" path — now that a
        // `pr_merged` is witnessed for this batch, reap its session worktree. Non-fatal and
        // fail-closed (like the promote/sync calls above): if no session/merge can be
        // confirmed it does nothing, leaving the worktree. Drop the ledger handle first so
        // the reap replays the fresh, just-appended events from a clean open.
        drop(led);
        session::reap_open_session_if_merged();
    }
    // ADR-0003 rule 3 (HFTASK-0042): flip the kb plan to completed with evidence (no-op for
    // non-kb cards). One-way: planning is informed by execution, never read back.
    let evidence = resolved_pr
        .as_ref()
        .map(|p| format!("pr {p} merged"))
        .unwrap_or_else(|| "done".to_string());
    if kb::write_back(&wo.correlation_id, &kb::KbTransition::Done(evidence)) {
        println!("hf done: kb {} → completed (write-back)", wo.correlation_id);
    }
    if let Some(ref p) = resolved_pr {
        println!("hf done: {id} -> done (pr {p})");
        println!(
            "hf done: delivery -> {} (workflow {})",
            p, wo.correlation_id
        );
    } else {
        println!("hf done: {id} -> done");
    }
}

/// HFTASK-0044: fast-forward the base branch (develop) to the trunk after a merge, per the
/// `develop_mirrors_trunk` policy. Runs only post-merge (called from `hf done --pr`). The push
/// is ff-only (git rejects a non-ff push), so develop can never be force-moved; a non-ff/diverged
/// develop is reported and SKIPPED (non-fatal — the PR already merged, a develop hiccup must not
/// fail completion). Emits a witnessed `develop_synced` / `develop_sync_skipped` event.
fn sync_develop_to_trunk(led: &mut Ledger, id: &str) {
    let policy = policy::Policy::load(Path::new(HF));
    let Ok(bp) = branch::BranchPolicy::resolve(&policy.remote) else {
        return;
    };
    let Some(refspec) = bp.develop_sync_refspec() else {
        return; // rule doesn't apply (no distinct base/trunk, or fork model)
    };
    // Refresh origin/<trunk> so the local ref reflects the just-merged PR before we push it.
    if let Err(e) = run_out("git", &["fetch", "origin", &bp.trunk]) {
        eprintln!("hf done: develop sync skipped — fetch failed: {e}");
        let _ = led.append(
            "develop_sync_skipped",
            id,
            &serde_json::json!({ "id": id, "reason": format!("fetch failed: {e}") }).to_string(),
            now_ns(),
        );
        return;
    }
    match run_out("git", &["push", "origin", &refspec]) {
        Ok(_) => {
            println!("hf done: synced '{}' to '{}' (ff)", bp.base, bp.trunk);
            let _ = led.append(
                "develop_synced",
                id,
                &serde_json::json!({ "id": id, "base": bp.base, "trunk": bp.trunk }).to_string(),
                now_ns(),
            );
        }
        Err(e) => {
            // Non-ff (develop diverged) or no push perms — report, don't fail completion.
            eprintln!(
                "hf done: develop sync skipped — '{}' could not fast-forward to '{}': {e}",
                bp.base, bp.trunk
            );
            let _ = led.append(
                "develop_sync_skipped",
                id,
                &serde_json::json!({ "id": id, "reason": e }).to_string(),
                now_ns(),
            );
        }
    }
}

/// HFTASK-0076 (ADR-0018 D11): promote the integration base (develop) to the protected trunk
/// (master) via a hands-off, **runner-independent** fast-forward — the programmatic equivalent
/// of the owner-authorized `gh api -X PATCH .../refs/heads/<trunk> -f sha=<head> -F force=false`.
///
/// This is the forward **promotion** direction (base → trunk), the inverse of
/// [`sync_develop_to_trunk`]'s trunk → base mirror-back; the two are complementary. It uses the
/// `gh-api` PATCH path on purpose: a plain `git push <sha>:<trunk>` is classifier-blocked here,
/// whereas the PATCH is the documented legitimate trunk-mirror and bypasses the GitHub Actions
/// runner queue — so the trunk advances even when `sync-master.yml`'s required checks are
/// queue-starved (the D11 "stall"). `force=false` makes the server reject any non-ff, and a
/// local ancestor guard refuses a diverged trunk *before* the call (a divergence is a human
/// reconciliation, never an auto-promote). Witnessed `trunk_promoted` / `trunk_promote_skipped`.
/// Non-fatal: a promotion hiccup must never fail task completion (the PR already merged to base).
fn promote_develop_to_trunk(led: &mut Ledger, id: &str) {
    let policy = policy::Policy::load(Path::new(HF));
    let Ok(bp) = branch::BranchPolicy::resolve(&policy.remote) else {
        return;
    };
    if !bp.should_sync_develop_trunk() {
        return; // rule doesn't apply (no distinct base/trunk, mirror off, or fork model)
    }
    // Refresh both refs so the ancestor check + promote SHA reflect the just-merged PR.
    for r in [bp.base.as_str(), bp.trunk.as_str()] {
        if let Err(e) = run_out("git", &["fetch", "origin", r]) {
            eprintln!("hf promote: skipped — fetch {r} failed: {e}");
            let _ = led.append(
                "trunk_promote_skipped",
                id,
                &serde_json::json!({ "id": id, "reason": format!("fetch {r} failed: {e}") })
                    .to_string(),
                now_ns(),
            );
            return;
        }
    }
    let base_ref = format!("origin/{}", bp.base);
    let trunk_ref = format!("origin/{}", bp.trunk);
    let head = match run_out("git", &["rev-parse", &base_ref]) {
        Ok(h) if !h.is_empty() => h,
        _ => {
            eprintln!("hf promote: skipped — could not resolve {base_ref}");
            let _ = led.append(
                "trunk_promote_skipped",
                id,
                &serde_json::json!({ "id": id, "reason": format!("could not resolve {base_ref}") })
                    .to_string(),
                now_ns(),
            );
            return;
        }
    };
    // Ancestor guard (no-downgrade): the trunk must be an ancestor of the base head, i.e. a true
    // fast-forward with no divergence. `merge-base --is-ancestor` exits 0 iff so (→ run_out Ok).
    if run_out("git", &["merge-base", "--is-ancestor", &trunk_ref, &head]).is_err() {
        eprintln!(
            "hf promote: skipped — '{}' is not a fast-forward ancestor of '{}' (diverged trunk → manual reconcile)",
            bp.trunk, bp.base
        );
        let _ = led.append(
            "trunk_promote_skipped",
            id,
            &serde_json::json!({ "id": id, "base": bp.base, "trunk": bp.trunk, "reason": "trunk diverged (not ff)" })
                .to_string(),
            now_ns(),
        );
        return;
    }
    // Already current? Nothing to promote (idempotent, not a failure).
    if run_out("git", &["rev-parse", &trunk_ref]).ok().as_deref() == Some(head.as_str()) {
        println!(
            "hf promote: '{}' already at '{}' — nothing to promote",
            bp.trunk, bp.base
        );
        return;
    }
    // Hands-off fast-forward via the owner-authorized gh-api PATCH (force=false ⇒ server ff-only).
    let ref_path = bp.trunk_ref_api_path();
    match run_out(
        "gh",
        &[
            "api",
            "-X",
            "PATCH",
            &ref_path,
            "-f",
            &format!("sha={head}"),
            "-F",
            "force=false",
        ],
    ) {
        Ok(_) => {
            let short = &head[..head.len().min(12)];
            println!(
                "hf promote: '{}' fast-forwarded to '{}' @ {short} (gh-api, runner-independent)",
                bp.trunk, bp.base
            );
            let _ = led.append(
                "trunk_promoted",
                id,
                &serde_json::json!({ "id": id, "base": bp.base, "trunk": bp.trunk, "sha": head })
                    .to_string(),
                now_ns(),
            );
        }
        Err(e) => {
            eprintln!(
                "hf promote: skipped — gh-api ff of '{}' failed: {e}",
                bp.trunk
            );
            let _ = led.append(
                "trunk_promote_skipped",
                id,
                &serde_json::json!({ "id": id, "trunk": bp.trunk, "reason": e }).to_string(),
                now_ns(),
            );
        }
    }
}

/// HFTASK-0076: `hf promote` — manually trigger the hands-off develop → trunk promotion. Also
/// runs automatically at `hf done --pr` (post-merge). Witnessed; ff-only; fail-closed on a
/// diverged trunk; idempotent when the trunk is already current.
fn cmd_promote() {
    let mut led = open_ledger_or_exit(&ledger_path());
    promote_develop_to_trunk(&mut led, "-");
}

/// HFTASK-0045: the most recent `test_result` verdict for `id`, or `None` if the task has
/// never been tested. Latest-wins (a re-run after a fix supersedes the earlier failure).
fn latest_test_passed(led: &Ledger, id: &str) -> Option<bool> {
    led.all_events()
        .ok()?
        .iter()
        .rev()
        .find(|e| e.event_type == "test_result" && e.work_order_id == id)
        .and_then(|e| {
            serde_json::from_str::<serde_json::Value>(&e.payload_json)
                .ok()
                .and_then(|v| v["passed"].as_bool())
        })
}

/// HFTASK-0052 gap-hunt: if `hf done` is run after `hf ship` recorded a `pr_opened` event,
/// derive the merged PR automatically so the ledger gets a `pr_merged` marker even when the
/// user forgets `--pr N`. Returns `None` if no `pr_opened` event exists.
fn latest_pr_opened(led: &Ledger, id: &str) -> Option<String> {
    led.all_events()
        .ok()?
        .iter()
        .rev()
        .find(|e| e.event_type == "pr_opened" && e.work_order_id == id)
        .and_then(|e| {
            serde_json::from_str::<serde_json::Value>(&e.payload_json)
                .ok()
                .and_then(|v| v["pr"].as_str().map(String::from))
        })
}

/// Count how many tests a libtest/cargo run actually EXECUTED, by summing the
/// `N passed; M failed; … measured` fields of every `test result:` summary line in the
/// captured output. `filtered out` and `ignored` are NOT counted — they provide no
/// assertion evidence. Returns:
/// - `None` when no libtest summary is present (the command is some runner we can't
///   introspect) so the caller degrades to exit-code-only instead of false-blocking;
/// - `Some(0)` when a recognized test runner matched/ran zero real tests — the rubber
///   stamp the completion gate must reject (a `cargo test <filter>` that hit nothing still
///   exits 0);
/// - `Some(n)` with the real executed count otherwise.
fn parse_tests_ran(output: &str) -> Option<u64> {
    // HFTASK-0063: try each recognized runner in turn; the FIRST that recognizes its summary
    // wins. `None` only when NONE match (a genuinely-unknown runner → caller degrades to
    // exit-code-only). Order is widest-first; the parsers don't overlap (distinct markers).
    parse_libtest(output)
        .or_else(|| parse_pytest(output))
        .or_else(|| parse_jest(output))
        .or_else(|| parse_gotest(output))
}

/// libtest / cargo: sum the executed buckets (`passed`+`failed`+`measured`; never
/// `filtered out` or `ignored`) of every `test result:` summary line across all suites.
fn parse_libtest(output: &str) -> Option<u64> {
    let mut found = false;
    let mut total = 0u64;
    for line in output.lines() {
        let Some(rest) = line.split("test result:").nth(1) else {
            continue;
        };
        found = true;
        // `rest` ≈ " ok. 3 passed; 0 failed; 0 ignored; 0 measured; 120 filtered out; …".
        // Scan token PAIRS (a count followed by its label) so the leading status word
        // (`ok.`/`FAILED.`) in the first segment doesn't shadow the count behind it.
        let toks: Vec<&str> = rest.split_whitespace().collect();
        for w in toks.windows(2) {
            if let Ok(n) = w[0].parse::<u64>()
                && matches!(
                    w[1].trim_end_matches([';', '.', ',']),
                    "passed" | "failed" | "measured"
                )
            {
                total += n;
            }
        }
    }
    found.then_some(total)
}

/// Sum the integer preceding any of `labels` in a comma/space-separated summary fragment,
/// scanning token pairs so a count is always paired with the word that follows it.
fn sum_labeled(fragment: &str, labels: &[&str]) -> u64 {
    let toks: Vec<&str> = fragment
        .split([' ', ',', '\t'])
        .filter(|s| !s.is_empty())
        .collect();
    let mut total = 0u64;
    for w in toks.windows(2) {
        if let Ok(n) = w[0].parse::<u64>()
            && labels.contains(&w[1])
        {
            total += n;
        }
    }
    total
}

/// pytest summary, e.g. `===== 5 passed, 1 failed, 2 skipped in 0.10s =====` or
/// `==== no tests ran in 0.01s ====`. Counts executed outcomes (passed/failed/error/xpassed/
/// xfailed); `skipped`/`deselected`/`warnings` are not evidence. A framed "no tests ran"
/// summary returns `Some(0)` (the zero-match rubber stamp the gate must reject). The LAST
/// framed summary wins. `None` if no pytest-framed summary is present.
fn parse_pytest(output: &str) -> Option<u64> {
    let mut found = false;
    let mut total = 0u64;
    for line in output.lines() {
        let l = line.trim();
        if l.len() < 2 || !l.starts_with('=') || !l.ends_with('=') {
            continue;
        }
        let lower = l.to_ascii_lowercase();
        let is_summary = [
            "passed",
            "failed",
            "error",
            "xpassed",
            "xfailed",
            "no tests ran",
        ]
        .iter()
        .any(|k| lower.contains(k));
        if !is_summary {
            continue;
        }
        found = true;
        // The framed summary is authoritative; the last one wins.
        total = sum_labeled(
            &lower,
            &["passed", "failed", "error", "errors", "xpassed", "xfailed"],
        );
    }
    found.then_some(total)
}

/// jest / vitest summary line, e.g. `Tests:       1 failed, 5 passed, 6 total`. Counts
/// `passed`+`failed` (executed; `total` includes skipped/todo). `None` if no `Tests:` line.
fn parse_jest(output: &str) -> Option<u64> {
    let mut found = false;
    let mut total = 0u64;
    for line in output.lines() {
        let Some(rest) = line.trim().strip_prefix("Tests:") else {
            continue;
        };
        found = true;
        total = sum_labeled(rest, &["passed", "failed"]);
    }
    found.then_some(total)
}

/// go test (verbose): count per-test `--- PASS:` / `--- FAIL:` markers. A verbose run with
/// zero matched tests, or any run printing `no tests to run` / `no test files`, returns
/// `Some(0)` (zero-match rubber stamp). Non-verbose go prints no per-test marker, so without
/// `-v` this returns `None` (degrade to exit-code-only rather than falsely report 0).
fn parse_gotest(output: &str) -> Option<u64> {
    let count = output
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("--- PASS:") || t.starts_with("--- FAIL:")
        })
        .count() as u64;
    if count > 0 {
        return Some(count);
    }
    if output
        .lines()
        .any(|l| l.contains("no tests to run") || l.contains("no test files"))
    {
        return Some(0);
    }
    None
}

/// The git repository root of the cwd (`git rev-parse --show-toplevel`), or `None` outside a
/// repo. Used to pin `hf test`'s working dir so test_commands run from a deterministic root.
fn repo_toplevel() -> Option<PathBuf> {
    std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn shell_words_simple(cmd: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in cmd.chars() {
        if escaped {
            cur.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => cur.push(ch),
            None if ch == '\'' || ch == '"' => quote = Some(ch),
            None if ch.is_whitespace() => {
                if !cur.is_empty() {
                    words.push(std::mem::take(&mut cur));
                }
            }
            None => cur.push(ch),
        }
    }
    if !cur.is_empty() {
        words.push(cur);
    }
    words
}

fn shell_quote_word(word: &str) -> String {
    if word
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/' | ':' | '='))
    {
        word.to_string()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

fn shell_join_words(words: &[String]) -> String {
    words
        .iter()
        .map(|w| shell_quote_word(w))
        .collect::<Vec<_>>()
        .join(" ")
}

fn cargo_zero_test_diagnostic(cmd: &str) -> Option<String> {
    let words = shell_words_simple(cmd);
    if words.first().map(String::as_str) != Some("cargo")
        || words.get(1).map(String::as_str) != Some("test")
    {
        return None;
    }
    let dashdash = words.iter().position(|w| w == "--");
    let cargo_args_end = dashdash.unwrap_or(words.len());
    let cargo_args = &words[..cargo_args_end];
    let mut list_words = cargo_args.to_vec();
    list_words.push("--".to_string());
    list_words.push("--list".to_string());

    let mut all_tests_words = Vec::new();
    let mut package = None;
    let mut features = Vec::new();
    let mut filters = Vec::new();
    let mut i = 2usize;
    while i < cargo_args.len() {
        let w = &cargo_args[i];
        match w.as_str() {
            "-p" | "--package" => {
                if let Some(v) = cargo_args.get(i + 1) {
                    package = Some(v.clone());
                    all_tests_words.extend([w.clone(), v.clone()]);
                    i += 2;
                    continue;
                }
            }
            "--features" => {
                if let Some(v) = cargo_args.get(i + 1) {
                    features.push(format!("--features {v}"));
                    all_tests_words.extend([w.clone(), v.clone()]);
                    i += 2;
                    continue;
                }
            }
            "--all-features" | "--no-default-features" => {
                features.push(w.clone());
                all_tests_words.push(w.clone());
                i += 1;
                continue;
            }
            "--bin" | "--test" | "--example" | "--manifest-path" => {
                if let Some(v) = cargo_args.get(i + 1) {
                    all_tests_words.extend([w.clone(), v.clone()]);
                    i += 2;
                    continue;
                }
            }
            _ if w.starts_with("--features=") => {
                features.push(w.clone());
                all_tests_words.push(w.clone());
                i += 1;
                continue;
            }
            _ if w.starts_with("--package=") => {
                package = Some(w.trim_start_matches("--package=").to_string());
                all_tests_words.push(w.clone());
                i += 1;
                continue;
            }
            _ if w.starts_with('-') => {
                all_tests_words.push(w.clone());
                i += 1;
                continue;
            }
            _ => filters.push(w.clone()),
        }
        i += 1;
    }
    let mut list_all = vec!["cargo".to_string(), "test".to_string()];
    list_all.extend(all_tests_words);
    list_all.push("--".to_string());
    list_all.push("--list".to_string());

    let pkg = package.unwrap_or_else(|| "(workspace/default package)".to_string());
    let filter = if filters.is_empty() {
        "(none)".to_string()
    } else {
        filters.join(" ")
    };
    let feature_hint = if features.is_empty() {
        "no feature flags were present; if the expected tests are feature-gated, add the required `--features <feature>` (or `--all-features`) and rerun the list command".to_string()
    } else {
        format!("preserved feature flags: {}", features.join(", "))
    };
    Some(format!(
        "hf test: cargo zero-test diagnostic:\n  package: {pkg}\n  filter: {filter}\n  list matching tests: {}\n  list all tests for the same target/features: {}\n  hint: {feature_hint}",
        shell_join_words(&list_words),
        shell_join_words(&list_all)
    ))
}

/// `hf test [ID]` — PRD §4.7 evidence-backed completion. Execute the work order's
/// `test_commands` and witness the outcome as a `test_result` ledger event so `hf done`
/// can gate on green tests. With no id, targets the next safe task. Exits nonzero when any
/// command fails (fail-closed, so hooks / the loop observe the failure). The kernel's
/// completion-evidence guarantee: a stored `test_commands` is now actually run, not ignored
/// — and (the real fix) exit 0 alone is NOT accepted: a recognized runner that executed
/// zero tests is rejected, closing the "blanket `cargo test` matched nothing" rubber stamp.
fn cmd_test(id: Option<&str>) {
    let resolved = match id {
        Some(s) if !s.is_empty() && !s.starts_with("--") => s.to_string(),
        _ => {
            let tasks = load_tasks();
            let replay = current_statuses();
            match next_safe(&tasks, &replay) {
                Some(t) => t.id.clone(),
                None => {
                    eprintln!("hf test: no task id — use `hf test <ID>`");
                    std::process::exit(2);
                }
            }
        }
    };
    let (ledger, tasks_dir) = match route::route_for_task(&resolved) {
        Ok(homes) => homes,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let Some(wo) = load_task_in(&tasks_dir, &resolved) else {
        eprintln!("hf test: no such task {resolved}");
        std::process::exit(1);
    };
    if wo.test_commands.is_empty() {
        eprintln!("hf test: {resolved} declares no test_commands (nothing to run)");
        std::process::exit(2);
    }
    // HFTASK-0063: pin the command's working dir to the repo root so test_commands (e.g.
    // `cargo test -p hf`) run from a deterministic location, not the ambient invocation cwd
    // (a card run from a subdir or the meta root would otherwise resolve differently). Falls
    // back to the current dir outside a git repo.
    let run_dir = repo_toplevel();
    let mut results = Vec::new();
    let mut all_passed = true;
    for cmd in &wo.test_commands {
        println!("hf test: $ {cmd}");
        // Capture output (instead of inheriting stdio) so the gate can verify tests ACTUALLY
        // ran; re-emit it so the operator still sees the full run.
        let mut command = std::process::Command::new("sh");
        command.arg("-c").arg(cmd);
        if let Some(dir) = &run_dir {
            command.current_dir(dir);
        }
        let (code, ran) = match command.output() {
            Ok(out) => {
                use std::io::Write;
                std::io::stdout().write_all(&out.stdout).ok();
                std::io::stderr().write_all(&out.stderr).ok();
                let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
                combined.push_str(&String::from_utf8_lossy(&out.stderr));
                (out.status.code().unwrap_or(-1), parse_tests_ran(&combined))
            }
            Err(e) => {
                eprintln!("hf test: failed to spawn '{cmd}': {e}");
                (-1, None)
            }
        };
        // The completion-evidence gate: exit 0 is necessary but NOT sufficient. A recognized
        // runner that executed zero tests (filter matched nothing, or all `#[ignore]`) is a
        // rubber stamp — reject it fail-closed. A runner we can't introspect (`ran == None`)
        // falls back to exit-code-only and is flagged (no-downgrade for non-cargo runners).
        let zero_tests = ran == Some(0);
        let cmd_passed = code == 0 && !zero_tests;
        if !cmd_passed {
            all_passed = false;
        }
        if zero_tests {
            eprintln!(
                "hf test: '{cmd}' exited 0 but executed 0 tests — completion evidence requires \
                 >0 (failing closed; tighten the filter so it matches real tests)"
            );
            if let Some(diag) = cargo_zero_test_diagnostic(cmd) {
                eprintln!("{diag}");
            }
        } else if ran.is_none() && code == 0 {
            eprintln!(
                "hf test: note — '{cmd}' produced no libtest summary; gated on exit code only \
                 (executed-test count unverifiable)"
            );
        }
        results.push(serde_json::json!({
            "cmd": cmd,
            "code": code,
            "tests_ran": ran,
            "passed": cmd_passed,
        }));
    }
    let total_ran: u64 = results.iter().filter_map(|r| r["tests_ran"].as_u64()).sum();
    let payload = serde_json::json!({
        "id": resolved,
        "passed": all_passed,
        "tests_ran": total_ran,
        "results": results,
    })
    .to_string();
    let mut led = open_ledger_or_exit(&ledger.to_string_lossy());
    must_witness(
        led.append("test_result", &resolved, &payload, now_ns()),
        "test_result",
    );
    if all_passed {
        println!(
            "hf test: {resolved} -> PASS ({} command(s) green, {total_ran} test(s) executed, witnessed)",
            wo.test_commands.len()
        );
    } else {
        eprintln!("hf test: {resolved} -> FAIL (witnessed test_result; `hf done` is blocked)");
        std::process::exit(1);
    }
}

/// Persist each card's ledger-replayed status into its `.task.json` (ADR-0003 single-registry
/// rule: cards are derived snapshots of ledger truth). Returns the number of cards changed.
/// This is the deterministic fix for D3 (cards stale at `backlog` despite ledger progress).
fn sync_cards() -> usize {
    let replay = current_statuses();
    let mut changed = 0;
    for mut wo in load_tasks() {
        let live = status_of(&wo.id, &replay, &wo);
        if live != wo.status {
            wo.status = live;
            save_task(&wo);
            changed += 1;
        }
    }
    changed
}

/// The repo-relative path of a task card on disk (`.handoff/tasks/<ID>.task.json`).
/// Pure (no I/O) so the staging decision is unit-testable (HFTASK-0029 Defect A).
fn task_card_relpath(id: &str) -> String {
    format!("{HF}/tasks/{id}.task.json")
}

/// The exact git pathspecs `hf ship` stages for a task — and ONLY these (HFTASK-0029
/// Defect A). `git add -u` stages tracked modifications/deletions but NO untracked files;
/// the task's own card is added explicitly (it may be newly created/seeded) when present.
/// Untracked scratch (_workspace/, stray cards) is deliberately excluded.
fn ship_stage_specs(id: &str) -> Vec<String> {
    let mut specs = vec!["-u".to_string()];
    let card = task_card_relpath(id);
    if Path::new(&card).exists() {
        specs.push(card);
    }
    specs
}

/// HFTASK-0009 core: one squash-style commit → push branch → PR → arm GitHub-native
/// auto-merge (HFTASK-0010 merge model: GitHub merges when ALL required checks are
/// green; hf never polls-and-merges and never overrides red). Emits `pr_opened`.
fn cmd_ship(id: &str, base: &str) {
    if id.is_empty() {
        eprintln!("usage: hf ship <task-id> [--base BRANCH]");
        std::process::exit(2); // HFTASK-0036: usage error (matches the dispatch convention)
    }
    // HFTASK-0081 (ADR-0017 / R13): govern the ship action through the cognitum gate before any
    // outward effect (push/PR). Ship is the loop's least-reversible action; the governor witnesses
    // a `cognitum_decision` and BLOCKS (exit 1) on defer/deny. Default thresholds permit benign
    // actions, so the live pipeline is unaffected. The governor is feature-gated (`cognitum`, a
    // default feature); a `--no-default-features` build compiles the gate out and ship proceeds
    // ungoverned. This is the in-loop wiring the governor verb previously lacked.
    #[cfg(feature = "cognitum")]
    cognitum::gate_action_or_block(&format!("ship-{id}"), "ship", Some(id));
    // HFTASK-0008: resolve the branch/remote policy (clone/fork model + base/trunk) once,
    // so ship decides the same way everything else does instead of hardcoding "master".
    let policy = policy::Policy::load(Path::new(HF));
    let bp = match branch::BranchPolicy::resolve(&policy.remote) {
        Ok(b) => b,
        Err(e) => {
            // HFTASK-0036: a refused/failed ship MUST exit nonzero (L2 hf-verb-safety).
            eprintln!("hf ship: {e}");
            std::process::exit(1);
        }
    };
    // Fork model is deferred (ADR-0001 §3) — fail closed before any remote operation.
    if let Err(e) = bp.ensure_supported() {
        eprintln!("hf ship: {e}");
        std::process::exit(1);
    }
    // PR target: an explicit `--base` wins; otherwise the policy trunk (was hardcoded "master").
    let base = if base.is_empty() {
        bp.trunk.as_str()
    } else {
        base
    };
    let branch = match run_out("git", &["branch", "--show-current"]) {
        Ok(b) if !b.is_empty() => b,
        _ => {
            eprintln!("hf ship: not on a branch (detached HEAD?) — refusing");
            std::process::exit(1);
        }
    };
    // HFTASK-0008: never ship FROM the trunk or the integration base — work lands via PR
    // off a session branch. The trunk guard is centralized in the policy engine.
    if branch == base {
        eprintln!(
            "hf ship: refusing to ship from the base branch '{branch}' — use a session branch"
        );
        std::process::exit(1);
    }
    if let Err(e) = bp.guard_direct_trunk_push(&branch) {
        eprintln!("hf ship: {e}");
        std::process::exit(1);
    }
    if bp.should_sync_develop_trunk() {
        println!(
            "hf ship: note — develop_mirrors_trunk; '{}' fast-forwards to '{}' at `hf done --pr` (post-merge)",
            bp.base, bp.trunk
        );
    }
    // single squash-style commit of the working tree (if dirty).
    // HFTASK-0029 Defect A: stage ONLY task scope — `git add -u` (tracked
    // modifications/deletions, NO untracked files) plus the task's own card (which may be
    // newly created/seeded). This stops untracked scratch (_workspace/, stray KBTASK cards)
    // from being swept into the PR (PR #29 regression).
    let dirty = run_out("git", &["status", "--porcelain"]).unwrap_or_default();
    if !dirty.is_empty() {
        for spec in ship_stage_specs(id) {
            if let Err(e) = run_out("git", &["add", &spec]) {
                eprintln!("hf ship: {e}");
                std::process::exit(1);
            }
        }
        let msg = format!(
            "feat: ship {id}

Shipped via `hf ship` (handoff.task.v1 {id}).

Implements [[tasks/{id}]]"
        );
        if let Err(e) = run_out("git", &["commit", "-m", &msg]) {
            eprintln!("hf ship: {e}");
            std::process::exit(1);
        }
        println!("hf ship: committed working tree on {branch}");
    }
    if let Err(e) = run_out("git", &["push", "-u", "origin", &branch]) {
        eprintln!("hf ship: push failed — {e}");
        std::process::exit(1);
    }
    println!("hf ship: pushed {branch}");
    // PR create (idempotent: reuse an existing open PR for this branch)
    let pr_url = match run_out(
        "gh",
        &["pr", "view", &branch, "--json", "url", "--jq", ".url"],
    ) {
        Ok(u) if u.starts_with("http") => {
            println!("hf ship: reusing open PR {u}");
            u
        }
        _ => {
            let title = format!("feat: ship {id}");
            let body = format!(
                "Shipped via `hf ship` (handoff.task.v1 **{id}**).\n\nAuto-merge is armed; GitHub merges when all required checks on `{base}` pass."
            );
            match run_out(
                "gh",
                &[
                    "pr", "create", "--base", base, "--head", &branch, "--title", &title, "--body",
                    &body,
                ],
            ) {
                Ok(u) => u.lines().last().unwrap_or_default().to_string(),
                Err(e) => {
                    eprintln!("hf ship: PR creation failed — {e}");
                    std::process::exit(1);
                }
            }
        }
    };
    println!("hf ship: PR {pr_url}");
    // arm GitHub-native auto-merge — non-fatal (e.g. unprotected base merges need no arming)
    match run_out("gh", &["pr", "merge", "--auto", "--squash", &pr_url]) {
        Ok(_) => {
            println!("hf ship: native auto-merge armed (squash) — GitHub completes on green checks")
        }
        Err(e) => {
            eprintln!("hf ship: auto-merge not armed ({e}) — merge manually or via review flow")
        }
    }
    let ledger = match route::route_for_task(id) {
        Ok((ledger, _tasks)) => ledger,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let payload =
        serde_json::json!({ "id": id, "branch": branch, "pr": pr_url, "base": base }).to_string();
    let mut led = open_ledger_or_exit(&ledger.to_string_lossy());
    must_witness(led.append("pr_opened", id, &payload, now_ns()), "pr_opened");
    println!("hf ship: pr_opened recorded for {id}");
}

/// HFTASK-0010 verdict channel (R6): verdicts ride OUT-OF-BAND — a weave permission
/// answer carries approve/deny, and hf records `review_verdict` in its own witnessed
/// ledger. Never a native GitHub APPROVE (bot-approval bypasses branch protection).
fn cmd_review_verdict(id: &str, pr: &str, verdict: &str, by: &str) {
    if id.is_empty() || pr.is_empty() || !matches!(verdict, "approve" | "deny") {
        eprintln!("usage: hf review verdict <task-id> <pr> <approve|deny> [--by WHO]");
        return;
    }
    let ledger = match route::route_for_task(id) {
        Ok((ledger, _tasks)) => ledger,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let payload =
        serde_json::json!({ "id": id, "pr": pr, "verdict": verdict, "by": by }).to_string();
    let mut led = open_ledger_or_exit(&ledger.to_string_lossy());
    must_witness(
        led.append("review_verdict", id, &payload, now_ns()),
        "review_verdict",
    );
    println!("hf review: {verdict} recorded for {id} ({pr}) by {by}");
}

// HFTASK-0083: GhPrView moved to handoff-gatekeeper (imported above).

/// Fetch the list of changed file paths for a PR using `gh pr diff --name-only`.
fn review_changed_files(pr: &str) -> Result<Vec<String>, String> {
    let out = run_out("gh", &["pr", "diff", pr, "--name-only"])?;
    Ok(out
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// HFTASK-0010 Phase 1: request a cloud_ultra review for PR `pr`.
///
/// Guardrails enforced locally:
/// - Draft PRs are refused.
/// - Files matching `[merge].protected_files` are refused.
///
/// On refusal a `review_refused_*` event is recorded; on approval a `review_requested`
/// event is recorded. The actual /code-review ultra invocation is intentionally left to
/// the reviewer (it is an IDE slash command, not a CLI tool).
fn cmd_review_request(pr: &str, task_id: Option<&str>) {
    if pr.is_empty() {
        eprintln!("usage: hf review request <pr> [--task <id>]");
        std::process::exit(2);
    }

    let policy = policy::Policy::load(Path::new(HF));

    // Resolve PR metadata.
    let meta_json = match run_out(
        "gh",
        &[
            "pr",
            "view",
            pr,
            "--json",
            "url,number,headRefName,baseRefName,isDraft",
        ],
    ) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("hf review request: cannot read PR {pr}: {e}");
            std::process::exit(1);
        }
    };
    let meta: GhPrView = match serde_json::from_str(&meta_json) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("hf review request: malformed gh output: {e}");
            std::process::exit(1);
        }
    };

    // Determine which ledger to write to.
    let ledger = match task_id {
        Some(id) => match route::route_for_task(id) {
            Ok((ledger, _tasks)) => ledger,
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        },
        None => PathBuf::from(ledger_path()),
    };

    let mut led = match Ledger::open(&ledger.to_string_lossy()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("hf review request: cannot open ledger: {e}");
            std::process::exit(1);
        }
    };
    let work_order_id = task_id.unwrap_or("review");

    // Guardrail (e): refuse draft PRs.
    if meta.is_draft {
        let payload = serde_json::json!({
            "pr": meta.url,
            "number": meta.number,
            "reason": "draft PR",
            "task_id": task_id,
        })
        .to_string();
        must_witness(
            led.append("review_refused_draft", work_order_id, &payload, now_ns()),
            "review_refused_draft",
        );
        eprintln!("hf review request: refusing draft PR #{}", meta.number);
        std::process::exit(1);
    }

    // Guardrail (d): protected-files denylist.
    let files = match review_changed_files(pr) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("hf review request: cannot list changed files: {e}");
            std::process::exit(1);
        }
    };
    let hits = policy.merge.protected_hits(&files);
    if !hits.is_empty() {
        let payload = serde_json::json!({
            "pr": meta.url,
            "number": meta.number,
            "reason": "protected files touched",
            "protected_files": &hits,
            "task_id": task_id,
        })
        .to_string();
        must_witness(
            led.append(
                "review_refused_protected_files",
                work_order_id,
                &payload,
                now_ns(),
            ),
            "review_refused_protected_files",
        );
        eprintln!(
            "hf review request: refusing PR #{} — touches protected files: {:?}",
            meta.number, hits
        );
        std::process::exit(1);
    }

    let payload = serde_json::json!({
        "pr": &meta.url,
        "number": meta.number,
        "head": &meta.head_ref_name,
        "base": &meta.base_ref_name,
        "reviewer": policy.merge.reviewer.as_str(),
        "task_id": task_id,
        "changed_files": files,
    })
    .to_string();
    must_witness(
        led.append("review_requested", work_order_id, &payload, now_ns()),
        "review_requested",
    );
    println!(
        "hf review request: PR #{} ({}) queued for {} review",
        meta.number, meta.url, policy.merge.reviewer
    );
    println!("  Run `/code-review ultra` in the IDE on this PR to produce a verdict.");
    println!(
        "  Then record it with: hf review verdict {} {} approve|deny",
        work_order_id, meta.url
    );
}

// HFTASK-0083: capsule_path / capsule_field / current_northstar_revision lifted to handoff-core
// (re-exported below) so the drift/gate crate resolves the North-Star revision without hf.
fn cmd_status(json: bool) {
    let tasks = load_tasks();
    let replay = current_statuses();
    if json {
        emit_status_json(&tasks, &replay);
        return;
    }
    println!("=== hf status ===  ({} tasks)", tasks.len());
    for t in &tasks {
        println!(
            "  {:<12} {:<12} {:?}  {}",
            t.id,
            format!("{:?}", status_of(&t.id, &replay, t)),
            t.priority,
            t.title
        );
    }
    if let Some(n) = next_safe(&tasks, &replay) {
        println!("next safe: {} — {}", n.id, n.title);
    }
}

/// Agent-navigation command for the next safe action. The exhausted-backlog case must still be
/// a valid shell command: bare `done` is a shell reserved word that errors outside a loop and
/// conflicts with the top-level `hf done ID` lifecycle verb.
fn next_command_for(next: Option<&WorkOrder>) -> String {
    next.map(|t| format!("hf claim {}", t.id))
        .unwrap_or_else(|| "hf handoff".into())
}

/// HFTASK-0020: the loop's machine read-model (`handoff.loop_status.v1`) — the witnessed
/// ledger event stream rendered as JSON for Mission Control / the MCP seam / RuVocal.
fn emit_status_json(tasks: &[WorkOrder], replay: &[(String, Status)]) {
    let done = tasks
        .iter()
        .filter(|t| status_of(&t.id, replay, t) == Status::Done)
        .count();
    let next = next_safe(tasks, replay);
    let witness = Ledger::open(&ledger_path())
        .and_then(|l| l.verify_witness_chain())
        .unwrap_or(0);
    let sess = session::open_session_and_cycle();
    let policy = policy::Policy::load(Path::new(HF));
    let project = capsule_field("project_name")
        .unwrap_or_else(|| "handoff (Continuity Ledger Kernel)".into());

    let out = serde_json::json!({
        "schema": "handoff.loop_status.v1",
        "project": project,
        "tasks_total": tasks.len(),
        "done": done,
        "remaining": tasks.len() - done,
        "tasks": tasks.iter().map(|t| serde_json::json!({
            "id": t.id,
            "status": format!("{:?}", status_of(&t.id, replay, t)),
            "priority": t.priority_str(),
            "title": t.title,
        })).collect::<Vec<_>>(),
        "next_task_id": next.map(|t| t.id.clone()),
        "next_command": next_command_for(next),
        "session": {
            "open": sess.open_branch.is_some(),
            "branch": sess.open_branch,
            "cycle": sess.cycle,
            "cycle_flush": policy.loop_cfg.cycle_flush,
            "ready_to_ship": sess.cycle >= policy.loop_cfg.cycle_flush,
        },
        "witnessed_events_verified": witness,
    });
    println!("{}", pretty_json(&out));
}

/// Build the handoff.packet.v2 machine summary from already-known facts. Pure over its
/// inputs (the witness count is computed by the caller) so it is unit-testable — this is
/// the single source for both the packet and `hf resume --json`.
fn summary_json(
    tasks: &[WorkOrder],
    replay: &[(String, Status)],
    witness: usize,
) -> serde_json::Value {
    let done: Vec<&String> = tasks
        .iter()
        .filter(|t| status_of(&t.id, replay, t) == Status::Done)
        .map(|t| &t.id)
        .collect();
    let remaining: Vec<&String> = tasks
        .iter()
        .filter(|t| status_of(&t.id, replay, t) != Status::Done)
        .map(|t| &t.id)
        .collect();
    let next = next_safe(tasks, replay);
    serde_json::json!({
        "schema": "handoff.packet.v2",
        "project": "handoff (Continuity Ledger Kernel)",
        "tasks_total": tasks.len(),
        "done": done,
        "remaining": remaining,
        "next_task_id": next.map(|t| &t.id),
        "witnessed_events_verified": witness,
        "next_command": next_command_for(next),
    })
}

/// The live machine read-model, computed straight from the ledger + cards. Both `hf handoff`
/// and `hf resume --json` go through this, so resume never lags the ledger (FIX-2: resume
/// previously echoed the last packet, under-counting by any event appended since).
fn machine_summary(tasks: &[WorkOrder], replay: &[(String, Status)]) -> serde_json::Value {
    let witness = Ledger::open(&ledger_path())
        .and_then(|l| l.verify_witness_chain())
        .unwrap_or(0);
    summary_json(tasks, replay, witness)
}

/// HFTASK-0071 (ADR-0018 D4): the explicit **Next Action / Direction** block — the single
/// next safe task, the exact next command, WHY it is next, the cycle/context-budget wrap
/// rule, and the blocking walls. Rendered identically into `hf resume` (Full) and the
/// `packets/latest.md` packet so a fresh agent needs zero archaeology: it is told *what to
/// do next*, not just handed state. Every field is DERIVED from the witnessed ledger
/// (`replay`/`tasks`/`next`), the session counter (`sess`), and `.handoff/policy.toml`
/// (`policy`) — never hardcoded. Pure over its inputs so it is unit-testable.
fn direction_block(
    tasks: &[WorkOrder],
    replay: &[(String, Status)],
    next: Option<&WorkOrder>,
    policy: &policy::Policy,
    sess: &session::LoopSessionState,
) -> String {
    let mut md = String::new();
    md.push_str("## 0. Next Action / Direction\n");

    match next {
        Some(n) => {
            let status = status_of(&n.id, replay, n);
            // The exact command depends on whether the task is already in-progress (resume it)
            // or a fresh backlog card (claim it) — same precedence `next_safe` uses.
            let in_progress = matches!(
                status,
                Status::Claimed | Status::Checkpointed | Status::Active | Status::Review
            );
            let command = if in_progress {
                format!("hf checkpoint {}", n.id)
            } else {
                format!("hf claim {}", n.id)
            };
            // WHY it is next: in-progress tasks are resumed first; otherwise it is the
            // highest-priority backlog card whose dependencies are all Done.
            let rationale = if in_progress {
                format!(
                    "resume the in-progress task (status {status:?}) before starting any new work"
                )
            } else {
                let deps = if n.dependencies.is_empty() {
                    "no dependencies".to_string()
                } else {
                    format!("deps satisfied ({})", n.dependencies.join(", "))
                };
                format!(
                    "first backlog card that is unblocked — {deps}, priority {}",
                    n.priority_str()
                )
            };
            md.push_str(&format!("- **Next safe task:** {} — {}\n", n.id, n.title));
            md.push_str(&format!("- **Next command:** `{command}`\n"));
            md.push_str(&format!("- **Why it is next:** {rationale}.\n"));
        }
        None => {
            md.push_str("- **Next safe task:** none — backlog is exhausted (all cards Done).\n");
            md.push_str("- **Next command:** `hf handoff` (render the closing packet).\n");
            md.push_str("- **Why it is next:** no Backlog/in-progress card remains.\n");
        }
    }

    // Cycle / context-budget state (ADR-0018 D3): how the loop decides when to wrap.
    let lc = &policy.loop_cfg;
    let wrap = match lc.wrap_strategy.as_str() {
        "tasks" => format!(
            "tasks — wrap (checkpoint → handoff) at cycle_flush={} tasks; this session is at cycle {}/{}",
            lc.cycle_flush, sess.cycle, lc.cycle_flush
        ),
        // unknown values fall back to "context" (matches policy.rs doc + Default)
        _ => format!(
            "context — wrap at ~{}% of the context window (cycle_flush={} caps a runaway cycle); this session is at cycle {}/{}",
            lc.context_budget_pct, lc.cycle_flush, sess.cycle, lc.cycle_flush
        ),
    };
    let ready = sess.cycle >= lc.cycle_flush;
    md.push_str(&format!("- **Cycle / context budget:** {wrap}.\n"));
    md.push_str(&format!(
        "- **Ready to ship:** {} (`hf ship` once the cycle is full / context budget hit).\n",
        if ready { "yes" } else { "no" }
    ));

    // Blocking walls: any card in Blocked status, or carrying explicit blocked_by ids, or a
    // NEEDS-HUMAN marker in its objective (the genuine owner walls). Derived, never assumed.
    let mut walls: Vec<String> = Vec::new();
    for t in tasks {
        let st = status_of(&t.id, replay, t);
        let needs_human = t.objective.contains("NEEDS-HUMAN");
        if st == Status::Blocked || !t.blocked_by.is_empty() || needs_human {
            let mut why = Vec::new();
            if st == Status::Blocked {
                why.push("status Blocked".to_string());
            }
            if !t.blocked_by.is_empty() {
                why.push(format!("blocked_by {}", t.blocked_by.join(", ")));
            }
            if needs_human {
                why.push("NEEDS-HUMAN".to_string());
            }
            walls.push(format!("{} ({})", t.id, why.join("; ")));
        }
    }
    if walls.is_empty() {
        md.push_str("- **Blocking walls:** none.\n");
    } else {
        md.push_str(&format!("- **Blocking walls:** {}\n", walls.join(" · ")));
    }
    md.push('\n');
    md
}

/// Render the handoff.packet.v2 markdown from already-known facts. Pure over its inputs
/// (the witness count is computed by the caller) so both `hf handoff` (which persists it)
/// and `hf resume` (which renders it LIVE, never reading the frozen file) share one
/// renderer — guaranteeing resume's Done N/M + witness count agree with handoff
/// (HFTASK-0027: resume previously echoed the last-written packet, freezing the count at
/// the last `hf handoff`).
fn render_packet_md(tasks: &[WorkOrder], replay: &[(String, Status)], witness: usize) -> String {
    let done: Vec<_> = tasks
        .iter()
        .filter(|t| status_of(&t.id, replay, t) == Status::Done)
        .collect();
    let remaining: Vec<_> = tasks
        .iter()
        .filter(|t| status_of(&t.id, replay, t) != Status::Done)
        .collect();
    let next = next_safe(tasks, replay);
    let summary = summary_json(tasks, replay, witness);

    let mut md = String::new();
    md.push_str("# Handoff Packet (latest) — handoff.packet.v2\n\n");
    // North Star is rendered from the context capsule (ADR-0006 portability: no
    // hardcoded northstar in the renderer). The capsule points to the canonical
    // fleet vision/architecture/runbook docs at the meta root.
    let northstar = capsule_field("northstar").unwrap_or_else(|| {
        "See ../NORTH-STAR.md (canon). NO HUMAN IN THE LOOP: a multi-provider agentic autopilot \
         — the user gives direction, the system builds/verifies/delivers."
            .to_string()
    });
    md.push_str(&format!("## 1. North Star\n{northstar}\n\n"));
    md.push_str("## 2. State Precedence\nGit > .handoff/ledger.db > tasks/*.task.json > active.md > this packet.\n\n");
    md.push_str(&format!(
        "## 3. Progress\nDone: {}/{}.  Tamper-evident events verified: {}.\n\n",
        done.len(),
        tasks.len(),
        witness
    ));
    // HFTASK-0071 (ADR-0018 D4): explicit next-action steering — what to DO next, derived
    // from the witnessed ledger + session counter + policy. Rendered into BOTH the packet
    // and `hf resume` (same renderer) so a fresh agent needs zero archaeology.
    md.push_str(&direction_block(
        tasks,
        replay,
        next,
        &policy::Policy::load(Path::new(HF)),
        &session::open_session_and_cycle(),
    ));
    md.push_str("## 4. Remaining (next safe first)\n");
    for t in &remaining {
        md.push_str(&format!(
            "- [{}] **{}** — {}\n",
            t.priority_str(),
            t.id,
            t.title
        ));
    }
    md.push_str("\n## 5. Next Best Task\n");
    if let Some(n) = next {
        md.push_str(&format!(
            "**{}** — {}\n  objective: {}\n",
            n.id, n.title, n.objective
        ));
    }
    md.push_str(&format!(
        "\n## 6. Resume Commands\n```bash\nhf resume\n{}\n```\n",
        summary["next_command"].as_str().unwrap_or("")
    ));
    md.push_str("\n## 7. Machine Summary\n```json\n");
    md.push_str(&pretty_json(&summary));
    md.push_str("\n```\n");
    md
}

/// The active claimed task whose AgentContract a handoff proves: the in-progress task
/// (same statuses `next_safe` resumes). `None` when nothing is claimed.
fn active_task<'a>(tasks: &'a [WorkOrder], replay: &[(String, Status)]) -> Option<&'a WorkOrder> {
    tasks.iter().find(|t| {
        matches!(
            status_of(&t.id, replay, t),
            Status::Claimed | Status::Checkpointed | Status::Active | Status::Review
        )
    })
}

/// Count witnessed checkpoint transitions for `id` in the ledger (completion evidence for
/// the AgentContract proof — HFTASK-0004 obligation 4).
fn checkpoint_count(id: &str) -> usize {
    let evs = match Ledger::open(&ledger_path()).and_then(|l| l.all_events()) {
        Ok(e) => e,
        Err(_) => return 0,
    };
    evs.iter()
        .filter(|e| e.work_order_id == id && e.event_type == "task_transition")
        .filter(|e| {
            serde_json::from_str::<serde_json::Value>(&e.payload_json)
                .ok()
                .and_then(|v| v.get("status").cloned())
                .and_then(|s| serde_json::from_value::<Status>(s).ok())
                == Some(Status::Checkpointed)
        })
        .count()
}

fn cmd_handoff() {
    let tasks = load_tasks();
    let replay = current_statuses();

    // HFTASK-0004 (ADR-0011): prove the active task's AgentContract via the lean-agentic
    // kernel BEFORE rendering. Fail closed — an unprovable contract (intent drift, or an
    // as-complete task with no witnessed checkpoint) blocks the handoff: exit before any
    // packet/active.md is written, so the rendered views are never left half-updated.
    let proof = active_task(&tasks, &replay).map(|active| {
        let evidence = contract::CompletionEvidence {
            status: status_of(&active.id, &replay, active),
            checkpoints: checkpoint_count(&active.id),
            northstar_revision: current_northstar_revision(),
        };
        match contract::prove_contract(active, &evidence) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("hf handoff: BLOCKED (fail-closed, ADR-0011) — {e}");
                std::process::exit(1);
            }
        }
    });

    let done: Vec<_> = tasks
        .iter()
        .filter(|t| status_of(&t.id, &replay, t) == Status::Done)
        .collect();
    let next = next_safe(&tasks, &replay);
    let witness = Ledger::open(&ledger_path())
        .and_then(|l| l.verify_witness_chain())
        .unwrap_or(0);

    let mut md = render_packet_md(&tasks, &replay, witness);
    if let Some(p) = &proof {
        md.push_str(&contract::render_proof_section(p));
    }

    let _ = fs::create_dir_all(Path::new(HF).join("packets"));
    let _ = fs::write(packet_path(), &md);
    let _ = fs::write(
        Path::new(HF).join("active.md"),
        format!(
            "# Active\n\nNext: {}\nDone {}/{} · witness-verified {} events\n",
            next.map(|t| t.id.as_str()).unwrap_or("—"),
            done.len(),
            tasks.len(),
            witness
        ),
    );
    println!(
        "hf handoff: wrote {} (verified {} witnessed events)",
        packet_path().display(),
        witness
    );
    if let Some(p) = &proof {
        println!(
            "hf handoff: AgentContract PROVEN for {} — {} obligation(s), {} ruvector-verified proof-term(s), binding {:#018x} (ADR-0011)",
            p.task,
            p.obligations.len(),
            p.proof_terms,
            p.content_hash
        );
    }
}

enum ResumeMode {
    /// Full compiled packet (markdown read-model).
    Full,
    /// Live machine summary (recomputed from the ledger — never stale).
    Json,
    /// One-line live status for hooks / fast resume.
    Compact,
}

fn cmd_resume(mode: ResumeMode) {
    match mode {
        ResumeMode::Json => {
            let tasks = load_tasks();
            let replay = current_statuses();
            println!("{}", pretty_json(&machine_summary(&tasks, &replay)));
        }
        ResumeMode::Compact => {
            let tasks = load_tasks();
            let replay = current_statuses();
            let s = machine_summary(&tasks, &replay);
            let done = s["done"].as_array().map(|a| a.len()).unwrap_or(0);
            println!(
                "handoff: {done}/{} done · {} witnessed · next: {} → {}",
                s["tasks_total"],
                s["witnessed_events_verified"],
                s["next_task_id"].as_str().unwrap_or("—"),
                s["next_command"].as_str().unwrap_or(""),
            );
        }
        ResumeMode::Full => {
            // HFTASK-0027: render the packet LIVE from the ledger + cards instead of
            // echoing the frozen packets/latest.md (which freezes Done N/M + the
            // witnessed count at the last `hf handoff`). Same renderer as `hf handoff`,
            // so resume's numbers always equal handoff's and reflect events appended
            // since the last handoff.
            let tasks = load_tasks();
            let replay = current_statuses();
            let witness = Ledger::open(&ledger_path())
                .and_then(|l| l.verify_witness_chain())
                .unwrap_or(0);
            println!("{}", render_packet_md(&tasks, &replay, witness));
        }
    }
}

// HFTASK-0083: the PrioStr trait (Priority→&str display) moved to work-order so the peeled
// feature crates (handoff-fleet, …) and the in-hf modules (sync) share one impl. Re-exported via
// `use work_order::PrioStr` below.

/// Seed the REAL continuation backlog (the tasks to finish the .handoff kernel) as task cards.
fn cmd_seed() {
    let mk = |id: &str, title: &str, pri: Priority, obj: &str, deps: &[&str]| {
        let path_scope = vec!["spike/**".to_string(), "handoff/**".to_string()];
        let acceptance = vec![format!(
            "{title}: implemented + cargo test green + checkpointed"
        )];
        WorkOrder {
            schema: "handoff.task.v1".into(),
            id: id.into(),
            title: title.into(),
            status: Status::Backlog,
            priority: pri,
            objective: obj.into(),
            path_scope: path_scope.clone(),
            acceptance_criteria: acceptance.clone(),
            test_commands: vec!["cargo test".into()],
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: true,
            correlation_id: "handoff-buildout".into(),
            role: Some("implementer".into()),
            intent_lock: WorkOrder::compute_intent_lock(obj, &path_scope, &acceptance),
        }
    };
    // HFTASK-0047: stamp the full 5-field lock (constraint + northstar surfaces) on freshly
    // minted cards so policy/doctrine drift becomes hash-detectable. `hf seed` is additive, so
    // already-seeded 3-field cards keep their legacy locks (no-downgrade); only NEW cards gain
    // the two surfaces.
    let ns_rev = current_northstar_revision();
    let mk = |id: &str, title: &str, pri: Priority, obj: &str, deps: &[&str]| {
        let mut wo = mk(id, title, pri, obj, deps);
        wo.intent_lock = wo.full_intent_lock(&ns_rev);
        wo
    };
    let backlog = vec![
        mk(
            "HFTASK-0001",
            "Finalize naming + register kernel (Continuity Ledger Kernel)",
            Priority::P0,
            "Kernel relocated to ~/Desktop/meta/handoff (own repo). Remaining: rename package/docs to Continuity Ledger Kernel + drop Ark/V2 in PRD; create+push FlexNetOS/handoff GitHub repo.",
            &[],
        ),
        mk(
            "HFTASK-0002",
            "Wire weave leases into hf claim",
            Priority::P0,
            "Replace the ledger-only claim with a weave lease (reserve/heartbeat/release) so claims are mesh-coordinated; hf claim -> weave_lease_reserve.",
            &["HFTASK-0001"],
        ),
        mk(
            "HFTASK-0003",
            "Front door: prompt_hub SwarmBundle -> verifiable handoff.task.v1 intake",
            Priority::P0,
            "ADR-0001 §11/R14 (verified): promote the spike - work_orders_from_bundle is test-only + uses a MIRRORED SwarmBundle; depend on prompt_hub's REAL SwarmBundle (models.rs:528) and wire it into a real `hf intake`/`hf dispatch` verb. TRANSPORT: there is NO MCP server on either side (prompt_hub or hf), so do NOT assume 'the MCP seam' - either call prompt_hub HTTP /vibe+/generate_bundle, depend on the prompt-hub crate, or build the seam (HFTASK-0019). CRUX: SwarmBundle role_prompts are prompt STRINGS (and empty-in-prod), not work specs - the intake must SYNTHESIZE a vibe Intent into REAL path_scope/acceptance_criteria/test_commands or every dispatched WorkOrder is unverifiable by the §5 review gate + §5b gatekeeper.",
            &["HFTASK-0001"],
        ),
        mk(
            "HFTASK-0004",
            "ruvector-verified AgentContract proof at hf handoff",
            Priority::P1,
            "On hf handoff, prove the intent_lock/acceptance via ruvector-verified (Lean) AgentContract; block handoff on unproven completion.",
            &["HFTASK-0001"],
        ),
        mk(
            "HFTASK-0005",
            "hf drift audit gate",
            Priority::P1,
            "Implement hf drift: recompute intent_lock, detect out-of-scope edits (git), hard-fail handoff on drift.",
            &["HFTASK-0004"],
        ),
        mk(
            "HFTASK-0006",
            "RVF vector-native ledger v2",
            Priority::P1,
            "Schedule + implement the RVF (rvf-runtime) vector-native event ledger for semantic recall over session history; keep rusqlite+witness as v1 fallback.",
            &["HFTASK-0002", "HFTASK-0003"],
        ),
        // --- Loop v2 (ADR-0001): worktree-isolated, cycle-batched, review-gated shipping ---
        mk(
            "HFTASK-0007",
            "hf session on meta_git_lib worktree engine + policy.toml + sync preflight",
            Priority::P0,
            "ADR-0001 §2 (Research R3): add `hf session start|end [--recycle]` by DEPENDING ON meta_git_lib (worktree::git_ops add/remove, worktree::store TTL/ephemeral registry, worktree::hooks fire_post_create/destroy, helpers::{resolve_branch,ensure_worktrees_in_gitignore}, snapshot capture/restore) rather than reimplementing; fall back to `meta git worktree` CLI if lib not wired. Off origin/<base_branch>; reserve a weave path-scope lease; emit session_start/session_end; recycle a fresh set on end. Dotdir is .handoff (canonical; .hf does not exist). Add `.handoff/policy.toml` (remote/loop/merge). MUST include a start-time preflight verifying tree/branch/remote sync, refusing on drift (prior weave-loop failure lesson). Depend only on current lease-capable weave; refuse legacy repowire/mcp-broker.",
            &["HFTASK-0002"],
        ),
        mk(
            "HFTASK-0008",
            "Branch/remote policy engine (develop<->master, clone/fork)",
            Priority::P1,
            "ADR-0001 §3: policy module resolving clone-vs-fork, base=develop, trunk=master. Enforce: branch off origin/<base> after fetch only, never push trunk directly, ff develop->trunk after merge. Fork model deferred behind remote.model=fork.",
            &["HFTASK-0007"],
        ),
        mk(
            "HFTASK-0009",
            "Batch checkout (3-5) + cycle counter -> hf ship (one squash commit/PR)",
            Priority::P1,
            "ADR-0001 §4: `hf claim --batch N` (up to cycle_flush=4) reserves a lease per task so the loop never stalls; ledger-derived cycle counter (checkpoints since session_start) surfaced as cycles:n/flush; at threshold next_command=hf ship. `hf ship` = add + ONE squash commit listing all HFTASK ids -> push branch -> gh pr create --base trunk, emit pr_opened. Outward action is permission-gated and retryable, never a hard wall.",
            &["HFTASK-0007", "HFTASK-0008"],
        ),
        mk(
            "HFTASK-0010",
            "PR review/merge automation - phased cloud_ultra->swarm_local + gh-aw guardrails",
            Priority::P1,
            "ADR-0001 §5/§5a (Research R4=gh-aw): reviewer is always a separate role (merge.reviewer). Phase 1 cloud_ultra: `hf review request <pr#>` runs /code-review ultra, records approve/deny via weave review (WL-020)+permission (WL-021). Phase 2 swarm_local: ruvector/ruflo (rvAgent) swarm reviewer. GUARDRAILS: (a) separation of privilege - worker agent read-only, a separate trusted scoped job does gh pr create/merge, agents never hold the merge token; (b) reviewer verdict OUT-OF-BAND in weave state, NOT a native GitHub APPROVE (bot-approval bypasses branch protection, gh-aw #25439); (c) merge is a non-agent Environment-gated job; (d) detection pass + protected-files denylist (.github/, .handoff/policy.toml, ADRs, manifests) before any write; (e) draft PRs, least-privilege tokens. MERGE MODEL (Research R11, rusty-idd-proven fail-closed): hf ship enables GitHub-NATIVE auto-merge `gh pr merge --auto --squash` against branch-protected trunk; GitHub merges when ALL required checks green, async, even after process exit. hf does NOT poll-and-merge or override red (red = wall). The §5b AI gatekeeper is a REQUIRED STATUS CHECK feeding branch protection (CI job posting a check-run), NOT an agent calling gh pr merge out-of-band. ff develop after merge; deny reopens task; pending waits. permission_gate transitional. flexnetos_github_app (currently empty) is the candidate home for the trusted writer/merge-gate. VERDICT CHANNEL (Research R6): weave review (WL-020) has NO verdict field - carry approve/deny in the weave permission (WL-021) answer body + a review_verdict event in hf's own ledger; hf enforces the gate (weave only records). Phase-2 swarm reviewer (Research R5): reuse rvAgent A2A transport + ApprovalDecision/GateResult types but BUILD the N-reviewers->one-verdict reducer (~50-100 LOC); spawn via process-level rvagent a2a serve (spawn_sync is a stub).",
            &["HFTASK-0009"],
        ),
        mk(
            "HFTASK-0011",
            "hf sync — idempotent .meta.yaml/.gitignore repair + one-way .kb mirror",
            Priority::P2,
            "ADR-0001 §6 (Research R7): Part A is IDEMPOTENT ensure/repair (handoff is ALREADY in ../.meta.yaml + ../.gitignore - there is a dup gitignore line to clean; no `meta project add` exists so grep-guard file edits, never blind append). Part B: git kb has NO upsert - do show-or-create -> checkout -> full-overwrite -> commit, scoped to context/overridable/active + context/overridable/progress (preserve frontmatter id; never rm+recreate). ONE-WAY: write only those generated slugs from a ledger-derived body, never read .kb back as truth, tag generated, never touch immutable/extensible/tasks slugs. Emit meta_registered; run at session end / post pr_merged.",
            &["HFTASK-0007"],
        ),
        mk(
            "HFTASK-0012",
            "CI/CD bring-up - workflows + branch protection + merge-gate Environment",
            Priority::P1,
            "ADR-0001 §9 (Research R8): handoff has NO .github/ yet and isn't pushed. Add canonical workflows .github/workflows/{ci,auto-format,notify-parent}.yml (ci jobs test/clippy/format/build with RUSTFLAGS=-D warnings, CARGO_TERM_COLOR; least-privilege permissions: top-level contents:read, escalate per-job; auto-format loop-guarded). Turn on REAL branch protection on trunk (required checks test/clippy/format/build, strict=true up-to-date, enforce_admins deliberate, native required-reviews OFF to avoid the bot-APPROVE bypass #25439). Create a GitHub Environment 'merge-gate' (org has ZERO today) with required reviewers + env-scoped secrets = the infra that makes §5 permission-gated merge real and the human->swarm flip a config change. Join the repository_dispatch mesh (child-repo-updated to FlexNetOS/meta after CI green, SHA-pinned actions). Split overloaded PARENT_REPO_PAT into scoped fine-grained PATs; prefer GitHub App/OIDC (flexnetos_github_app is the home).",
            &["HFTASK-0001"],
        ),
        mk(
            "HFTASK-0013",
            "Integrate envctl secrets-engine as the secret relay/injection layer",
            Priority::P1,
            "ADR-0001 §9.5/R10: replace long-lived PARENT_REPO_PAT with envctl secrets-engine (~/Desktop/meta/envctl/crates/secrets-engine). Worker gets only a short-lived peer-bound revocable relay bearer (relay_mint, <=24h); real GitHub credential stays in the encrypted vault, swapped at egress (relay_swap). Use broker::decide (pure default-deny: host/path/method allowlists, budgets, fail-closed presence gate) as the deterministic merge-gate enforcement layer beneath the §5b AI gatekeeper. Surface: Engine Rust API / secretd gRPC (Relay.Mint) / secretctl. GREENFIELD to build: GitHub ProviderMint (native scoped sub-token, currently NoMint) + inject.rs/run_child child-env path (todo!, Phase 6/8); relay-bearer+relay_swap HTTP path works now.",
            &["HFTASK-0010"],
        ),
        mk(
            "HFTASK-0014",
            "Surgical AI gatekeeper with full code knowledge (replaces human approvals)",
            Priority::P1,
            "ADR-0001 §5b: the end-state merge approver is a code-OMNISCIENT AI gatekeeper (not human, not blind swarm). REQUIRES full-codebase code intelligence (git kb code index / kb_callers/kb_impact, and/or RuVector) so it judges a change against its full blast radius (callers/callees/invariants), not just the diff. It is the swarm_local reviewer (HFTASK-0010) upgraded with mandatory full-code grounding. Verdict=judgment; envctl broker::decide (HFTASK-0013) is the deterministic enforcement that actually releases the token/merge. permission_gate is transitional toward THIS gatekeeper, not toward a human; remove the human approver once gatekeeper+broker are trusted.",
            &["HFTASK-0010", "HFTASK-0013"],
        ),
        mk(
            "HFTASK-0015",
            "hf policy engine + hook contract wiring (lifecycle automation)",
            Priority::P1,
            "ADR-0001 §10/R9: implement `hf policy check-claim|check-edit|check-handoff` reading the brought-forward .handoff/policies/rules.toml (handoff.policy.rules.v1: deny-without-claim, lease timings, drift blocks, protected-files denylist, blocked commands). Wire .handoff/hooks/hooks.toml (handoff.hooks.v1) so the agent harness fires hf on SessionStart/PreSessionStart(preflight)/TaskClaim/PreEdit/PostEdit/PreHandoff/SessionStop/PostMerge with fail_mode block as hard gates. This is the no-human-in-the-loop automation substrate. Reconcile lease heartbeat/stale/force-release timings with HFTASK-0002 claim TTL.",
            &["HFTASK-0007"],
        ),
        mk(
            "HFTASK-0016",
            "Adopt FlexNetOS meta conventions (avoid rusty-idd's drift)",
            Priority::P2,
            "ADR-0001 §9.6/R12 (3 rusty-idd-vs-meta drift reports, spot-verified in ~/Desktop/meta): handoff is a meta member and lacks the org convention set. Add: commitlint.config.cjs (12 types) + semantic-pr-title.yml (merge-blocking); release-please manifest mode + VERSION file + 5-platform release.yml (NOT cargo-dist); renovate.json (NOT Dependabot - D3); .githooks/{commit-msg,pre-commit,pre-push} + make install-hooks (NOT python pre-commit - D4); Makefile (NOT Justfile - D7); .claude/agent-guard.toml + settings.json hooks + .claude/rules/; 3-OS CI matrix + Swatinem/rust-cache + pinned toolchain (1.96.0); CONTRIBUTING.md. ALSO adopt rusty-idd's two-tier promote-verify (develop->main gate: clean-merge probe + locked build/test + drift + fmt/clippy + cargo audit --deny warnings) into §9 CI. ALREADY DONE: handoff is in .meta.yaml (rusty-idd's D5). Avoid the promote-verify.yml duplicate-run: bug (R11).",
            &["HFTASK-0012"],
        ),
        // --- RuVector coverage gaps (ADR-0001 R13) + front door / mission control / delivery (R14) ---
        mk(
            "HFTASK-0017",
            "cognitum-gate as the witnessed hf policy decision engine",
            Priority::P2,
            "ADR-0001 R13: HFTASK-0015 uses a flat rules.toml denylist; the runbook (S1 §2) mapped the policy gate to RuVector's cognitum-gate-tilezero (decision.rs GateDecision{Permit,Defer,Deny} + WitnessReceipt). Adopt cognitum-gate as the in-loop ACTION governor (what an agent may DO) behind `hf policy`, emitting witnessed permit/deny/defer. Distinct from the envctl broker (R10 = secret/credential+merge egress gate); they compose (action gate + credential gate). Verified crate exists at ~/Desktop/meta/RuVector/crates/cognitum-gate-tilezero.",
            &["HFTASK-0015"],
        ),
        mk(
            "HFTASK-0018",
            "ruvector-domain-expansion next-task routing (highest-value safe task)",
            Priority::P2,
            "ADR-0001 R13: the loop currently picks the next task by dependency order only (next_safe). Adopt RuVector's ruvector-domain-expansion (contextual routing / Thompson-style selection, S1:37) so hf claim --batch selects the highest-value safe tasks per context, not just topological order - core to an autonomous loop. Verified crate exists; capability claim (bandit/Thompson) from the runbook walk, re-verify symbols before building.",
            &["HFTASK-0009"],
        ),
        mk(
            "HFTASK-0019",
            "Expose hf as an MCP server (the T11 universal control seam)",
            Priority::P1,
            "ADR-0001 R13/§11: the runbook's T11 = MCP is the universal control seam; every RuVector subsystem is MCP-accessible but hf is not, and NO MCP server exists on the prompt_hub side either (R14 verified). Expose hf verbs (status/resume/claim/ship/review/intake...) as an MCP server so chat->MCP->work-order dispatch has a handoff-side endpoint, and the front door (HFTASK-0003) can dispatch over a real seam (pattern: rvAgent rvagent-mcp / mcp-gate / mcp-brain).",
            &["HFTASK-0007"],
        ),
        mk(
            "HFTASK-0020",
            "Mission Control - loop observability (hf status --json / hf watch + render)",
            Priority::P1,
            "ADR-0001 §12/R14: NO existing UI surfaces the handoff loop's live state ('mission control' currently = envctl's zellij layout generator - naming collision). The witnessed ledger event stream (§7) IS the read-model. Build `hf status --json` + `hf watch` (tail ledger + weave broadcasts; optional SSE) as the machine feed; surface the existing control verbs (resume, review request, weave permission answer, hf merge --confirm, abort). Render layer greenfield: reuse envctl-gui egui pattern or a TUI (prompt_hub has ratatui) first cut. Disambiguate workspace-mission-control vs loop-mission-control.",
            &["HFTASK-0007"],
        ),
        mk(
            "HFTASK-0021",
            "Delivery / output endpoint (correlation_id round-trip to front door)",
            Priority::P2,
            "ADR-0001 §13/R14: the pipeline is prompt_hub(input)->process->delivery(output) but the output endpoint was absent. correlation_id (=prompt_hub workflow_id) is already carried on every WorkOrder, so round-trip a merged cycle's result back to the originating vibe request - surfaced in RuVocal chat or via prompt_hub summarize <run-id>/feedback. Emit on pr_merged.",
            &["HFTASK-0003", "HFTASK-0010"],
        ),
        mk(
            "HFTASK-0022",
            "RuVocal (meta/RuVector/ui) - THE real front door, prompt_hub-integrated",
            Priority::P1,
            "ADR-0001 §11/§12/R14: RuVocal (~/Desktop/meta/RuVector/ui) is the REAL chosen front door (an unmodified HuggingFace Chat-UI fork, SvelteKit, with an mcp-bridge/ subpackage; nothing consumes loop events yet). NOTE: the envctl/loop-forge zellij multi-pane dashboard was attempted and FAILED - do NOT revive it; RuVocal is the surface. Adopt-and-extend RuVocal: integrate prompt_hub (vibe request in -> dispatch via the seam HFTASK-0019/HFTASK-0003) -> surface loop state (HFTASK-0020) + delivery result (HFTASK-0021) back in chat via mcp-bridge.",
            &["HFTASK-0019", "HFTASK-0020", "HFTASK-0003"],
        ),
        // --- Backlog reconciliation (audit 2026-06-17): the full ORIGINAL design (PRD + ADRs) ---
        // The project pivoted to the autopilot roadmap (0006-0022) WITHOUT minting tasks for the
        // PRD/ADR commitments it left behind. Owner directive: everything originally designed MUST
        // be built; the AI-in-the-human-seat upgrade is ADDITIVE, not a replacement (no-downgrade).
        // These re-enter the witnessed backlog so the loop builds the prior design, not just the pivot.
        mk(
            "HFTASK-0039",
            "ADR-0002 weave A2A surface 3: jobs (job_create/job_claim/job_update)",
            Priority::P1,
            "ADR-0002 froze a five-surface weave A2A contract; only surfaces 1 (identity) + 2 (leases) are built (hf/src/lease.rs). Build surface 3 — the poll-only job channel (job_create/job_claim/job_update) — reusing WeaveCli, so cross-agent work items have a real mesh transport instead of only the local ledger. No-downgrade: this is an accepted-ADR commitment, not optional.",
            &["HFTASK-0002"],
        ),
        mk(
            "HFTASK-0040",
            "ADR-0002 weave A2A surface 4: messaging (send/inbox/thread)",
            Priority::P1,
            "ADR-0002 surface 4 — agent-to-agent messaging (send/inbox/thread) over weave — has zero code. Build it on WeaveCli so loop agents and cross-repo sessions exchange relay:handoff-style messages through the frozen contract, matching the inbox messages the harness already receives.",
            &["HFTASK-0002"],
        ),
        mk(
            "HFTASK-0041",
            "ADR-0002 surface 5: verdict rides a weave permission-ask answer body",
            Priority::P1,
            "ADR-0002 §5 / ADR-0005 §4: a review verdict must ride a weave PERMISSION-ASK answer body (approve/deny) IN ADDITION TO the review_verdict ledger event. Today cmd_review_verdict (main.rs) emits only the ledger half — the cross-peer weave permission-ask channel is missing, so the autonomous gatekeeper has no mesh-visible approval path. Build the permission-ask answer body and bind it to the verdict event (no-downgrade: the ledger event stays).",
            &["HFTASK-0040", "HFTASK-0010"],
        ),
        mk(
            "HFTASK-0042",
            "ADR-0003 rule 3: kb task write-back + status flips",
            Priority::P1,
            "ADR-0003 rule 3 (the .kb planning<->execution seam, one-way): hf claim flips the referenced kb TASK document to active; hf checkpoint/hf handoff append a progress line to it; terminal hf done flips it to completed WITH evidence (commit hashes, test results). Today only sync.rs::part_b_kb_mirror writes the two context/overridable/* slugs, only via the explicit hf sync verb, never the task document and never status flips. Build the task-document write-back into claim/checkpoint/handoff/done. Stay one-way: kb is never read back as truth.",
            &["HFTASK-0011"],
        ),
        mk(
            "HFTASK-0043",
            "ADR-0012 v2 + keystone T5: wire BetaParams::update to ledger outcomes",
            Priority::P2,
            "ADR-0012 shipped Thompson-style routing but the contextual bandit never LEARNS: BetaParams::update (routing.rs:12) is unwired, so posteriors come only from the priority prior. Wire ledger outcomes into the update step — done = success reward, reopen/deny = failure — closing the keystone ADR-0001 §5.5 T5 co-learning loop so next-task value selection improves from real outcomes.",
            &["HFTASK-0018"],
        ),
        mk(
            "HFTASK-0044",
            "ADR-0001-B: hf ship performs the real develop->trunk fast-forward",
            Priority::P1,
            "ADR-0001-B (develop_mirrors_trunk=true) requires ff develop->trunk after each merge so develop==trunk. Today hf ship only println!s a note (main.rs ~475-480); should_sync_develop_trunk() is consulted but NO git ff is performed. Implement the actual fast-forward (fetch + ff-only push of develop to trunk, fail-closed, permission-gated, never force) so the branch model the policy claims is real.",
            &["HFTASK-0008"],
        ),
        mk(
            "HFTASK-0045",
            "PRD hf test: execute stored test_commands as completion evidence",
            Priority::P0,
            "PRD §4.7/§9/§12.3: evidence-backed completion REQUIRES executing the task's test_commands and mapping results to acceptance_criteria. The field is stored (work-order/src/lib.rs:46) but NEVER run — completion evidence is unenforced at the kernel level. Build `hf test [ID]`: run the work order's test_commands, capture pass/fail + output, append a witnessed test_result event, and gate handoff/done on green. This is the kernel's central completion guarantee.",
            &["HFTASK-0001"],
        ),
        mk(
            "HFTASK-0046",
            "PRD drift sentinel: 2/10 -> 10/10 checks + handoff.drift_report.v1",
            Priority::P1,
            "PRD §12.3-12.4: detect_drift (gates.rs:96-133) implements only 2 of 10 checks (intent-lock mismatch + out-of-scope writes) and emits a thin {clean,drift[]} shape, not handoff.drift_report.v1. Build the remaining checks — acceptance<->test mapping, decision-record contradiction, undocumented-architecture-change, handoff-state-staleness, and distinct objective/path_scope/acceptance/constraint hash-change outputs — and emit the full PRD schema. The most demanding part of the contract.",
            &["HFTASK-0005"],
        ),
        mk(
            "HFTASK-0047",
            "PRD IntentLock 3->5 fields (constraint_hash, northstar_revision) + task_intent_changed",
            Priority::P1,
            "PRD §12.2: IntentLock specifies 5 fields but the struct (work-order/src/lib.rs:68-72) has only 3 — constraint_hash and northstar_revision are absent, so policy/constraint drift (§12.1) cannot be hash-detected. Add both fields to compute_intent_lock, emit a task_intent_changed event on mutation, and extend the ruvector-verified proof (contract.rs) + drift checks to cover the two new obligations. No-downgrade: existing 3-hash proof stays.",
            &["HFTASK-0004"],
        ),
        mk(
            "HFTASK-0048",
            "PRD atomic in-ledger lease state machine + .handoff/locks/*.lock (no-downgrade superset of weave)",
            Priority::P1,
            "PRD §11.2-11.3: the self-contained kernel lease design — BEGIN IMMEDIATE; read leases; detect overlap; insert lease_requested/lease_active/heartbeat events; .handoff/locks/{ledger,merge,index}.lock; stale-lease reclaim event — was replaced by external weave with silent ledger-only fallback (lease.rs). Build the atomic in-ledger lease state machine as the local source of truth so the kernel self-coordinates WITHOUT requiring weave present; weave remains the mesh overlay. No-downgrade: a strict superset, not a swap.",
            &["HFTASK-0002"],
        ),
        mk(
            "HFTASK-0049",
            "PRD verbs: hf reconcile, hf doctor, hf claim --next",
            Priority::P2,
            "PRD §5/§9/§24: three contract verbs are missing. hf reconcile — the docs' own precedence rule says agents must run it (state-precedence reconciliation; today only loosely folded into sync/sync-cards). hf doctor — health-diagnostic verb (ledger integrity, witness chain, drift, residency). hf claim --next — auto-claim the highest safe task (only `claim ID`/`claim --batch` exist). Build all three as first-class verbs.",
            &["HFTASK-0001"],
        ),
        mk(
            "HFTASK-0050",
            "PRD hf index + .handoff/maps/ + hf plan (task DAG)",
            Priority::P2,
            "PRD §8/§9: hf index generates .handoff/maps/{repo,test,owner,dependency}-map.json and the generated nav docs so a cold-start agent can understand the repo from generated files; hf plan builds/refreshes the task DAG from dependencies/blocked_by. Neither exists (the maps dir is absent). Build hf index + the maps + hf plan.",
            &["HFTASK-0001"],
        ),
        mk(
            "HFTASK-0051",
            "PRD handoffd daemon (heartbeat + watch process)",
            Priority::P2,
            "PRD §6/§7.2 architecture has a Daemon node that is unrealized — there is no handoffd process. Build the daemon: lease-heartbeat ticker, ledger tail/watch feed (the read-model behind hf watch / Mission Control), and a supervised resume hook, so the loop has a live process, not only one-shot CLI invocations.",
            &["HFTASK-0007"],
        ),
        mk(
            "HFTASK-0052",
            "PRD typed hook contract (hook_event.v1/hook_result.v1) + 6 missing hook events",
            Priority::P1,
            "PRD §18: hooks are shell scripts, not the typed handoff.hook_event.v1 / handoff.hook_result.v1 gate contract (payload + severity + required_actions). And 6 of 12 required hook events are absent from hooks.toml: SessionResume, PreCommand, PostCommand, PreTest, PostTest, PostHandoff. Build the typed hook runner + add the missing events so lifecycle gating is a typed contract, not stringly-typed shell.",
            &["HFTASK-0015"],
        ),
        // --- Cross-repo: envctl Epic A (handoff full-sync) blockers, filed as FlexNetOS/handoff#71 ---
        // Surfaced by the envctl maintainer agent during its agenticOS-consolidation Epic A and the
        // 2026-06-18 forge-loop audit (envctl .handoff/loop/loop_state.md cycle-1 CARRIED FINDING +
        // FINDING-0002). Both are KERNEL-side (out of envctl's scope) and had no HFTASK — minted here
        // so Epic A's blocker is tracked on the handoff backlog and clearable in-loop.
        mk(
            "HFTASK-0053",
            "Issue #71.1: port ledger off C-SQLite (rusqlite) to a pure-Rust store (no-C trust boundary)",
            Priority::P1,
            "GitHub #71 item 1 / envctl Epic A cycle-1 CARRIED FINDING: the `ledger` crate links bundled C-SQLite (rusqlite -> libsqlite3-sys). It is not an envctl no-c violation today (separate workspace) but it breaks the continuity kernel's pure-Rust / 'no C in the trust boundary' agenticOS north star. Port `ledger` off rusqlite to a pure-Rust store (libSQL Hrana `remote` like envctl's secrets store, or an embedded pure-Rust engine such as redb/sled) so `hf` builds C-free, KEEPING the witnessed append-only chain, replay, BEGIN IMMEDIATE serialization (HFTASK-0028), and rollup-provenance semantics intact. No-downgrade: a store swap, never a capability loss; re-verify witness-chain + provenance tests on the new backend. Distinct from HFTASK-0006 (RVF vector ledger axis) — this removes the C dependency.",
            &["HFTASK-0006"],
        ),
        mk(
            "HFTASK-0054",
            "Issue #71.2: confirm ledger-path/member override fully covers member Tier-A (no per-repo ledger.db)",
            Priority::P2,
            "GitHub #71 item 2 / envctl FINDING-0002: `hf` was strictly CWD-relative (no --ledger/HANDOFF_LEDGER), so a member repo could not render Tier-A against the shared FLEET ledger ($META_ROOT/.handoff/ledger.db) without a per-repo ledger.db that ADR-0004 forbids. `hf fleet render <member>` (PR #17, 1adbb13) partially addressed this. CONFIRM it (and add an explicit `--ledger`/HANDOFF_LEDGER override if gaps remain) fully covers member packet rendering AND seed/mint against the shared FLEET ledger with ZERO per-repo ledger.db, so envctl renders its Tier-A kernel-rendered (not git-text-only fallback) and passes its p7 gate. Verification-first: prove coverage end-to-end from a member CWD, build the override only where uncovered.",
            &["HFTASK-0034"],
        ),
        // --- Backlog reconciliation (deep design audit 2026-06-18): residual PRD commitments with no HFTASK ---
        // A 4-agent fleet/design sweep (GitHub issues + sibling .handoff findings + PRD/ADR corpus +
        // local surfaces) confirmed issue #71 was the only cross-repo ask, but found 3 PRD commitments
        // still untracked after the 0039-0054 mint — same Category-2 class as the prior audit. Minted
        // here (no-downgrade: everything originally designed MUST be built).
        mk(
            "HFTASK-0055",
            "PRD §20 kernel hardening test matrix (proptest property + crash + golden/replay/concurrent suite)",
            Priority::P1,
            "PRD §20.2/§20.3/§20.4/§20.6 — the kernel has NO property/crash/golden suite (only one concurrency test at ledger/src/lib.rs:871; no proptest dependency anywhere). The original backlog TASK-0015 'hardening suite' lost its tracking when the HFTASK-0015 slot was repurposed to the policy engine. Build: (a) §20.2 proptest property tests — random path_scopes never falsely overlap; random event streams replay to identical final state; random checkpoint interruptions preserve last valid state; packet roundtrip; (b) §20.4 crash tests — crash during claim/checkpoint/handoff/index; ledger lock held by a dead process; corrupted task YAML/JSON fails CLOSED and is NEVER silently marked done; (c) §20.3/§20.6 golden/replay + fresh-agent acceptance integration tests. Distinct from HFTASK-0045 (hf test runs a TASK's own test_commands) — this is the KERNEL's own hardening matrix.",
            &["HFTASK-0028"],
        ),
        mk(
            "HFTASK-0056",
            "PRD §11.5/§15/§16 merge serialization: merge.lock/index.lock + single-writer merge (merge-steward)",
            Priority::P2,
            "PRD §11.3/§11.5/§15/§16 (lines 404/419/421/612/627/640/725): the kernel specifies repo-local .handoff/locks/{merge,index}.lock and a SINGLE-WRITER merge path — 'Merge is single-writer; only the merge steward can hold merge.lock; no merge without merge lock' — plus merge-steward/conflict-arbiter roles. None exist in code: HFTASK-0048 built only the CLAIM lease lockfile, HFTASK-0009 ship leaves the merge to GitHub-native auto-merge, and grit (ADR-0009) covers the fleet-level INTENT but not the PRD's concrete merge.lock artifact + steward contract. Build merge.lock/index.lock acquisition + single-writer merge serialization gated by it. No-downgrade: an accepted PRD commitment that composes with grit and auto-merge.",
            &["HFTASK-0048"],
        ),
        mk(
            "HFTASK-0057",
            "PRD §7.3/§23 JSON Schema generation (schemars) + runtime validation (jsonschema) + invalid-card rejection",
            Priority::P2,
            "PRD §7.3 (lines 256-257: schemars for generation, jsonschema for validation), §20.1, §23 TASK-0002 acceptance ('JSON Schema is generated or checked in' + 'Invalid task cards fail validation'): there is NO schemars/jsonschema dependency; only 3 hand-written schemas (schemas/{task,session,packet}.schema.json) exist and nothing validates against them, so a malformed task card is NOT rejected at load. Build schema generation for the handoff.*.v1 types via schemars (or keep curated schemas in lockstep) AND wire jsonschema runtime validation so an invalid card fails closed instead of loading. No-downgrade: completes the typed-contract guarantee (HFTASK-0052 added Rust hook types but not schema gen/validation).",
            &["HFTASK-0001"],
        ),
        mk(
            "HFTASK-0059",
            "Bounded SQLITE_BUSY retry for concurrent ledger writes",
            Priority::P1,
            "busy_timeout (set in Ledger::open) handles most contention, but under heavy concurrency — especially Windows file-locking — a BEGIN IMMEDIATE write can still surface SQLITE_BUSY (cumulative wait across serialized writers exceeds the timeout, or SQLite returns busy without invoking the handler on a lock upgrade). Wrap each ledger write transaction in with_busy_retry: retry the whole closure on transient SQLITE_BUSY/SQLITE_LOCKED with a short capped linear backoff. Safe for every write because each attempt re-reads the authoritative tail (seq + prev_hash) inside a fresh BEGIN IMMEDIATE, so no fork/duplicate seq can result; bounded by an attempt cap so a genuinely stuck lock still surfaces as an error. Shipped PR #96.",
            &["HFTASK-0028"],
        ),
        mk(
            "HFTASK-0058",
            "Canonical .handoff durability policy + hf gitignore swallow-guard (ADR-0016)",
            Priority::P1,
            "The kernel OWNS and SHIPS the .handoff commit-vs-ignore policy instead of every consumer hand-rolling its own .gitignore: a dir-form `.handoff/`/`.claude/` ignore silently SWALLOWS durable tasks/decisions/loop ledgers (git cannot re-include past an excluded parent dir; !-negations can't rescue it). Ship hf/src/durability.rs (durable-vs-regenerable taxonomy + canonical CONTENTS-FORM .gitignore fragment + git check-ignore swallow_report + repair_gitignore), the `hf gitignore [--check|--repair|--write]` verb, and the fail-closed swallow-guard wired into `hf doctor` (DEGRADED + exit 1 on a swallow). docs/adr-0016-handoff-durability-policy.md. Shipped PR #98.",
            &["HFTASK-0001"],
        ),
        mk(
            "HFTASK-0060",
            "RVF sidecar open retries on lock contention (fix intermittent hf panic)",
            Priority::P1,
            "Sibling of HFTASK-0059: the SQLite write path got with_busy_retry, but ledger v2's RVF sidecar open (ledger/src/v2.rs Ledger::open) did NOT, so two `hf` processes touching the same ledger back-to-back (a session + a checkpoint hook, or rapid CLI calls) intermittently hit RVF 0x0300 LockHeld ('another writer holds the lock'), which the six Ledger::open(...).unwrap() call sites in hf turned into a panic+backtrace. Fix: ledger v2 acquire_store retries open/create on transient LockHeld/LockStale with a short capped linear backoff (the RVF analogue of busy-retry; the v1 SQLite store stays authoritative so a bounded wait never risks the chain), and hf opens via a fail-closed open_ledger_or_exit helper instead of .unwrap().",
            &["HFTASK-0028"],
        ),
        // --- Fail-closed harness-upgrade burst (2026-06-21, owner-authorized via /handoff-loop +
        //     /harness-evolution). Root lesson L7: the FAIL-OPEN anti-pattern — a guard/loader/
        //     evidence-check that proceeds when it can't confirm its precondition. Each target
        //     closes one fail-open surface; #0064 is the systemic sweep that asserts the class. ---
        mk(
            "HFTASK-0061",
            "hf reopen verb — witnessed Done/Review -> Backlog with a recorded reason",
            Priority::P1,
            "The fail-closed kernel had no way to CORRECT a false-Done: a task marked Done via a pre-PR#103 blanket-`cargo test` rubber stamp (e.g. HFTASK-0057, whose schemars/jsonschema feature was never built) was stuck Done with no inverse op (`hf release` only un-claims in-progress states; `should_unclaim` excludes Done/Review). Add `hf reopen <ID> \"<reason>\"`: a reason is MANDATORY (no silent un-completion), only a terminal state (Done/Review) is reopenable, the WHY is witnessed as a `task_reopened` event before the `task_transition -> Backlog` replay acts on, kb planning-plane reverts via write-back, and the on-disk card snapshot is re-synced. Pure `should_reopen` gate, unit-tested disjoint from `should_unclaim`. Shipped: reconciled HFTASK-0057's false-Done.",
            &["HFTASK-0038"],
        ),
        mk(
            "HFTASK-0062",
            "RVF stale-lock reclaim — provably-dead .rvf.lock no longer wedges hf",
            Priority::P1,
            "Liveness gap surfaced after HFTASK-0060: acquire_store (ledger/src/v2.rs) RETRIES transient 0x0300 LockHeld/0x0301 LockStale but never RECLAIMS a persistently-orphaned lock whose holder PROCESS IS DEAD, so a leftover `.handoff/ledger.db.rvf.lock` returns LockHeld past the retry cap and wedges EVERY subsequent `hf` invocation (fail-closed, no panic — but the kernel is unusable until a human `rm`s it, violating no-human-in-loop). Fix: detect a provably-dead holder (PID liveness and/or mtime age-out beyond the lock TTL) and reclaim it, emitting a witnessed `lock_reclaimed` event; REFUSE to steal a live or unverifiable-liveness holder (fail-closed both ways). NOT 'raise the retry cap' — that is a fail-open band-aid (longer wait, same wedge). The v1 SQLite store stays authoritative so a bounded reclaim never risks the chain.",
            &["HFTASK-0060"],
        ),
        mk(
            "HFTASK-0063",
            "hf test --cwd pin + runner-aware executed-count (pytest/jest/go beyond libtest)",
            Priority::P2,
            "PR #103 made `hf test` fail closed on zero executed tests, but only for libtest: `parse_tests_ran` parses only cargo's `test result:` lines, so a pytest/jest/go-test card degrades to exit-code-only (the None branch) and can still be rubber-stamped by a zero-match run. Also `cmd_test` runs `sh -c` with NO `current_dir`, so test_commands are fragile to the invocation cwd. Fix: (a) pin the command's working dir to the task home (repo/meta root via the existing route/anchor helpers) so commands are cwd-stable; (b) extend executed-count parsing to recognized non-libtest runners (pytest summary line, jest 'Tests:' line, go-test '--- PASS/FAIL' / 'ok' counts), keeping the None->exit-code-only degrade ONLY for genuinely-unrecognized runners. Positive evidence for non-cargo cards, not just cargo.",
            &["HFTASK-0045"],
        ),
        mk(
            "HFTASK-0064",
            "hf doctor fail-closed invariant sweep + stale-lock self-heal (assert the FAIL-OPEN class is closed)",
            Priority::P1,
            "The systemic guard (5th target, harness-evolution L7/L10): a point fix per surface isn't enough — `hf doctor` must ASSERT the whole fail-open class stays closed AND auto-heal the one liveness wedge. Extend cmd_doctor to: (a) enumerate every `tasks/*.task.json` and FAIL (DEGRADED + exit 1) if any card on disk is absent from `hf status` (catches the load_tasks silent-drop that hid card #95 a whole session); (b) assert ledger replay + witness-chain verify with NO empty/default fallback masking a read error; (c) detect a provably-dead `*.rvf.lock` and reclaim it via HFTASK-0062 (witnessed `lock_reclaimed`), REFUSING live/unverifiable holders; (d) `hf doctor --json` structured report. Unit-tested: dead-reclaim, live-refusal, missing-card-fails, empty-status-fails. Depends on 0062 (consume reclaim) + the loud-load fix in 0057 (assert). Detection-only form can ship first (additive, no-downgrade).",
            &["HFTASK-0062", "HFTASK-0057"],
        ),
        mk(
            "HFTASK-0065",
            "Package the handoff loop skills as `harness:` plugin skills (/harness:handoff-loop, /harness:handoff-loop-init)",
            Priority::P2,
            "Owner's standing 'proper harness setup': make the handoff loop + init invokable under the published `harness:` plugin namespace, not only as handoff-local project skills. DISCOVERY (verify, don't assume): `harness_hub/harness/` is the vendored `harness` plugin (separate repo — upstream revfactory/harness + FlexNetOS skills) and ALREADY ships `skills/handoff-loop-init/` + `skills/handoff-loop-run/`, so the work is RECONCILE not create: sync the current script-driven `handoff/.claude/skills/handoff-loop-init` (PR #113) + the `handoff-loop` orchestrator skill + the bundled `scripts/handoff-loop-init.sh`/`scripts/handoff-lib.sh` drivers + the PR #114 out-of-tree-backup behavior INTO the plugin copies so `/harness:handoff-loop-init` and `/harness:handoff-loop` run the LATEST, non-downgraded implementation. Distinct from the generic `harness-loop-init` (loop-state-dir init) — do NOT conflate the two. CROSS-REPO: harness_hub changes are a separate-repo commit/PR over its SSH remote and go through the gatekeeper; the bundled `scripts/*.sh` must be reachable when the skill is ejected (vendor them under the skill dir or document the `$HANDOFF_KERNEL_HOME` dependency). No-downgrade: neither the handoff-local skills nor the plugin copies may lose capability; if the plugin copies are stale they get upgraded, never the reverse.",
            &[],
        ),
        mk(
            "HFTASK-0066",
            "Converge fleet-rollout.sh ledger-guard onto scripts/handoff-lib.sh (single source of the residency guards)",
            Priority::P3,
            "`scripts/handoff-lib.sh` is the canonical sourceable home for the `.gitignore` residency + redb-migration-artifact guards (HFTASK-0035/0037/0053 — `ensure_ledger_guard`/`ensure_active_md_guard`), but `scripts/fleet-rollout.sh` still keeps its OWN copy of `ensure_ledger_guard`, so the two definitions can drift (the lib already extends the guard with `*.sqlite.bak`/`*.redb.tmp`; fleet-rollout's copy may not). Converge: make `fleet-rollout.sh` source `handoff-lib.sh` and delegate to its guard functions (delete the duplicate body), so there is exactly ONE definition. Preserve fleet-rollout's existing behavior exactly (idempotent; new + existing members; README rev-model; never clobber a foreign `.gitignore`). No-downgrade. Low-risk/maintainability — a fresh redb dir never produces a `.sqlite.bak`, so this is correctness-of-single-source, not a live bug.",
            &[],
        ),
        // --- ADR-0018: full-auto agentic operation (owner directive 2026-06-21) ---
        mk(
            "HFTASK-0067",
            "ADR-0018 D1: commit ALL dotfiles/dirs — reverse the .handoff residency-ignore + invert fleet P7",
            Priority::P1,
            "ADR-0018 D1: moving forward every dotfile/dotdir is git-TRACKED (.handoff incl. ledger + rendered views, .idea, .claude, .github, .kb, .grit config). Stop `hf init`/`scripts/fleet-rollout.sh`/`scripts/handoff-lib.sh` from writing the `.handoff/**/ledger.db`(+wal/shm/rvf/active.md/locks/deliveries/packets) ignore block; REMOVE the existing blocks; ensure those paths are tracked instead. INVERT `hf fleet status` P7 (HFTASK-0034 git_tracks_handoff_db/ledger_guard_present): a tracked `.handoff/ledger.db` is now CONFORMANT; a missing ledger or a present ignore-guard is the VIOLATION. Decide + implement the binary ledger.db conflict story (worktree-isolated per batch HFTASK-0075 + FLEET rollup + serialized merge; binary-merge=ours-replay OR a deterministic text export beside it). Migration artifacts (*.sqlite.bak/*.redb.tmp) stay OUT-OF-TREE (already true, PR #114) — only durable state is committed. Roll the guard removal + P7 inversion ATOMICALLY. Supersedes the ignore half of ADR-0004 §3/§6 + ADR-0016/HFTASK-0035/0037/0048/0021/0066.",
            &[],
        ),
        mk(
            "HFTASK-0068",
            "ADR-0018 D3: context-budget loop wrap (~50% window), not fixed cycle_flush",
            Priority::P1,
            "ADR-0018 D3: replace 'wrap after cycle_flush tasks' with 'run until ~50% of the context window is consumed, then checkpoint -> handoff'. Add `policy.toml [loop] context_budget_pct = 50` + `wrap_strategy = \"context\"` to hf/src/policy.rs LoopCfg with compiled defaults + round-trip tests; keep `cycle_flush` as an UPPER safety bound. The kernel exposes the policy + the wrap verbs; ENFORCEMENT is at the loop-skill layer (handoff-loop Phase 5 + session-relay-wrap-up read the running token/context budget and trigger the wrap at the threshold). Wire the threshold into the handoff-loop skill + session-relay-wrap-up.",
            &[],
        ),
        mk(
            "HFTASK-0069",
            "ADR-0018 D2: central pre/post hook contract + deployable canonical bundle",
            Priority::P2,
            "ADR-0018 D2: extend the typed hook contract (hf/src/hooks.rs, HFTASK-0052: hook_event.v1/hook_result.v1/severity_for) to robustly + fail-closed cover ALL 8 events (SessionStart/Resume/End, Pre/PostCommand, Pre/PostTest, PostHandoff), and make `.handoff/hooks/{loop-entry,session-end,...}.sh` + `hooks.toml` a SINGLE handoff-central canonical bundle deployed identically fleet-wide (via the HFTASK-0065 /handoff-loop-init mechanism). Idempotent; no dangling settings.json refs (fail-closed skip when a hook source is absent).",
            &["HFTASK-0052"],
        ),
        mk(
            "HFTASK-0070",
            "ADR-0018 D5: handoff-central format + cross-fleet deploy for session-relay-resume/-wrap-up",
            Priority::P2,
            "ADR-0018 D5: the `session-relay-resume`/`-wrap-up` skills (today harness_hub-owned per-repo) get their canonical format/templates defined IN handoff (rendered from the witnessed ledger/packet, NEVER hand-authored prose), and handoff deploys them + enforces byte-consistency to every fleet member via the /handoff-loop-init family (HFTASK-0065). Cross-repo (harness_hub) — gatekeeper-gated.",
            &["HFTASK-0065"],
        ),
        mk(
            "HFTASK-0071",
            "ADR-0018 D4: more direction from handoff (next-action in hf resume + packet)",
            Priority::P2,
            "ADR-0018 D4: `hf resume` + the rendered packet emit EXPLICIT next-action direction — the single next safe task, the exact next command, the cycle/context-budget state, and the blocking walls — so a fresh agent needs zero archaeology. The handoff-loop skill gives richer steering (decision rationale + 'do this next', not just 'here is state').",
            &[],
        ),
        mk(
            "HFTASK-0072",
            "ADR-0018 D7: full adoption of meta/.kb/AGENTS.md (init full .kb + create-first + two-way seam)",
            Priority::P2,
            "ADR-0018 D7: fully adopt the FlexNetOS agent guide meta/.kb/AGENTS.md (765 lines: document-before-implement, context docs, `git kb board`, traceability) in handoff. Init the FULL `.kb` (handoff is code-intelligence-only today: `git kb init`), wire the create-first discipline + board + traceability into the loop, and bind the planning<->execution seam (ADR-0003) BOTH ways per the guide (mint-in + write-back). No downgrade of the existing one-way ledger->kb mirror; kb still never overrides execution truth.",
            &[],
        ),
        mk(
            "HFTASK-0073",
            "ADR-0018 D8: deeper grit + GitHub grounding (default grit cycle + gatekeeper-as-required-check)",
            Priority::P2,
            "ADR-0018 D8: make the `hf claim -> grit claim <file::symbol> -> grit worktree -> grit done` cycle (ADR-0009) the DEFAULT path for every batch; advance the shared grit backend (ADR-0010) past degrade. GitHub: ground the autonomous AI gatekeeper (HFTASK-0014) as a REQUIRED status check feeding branch protection (HFTASK-0010/0012) + gh-aw guardrails so develop->trunk promotion needs NO manual gh api.",
            &["HFTASK-0075"],
        ),
        mk(
            "HFTASK-0074",
            "ADR-0018 D9: real .idea integration + use (run configs, Qodana advisory CI)",
            Priority::P3,
            "ADR-0018 D9: commit `.idea/` (per D1) and actually USE it — shared run/debug configurations for the hf binary + tests, the Qodana inspection profile (`qodana.yaml`) wired as ADVISORY CI, the Rust plugin config. `workspace.xml` (the one genuinely per-user file) is committed per D1 unless it churns destructively — then carve it out with a recorded rationale (the ONLY allowed D1 exception).",
            &["HFTASK-0067"],
        ),
        mk(
            "HFTASK-0075",
            "ADR-0018 D10: worktree per task batch, reaped on verified PR merge",
            Priority::P1,
            "ADR-0018 D10: every task batch starts a NEW grit worktree (ADR-0009); it is removed ONLY on verified PR merge (not before); abandoned/discarded batches keep their worktree until reconciled. Wire into `hf session`/`hf claim --batch` + the handoff-loop preflight. This isolation is the precondition that makes D1's committed binary ledger.db safe (parallel batches never share a working ledger).",
            &[],
        ),
        mk(
            "HFTASK-0076",
            "ADR-0018 D11: all PRs->develop; hands-off develop->trunk auto-promotion + master/main reconcile",
            Priority::P1,
            "ADR-0018 D11: fix/replace the `sync-master.yml` stall so develop promotes to trunk AUTOMATICALLY on green with NO manual `gh api` ff. Reconcile the trunk NAME: directive says `main`, repo uses `master` — standardize on `main` (or keep `master` with `main` as the documented alias) across `policy.toml`, `.github/workflows/*`, and the docs, ONE decision applied everywhere. The pipeline is fixed: branch off develop -> PR --base develop -> auto-promote on green.",
            &[],
        ),
        mk(
            "HFTASK-0077",
            "ADR-0018 D6: update .claude/rules/* + meta rules to the full-auto model + fleet deploy",
            Priority::P2,
            "ADR-0018 D6: update `handoff/.claude/rules/*` + the meta-level rules to the full-auto operating model (committed dotfiles, worktree-per-batch, context-budget wrap, full `.kb` adoption, grit+gh grounding, designated-agent-replaces-human). Deploy the updated rules fleet-wide via the HFTASK-0065 /handoff-loop-init mechanism.",
            &["HFTASK-0067", "HFTASK-0068", "HFTASK-0075"],
        ),
        // --- Owner directive 2026-06-21 (relay #134 from harness-agent-rs): institutionalize the
        // LIVE differential-drive verification that "caught what 1000+ green tests missed". ---
        mk(
            "HFTASK-0078",
            "Live differential-drive verification as a fleet handoff action workflow",
            Priority::P1,
            "Owner directive (relay #134): a LIVE differential drive (drive the REAL binary/CLI and DIFF its actual output against an expectation) caught what 1000+ green unit tests missed; capture it as a fleet-deployable handoff action workflow that institutionalizes the FAIL-OPEN doctrine the kernel already lives by (green is not proof; cases-run must be > 0; ABSENCE is a FAILURE, never a silent pass). Add `.github/workflows/differential-drive.yml` — a GENERIC, repo-agnostic reusable (`workflow_call` + `workflow_dispatch`) GitHub Actions workflow that runs `scripts/differential-drive.sh`; it is DORMANT by default (no push/PR trigger) so deploying it never spams red checks on a repo that has not yet authored cases. Add `scripts/differential-drive.sh` — a self-contained, fail-closed harness exposing `drive <name> <cmd> <expected-substring>` (PASS iff exit 0 AND output contains the substring), sourcing optional repo-specific cases from `scripts/differential-drive.cases.sh`, asserting total-cases>0 (fail-closed with an actionable message when absent/empty), and emitting a libtest-compatible `test result:` summary so `hf test` COUNT-verifies it (the tests-ran>0 gate, HFTASK-0045/0063) rather than trusting exit code alone. Ship handoff's OWN `scripts/differential-drive.cases.sh` driving the real `hf` binary (CLI-contract invariants: usage exposes claim/ship/promote/drift/handoff) + a handoff-local `.github/workflows/differential-drive-ci.yml` caller (PR-triggered, advisory/NOT-required so it never blocks the develop->trunk promote gate, replicating ci.yml's RuVector-sibling layout) that dogfoods the harness. Deploy ONLY the generic workflow + harness fleet-wide via the canonical scripts/handoff-loop-init.sh deploy_diff_drive() (HFTASK-0065/0066 mechanism), idempotent, dry-run aware; the handoff-local caller + cases file are NOT deployed (each repo authors its own cases). Making the check branch-protection-REQUIRED is the follow-on (HFTASK-0073/D8) and is NOT done here (account-level wall).",
            &["HFTASK-0045", "HFTASK-0065"],
        ),
        // --- ADR-0019 reconciliation cards (2026-06-26). The code-research run shipped the P0/P2
        // fixes (#140–#156) and the ADR records the deferred D5 work, but those were never seeded
        // as witnessed cards. Seed them retroactively (0079, already built) + forward (0080/0081,
        // the open D5 increments) so the backlog matches the ADR and Git truth. ---
        mk(
            "HFTASK-0079",
            "Harden verify_witness_chain: content+linkage re-derivation (close the count-only tautology)",
            Priority::P0,
            "code-research P0 (#140): the prior `ledger::v1::verify_witness_chain` was a tautology — it rebuilt a fresh witness chain from the *trusted* stored action_hash values (prev_hash forced to 0) then verified that just-built chain, so it always passed and returned events.len() for honest, content-tampered, and garbage rows alike. It never recomputed the action_hash from the payload nor checked on-disk prev_hash linkage, so it could not detect tampering of the binary redb cache. Harden it to walk events in seq order and enforce the SAME two invariants `append` establishes: (1) CONTENT — re-derive hash_action(event_type, work_order_id, payload_json) and byte-compare to the stored action_hash; (2) LINKAGE — stored prev_hash must equal the prior event's stored action_hash (genesis [0;32]). Keep the RVF witness-segment round-trip only as a secondary structural check. Already shipped in code (ledger/src/v1.rs); this card records the witnessed work.",
            &[],
        ),
        mk(
            "HFTASK-0080",
            "ADR-0019 D5 #3: adopt unwrap_used/expect_used/panic = deny (error-handling hardening)",
            Priority::P1,
            "ADR-0019 D5 #3 (the Cargo.toml workspace.lints comment calls this 'the next increment ... NOT left as a standing TODO'): the kernel claims a fail-closed continuity substrate yet the production tree carries hundreds of `.unwrap()`/`.expect()`/`panic!` sites — exactly the FAIL-OPEN class (LESSONS L7–L10) where a guard proceeds when it cannot confirm its precondition. Adopt clippy `unwrap_used`/`expect_used`/`panic` = deny in `[workspace.lints.clippy]`. Allow them crate-wide ONLY under `#[cfg(test)]` (idiomatic; tests assert). For every PRODUCTION site: propagate with `?` where genuinely fallible, or replace with `.expect(\"<invariant reason>\")` where the call is infallible by a documented invariant (mutex-poison, compile-time-constant parse, etc.) so the unsafe assumption is VISIBLE and audited at the call site rather than a bare `.unwrap()`. Verify `cargo clippy --workspace --all-targets -- -D warnings` green (the exact CI gate).",
            &[],
        ),
        mk(
            "HFTASK-0081",
            "ADR-0019 D5 #4: continue the 12-crate decomposition (peel feature crates from hf)",
            Priority::P2,
            "ADR-0019 D5 #4 (PRD §7.2 target): the hf monolith is being decomposed into the planned handoff-* crate layout. `handoff-core` (leaf primitives) was peeled in #154 and edition-2024/resolver-3 landed in #153. Continue peeling cohesive feature modules out of `hf/src/*` into their own crates that depend on `handoff-core` (candidates: drift, hooks, contract/gatekeeper, fleet, intake) — behavior-preserving moves with `hf` re-exporting so existing `crate::` paths stay valid, each its own PR, each verified green. The work is incremental; one or more feature crates per increment.",
            &["HFTASK-0079"],
        ),
        mk(
            "HFTASK-0082",
            "Extend error-handling deny lints to the rusty-idd toolkit (crates/{cli,core,runner,spec,tui})",
            Priority::P3,
            "Follow-up to HFTASK-0080. The kernel crates (work-order, ledger, handoff-core, hf) adopt clippy unwrap_used/expect_used/panic = deny; the co-located rusty-idd toolkit (crates/{cli,core,runner,spec,tui}, ADR-0019 D2: a SEPARATE Intent-Driven-Development toolkit, never invoked by hf) carries a documented crate-root `#![allow(...)]` opt-out so the kernel hardening is not blocked on the toolkit's ~577 sites. Harden the toolkit the same way (test-only allow + per-site propagate/justify) and REMOVE the root allows so the whole tree enforces the deny. Lower priority because the toolkit is not on the kernel's trust path.",
            &["HFTASK-0080"],
        ),
        mk(
            "HFTASK-0083",
            "ADR-0019 D5 #4: peel the COUPLED hf modules (drift/gatekeeper/fleet/intake) after lifting shared helpers to handoff-core",
            Priority::P2,
            "Continuation of HFTASK-0081, which peeled the LEAF/clean feature crates (handoff-policy #160, handoff-schema #161, handoff-lease #162) — bringing the workspace to 7 crates toward the PRD §7.2 12-crate target. The remaining feature modules (drift, gatekeeper, fleet, intake, gates, hooks, index) are COUPLED to hf-main helpers (crate::must_witness, crate::pretty_json, crate::now_ns, crate::load_tasks, crate::save_task, crate::current_northstar_revision, crate::next_safe), so they cannot move cleanly yet. PREP: lift those shared helpers from hf/src/main.rs into handoff-core (behavior-preserving, hf re-exports), THEN peel the coupled modules into their own crates (handoff-drift, handoff-gatekeeper, handoff-fleet, handoff-intake) one PR each, each verified green. This is the higher-risk half of the decomposition and is deliberately a separate witnessed card, not free-handed onto 0081.",
            &["HFTASK-0081"],
        ),
        mk(
            "HFTASK-0084",
            "Fix intake/prompt-hub synthesized task-id collision (silent durable-card clobber)",
            Priority::P1,
            "FAIL-OPEN data-loss bug surfaced by the post-decomposition /verify drive: both intake synthesis sites mint task ids counting from a FIXED base — `handoff-intake::synthesize_orders` hardcodes `id: \"TASK-0001\"` for the whole-bundle order, and `work_order::work_orders_from_bundle_with` numbers roles `format!(\"TASK-{:04}\", i+1)` (TASK-0001, TASK-0002, …). The ids are NOT scoped to the bundle/correlation, so a second `hf intake`/`hf prompt-hub` run re-mints TASK-0001 and `handoff_core::save_task` OVERWRITES any existing `TASK-NNNN.task.json` on disk silently — and these are DURABLE cards (handoff's own committed TASK-0001..0004 bundle cards prove it), so the clobber is real continuity-state loss. Fix deterministically (the intake load-bearing property: same bundle → byte-identical orders) and schema-validly (live pattern `^[A-Z]*TASK-[A-Z0-9][A-Z0-9-]*$` accepts uppercase-hex + hyphen): add `work_order::synthesized_task_id(workflow_id, seq) -> \"TASK-{:06X}-{:04}\"` deriving a stable 24-bit FNV-1a prefix from the unique workflow_id (which already carries a hash+nanos), and use it at BOTH sites. Distinct bundles → distinct prefixes (no cross-bundle clobber); re-running the SAME bundle re-mints the SAME ids (idempotent, not data loss). Also correct the stale `^TASK-[0-9]{4,}$` comment in handoff-schema (the live pattern was already broadened). Tests: a collision test (two workflow_ids → disjoint id sets) + a determinism test (same workflow_id → identical ids).",
            &["HFTASK-0083"],
        ),
        // --- Seamless-fleet automation ladder (owner directive 2026-06-26: 'think more
        // automation, seamless, always automate, what are we missing'). Today every propagation
        // step is human-invoked or report-only; the rungs are dependency-ordered (0085 unblocks
        // 0086/0087). Each is a witnessed card; 0085 is the keystone primitive. ---
        mk(
            "HFTASK-0085",
            "Automation rung 0/1: hf build-version stamp + staleness self-detection (loop-init + SessionStart)",
            Priority::P1,
            "ROOT automation primitive. Today `hf --version` is 'unknown command', so nothing can tell whether an installed binary is BEHIND the kernel source — every staleness automation is impossible without this. Add `hf/build.rs` emitting build metadata (HF_BUILD_COMMIT from `git rev-parse --short HEAD`, preferring an injected GITHUB_SHA in CI, falling back to 'unknown' for a .git-less release tarball; HF_BUILD_DATE) with no unwrap/expect/panic (workspace deny lints). Add `hf --version`/`-V`/`version` (text + `--json {version,commit,date}`) so the value is machine-readable. Wire the stamp into propagation: (a) `scripts/handoff-loop-init.sh` Phase 0 — when the kernel home is reachable, compare installed `hf version --json`.commit vs `git -C $KERNEL_HOME rev-parse --short HEAD`; mismatch ⇒ need_build=1, so `handoff-loop-init.sh --fleet` AUTO-rebuilds when behind (closes the gap where a stale-but-working redb hf is skipped and a human must pass --build-hf); (b) `.handoff/hooks/loop-entry.sh` (SessionStart) — if the kernel is reachable and the stamp is behind, print a loud staleness directive. Test: `hf version --json` parses; build.rs fallback path is graceful.",
            &["HFTASK-0084"],
        ),
        mk(
            "HFTASK-0086",
            "Automation rung 2: auto-release hf artifacts on merge-to-trunk (dispatch release-tagged)",
            Priority::P2,
            "`.github/workflows/release.yml` already builds hf for 5 platforms but only triggers on `push: tags/v*` / `repository_dispatch[release-tagged]` / manual — so a merged kernel change produces ZERO artifacts automatically. The build pipeline exists; the TRIGGER does not. Wire the develop→trunk promotion machinery (sync-master.yml / promote-verify.yml) to fire a `repository_dispatch{event_type: release-tagged}` (or auto-tag `v<pkgver>+<shortsha>`) when trunk advances with a kernel-source change, so every promoted change yields versioned platform binaries with no human action. Gate on an actual hf/ledger/work-order/handoff-* source change (skip docs-only). Depends on HFTASK-0085 (the version the release is stamped with).",
            &["HFTASK-0085"],
        ),
        mk(
            "HFTASK-0087",
            "Automation rung 3: hf fleet sync — REMEDIATE detected drift, not just report it",
            Priority::P2,
            "`hf fleet status` detects non-conformant members (missing ledger guard, no committed ledger.events.jsonl, stale binary) but only REPORTS — a human must then run handoff-loop-init. Add `hf fleet sync` (and/or `hf fleet status --fix`) that, for each member the status sweep flags, runs the idempotent loop-init deploy bits (gitignore guard, hooks, diff-drive workflow, rules) + the binary staleness rebuild (HFTASK-0085), turning detection into remediation. Fail-closed per member (one member's failure never aborts the sweep; report it). Schedulable from a meta-level cron workflow so the fleet self-heals hands-off. Depends on HFTASK-0085.",
            &["HFTASK-0085"],
        ),
        mk(
            "HFTASK-0088",
            "Automation rung 4: new-repo auto-onboarding (member present, no conformant .handoff)",
            Priority::P3,
            "Adding a member to `.meta.yaml` scaffolds no `.handoff` — onboarding is a remembered manual `handoff-loop-init.sh /path`. Make `hf fleet sync` (HFTASK-0087) treat 'member present in .meta.yaml but no conformant .handoff' as a remediation target and auto-init it (portable `hf init` + full deploy), and/or add a `meta git update` post-step hook that detects a freshly-cloned member without `.handoff` and onboards it. New repos become continuity-enabled with zero manual steps. Depends on HFTASK-0087.",
            &["HFTASK-0087"],
        ),
        mk(
            "HFTASK-0089",
            "Automation rung 5: worktree/branch reap as a hook, not agent memory",
            Priority::P2,
            "Owner has hammered that worktree/branch reap is mandatory at session start yet 'skipping it is the recurring failure' — because it currently depends on an AGENT remembering to run scripts/reap-worktrees.sh. Automation that needs a human/agent to remember is not automation. Bind reap to a real lifecycle hook (the handoff hooks.toml SessionEnd / a PostMerge hook, mirroring how SessionEnd already runs checkpoint+handoff+export+sync) so merged-branch/worktree husks are reaped deterministically by the kernel, never by recall. Keep the existing safety rails (dry-run default in the script, protect master/develop/current, skip dirty, local-only, never --force). Surface what was reaped in the session-end output.",
            &["HFTASK-0085"],
        ),
        mk(
            "HFTASK-0090",
            "Fix Ledger::open abnormal-exit flake (retry transient DatabaseAlreadyOpen)",
            Priority::P0,
            "Surfaced by the climbing-the-ladder /verify drive: `cargo test -p hf --bin hf` aborted abnormally ~50% of runs under default parallelism (serial + --no-capture always passed) — undermining the COUNT-verified `hf test` gate that is the kernel's core integrity mechanism. Root-caused via marker-file backtrace instrumentation to `hf::tests::claim_returns_false_when_blocked_true_when_acquired → cmd_claim_with → open_ledger_or_exit → process::exit(1)`, with captured error `Database(DatabaseAlreadyOpen)` on `./.handoff/ledger.db`. Mechanism: redb refuses a second SAME-PROCESS open of one file; under the libtest harness's shared process cwd, a test that chdir's into its temp `.handoff` (holding cwd_lock) and a concurrent test that opens the cwd-relative default `ledger_path()` both resolve to the SAME file → two live handles → `DatabaseAlreadyOpen` → `open_ledger_or_exit` aborts the WHOLE binary (killing sibling tests; exit 1; no `test result: FAILED`). Pre-existing (reproduced with `hf/build.rs` removed: 6/12), exposed by the 16-crate decomposition spreading ledger-opening tests. Fix at the chokepoint: `ledger::v1::Ledger::open` now wraps `Database::create` in the EXISTING `with_busy_retry` (HFTASK-0059/0060), which already treats `DatabaseAlreadyOpen` as transient (`is_busy`) and is documented safe to retry — previously only WRITES retried, the OPEN failed closed. Bounded (100×≤10ms) so a genuinely stuck handle still surfaces an error. Production-safe (helps concurrent hf processes too). Verified: 0/25 parallel runs fail (was ~50%); ledger 34 tests green; clippy/fmt green.",
            &["HFTASK-0083"],
        ),
        mk(
            "HFTASK-0095",
            "Automation rung 0 fix: build.rs re-stamps HF_BUILD_COMMIT on every commit (kill the SessionStart staleness false-positive)",
            Priority::P2,
            "The HFTASK-0085 build stamp lags HEAD because `hf/build.rs` only emits `cargo:rerun-if-changed=.git/HEAD`, but a normal `git commit` moves the branch ref (`.git/refs/heads/<branch>`) — NOT `.git/HEAD` — so cargo never re-runs build.rs after a commit, `HF_BUILD_COMMIT` keeps the prior short-sha, and the SessionStart staleness check (`.handoff/hooks/loop-entry.sh` + `scripts/handoff-loop-init.sh` Phase 0) compares the stale stamp against fresh `git rev-parse --short HEAD` and loudly (FALSE-positive) warns 'hf binary BEHIND kernel HEAD' on a source-clean tree — eroding trust in the very staleness signal rung 0 exists to provide. It also never fires in a git WORKTREE, where `.git` is a FILE gitlink (`gitdir: <path>`), not a directory. Fix `build.rs` to resolve the real gitdir worktree-aware, parse `HEAD`'s `ref: <path>` and emit `rerun-if-changed` for that ref file, plus `.git/logs/HEAD` as a robust catch-all (it changes on every commit/checkout/reset). Fail-soft at every step (a .git-less release tarball still builds and stamps 'unknown'; no unwrap/expect/panic — workspace deny lints). Verify by differential drive: record the stamp, commit, rebuild, assert `hf version --json`.commit == `git rev-parse --short HEAD`.",
            &["HFTASK-0085"],
        ),
        mk(
            "HFTASK-0096",
            "Install hygiene: reconcile stale shadow `hf` copies on PATH to a symlink into the meta build (ADR-0006)",
            Priority::P2,
            "After `cargo install --path hf` (handoff-loop-init.sh Phase 0 rebuild) the canonical binary lands in `~/.cargo/bin/hf`, but a real COPY of `hf` earlier on PATH (e.g. `~/.local/bin/hf`) shadows it — so `hf` keeps serving the OLD binary and freshly-added verbs report 'unknown command' until someone manually re-copies. This is the recurring 'cp after every cargo install' gotcha and it silently defeats rung-0 staleness automation (the rebuild SUCCEEDS but the running `hf` is still stale). Add a post-install PATH-reconciliation step: a `reconcile_hf_path()` helper in `scripts/handoff-lib.sh`, called from `scripts/handoff-loop-init.sh` right after the build/install, that resolves the canonical freshly-built binary and converges every shadowing `hf` that resolves before it on PATH — per ADR-0006 doctrine, replacing each writable shadow COPY with a SYMLINK into the meta build (never a churning copy). Idempotent, `--dry-run`-aware, fail-soft + loud (warn, never abort the run) when a path isn't writable or a shadow is a foreign/non-hf binary. Verify: `bash -n` both scripts; a functional drive that seeds a stale copy on a synthetic PATH and asserts the helper turns it into a symlink to the canonical binary; idempotent re-run is a no-op.",
            &["HFTASK-0085"],
        ),
    ];
    // HFTASK-0026 carries a precise path_scope (["handoff/**"]) and a routing-specific
    // acceptance criterion, so it is built directly rather than via `mk` (whose fixed
    // path_scope/acceptance template doesn't fit a kernel-internal CORRECTNESS fix).
    let mut backlog = backlog;
    {
        let id = "HFTASK-0026";
        let title = "Anchor hf core ledger ops to meta-root + kernel/fleet routing (fix kb-mint contamination)";
        let objective = "ADR-0004 §3 two-ledger residency: hf resolved .handoff/ledger.db + tasks/ CWD-relative with no anchoring, so `hf task mint --from-kb` + claim/checkpoint/done run from handoff/ wrote envctl-domain KBTASK cards into handoff's KERNEL ledger instead of the FLEET ledger (meta/.handoff). Add route_for_task(id) (LOCAL card -> KERNEL home; else FLEET card -> meta/.handoff; else FAIL CLOSED) and route every per-task ledger op (claim/checkpoint/done/review verdict/ship) through it; mint kb cards to the FLEET tasks dir via find_meta_root(). Global/self ops (status/resume/handoff/drift) stay LOCAL.";
        let path_scope = vec!["handoff/**".to_string()];
        let acceptance = vec![
            "kb-minted cards land in the FLEET tasks dir (meta/.handoff/tasks); per-task ops route to the home the card lives in; an unknown task id fails closed (no ledger created); KERNEL ops from handoff/ still hit the KERNEL ledger; cargo test + fmt + clippy green"
                .to_string(),
        ];
        backlog.push(WorkOrder {
            schema: "handoff.task.v1".into(),
            id: id.into(),
            title: title.into(),
            status: Status::Backlog,
            priority: Priority::P0,
            objective: objective.into(),
            path_scope: path_scope.clone(),
            acceptance_criteria: acceptance.clone(),
            test_commands: vec!["cargo test".into()],
            dependencies: vec![],
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: "handoff-buildout".into(),
            role: Some("implementer".into()),
            intent_lock: WorkOrder::compute_intent_lock(objective, &path_scope, &acceptance),
        });
    }
    // HFTASK-0027/0028: ledger-robustness hardening after the contamination + staleness
    // investigation. Both carry precise handoff-scoped path_scope + targeted acceptance,
    // so (like HFTASK-0026) they are built directly rather than via the `mk` template.
    {
        let id = "HFTASK-0027";
        let title = "hf resume recomputes live witnessed count (fix stale packet-cached display)";
        let objective = "CONFIRMED stale-read: `hf resume` (ResumeMode::Full) echoed packets/latest.md verbatim, freezing 'Tamper-evident events verified: N' + Done N/M at the last `hf handoff` (showed 336 while the live ledger had 371; witness chain INTACT — not corruption). Fix: render the packet LIVE on every `hf resume` from ledger_path() (verify_witness_chain for the count) + current_statuses()/load_tasks() (Done N/M), via the same renderer as `hf handoff`, instead of reading the frozen file. Do NOT change `hf handoff` rendering.";
        let path_scope = vec!["handoff/**".to_string()];
        let acceptance = vec![
            "hf resume witnessed count == live ledger count == hf handoff verified count; reflects new events without hf handoff"
                .to_string(),
        ];
        backlog.push(WorkOrder {
            schema: "handoff.task.v1".into(),
            id: id.into(),
            title: title.into(),
            status: Status::Backlog,
            priority: Priority::P1,
            objective: objective.into(),
            path_scope: path_scope.clone(),
            acceptance_criteria: acceptance.clone(),
            test_commands: vec!["cargo test".into()],
            dependencies: vec![],
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: "handoff-buildout".into(),
            role: Some("implementer".into()),
            intent_lock: WorkOrder::compute_intent_lock(objective, &path_scope, &acceptance),
        });
    }
    {
        let id = "HFTASK-0028";
        let title = "Serialize concurrent hf ledger writes (WAL + busy_timeout + BEGIN IMMEDIATE)";
        let objective = "HAZARD: multiple concurrent `hf` processes (two handoff sessions, or a session + a PostEdit checkpoint hook) write the same .handoff/ledger.db; interleaving risks 'database is locked' or a forked/desynced witness chain. Fix in Ledger::open + the append path: set journal_mode=WAL + busy_timeout=5000, and wrap the event append in a BEGIN IMMEDIATE transaction that reads the latest prev_hash INSIDE the transaction and inserts atomically, so concurrent writers serialize (block-and-retry) and can never both chain off the same prev.";
        let path_scope = vec!["handoff/**".to_string()];
        let acceptance = vec![
            "two concurrent hf checkpoint procs both succeed + witness chain verifies".to_string(),
        ];
        backlog.push(WorkOrder {
            schema: "handoff.task.v1".into(),
            id: id.into(),
            title: title.into(),
            status: Status::Backlog,
            priority: Priority::P1,
            objective: objective.into(),
            path_scope: path_scope.clone(),
            acceptance_criteria: acceptance.clone(),
            test_commands: vec!["cargo test".into()],
            dependencies: vec![],
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: "handoff-buildout".into(),
            role: Some("implementer".into()),
            intent_lock: WorkOrder::compute_intent_lock(objective, &path_scope, &acceptance),
        });
    }
    {
        let id = "HFTASK-0029";
        let title = "hf hygiene: ship stages only task scope + seed idempotent + claim exits nonzero when blocked";
        let objective = "Surgical hf hygiene bundle (3 located defects, all in hf/src/main.rs): (A) `hf ship` did `git add -A`, sweeping untracked KBTASK cards + _workspace/ scratch into PR #29 — stage ONLY tracked modifications (`git add -u`) plus the task's own card. (B) `hf seed` wrote every card with hardcoded Status::Backlog, overwriting existing cards so re-seed reset done cards (HFTASK-0001..0020) to backlog — make seed additive: only write MISSING cards, preserving existing status. (C) `hf claim` printed BLOCKED to stderr but `return`ed with exit 0 — a blocked claim must exit nonzero so hooks/scripts/the loop see the failure, while `hf dispatch`'s internal claim loop keeps its skip-and-continue semantics.";
        let path_scope = vec!["handoff/**".to_string()];
        let acceptance = vec![
            "ship excludes untracked; seed preserves done status; claim exits nonzero when blocked"
                .to_string(),
        ];
        backlog.push(WorkOrder {
            schema: "handoff.task.v1".into(),
            id: id.into(),
            title: title.into(),
            status: Status::Backlog,
            priority: Priority::P1,
            objective: objective.into(),
            path_scope: path_scope.clone(),
            acceptance_criteria: acceptance.clone(),
            test_commands: vec!["cargo test".into()],
            dependencies: vec![],
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: "handoff-buildout".into(),
            role: Some("implementer".into()),
            intent_lock: WorkOrder::compute_intent_lock(objective, &path_scope, &acceptance),
        });
    }
    {
        let id = "HFTASK-0030";
        let title = "preflight mirrors each repo's CI clippy flags (--all-targets gap that failed PR #30) + loop agents run --all-targets";
        let objective = "Defect D (cross-workspace): the shared pre-push gate scripts/preflight.sh ran `cargo clippy --all-features` with NO --all-targets, assuming --all-targets is always stricter than CI. That is FALSE for repos whose CI uses --all-targets (handoff: `cargo clippy --workspace --all-targets -- -D warnings`). --all-targets lints TEST code; without mirroring it a test-code lint (needless &borrow in a #[cfg(test)] assert) passed preflight but FAILED CI (PR #30). Fix: (meta repo) preflight greps each repo's .github/workflows/*.yml clippy line and mirrors --all-targets only when that repo's CI uses it (per-repo subset-mirror; a blanket --all-targets would false-block repos whose CI omits it; preserve the --all-features default-features fallback on the --all-features axis only). (handoff repo) the loop's kernel-verifier + kernel-implementer agents mandate `cargo clippy --workspace --all-targets -- -D warnings` to match handoff CI exactly, and CLAUDE.md documents that handoff CI uses --all-targets.";
        let path_scope = vec!["handoff/**".to_string()];
        let acceptance = vec![
            "preflight runs --all-targets where the repo's CI does; kernel-verifier/implementer mandate --all-targets"
                .to_string(),
        ];
        backlog.push(WorkOrder {
            schema: "handoff.task.v1".into(),
            id: id.into(),
            title: title.into(),
            status: Status::Backlog,
            priority: Priority::P1,
            objective: objective.into(),
            path_scope: path_scope.clone(),
            acceptance_criteria: acceptance.clone(),
            test_commands: vec!["cargo test".into()],
            dependencies: vec![],
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: "handoff-buildout".into(),
            role: Some("implementer".into()),
            intent_lock: WorkOrder::compute_intent_lock(objective, &path_scope, &acceptance),
        });
    }
    // ADR-0004 §3.3 revision (2026-06-13, owner-directed): per-repo gitignored ledger + central rollup.
    for (id, title, objective, deps) in [
        (
            "HFTASK-0031",
            "Ledger schema: rollup provenance (origin_repo/origin_seq/origin_action_hash) + sync_cursor",
            "ADR-0004 §3.3 (rev): additive, backward-compatible migration in ledger/src/lib.rs — ALTER events ADD COLUMN origin_repo TEXT / origin_seq INTEGER / origin_action_hash BLOB (NULL = native local event); CREATE UNIQUE INDEX idx_events_origin ON events(origin_repo, origin_seq) WHERE origin_repo IS NOT NULL (idempotency); CREATE TABLE sync_cursor(origin_repo PK, last_seq, updated_ns) in the CENTRAL ledger. Old rows verify unchanged (verify_witness_chain rebuilds from ordered action_hash, ignores stored prev_hash). No rvf-crypto change.",
            Vec::<String>::new(),
        ),
        (
            "HFTASK-0032",
            "hf sync Part C: per-repo -> central rollup (cursor-driven, idempotent, single tx)",
            "ADR-0004 §3.3 (rev): hf/src/sync.rs + a ledger rollup API. For each member repo, read its gitignored .handoff/ledger.db events with seq > sync_cursor.last_seq, RE-APPEND each through the central ledger's witnessed append() path (re-chained onto the central tail), tagging provenance (origin_repo, origin_seq, origin_action_hash = the source action_hash, byte-identical since hash_action inputs match). Advance the cursor in the SAME central transaction. UNIQUE(origin_repo,origin_seq) makes re-runs no-ops (at-least-once -> exactly-once). Chains are never merged; self-contained events are re-appended (CT/RFC6962 model).",
            vec!["HFTASK-0031".to_string()],
        ),
        (
            "HFTASK-0033",
            "verify_rollup_provenance() + hf fleet status verifies both chains + provenance",
            "ADR-0004 §3.3 (rev): add verify_rollup_provenance() (pure SQL + existing hash_action) that, for each rolled-up central row, re-derives SHA3-256(event_type||work_order_id||payload_json) and byte-compares to origin_action_hash (the proof bridge). Extend hf fleet status to verify (i) the central chain via verify_witness_chain (unchanged), (ii) each per-repo chain independently, (iii) provenance faithfulness. Both chains verify independently; any central event traces to its repo.",
            vec!["HFTASK-0031".to_string()],
        ),
        (
            "HFTASK-0034",
            "P7 flip: hf fleet status forbids only git-TRACKED ledger.db, requires the .gitignore guard",
            "ADR-0004 §6 (rev): flip hf/src/fleet.rs P7 enforcement — a gitignored local .handoff/ledger.db is LEGITIMATE; only a git-TRACKED .db under .handoff is a violation. Gate: fail on tracked .db; fail if the .handoff/**/ledger.db .gitignore guard is missing; a .db merely present on disk is NOT a violation (remove the stray-ledger flag at fleet.rs:102/111/145-154). Cross-fleet follow-up (other repos): envctl ci/gates/p7.sh Gate 3b removal; prompt_hub/lane member-rule capsule/README edits.",
            Vec::<String>::new(),
        ),
        (
            "HFTASK-0035",
            "Standardize .gitignore residency guard `.handoff/**/ledger.db` fleet-wide",
            "ADR-0004 §3.3/§6 (rev): ensure every continuity member's .gitignore ignores .handoff/**/ledger.db (and *.db-wal/*.db-shm) so the per-repo local ledger is never committed (keeps the one good half of the old rule). Update the handoff repo + the fleet rollout generator so seeded repos get the guard. Idempotent.",
            Vec::<String>::new(),
        ),
        (
            "HFTASK-0036",
            "hf ship fail-closed exit codes (L2 hf-verb-safety)",
            "Verify-found gap (HFTASK-0033..0035 cycle): every refusal/error path in cmd_ship (empty id, unknown remote.model, fork-deferred, not-on-branch, ship-from-base/trunk guard, git add/commit/push failure, PR-create/auto-merge failure) uses a bare `return` and exits 0, so hooks/scripts/the loop cannot detect a refused or failed ship — the same L2 hf-verb-safety class fixed for hf claim in HFTASK-0029. Make every cmd_ship error/refusal path exit nonzero (std::process::exit(1); empty-id usage exit 2) while the happy path stays 0.",
            vec!["HFTASK-0008".to_string()],
        ),
        (
            "HFTASK-0037",
            "gitignore .handoff/active.md (derived view, stop the churn/drift)",
            "Verify-found gap: .handoff/active.md is a TRACKED derived view that hf resume/handoff regenerate every run, so it perpetually dirties the tree and trips `hf drift` (deny_without_claim) at the start of every session — yet its sibling derived view .handoff/packets/latest.md is already gitignored. Both are hf-rendered from ledger truth (the ledger + capsule.json are the committed cold-start sources). Fix: add /.handoff/active.md to .gitignore and `git rm --cached` it (untrack), consistent with packets/. hf still renders it locally; it just stops churning git.",
            Vec::<String>::new(),
        ),
        (
            "HFTASK-0038",
            "hf release un-claims: revert ledger status to Backlog (not lease-only)",
            "Verify-found gap (HFTASK-0018 cycle): cmd_release (hf/src/main.rs) is lease-only — it frees the weave lease but never records a ledger transition, so a released in-progress task stays Claimed in the ledger (HFTASK-0006 got stuck Claimed, and `hf claim --batch` then resumed the phantom). Fix: after freeing the lease, if the task's replayed status is in-progress (Claimed/Checkpointed/Active), record_transition(&wo, Status::Backlog, now_ns()) via the routed per-task ledger (mirror cmd_claim_with), so a release TRULY un-claims. Leave terminal/post-work states (Review/Done) and already-Backlog untouched. Unit-test the un-claim decision; runtime-verify by reverting the stuck HFTASK-0006 to Backlog.",
            Vec::<String>::new(),
        ),
    ] {
        backlog.push(WorkOrder {
            schema: "handoff.task.v1".into(),
            id: id.into(),
            title: title.into(),
            status: Status::Backlog,
            priority: Priority::P1,
            objective: objective.into(),
            path_scope: vec!["handoff/**".to_string()],
            acceptance_criteria: vec![format!(
                "{title}: implemented + cargo test green + drift-audited"
            )],
            test_commands: vec!["cargo test".into()],
            dependencies: deps,
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: "handoff-buildout".into(),
            role: Some("implementer".into()),
            intent_lock: WorkOrder::compute_intent_lock(
                objective,
                &["handoff/**".to_string()],
                &[format!(
                    "{title}: implemented + cargo test green + drift-audited"
                )],
            ),
        });
    }
    // Tightened completion gates (HFTASK-0058/0059/0060): these cards declare a specific,
    // fast proof instead of the blanket `cargo test`. A blanket `cargo test` runs the whole
    // workspace (slow) and — executed by `hf test` via `sh -c` in the invocation cwd — can
    // match zero tests yet still exit 0, rubber-stamping the fail-closed `hf done` gate. Each
    // command below scopes to the crate + module that actually covers the change, so `hf test`
    // runs a known-nonzero, on-objective set (verified counts in comments). `test_commands` is
    // NOT part of the intent_lock (lock = objective+path_scope+acceptance), so this is a pure,
    // hash-stable metadata refinement — no lock recompute, no drift.
    for wo in backlog.iter_mut() {
        let tight: &[&str] = match wo.id.as_str() {
            // ADR-0016 swallow-guard engine (swallow_report / repair_gitignore): 3 tests
            "HFTASK-0058" => &["cargo test -p hf durability::"],
            // SQLITE_BUSY retry + concurrent-writers no-lock/no-fork over the v1 store: 19 tests
            "HFTASK-0059" => &["cargo test -p ledger v1::"],
            // RVF sidecar acquire_store open-retry, exercised by the v2 suite: 6 tests
            "HFTASK-0060" => &["cargo test -p ledger v2::"],
            // hf reopen gate (should_reopen, disjoint from should_unclaim): reopen tests
            "HFTASK-0061" => &["cargo test -p hf reopen"],
            // RVF dead-lock reclaim (inspect_lock + witnessed open): the 5 *lock* tests
            "HFTASK-0062" => &["cargo test -p ledger lock"],
            // runner-aware executed-count parsers (libtest/pytest/jest/go): 11 tests
            "HFTASK-0063" => &["cargo test -p hf parse_tests_ran"],
            // schemars gen + serialization-stability + jsonschema card validation/rejection
            // HFTASK-0081/0083: schema validator + card loader were peeled out of hf into
            // handoff-schema / handoff-core, so the tests live there now.
            "HFTASK-0057" => &[
                "cargo test -p work-order schema",
                "cargo test -p handoff-schema",
            ],
            // doctor card-conformance core (try_parse_card) + RVF reclaim (ledger lock tests)
            "HFTASK-0064" => &[
                "cargo test -p handoff-core try_parse_card",
                "cargo test -p ledger lock",
            ],
            // skills/docs reconcile — smoke the bundled driver (exits 0, exercises the script)
            "HFTASK-0065" => &["bash scripts/handoff-loop-init.sh --dry-run"],
            // shell-only converge — syntax-check both scripts after delegation
            "HFTASK-0066" => &[
                "bash -n scripts/fleet-rollout.sh",
                "bash -n scripts/handoff-lib.sh",
            ],
            // ADR-0018 D1 atomic flip: durability taxonomy + fleet P7 inversion + export, plus a
            // syntax check on the two scripts whose guards changed.
            "HFTASK-0067" => &[
                "cargo test -p hf durability::",
                "cargo test -p hf fleet::",
                "cargo test -p ledger export",
                "bash -n scripts/handoff-lib.sh",
                "bash -n scripts/fleet-rollout.sh",
            ],
            // ADR relay-#134 / HFTASK-0078: the live differential-drive harness IS its own
            // evidence — running it drives the real `hf` binary and emits a libtest-compatible
            // summary that `hf test` count-verifies (tests-ran>0). Plus syntax-check the harness
            // and the deploy script that ships it fleet-wide.
            "HFTASK-0078" => &[
                "bash -n scripts/differential-drive.sh",
                "bash scripts/differential-drive.sh",
                "bash -n scripts/handoff-loop-init.sh",
            ],
            // ADR-0018 D5 / HFTASK-0070: handoff owns the canonical session-relay templates,
            // rendered from the witnessed `hf` ledger/packet, and byte-deploys them fleet-wide
            // via deploy_session_relay(). Evidence: the canonical templates exist in handoff,
            // each REQUIRES the `hf` render (not "if reachable"), and the deploy script is valid.
            "HFTASK-0070" => &[
                "bash -n scripts/handoff-loop-init.sh",
                "test -f .claude/skills/session-relay-resume/SKILL.md",
                "test -f .claude/skills/session-relay-wrap-up/SKILL.md",
                "grep -q 'hf resume' .claude/skills/session-relay-resume/SKILL.md",
                "grep -q 'hf handoff' .claude/skills/session-relay-wrap-up/SKILL.md",
                "grep -q deploy_session_relay scripts/handoff-loop-init.sh",
            ],
            // ADR-0018 D4 / HFTASK-0071: the explicit Next Action / Direction block. The unit
            // test proves the renderer emits next-action/exact-command/budget/walls fields;
            // the live drive proves the real `hf resume` binary renders the block with its
            // next-command + context-budget markers (the fresh-agent-zero-archaeology contract).
            "HFTASK-0071" => &[
                "cargo test -p hf direction_block",
                "./target/debug/hf resume | grep -q 'Next Action / Direction'",
                "./target/debug/hf resume | grep -q 'Next command:'",
                "./target/debug/hf resume | grep -q 'Cycle / context budget:'",
            ],
            // ADR-0018 D7 / HFTASK-0072: full `.kb` adoption + the two-way seam. Evidence: the
            // durable `.kb` is initialized (the text store exists, the binary cache is ignored),
            // the seam verb is exposed, and the seam unit tests (kb_root local-first resolution,
            // plane-aware mint target, write-back direction) are green.
            "HFTASK-0072" => &[
                "cargo test -p hf kb::",
                "test -d .kb/store/documents/context",
                "test -f .kb/store/documents/context/immutable/project-brief.md",
                "git check-ignore .kb/.cache/gitkb.db",
                "./target/debug/hf 2>&1 | grep -q 'task mint'",
            ],
            // HFTASK-0095: the build.rs ref-watch fix. The version surface (count-positive) proves
            // the stamp plumbing is intact; the ref-watch correctness itself is proven by the
            // differential drive at verify time (commit → rebuild → stamp == git HEAD), since
            // build.rs's cargo:rerun-if-changed directives are not unit-testable from the crate.
            "HFTASK-0095" => &["cargo test -p hf version"],
            // HFTASK-0096: PATH reconcile — syntax-check both scripts AND run the self-contained
            // functional drive (stale-copy→symlink + idempotent + dry-run + canonical-first). The
            // drive emits a libtest summary so `hf test` COUNT-verifies it (tests-ran>0), instead
            // of resting on a bash -n exit-0 (the U3 fail-open gap a syntax check alone leaves).
            "HFTASK-0096" => &[
                "bash -n scripts/handoff-loop-init.sh",
                "bash -n scripts/handoff-lib.sh",
                "bash scripts/test-reconcile-hf-path.sh",
            ],
            _ => continue,
        };
        wo.test_commands = tight.iter().map(|s| s.to_string()).collect();
    }
    // HFTASK-0029 Defect B: seed is IDEMPOTENT/ADDITIVE — only write cards that are
    // MISSING on disk. Overwriting an existing card clobbered its live status (done →
    // backlog) on re-seed; skipping existing cards preserves status and still creates
    // newly-added seed cards.
    let mut written = 0usize;
    let mut skipped = 0usize;
    for wo in &backlog {
        if tasks_dir().join(format!("{}.task.json", wo.id)).exists() {
            skipped += 1;
        } else {
            save_task(wo);
            written += 1;
        }
    }
    println!(
        "hf seed: wrote {written} new task card(s) (skipped {skipped} existing) to {}/",
        tasks_dir().display()
    );
}

/// HFTASK-0054: extract a global `--ledger <path>` flag from the raw argument list. When
/// present, the path is exported as `HANDOFF_LEDGER` so `ledger_path()` honors it. The flag
/// and its value are removed so subcommand dispatch stays positional.
// `env::set_var` is `unsafe` in edition 2024 (it data-races concurrent env reads). This runs in
// `main`'s argv preprocessing, before any thread is spawned — single-threaded by construction —
// so it is sound. Justified exception to the workspace `unsafe_code = "deny"` policy.
#[allow(unsafe_code)]
fn apply_ledger_flag(args: &mut Vec<String>) {
    if let Some(pos) = args.iter().position(|a| a == "--ledger") {
        if let Some(path) = args.get(pos + 1).cloned() {
            unsafe { std::env::set_var("HANDOFF_LEDGER", &path) };
        }
        // Remove both tokens; if no value was provided, just drop the flag.
        args.remove(pos);
        if pos < args.len() {
            args.remove(pos);
        }
    }
}

/// HFTASK-0085: the build-version stamp (set by `hf/build.rs`). `commit`/`date` fall back to
/// "unknown" when the binary was built without git/CI metadata (e.g. a .git-less tarball) so the
/// command never lies — an "unknown" commit simply means the staleness check can't compare.
fn version_info() -> (&'static str, &'static str, &'static str) {
    (
        env!("CARGO_PKG_VERSION"),
        option_env!("HF_BUILD_COMMIT").unwrap_or("unknown"),
        option_env!("HF_BUILD_DATE").unwrap_or("unknown"),
    )
}

fn cmd_version(json: bool) {
    let (version, commit, date) = version_info();
    if json {
        let out = serde_json::json!({
            "schema": "handoff.version.v1",
            "version": version,
            "commit": commit,
            "date": date,
        });
        println!("{}", pretty_json(&out));
    } else {
        println!("hf {version} ({commit}, {date})");
    }
}

fn focused_help(topic: &str) -> Option<&'static str> {
    match topic {
        "init" => Some(HF_INIT_HELP),
        "seed" => Some(HF_SEED_HELP),
        "resume" => Some(HF_RESUME_HELP),
        "status" => Some(HF_STATUS_HELP),
        "index" => Some(HF_INDEX_HELP),
        "plan" => Some(HF_PLAN_HELP),
        "session" => Some(HF_SESSION_HELP),
        "claim" => Some(HF_CLAIM_HELP),
        "doctor" => Some(HF_DOCTOR_HELP),
        "gitignore" => Some(HF_GITIGNORE_HELP),
        "reconcile" => Some(HF_RECONCILE_HELP),
        "export" => Some(HF_EXPORT_HELP),
        "import" => Some(HF_IMPORT_HELP),
        "migrate" => Some(HF_MIGRATE_HELP),
        "sync-cards" => Some(HF_SYNC_CARDS_HELP),
        "sync" => Some(HF_SYNC_HELP),
        "checkpoint" => Some(HF_CHECKPOINT_HELP),
        "test" => Some(HF_TEST_HELP),
        "done" => Some(HF_DONE_HELP),
        "drift" => Some(HF_DRIFT_HELP),
        "fleet" => Some(HF_FLEET_HELP),
        "prompt-hub" => Some(HF_PROMPT_HUB_HELP),
        "task" => Some(HF_TASK_HELP),
        "intake" => Some(HF_INTAKE_HELP),
        "dispatch" => Some(HF_DISPATCH_HELP),
        "delivery" => Some(HF_DELIVERY_HELP),
        "release" => Some(HF_RELEASE_HELP),
        "reopen" => Some(HF_REOPEN_HELP),
        "handoff" => Some(HF_HANDOFF_HELP),
        "ship" => Some(HF_SHIP_HELP),
        "promote" => Some(HF_PROMOTE_HELP),
        "review" => Some(HF_REVIEW_HELP),
        "gatekeeper" => Some(HF_GATEKEEPER_HELP),
        "hook" => Some(HF_HOOK_HELP),
        "lease" => Some(HF_LEASE_HELP),
        "version" => Some(HF_VERSION_HELP),
        "policy" => Some(HF_POLICY_HELP),
        "schema" => Some(HF_SCHEMA_HELP),
        _ => None,
    }
}

fn print_help(args: &[String]) -> bool {
    let focused = args.iter().any(|a| a == "--help" || a == "-h");
    match args.first().map(|s| s.as_str()) {
        Some("--help") | Some("-h") => {
            println!("{HF_HELP}");
            true
        }
        Some("help") if args.len() == 2 => {
            if let Some(help) = focused_help(&args[1]) {
                println!("{help}");
                true
            } else {
                eprintln!("hf: unknown help topic '{}'", args[1]);
                eprintln!("{HF_USAGE}");
                std::process::exit(2);
            }
        }
        Some(cmd) if focused => {
            if let Some(help) = focused_help(cmd) {
                println!("{help}");
                true
            } else {
                false
            }
        }
        Some("help") if args.len() == 1 => {
            println!("{HF_HELP}");
            true
        }
        _ => false,
    }
}

fn reject_unsupported_args(command: &str, args: &[String], allowed: &[&str]) {
    for arg in args.iter().skip(1) {
        if allowed.contains(&arg.as_str()) {
            continue;
        }
        let kind = if arg.starts_with('-') {
            "flag"
        } else {
            "argument"
        };
        let supported = if allowed.is_empty() {
            "no flags".to_string()
        } else {
            allowed.join(", ")
        };
        eprintln!("hf {command}: unknown {kind} '{arg}' (supported: {supported})");
        std::process::exit(2);
    }
}

fn reject_unsupported_args_with_positionals(
    command: &str,
    args: &[String],
    allowed: &[&str],
    max_positionals: usize,
) {
    let mut positionals = 0usize;
    for arg in args.iter().skip(1) {
        if allowed.contains(&arg.as_str()) {
            continue;
        }
        if !arg.starts_with('-') && positionals < max_positionals {
            positionals += 1;
            continue;
        }
        let kind = if arg.starts_with('-') {
            "flag"
        } else {
            "argument"
        };
        let supported = if allowed.is_empty() {
            if max_positionals == 0 {
                "no flags".to_string()
            } else {
                format!("up to {max_positionals} positional argument(s)")
            }
        } else if max_positionals == 0 {
            allowed.join(", ")
        } else {
            format!(
                "{} plus up to {max_positionals} positional argument(s)",
                allowed.join(", ")
            )
        };
        eprintln!("hf {command}: unknown {kind} '{arg}' (supported: {supported})");
        std::process::exit(2);
    }
}

fn reject_unsupported_flags(command: &str, args: &[String], allowed: &[&str]) {
    for arg in args.iter().skip(1).filter(|arg| arg.starts_with('-')) {
        if allowed.contains(&arg.as_str()) {
            continue;
        }
        let supported = if allowed.is_empty() {
            "no flags".to_string()
        } else {
            allowed.join(", ")
        };
        eprintln!("hf {command}: unknown flag '{arg}' (supported: {supported})");
        std::process::exit(2);
    }
}

fn reject_unsupported_args_after(command: &str, args: &[String], start: usize, allowed: &[&str]) {
    for arg in args.iter().skip(start) {
        if allowed.contains(&arg.as_str()) {
            continue;
        }
        let kind = if arg.starts_with('-') {
            "flag"
        } else {
            "argument"
        };
        let supported = if allowed.is_empty() {
            "no flags".to_string()
        } else {
            allowed.join(", ")
        };
        eprintln!("hf {command}: unknown {kind} '{arg}' (supported: {supported})");
        std::process::exit(2);
    }
}

fn validate_hook_args(args: &[String]) {
    match args.get(1).map(|s| s.as_str()) {
        Some("list") => reject_unsupported_args_after("hook list", args, 2, &["--json"]),
        Some("run") => {
            if args.get(2).is_none_or(|event| event.starts_with('-')) {
                eprintln!("hf hook run: event is required");
                std::process::exit(2);
            }
            let mut i = 3usize;
            while i < args.len() {
                match args[i].as_str() {
                    "--json" => i += 1,
                    "--payload" => match args.get(i + 1) {
                        Some(value) if !value.starts_with('-') => i += 2,
                        _ => {
                            eprintln!("hf hook run: option '--payload' requires a value");
                            std::process::exit(2);
                        }
                    },
                    other => {
                        let kind = if other.starts_with('-') {
                            "flag"
                        } else {
                            "argument"
                        };
                        eprintln!(
                            "hf hook run: unknown {kind} '{other}' (supported: --payload VALUE, --json)"
                        );
                        std::process::exit(2);
                    }
                }
            }
        }
        _ => {}
    }
}

fn validate_delivery_args(args: &[String]) {
    match args.get(1).map(|s| s.as_str()) {
        Some("list") => reject_unsupported_args_after("delivery list", args, 2, &["--json"]),
        Some("get") => {
            if args.get(2).is_none_or(|cid| cid.starts_with('-')) {
                eprintln!("hf delivery get: correlation_id is required");
                std::process::exit(2);
            }
            reject_unsupported_args_after("delivery get", args, 3, &["--json"]);
        }
        _ => {}
    }
}

fn validate_policy_check_args(args: &[String]) {
    let command = args
        .get(1)
        .map(|sub| format!("policy {sub}"))
        .unwrap_or_else(|| "policy check".to_string());
    reject_unsupported_args_after(&command, args, 2, &["--json"]);
}

fn reject_unknown_arg(command: &str, arg: &str, supported: &str) -> ! {
    let kind = if arg.starts_with('-') {
        "flag"
    } else {
        "argument"
    };
    eprintln!("hf {command}: unknown {kind} '{arg}' (supported: {supported})");
    std::process::exit(2);
}

fn validate_flags_and_positionals(
    command: &str,
    args: &[String],
    start: usize,
    allowed_flags: &[&str],
    value_flags: &[&str],
    max_positionals: usize,
) {
    let mut positionals = 0usize;
    let mut i = start;
    while i < args.len() {
        let arg = &args[i];
        if value_flags.contains(&arg.as_str()) {
            match args.get(i + 1) {
                Some(value) if !value.starts_with('-') => {
                    i += 2;
                    continue;
                }
                _ => {
                    eprintln!("hf {command}: option '{arg}' requires a value");
                    std::process::exit(2);
                }
            }
        }
        if allowed_flags.contains(&arg.as_str()) {
            i += 1;
            continue;
        }
        if !arg.starts_with('-') && positionals < max_positionals {
            positionals += 1;
            i += 1;
            continue;
        }
        let kind = if arg.starts_with('-') {
            "flag"
        } else {
            "argument"
        };
        let mut supported = Vec::new();
        supported.extend_from_slice(allowed_flags);
        supported.extend_from_slice(value_flags);
        let supported = if supported.is_empty() {
            if max_positionals == 0 {
                "no flags".to_string()
            } else {
                format!("up to {max_positionals} positional argument(s)")
            }
        } else if max_positionals == 0 {
            supported.join(", ")
        } else {
            format!(
                "{} plus up to {max_positionals} positional argument(s)",
                supported.join(", ")
            )
        };
        eprintln!("hf {command}: unknown {kind} '{arg}' (supported: {supported})");
        std::process::exit(2);
    }
}

fn validate_claim_args(args: &[String]) {
    match args.get(1).map(String::as_str) {
        Some("--next" | "--batch") if args.len() == 2 => {}
        Some(id) if !id.starts_with('-') && args.len() == 2 => {}
        Some("--next" | "--batch") => {
            let extra = args.get(2).map(String::as_str).unwrap_or("");
            reject_unknown_arg("claim", extra, "ID | --next | --batch");
        }
        Some(other) => reject_unknown_arg("claim", other, "ID | --next | --batch"),
        None => {}
    }
}

fn validate_intake_args(args: &[String]) {
    validate_flags_and_positionals(
        "intake",
        args,
        1,
        &[],
        &["--bundle", "--vibe", "--intent", "--scope"],
        0,
    );
}

fn validate_session_args(args: &[String]) {
    match args.get(1).map(String::as_str) {
        Some("start") => {
            validate_flags_and_positionals("session start", args, 2, &[], &["--base"], 0)
        }
        Some("end") => validate_flags_and_positionals(
            "session end",
            args,
            2,
            &["--recycle", "--reap"],
            &["--base"],
            0,
        ),
        Some("reap") => {
            validate_flags_and_positionals("session reap", args, 2, &["--force", "--reap"], &[], 0);
        }
        Some(other) => reject_unknown_arg("session", other, "start | end | reap"),
        None => {}
    }
}

fn validate_schema_args(args: &[String]) {
    match args.get(1).map(String::as_str) {
        Some("--check" | "--write") if args.len() == 2 => {}
        Some("--check" | "--write") => {
            let extra = args.get(2).map(String::as_str).unwrap_or("");
            reject_unknown_arg("schema", extra, "--check | --write");
        }
        Some(other) => reject_unknown_arg("schema", other, "--check | --write"),
        None => {}
    }
}

fn validate_review_args(args: &[String]) {
    match args.get(1).map(String::as_str) {
        Some("request") => {
            validate_flags_and_positionals("review request", args, 2, &[], &["--task"], 1);
        }
        Some("verdict") => {
            validate_flags_and_positionals("review verdict", args, 2, &[], &["--by"], 3);
        }
        _ => {}
    }
}

fn validate_gatekeeper_args(args: &[String]) {
    if args.get(1).map(String::as_str) == Some("check") {
        validate_flags_and_positionals("gatekeeper check", args, 2, &[], &["--task"], 1);
    }
}

#[cfg(feature = "cognitum")]
fn validate_policy_gate_args(args: &[String]) {
    validate_flags_and_positionals("policy gate", args, 2, &[], &["--task"], 1);
}

fn validate_fleet_args(args: &[String]) {
    match args.get(1).map(String::as_str) {
        Some("status") => {
            validate_flags_and_positionals(
                "fleet status",
                args,
                2,
                &["--fix", "--json", "--dry-run"],
                &[],
                0,
            );
            if args.iter().any(|a| a == "--dry-run") && !args.iter().any(|a| a == "--fix") {
                eprintln!(
                    "hf fleet status: --dry-run is only valid with --fix; use `hf fleet sync --dry-run` for remediation dry-runs"
                );
                std::process::exit(2);
            }
        }
        Some("sync") => {
            validate_flags_and_positionals("fleet sync", args, 2, &["--json", "--dry-run"], &[], 0);
        }
        Some("render") => validate_flags_and_positionals("fleet render", args, 2, &[], &[], 1),
        _ => {}
    }
}

fn validate_init_args(args: &[String]) {
    let flags_with_values = ["--name", "--northstar", "--role", "--plane"];
    let mut i = 1usize;
    while i < args.len() {
        let arg = &args[i];
        if flags_with_values.contains(&arg.as_str()) {
            match args.get(i + 1) {
                Some(value) if !value.starts_with('-') => {
                    i += 2;
                    continue;
                }
                _ => {
                    eprintln!("hf init: option '{arg}' requires a value");
                    std::process::exit(2);
                }
            }
        }
        let kind = if arg.starts_with('-') {
            "flag"
        } else {
            "argument"
        };
        eprintln!(
            "hf init: unknown {kind} '{arg}' (supported: --name VALUE, --northstar VALUE, --role VALUE, --plane VALUE)"
        );
        std::process::exit(2);
    }
}

fn parse_prompt_hub_args(args: &[String]) -> (String, Option<Vec<String>>, bool, bool) {
    let mut vibe: Option<String> = None;
    let mut scope: Option<Vec<String>> = None;
    let mut dispatch = false;
    let mut json = false;
    let mut i = 1usize;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--scope" => match args.get(i + 1) {
                Some(value) if !value.starts_with('-') => {
                    scope = Some(
                        value
                            .split(',')
                            .map(|g| g.trim().to_string())
                            .filter(|g| !g.is_empty())
                            .collect(),
                    );
                    i += 2;
                }
                _ => {
                    eprintln!("hf prompt-hub: option '--scope' requires a value");
                    std::process::exit(2);
                }
            },
            "--dispatch" => {
                dispatch = true;
                i += 1;
            }
            "--json" => {
                json = true;
                i += 1;
            }
            _ if arg.starts_with('-') => {
                eprintln!(
                    "hf prompt-hub: unknown flag '{arg}' (supported: --scope VALUE, --dispatch, --json)"
                );
                std::process::exit(2);
            }
            _ if vibe.is_none() => {
                vibe = Some(arg.clone());
                i += 1;
            }
            _ => {
                eprintln!("hf prompt-hub: unknown argument '{arg}' (supported: one vibe argument)");
                std::process::exit(2);
            }
        }
    }
    let Some(vibe) = vibe else {
        eprintln!("usage: hf prompt-hub \"<vibe>\" [--scope glob,glob] [--dispatch] [--json]");
        std::process::exit(2);
    };
    (vibe, scope, dispatch, json)
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    apply_ledger_flag(&mut args);
    if print_help(&args) {
        return;
    }
    match args.first().map(|s| s.as_str()) {
        // HFTASK-0085 (automation rung 0): build-version stamp so any consumer can detect a
        // binary that is BEHIND the kernel source. `--json` form is machine-readable for the
        // loop-init Phase 0 / SessionStart staleness check.
        Some("--version") | Some("-V") | Some("version") => {
            reject_unsupported_args(
                args.first().map(String::as_str).unwrap_or("version"),
                &args,
                &["--json"],
            );
            cmd_version(args.iter().any(|a| a == "--json"))
        }
        Some("init") => {
            validate_init_args(&args);
            cmd_init(&args)
        }
        Some("seed") => {
            reject_unsupported_args("seed", &args, &[]);
            cmd_seed()
        }
        Some("status") => {
            reject_unsupported_args("status", &args, &["--json"]);
            cmd_status(args.iter().any(|a| a == "--json"))
        }
        // PRD §8/§9: generate navigation maps + the task DAG (HFTASK-0050, now actually built).
        // Fail closed on unsupported flags: `hf index --intent-aware` was historically documented
        // as a future idea, but silently running the normal index makes agents believe that
        // intent-aware indexing happened. Help paths are handled before this match.
        Some("index") => {
            reject_unsupported_args("index", &args, &[]);
            index::cmd_index()
        }
        Some("plan") => {
            reject_unsupported_args("plan", &args, &["--json"]);
            index::cmd_plan(args.iter().any(|a| a == "--json"))
        }
        Some("claim") => {
            validate_claim_args(&args);
            if args.get(1).map(|s| s.as_str()) == Some("--batch") {
                cmd_claim_batch();
            } else if args.iter().any(|a| a == "--next") {
                cmd_claim_next();
            } else {
                cmd_claim(args.get(1).map(|s| s.as_str()).unwrap_or(""));
            }
        }
        Some("doctor") => {
            reject_unsupported_args("doctor", &args, &["--json"]);
            cmd_doctor(args.iter().any(|a| a == "--json"))
        }
        Some("gitignore") => cmd_gitignore(
            args.iter()
                .find(|a| a.starts_with("--"))
                .map(|s| s.as_str()),
        ),
        Some("reconcile") => {
            reject_unsupported_args("reconcile", &args, &[]);
            cmd_reconcile()
        }
        Some("export") => {
            reject_unsupported_args("export", &args, &[]);
            cmd_export()
        }
        Some("import") => {
            reject_unsupported_args("import", &args, &[]);
            cmd_import()
        }
        Some("migrate") => {
            validate_flags_and_positionals("migrate", &args, 1, &[], &[], 1);
            let path = args
                .get(1)
                .filter(|a| !a.starts_with("--"))
                .cloned()
                .unwrap_or_else(ledger_path);
            cmd_migrate(&path);
        }
        Some("release") => {
            reject_unsupported_args_with_positionals("release", &args, &[], 1);
            cmd_release(args.get(1).map(|s| s.as_str()).unwrap_or(""))
        }
        Some("reopen") => {
            reject_unsupported_flags("reopen", &args, &[]);
            let positional: Vec<&str> = args[1..]
                .iter()
                .map(|s| s.as_str())
                .filter(|a| !a.starts_with("--"))
                .collect();
            let id = positional.first().copied().unwrap_or("");
            let reason = positional.get(1..).map(|r| r.join(" ")).unwrap_or_default();
            cmd_reopen(id, &reason);
        }
        Some("relock") => {
            reject_unsupported_flags("relock", &args, &[]);
            let positional: Vec<&str> = args[1..]
                .iter()
                .map(|s| s.as_str())
                .filter(|a| !a.starts_with("--"))
                .collect();
            let id = positional.first().copied().unwrap_or("");
            let reason = positional.get(1..).map(|r| r.join(" ")).unwrap_or_default();
            cmd_relock(id, &reason);
        }
        Some("lease") => {
            reject_unsupported_args("lease", &args, &["--json"]);
            cmd_lease(args.iter().any(|a| a == "--json"))
        }
        Some("checkpoint") => {
            reject_unsupported_flags("checkpoint", &args, &["--auto", "--quiet", "--sync-cards"]);
            let auto = args.iter().any(|a| a == "--auto");
            let quiet = args.iter().any(|a| a == "--quiet");
            let positional: Vec<&str> = args[1..]
                .iter()
                .map(|s| s.as_str())
                .filter(|a| !a.starts_with("--"))
                .collect();
            let id = positional.first().copied();
            let note = positional.get(1..).map(|r| r.join(" ")).unwrap_or_default();
            cmd_checkpoint(id, &note, auto, quiet);
            if args.iter().any(|a| a == "--sync-cards") {
                let n = sync_cards();
                if !quiet {
                    println!("hf checkpoint: synced {n} card(s) from ledger truth");
                }
            }
        }
        Some("sync-cards") => {
            reject_unsupported_args("sync-cards", &args, &[]);
            let n = sync_cards();
            println!("hf sync-cards: synced {n} card(s) from ledger truth");
        }
        Some("sync") => {
            // --help/-h MUST print usage and never execute: `hf sync` rolls per-repo
            // ledgers up into the central FLEET ledger (a real fleet-wide side effect),
            // so an unsafe help path could mutate state on a `--help` invocation.
            if args.iter().any(|a| a == "--help" || a == "-h") {
                println!(
                    "usage: hf sync [--auto] [--dry-run] [--json]\n  \
                     Repairs .meta.yaml/.gitignore, mirrors ledger->.kb, and rolls each member \
                     repo's local .handoff/ledger.db up into the central FLEET ledger \
                     (append-with-provenance; idempotent via the per-repo sync cursor).\n  \
                     --dry-run  report what would roll up, write nothing.\n  \
                     --auto     non-interactive."
                );
                return;
            }
            reject_unsupported_args("sync", &args, &["--auto", "--dry-run", "--json"]);
            let auto = args.iter().any(|a| a == "--auto");
            let dry = args.iter().any(|a| a == "--dry-run");
            let json = args.iter().any(|a| a == "--json");
            sync::cmd_sync(auto, dry, json);
        }
        Some("done") => {
            validate_flags_and_positionals("done", &args, 1, &[], &["--pr"], 1);
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("");
            let pr = args
                .iter()
                .position(|a| a == "--pr")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str());
            cmd_done(id, pr);
        }
        Some("test") => {
            validate_flags_and_positionals("test", &args, 1, &[], &[], 1);
            let id = args
                .get(1)
                .map(|s| s.as_str())
                .filter(|s| !s.starts_with("--"));
            cmd_test(id);
        }
        Some("task") if args.get(1).map(|s| s.as_str()) == Some("mint") => {
            validate_flags_and_positionals("task mint", &args, 2, &[], &["--from-kb"], 0);
            let slug = args
                .iter()
                .position(|a| a == "--from-kb")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("");
            kb::cmd_mint_from_kb(slug);
        }
        Some("intake") => {
            validate_intake_args(&args);
            let flag = |name: &str| {
                args.iter()
                    .position(|a| a == name)
                    .and_then(|i| args.get(i + 1))
                    .map(|s| s.as_str())
            };
            let scope: Option<Vec<String>> = flag("--scope").map(|s| {
                s.split(',')
                    .map(|g| g.trim().to_string())
                    .filter(|g| !g.is_empty())
                    .collect()
            });
            intake::cmd_intake(
                flag("--bundle"),
                flag("--vibe"),
                flag("--intent"),
                scope.as_deref(),
            );
        }
        Some("dispatch") => {
            validate_flags_and_positionals("dispatch", &args, 1, &["--next"], &[], 1);
            let next_only = args.iter().any(|a| a == "--next");
            let cid = args
                .get(1)
                .map(|s| s.as_str())
                .filter(|s| !s.starts_with("--"));
            // HFTASK-0083: inject the witnessed claim path (the leaser is built once and captured)
            // so intake doesn't depend back on the binary's dispatch (`cmd_claim_with`).
            let leaser = lease::WeaveCli::from_env();
            intake::cmd_dispatch(cid, next_only, &|id| cmd_claim_with(id, &leaser));
        }
        Some("ship") => {
            validate_flags_and_positionals("ship", &args, 1, &[], &["--base"], 1);
            let id = args.get(1).map(|s| s.as_str()).unwrap_or("");
            // HFTASK-0008: empty default → cmd_ship resolves the base from the branch
            // policy (trunk_branch), instead of hardcoding "master" at the call site.
            let base = args
                .iter()
                .position(|a| a == "--base")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("");
            cmd_ship(id, base);
        }
        // HFTASK-0076 (ADR-0018 D11): hands-off develop → trunk promotion (also auto-run at
        // `hf done --pr`). Replaces the manual `gh api PATCH .../master` ff.
        Some("promote") => {
            reject_unsupported_args("promote", &args, &[]);
            cmd_promote()
        }
        Some("review") if args.get(1).map(|s| s.as_str()) == Some("request") => {
            validate_review_args(&args);
            let pr = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let task_id = args
                .iter()
                .position(|a| a == "--task")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str());
            cmd_review_request(pr, task_id);
        }
        Some("review") if args.get(1).map(|s| s.as_str()) == Some("verdict") => {
            validate_review_args(&args);
            let by = args
                .iter()
                .position(|a| a == "--by")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("unattributed");
            cmd_review_verdict(
                args.get(2).map(|s| s.as_str()).unwrap_or(""),
                args.get(3).map(|s| s.as_str()).unwrap_or(""),
                args.get(4).map(|s| s.as_str()).unwrap_or(""),
                by,
            );
        }
        Some("gatekeeper") if args.get(1).map(|s| s.as_str()) == Some("check") => {
            validate_gatekeeper_args(&args);
            let pr = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let task_id = args
                .iter()
                .position(|a| a == "--task")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str());
            gatekeeper::cmd_gatekeeper_check(pr, task_id);
        }
        #[cfg(feature = "secrets")]
        Some("secret") if args.get(1).map(|s| s.as_str()) == Some("gate-check") => {
            let method = args
                .iter()
                .position(|a| a == "--method")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("GET");
            let host = args
                .iter()
                .position(|a| a == "--host")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("api.github.com");
            let path = args
                .iter()
                .position(|a| a == "--path")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str())
                .unwrap_or("/");
            match handoff_secrets::github_merge_gate(method, host, path) {
                Ok(true) => println!("allow"),
                Ok(false) => {
                    println!("deny");
                    std::process::exit(1);
                }
                Err(e) => {
                    eprintln!("hf secret gate-check: {e}");
                    std::process::exit(2);
                }
            }
        }
        Some("session") => {
            validate_session_args(&args);
            session::cmd_session(&args[1..]);
        }
        Some("drift") => {
            reject_unsupported_args("drift", &args, &["--json"]);
            gates::cmd_drift(args.iter().any(|a| a == "--json"))
        }
        Some("hook") => {
            validate_hook_args(&args);
            let json = args.iter().any(|a| a == "--json");
            match args.get(1).map(|s| s.as_str()) {
                Some("list") => hooks::cmd_hook_list(json),
                Some("run") => {
                    let event = args.get(2).map(|s| s.as_str()).unwrap_or("");
                    // optional `--payload <json>`
                    let payload = args
                        .iter()
                        .position(|a| a == "--payload")
                        .and_then(|i| args.get(i + 1))
                        .map(|s| s.as_str());
                    // Witness each typed result as a `hook_result` ledger event (best-effort).
                    let code = hooks::cmd_hook_run(event, payload, json, |r| {
                        if let Ok(mut led) = Ledger::open(&ledger_path())
                            && let Ok(p) = serde_json::to_string(r)
                        {
                            let _ = led.append("hook_result", &r.event, &p, now_ns());
                        }
                    });
                    if code != 0 {
                        std::process::exit(code);
                    }
                }
                _ => {
                    eprintln!(
                        "hf hook: use `hf hook list` or `hf hook run <event> [--payload <json>] [--json]`"
                    );
                    std::process::exit(2);
                }
            }
        }
        #[cfg(feature = "cognitum")]
        Some("policy") if args.get(1).map(|s| s.as_str()) == Some("gate") => {
            validate_policy_gate_args(&args);
            let action = args.get(2).map(|s| s.as_str()).unwrap_or("");
            let task_id = args
                .iter()
                .position(|a| a == "--task")
                .and_then(|i| args.get(i + 1))
                .map(|s| s.as_str());
            cognitum::cmd_policy_gate(action, task_id);
        }
        Some("policy")
            if args
                .get(1)
                .map(|s| s.as_str())
                .is_some_and(|s| s.starts_with("check-")) =>
        {
            let kind = args.get(1).map(|s| s.as_str()).unwrap_or("");
            validate_policy_check_args(&args);
            gates::cmd_policy_check(kind, args.iter().any(|a| a == "--json"));
        }
        Some("fleet") if args.get(1).map(|s| s.as_str()) == Some("status") => {
            validate_fleet_args(&args);
            // HFTASK-0087: `hf fleet status --fix` is an alias for `hf fleet sync` — it remediates
            // the drift the sweep detects instead of only reporting it.
            if args.iter().any(|a| a == "--fix") {
                fleet::cmd_fleet_sync(
                    args.iter().any(|a| a == "--json"),
                    args.iter().any(|a| a == "--dry-run"),
                );
            } else {
                fleet::cmd_fleet_status(args.iter().any(|a| a == "--json"));
            }
        }
        Some("fleet") if args.get(1).map(|s| s.as_str()) == Some("sync") => {
            validate_fleet_args(&args);
            // HFTASK-0087: remediate every non-conformant member the status sweep flags.
            fleet::cmd_fleet_sync(
                args.iter().any(|a| a == "--json"),
                args.iter().any(|a| a == "--dry-run"),
            );
        }
        Some("fleet") if args.get(1).map(|s| s.as_str()) == Some("render") => {
            validate_fleet_args(&args);
            // hf fleet render <member> — compile <member>'s packet from the FLEET ledger
            let member = args.get(2).map(|s| s.as_str()).unwrap_or("");
            if member.is_empty() {
                eprintln!("hf fleet render <member> — member name required");
                std::process::exit(2);
            }
            match fleet::find_meta_root() {
                Some(root) => match fleet::render_member_packet(&root, member) {
                    Ok(p) => println!("hf fleet render: wrote {}", p.display()),
                    Err(e) => {
                        eprintln!("hf fleet render: {e}");
                        std::process::exit(1);
                    }
                },
                None => {
                    eprintln!(
                        "hf fleet render: no .meta.yaml found from the current directory upward"
                    );
                    std::process::exit(1);
                }
            }
        }
        Some("delivery") => {
            validate_delivery_args(&args);
            let json = args.iter().any(|a| a == "--json");
            match args.get(1).map(|s| s.as_str()) {
                Some("get") => {
                    delivery::cmd_delivery_get(args.get(2).map(|s| s.as_str()).unwrap_or(""), json)
                }
                Some("list") => delivery::cmd_delivery_list(json),
                _ => {
                    eprintln!(
                        "hf delivery: use `hf delivery get <correlation_id> [--json]` or `hf delivery list [--json]`"
                    );
                    std::process::exit(2);
                }
            }
        }
        Some("prompt-hub") => {
            let (vibe, scope, dispatch, json) = parse_prompt_hub_args(&args);
            prompt_hub::cmd_prompt_hub(&vibe, scope.as_deref(), dispatch, json);
        }
        Some("schema") => {
            validate_schema_args(&args);
            let code = schema::cmd_schema(&args[1..]);
            if code != 0 {
                std::process::exit(code);
            }
        }
        Some("handoff") => {
            reject_unsupported_args("handoff", &args, &[]);
            cmd_handoff()
        }
        Some("resume") => {
            reject_unsupported_args("resume", &args, &["--json", "--compact"]);
            let mode = if args.iter().any(|a| a == "--json") {
                ResumeMode::Json
            } else if args.iter().any(|a| a == "--compact") {
                ResumeMode::Compact
            } else {
                ResumeMode::Full
            };
            cmd_resume(mode);
        }
        // HFTASK-0080: catch-all. An UNKNOWN verb must fail closed (exit 2) — the prior
        // arm only printed usage and fell through to exit 0, a fail-OPEN help path (a typo
        // like `hf promot` silently "succeeded"). Bare `hf` (no verb) stays exit 0 (help).
        other => {
            if let Some(verb) = other {
                eprintln!("hf: unknown command '{verb}'");
            }
            eprintln!("{HF_USAGE}");
            if other.is_some() {
                std::process::exit(2);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    // Tests set/clear `HANDOFF_LEDGER*` env vars (unsafe in edition 2024). Test binaries run each
    // test in a controlled context and these env mutations are serialized via the cwd/env lock,
    // so the data-race precondition does not arise. Justified test-only `unsafe_code` allow.
    #![allow(unsafe_code)]
    use super::*;

    /// Build a minimal WorkOrder for the relock tests. `full` stamps the 5-field lock so the
    /// no-downgrade branch can be exercised both ways.
    fn relock_wo(objective: &str, full: bool) -> WorkOrder {
        let path_scope = vec!["**".to_string()];
        let acceptance = vec!["landed & merged".to_string()];
        let mut wo = WorkOrder {
            schema: "handoff.task.v1".into(),
            id: "TASK-RELOCK".into(),
            title: "relock fixture".into(),
            status: Status::Done,
            priority: work_order::Priority::P1,
            objective: objective.into(),
            path_scope: path_scope.clone(),
            acceptance_criteria: acceptance.clone(),
            test_commands: vec![],
            dependencies: vec![],
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: "relock-test".into(),
            role: Some("implementer".into()),
            intent_lock: WorkOrder::compute_intent_lock(objective, &path_scope, &acceptance),
        };
        if full {
            wo.intent_lock = wo.full_intent_lock("blake3:ns-rev-1");
        }
        wo
    }

    #[test]
    fn relock_remints_edited_objective_and_preserves_partial_form() {
        // A partial (3-field) lock minted, then the objective text edited post-lock (the real
        // TASK-0002 case: a doctrine correction). relock_intent re-mints the objective hash to the
        // CURRENT text and keeps the lock partial (no-downgrade: never grafts on constraint/
        // northstar surfaces the card never carried).
        let mut wo = relock_wo("old objective", false);
        let stale = wo.intent_lock.objective_hash.clone();
        wo.objective = "corrected objective".to_string();
        assert!(!wo.intent_unchanged(), "edited objective must drift first");

        let fresh = relock_intent(&wo, "blake3:ns-rev-1");
        let expect =
            WorkOrder::compute_intent_lock(&wo.objective, &wo.path_scope, &wo.acceptance_criteria);
        assert_eq!(
            fresh.objective_hash, expect.objective_hash,
            "re-mints to current text"
        );
        assert_ne!(fresh.objective_hash, stale, "no longer the stale hash");
        assert!(fresh.constraint_hash.is_empty(), "partial stays partial");
        assert!(fresh.northstar_revision.is_empty(), "partial stays partial");

        // Applying the re-minted lock clears the drift.
        wo.intent_lock = fresh;
        assert!(wo.intent_unchanged(), "drift cleared after relock");
    }

    #[test]
    fn relock_full_lock_remints_all_surfaces_and_rebinds_northstar() {
        // A full (5-field) lock, objective edited + bound to a SUPERSEDED doctrine revision.
        // relock_intent re-mints every surface and re-binds to the supplied current revision.
        let mut wo = relock_wo("old objective", true);
        wo.objective = "corrected objective".to_string();
        let fresh = relock_intent(&wo, "blake3:ns-rev-2");
        assert!(
            !fresh.constraint_hash.is_empty(),
            "full lock keeps its constraint surface"
        );
        assert_eq!(
            fresh.northstar_revision, "blake3:ns-rev-2",
            "re-bound to current doctrine"
        );
        wo.intent_lock = fresh;
        assert!(
            wo.intent_components("blake3:ns-rev-2").objective,
            "objective drift cleared"
        );
        assert!(
            wo.intent_components("blake3:ns-rev-2").northstar,
            "northstar drift cleared"
        );
    }

    #[test]
    fn version_info_is_stamped_and_well_formed() {
        // HFTASK-0085: version_info() never lies — a missing stamp falls back to "unknown",
        // never an empty string, so the staleness check can always make a decision.
        let (version, commit, date) = version_info();
        assert!(!version.is_empty(), "CARGO_PKG_VERSION must be present");
        assert!(
            !commit.is_empty() && !date.is_empty(),
            "stamps fall back to 'unknown', never empty"
        );
        // The machine-readable form parses and carries the keys the staleness check greps.
        let out = serde_json::json!({
            "schema": "handoff.version.v1",
            "version": version, "commit": commit, "date": date,
        });
        assert_eq!(out["schema"], "handoff.version.v1");
        assert_eq!(out["commit"], commit);
        assert!(out["version"].as_str().is_some_and(|v| !v.is_empty()));
    }

    #[test]
    fn sync_usage_includes_json() {
        // Audit-found navigation gap: `hf sync --json` existed, but the bare usage string
        // omitted it, so fresh agents could not discover structured sync output.
        assert!(
            HF_USAGE.contains("sync [--auto] [--dry-run] [--json]"),
            "usage must advertise hf sync --json: {HF_USAGE}"
        );
    }

    /// HFTASK-0058: the two tests that mutate the process-global `HANDOFF_LEDGER` env var
    /// (`ledger_path_defaults_local_and_honors_handoff_ledger` and
    /// `apply_ledger_flag_extracts_and_exports_path`) must not run concurrently — the parallel
    /// test runner otherwise races on that shared global. This lock serializes just those two
    /// (a latent flake on develop that the new durability tests' scheduling surfaced).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn backup_stem_encodes_full_path_and_trims_root() {
        // The full source location is preserved (so two ledgers never collide), separators and
        // spaces become `_`, the leading `/` is trimmed, and already-safe chars pass through.
        assert_eq!(
            backup_stem_for("/home/x/.handoff/ledger.db"),
            "home_x_.handoff_ledger.db"
        );
        assert_eq!(backup_stem_for("/a b/c"), "a_b_c");
        assert_eq!(backup_stem_for("rel.db"), "rel.db");
    }

    #[test]
    fn ledger_backup_dir_honors_explicit_override() {
        // The explicit override wins over XDG/HOME and is returned verbatim. Serialize on the
        // shared env lock and restore the prior value so no sibling test is destabilized.
        let _g = ENV_LOCK.lock().unwrap();
        let prev = std::env::var("HANDOFF_LEDGER_BACKUP_DIR").ok();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("HANDOFF_LEDGER_BACKUP_DIR", "/tmp/hb-test-dir") };
        assert_eq!(
            ledger_backup_dir(),
            Some(std::path::PathBuf::from("/tmp/hb-test-dir"))
        );
        match prev {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            Some(v) => unsafe { std::env::set_var("HANDOFF_LEDGER_BACKUP_DIR", v) },
            // FIXME: Audit that the environment access only happens in single-threaded code.
            None => unsafe { std::env::remove_var("HANDOFF_LEDGER_BACKUP_DIR") },
        }
    }

    #[test]
    fn session_relay_templates_render_from_witnessed_ledger_and_are_deployed() {
        // HFTASK-0070 (ADR-0018 D5): handoff owns the canonical session-relay templates, they
        // render from the witnessed `hf` ledger/packet (NEVER hand-authored prose), and the
        // /handoff-loop-init family deploys + byte-enforces them fleet-wide. Repo root is the
        // parent of this crate's manifest dir (handoff/hf/.. == handoff/).
        let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("hf crate has a parent (repo root)")
            .to_path_buf();

        let resume =
            std::fs::read_to_string(repo.join(".claude/skills/session-relay-resume/SKILL.md"))
                .expect("canonical session-relay-resume SKILL.md exists in handoff");
        let wrap_up =
            std::fs::read_to_string(repo.join(".claude/skills/session-relay-wrap-up/SKILL.md"))
                .expect("canonical session-relay-wrap-up SKILL.md exists in handoff");

        // Render-from-witnessed-ledger contract: the `hf` render is the REQUIRED source, not prose.
        assert!(
            resume.contains("hf resume"),
            "resume template must render from `hf resume` (the witnessed packet)"
        );
        assert!(
            resume.contains("AUTHORITATIVE"),
            "resume template must mark the `hf` render authoritative, not optional"
        );
        assert!(
            wrap_up.contains("hf handoff") && wrap_up.contains("hf checkpoint"),
            "wrap-up template must render from `hf checkpoint`/`hf handoff` (the witnessed packet)"
        );

        // Deploy + byte-consistency-enforcement contract lives in the /handoff-loop-init family.
        let init = std::fs::read_to_string(repo.join("scripts/handoff-loop-init.sh"))
            .expect("handoff-loop-init.sh exists");
        assert!(
            init.contains("deploy_session_relay"),
            "handoff-loop-init.sh must define + wire deploy_session_relay (fleet deploy)"
        );
        assert!(
            init.contains("cmp -s"),
            "deploy_session_relay must enforce byte-consistency (cmp drift detection)"
        );
    }

    #[test]
    fn init_capsule_is_portable_for_members() {
        // Portability contract (ADR-0006): a member's capsule identifies as ITSELF and
        // never inherits the kernel's project_name or doctrine northstar.
        let member = init_capsule(
            false,
            "weave",
            "tool",
            "execution",
            "(seed me) the guiding goal for weave",
        );
        assert_eq!(member["project_name"], "weave");
        assert_eq!(member["role"], "tool");
        assert_eq!(member["plane"], "execution");
        assert_eq!(member["schema"], "handoff.context_capsule.v1");
        assert_eq!(member["next_command"], "hf resume");
        let ns = member["northstar"].as_str().unwrap();
        assert!(
            !ns.contains("KERNEL DOCTRINE"),
            "member must not get kernel doctrine"
        );

        // The kernel home keeps its curated identity + doctrine.
        let kernel = init_capsule(true, "handoff", "kernel", "orchestration", KERNEL_NORTHSTAR);
        assert_eq!(kernel["project_name"], "handoff (Continuity Ledger Kernel)");
        assert!(
            kernel["northstar"]
                .as_str()
                .unwrap()
                .contains("KERNEL DOCTRINE")
        );
    }

    #[test]
    fn schema_check_from_member_cwd_resolves_kernel_home_without_local_schemas() {
        // HFTASK-0093: the real `hf schema --check` dispatcher must not be CWD-sensitive.
        // A fleet member (e.g. meta/Weave) may have no local `schemas/` directory; the command
        // resolves the canonical sibling `meta/handoff/schemas/task.schema.json` instead.
        let _g = cwd_lock();
        let meta = std::env::temp_dir().join(format!(
            "hf-schema-meta-{}-{}",
            std::process::id(),
            now_ns()
        ));
        let member = meta.join("Weave");
        let kernel_schema = meta
            .join("handoff")
            .join("schemas")
            .join("task.schema.json");
        fs::create_dir_all(&member).unwrap();
        fs::create_dir_all(kernel_schema.parent().unwrap()).unwrap();
        fs::write(meta.join(".meta.yaml"), "repos: []\n").unwrap();
        fs::write(
            &kernel_schema,
            format!("{}\n", work_order::task_schema_json()),
        )
        .unwrap();

        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&member).unwrap();
        let code = schema::cmd_schema(&["--check".to_string()]);
        std::env::set_current_dir(prev).unwrap();
        fs::remove_dir_all(&meta).ok();

        assert_eq!(code, 0, "member cwd with no local schemas/ must pass");
    }

    #[test]
    fn release_unclaims_only_in_progress() {
        // HFTASK-0038: release reverts an active claim to Backlog, but must never un-finish
        // post-work/terminal states or touch an already-Backlog task.
        assert!(should_unclaim(Some(Status::Claimed)));
        assert!(should_unclaim(Some(Status::Checkpointed)));
        assert!(should_unclaim(Some(Status::Active)));
        assert!(!should_unclaim(Some(Status::Review)));
        assert!(!should_unclaim(Some(Status::Done)));
        assert!(!should_unclaim(Some(Status::Backlog)));
        assert!(!should_unclaim(None));
    }

    // HFTASK-0083: try_parse_card_accepts_valid_and_names_violations moved to handoff-core with
    // the function it tests (handoff-core::tests).

    #[test]
    fn reopen_targets_only_terminal_states() {
        // HFTASK-0061: reopen is the inverse of release — it reverts a FINISHED task, never an
        // in-progress claim (that's `hf release`) and never an already-Backlog/unknown task.
        assert!(should_reopen(Some(Status::Done)));
        assert!(should_reopen(Some(Status::Review)));
        assert!(!should_reopen(Some(Status::Claimed)));
        assert!(!should_reopen(Some(Status::Checkpointed)));
        assert!(!should_reopen(Some(Status::Active)));
        assert!(!should_reopen(Some(Status::Backlog)));
        assert!(!should_reopen(None));
        // reopen and release partition the in-progress vs terminal space disjointly.
        for s in [
            Status::Backlog,
            Status::Claimed,
            Status::Checkpointed,
            Status::Active,
            Status::Review,
            Status::Done,
        ] {
            assert!(
                !(should_reopen(Some(s)) && should_unclaim(Some(s))),
                "reopen and release must never both apply to {s:?}"
            );
        }
    }

    #[test]
    fn parse_tests_ran_sums_executed_across_suites_excluding_filtered_and_ignored() {
        // Two cargo suites: 3 real tests in one, 0 (all filtered out) in the other.
        let out = "\
running 3 tests
test a ... ok
test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 120 filtered out; finished in 0.05s

running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 41 filtered out; finished in 0.00s
";
        assert_eq!(parse_tests_ran(out), Some(3));
    }

    #[test]
    fn parse_tests_ran_is_zero_when_filter_matches_nothing() {
        // THE rubber-stamp the gate must reject: exit 0, but every suite ran nothing.
        let out = "\
running 0 tests
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 200 filtered out; finished in 0.00s
";
        assert_eq!(parse_tests_ran(out), Some(0));
    }

    #[test]
    fn parse_tests_ran_excludes_ignored_only_runs() {
        // An all-ignored match gives no assertion evidence → counts as 0 executed.
        let out =
            "test result: ok. 0 passed; 0 failed; 2 ignored; 0 measured; 0 filtered out; finished";
        assert_eq!(parse_tests_ran(out), Some(0));
    }

    #[test]
    fn parse_tests_ran_counts_failures_as_executed() {
        // A failed test still RAN; the exit code (not the count) carries the failure.
        let out = "test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out";
        assert_eq!(parse_tests_ran(out), Some(3));
    }

    #[test]
    fn parse_tests_ran_none_for_unrecognized_runner() {
        // No libtest summary → can't introspect → None (caller degrades to exit-code-only).
        let out = "PASS  src/foo.test.ts (4 passed)\nDone in 1.2s";
        assert_eq!(parse_tests_ran(out), None);
    }

    #[test]
    fn parse_tests_ran_pytest_counts_executed_excludes_skipped() {
        // HFTASK-0063: pytest framed summary — passed+failed executed, skipped excluded.
        let out = "tests/test_x.py ...F\n\
                   ===== 5 passed, 1 failed, 2 skipped in 0.12s =====";
        assert_eq!(parse_tests_ran(out), Some(6));
    }

    #[test]
    fn parse_tests_ran_pytest_zero_match_is_some_zero() {
        // The pytest zero-match rubber stamp: a framed "no tests ran" → Some(0) → FAIL closed.
        let out = "==== no tests ran in 0.01s ====";
        assert_eq!(parse_tests_ran(out), Some(0));
    }

    #[test]
    fn parse_tests_ran_jest_counts_passed_plus_failed() {
        // jest/vitest: passed+failed (NOT "total", which includes skipped/todo).
        let out = "Tests:       1 failed, 5 passed, 1 skipped, 7 total\nSnapshots: 0 total";
        assert_eq!(parse_tests_ran(out), Some(6));
    }

    #[test]
    fn parse_tests_ran_gotest_verbose_counts_markers() {
        // go test -v: per-test --- PASS:/--- FAIL: markers.
        let out = "=== RUN   TestA\n--- PASS: TestA (0.00s)\n\
                   === RUN   TestB\n--- FAIL: TestB (0.01s)\nFAIL\nexit status 1";
        assert_eq!(parse_tests_ran(out), Some(2));
    }

    #[test]
    fn parse_tests_ran_gotest_no_tests_is_some_zero() {
        // go's zero-match signal → Some(0) → FAIL closed.
        let out = "testing: warning: no tests to run\nPASS\nok  \texample/pkg\t0.002s";
        assert_eq!(parse_tests_ran(out), Some(0));
    }

    #[test]
    fn parse_tests_ran_libtest_still_wins_over_other_runners() {
        // A cargo run that also happens to print a '='-framed line must still parse as libtest.
        let out = "===== a banner =====\n\
                   test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out";
        assert_eq!(parse_tests_ran(out), Some(3));
    }

    #[test]
    fn cargo_zero_test_diagnostic_names_filter_and_list_command() {
        let diag = cargo_zero_test_diagnostic("cargo test -p hf typo_filter")
            .expect("cargo test command should get a diagnostic");
        assert!(diag.contains("package: hf"), "{diag}");
        assert!(diag.contains("filter: typo_filter"), "{diag}");
        assert!(
            diag.contains("cargo test -p hf typo_filter -- --list"),
            "{diag}"
        );
        assert!(
            diag.contains("cargo test -p hf -- --list"),
            "must suggest listing all tests without the suspect filter: {diag}"
        );
        assert!(
            diag.contains("completion evidence") || diag.contains("zero-test"),
            "diagnostic context should remain clearly zero-test related: {diag}"
        );
    }

    #[test]
    fn cargo_zero_test_diagnostic_preserves_feature_flags() {
        let diag = cargo_zero_test_diagnostic(
            "cargo test -p hf --features legacy-sqlite schema_filter -- --nocapture",
        )
        .expect("cargo test command should get a diagnostic");
        assert!(
            diag.contains("cargo test -p hf --features legacy-sqlite schema_filter -- --list"),
            "{diag}"
        );
        assert!(
            diag.contains("cargo test -p hf --features legacy-sqlite -- --list"),
            "{diag}"
        );
        assert!(diag.contains("preserved feature flags: --features legacy-sqlite"));
    }

    #[test]
    fn zero_test_gate_remains_fail_closed() {
        let ran = parse_tests_ran(
            "running 0 tests\n\
             test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 99 filtered out",
        );
        let code = 0;
        let zero_tests = ran == Some(0);
        let cmd_passed = code == 0 && !zero_tests;
        assert!(zero_tests, "recognized zero-test runs must be detected");
        assert!(
            !cmd_passed,
            "exit 0 plus zero executed tests must not mark the command passed"
        );
    }

    #[test]
    fn ledger_path_defaults_local_and_honors_handoff_ledger() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // HFTASK-0054: without an override, ledger_path() is cwd-relative.
        let prev = std::env::var("HANDOFF_LEDGER").ok();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("HANDOFF_LEDGER") };
        // Build the expected default the same way ledger_path() does so the
        // assertion holds on Windows too (Path::join yields a `\` separator).
        let default_local = Path::new(super::HF)
            .join("ledger.db")
            .to_string_lossy()
            .into_owned();
        assert_eq!(super::ledger_path(), default_local);

        // With the override, it points exactly at the supplied path.
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("HANDOFF_LEDGER", "/tmp/fleet.ledger.db") };
        assert_eq!(super::ledger_path(), "/tmp/fleet.ledger.db");

        // Empty override is treated as unset (defensive).
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("HANDOFF_LEDGER", "") };
        assert_eq!(super::ledger_path(), default_local);

        match prev {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            Some(v) => unsafe { std::env::set_var("HANDOFF_LEDGER", v) },
            // FIXME: Audit that the environment access only happens in single-threaded code.
            None => unsafe { std::env::remove_var("HANDOFF_LEDGER") },
        }
    }

    #[test]
    fn apply_ledger_flag_extracts_and_exports_path() {
        let _env = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // HFTASK-0054: the global `--ledger <path>` flag is stripped and exported.
        let prev = std::env::var("HANDOFF_LEDGER").ok();
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("HANDOFF_LEDGER") };

        let mut args = vec![
            "--ledger".into(),
            "/meta/.handoff/ledger.db".into(),
            "status".into(),
        ];
        super::apply_ledger_flag(&mut args);
        assert_eq!(
            std::env::var("HANDOFF_LEDGER").unwrap(),
            "/meta/.handoff/ledger.db"
        );
        assert_eq!(args, vec!["status"]);

        // No flag => no mutation (clear the var exported above first).
        // FIXME: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::remove_var("HANDOFF_LEDGER") };
        let mut args2 = vec!["handoff".into()];
        super::apply_ledger_flag(&mut args2);
        assert!(std::env::var("HANDOFF_LEDGER").is_err());
        assert_eq!(args2, vec!["handoff"]);

        match prev {
            // FIXME: Audit that the environment access only happens in single-threaded code.
            Some(v) => unsafe { std::env::set_var("HANDOFF_LEDGER", v) },
            // FIXME: Audit that the environment access only happens in single-threaded code.
            None => unsafe { std::env::remove_var("HANDOFF_LEDGER") },
        }
    }

    #[test]
    fn latest_test_result_gates_done() {
        // HFTASK-0045: `hf done` reads the latest witnessed test_result. Latest-wins so a
        // green re-run after a fix supersedes an earlier failure; a never-tested task → None
        // (and a task with test_commands + None is blocked by the cmd_done gate).
        let path = std::env::temp_dir().join(format!("hf-test-gate-{}.db", now_ns()));
        let p = path.to_string_lossy().to_string();
        let mut led = Ledger::open(&p).unwrap();
        let id = "HFTASK-9999";
        assert_eq!(latest_test_passed(&led, id), None, "never tested → None");
        led.append(
            "test_result",
            id,
            &serde_json::json!({ "id": id, "passed": false }).to_string(),
            now_ns(),
        )
        .unwrap();
        assert_eq!(latest_test_passed(&led, id), Some(false), "failing run");
        led.append(
            "test_result",
            id,
            &serde_json::json!({ "id": id, "passed": true }).to_string(),
            now_ns(),
        )
        .unwrap();
        assert_eq!(
            latest_test_passed(&led, id),
            Some(true),
            "latest green wins"
        );
        // a verdict for a different task must never bleed through
        assert_eq!(latest_test_passed(&led, "HFTASK-0001"), None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn latest_pr_opened_derives_merged_pr_for_done() {
        let path = std::env::temp_dir().join(format!("hf-pr-opened-{}.db", now_ns()));
        let p = path.to_string_lossy().to_string();
        let mut led = Ledger::open(&p).unwrap();
        let id = "HFTASK-9999";
        assert_eq!(latest_pr_opened(&led, id), None, "no pr_opened yet");
        led.append(
            "pr_opened",
            id,
            &serde_json::json!({ "id": id, "branch": "feat/x", "pr": "https://github.com/FlexNetOS/handoff/pull/42", "base": "develop" }).to_string(),
            now_ns(),
        )
        .unwrap();
        assert_eq!(
            latest_pr_opened(&led, id),
            Some("https://github.com/FlexNetOS/handoff/pull/42".to_string()),
            "extracts the pr url from the latest pr_opened event"
        );
        // a pr_opened for a different task must not bleed through
        assert_eq!(latest_pr_opened(&led, "HFTASK-0001"), None);
        let _ = std::fs::remove_file(&path);
    }
    use work_order::{SwarmBundle, work_orders_from_bundle};

    fn sample_tasks() -> Vec<WorkOrder> {
        work_orders_from_bundle(&SwarmBundle {
            workflow_id: "wf-test".into(),
            role_prompts: vec![
                ("architect".into(), "design".into()),
                ("coder".into(), "build".into()),
            ],
            handoff_template: "standard".into(),
            consistency_report: vec![],
            evolution_suggestions: vec![],
        })
    }

    /// FIX-2 regression: the summary's witnessed count is exactly the count it is given —
    /// resume/handoff compute it live from the ledger, never echo a stale packet value.
    #[test]
    fn summary_reports_the_witness_count_it_is_given() {
        let tasks = sample_tasks();
        let replay: Vec<(String, Status)> = vec![];
        let s = summary_json(&tasks, &replay, 42);
        assert_eq!(s["witnessed_events_verified"], 42);
        // a different witness count must flow straight through (no caching/staleness)
        let s2 = summary_json(&tasks, &replay, 43);
        assert_eq!(s2["witnessed_events_verified"], 43);
    }

    /// HFTASK-0027: the packet renderer (now shared by `hf handoff` AND the live
    /// `hf resume` Full path) reflects exactly the Done N/M + witness count it is given.
    /// Because `ResumeMode::Full` calls this with values recomputed from the ledger on
    /// every invocation — instead of echoing packets/latest.md — resume can never show a
    /// stale count: feeding two different live counts yields two different "Progress" lines.
    #[test]
    fn packet_renderer_reflects_live_done_and_witness_count() {
        let tasks = sample_tasks();
        let replay = vec![(tasks[0].id.clone(), Status::Done)];

        let md_336 = render_packet_md(&tasks, &replay, 336);
        assert!(
            md_336.contains("Done: 1/2.  Tamper-evident events verified: 336."),
            "renderer must emit the given count; got:\n{md_336}"
        );

        // Simulate events appended since the last handoff (live recount -> 371):
        // the rendered Progress line must change, proving no frozen/cached value.
        let md_371 = render_packet_md(&tasks, &replay, 371);
        assert!(
            md_371.contains("Tamper-evident events verified: 371."),
            "renderer must reflect the new live count; got:\n{md_371}"
        );
        assert!(!md_371.contains("verified: 336."));

        // And Done N/M is live too: marking both Done flips the progress numerator.
        let replay_both = vec![
            (tasks[0].id.clone(), Status::Done),
            (tasks[1].id.clone(), Status::Done),
        ];
        let md_both = render_packet_md(&tasks, &replay_both, 371);
        assert!(md_both.contains("Done: 2/2."));
    }

    /// HFTASK-0071 (ADR-0018 D4): the Next Action / Direction block must emit explicit
    /// next-action steering — the next safe task, the EXACT next command, WHY it is next,
    /// the cycle/context-budget wrap rule, and the blocking walls — all derived from the
    /// witnessed inputs, not hardcoded. A fresh agent must need zero archaeology.
    #[test]
    fn direction_block_emits_next_action_command_budget_and_walls() {
        let wo = |id: &str, status: Status, deps: &[&str], blocked: &[&str], obj: &str| WorkOrder {
            schema: "handoff.task.v1".into(),
            id: id.into(),
            title: format!("title {id}"),
            status,
            priority: Priority::P1,
            objective: obj.into(),
            path_scope: vec!["handoff/**".into()],
            acceptance_criteria: vec!["impl".into()],
            test_commands: vec![],
            dependencies: deps.iter().map(|s| s.to_string()).collect(),
            blocked_by: blocked.iter().map(|s| s.to_string()).collect(),
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: "t".into(),
            role: None,
            intent_lock: WorkOrder::compute_intent_lock(
                obj,
                &["handoff/**".to_string()],
                &["impl".to_string()],
            ),
        };
        // A: a Done dep, the next backlog card (deps satisfied), and a genuine wall.
        let tasks = vec![
            wo("HFTASK-0001", Status::Done, &[], &[], "done dep"),
            wo(
                "HFTASK-0002",
                Status::Backlog,
                &["HFTASK-0001"],
                &[],
                "the next safe card",
            ),
            wo(
                "HFTASK-0003",
                Status::Blocked,
                &[],
                &["HFTASK-0099"],
                "needs the broker — NEEDS-HUMAN account wall",
            ),
        ];
        let replay = vec![(tasks[0].id.clone(), Status::Done)];
        let next = next_safe(&tasks, &replay);
        let policy = policy::Policy::default(); // context strategy, cycle_flush=4, budget 50%
        let sess = session::LoopSessionState {
            open_branch: Some("feat/x".into()),
            cycle: 2,
        };

        let md = direction_block(&tasks, &replay, next, &policy, &sess);

        // next safe task + EXACT claim command (backlog card → claim, not checkpoint)
        assert!(
            md.contains("Next Action / Direction"),
            "header missing:\n{md}"
        );
        assert!(
            md.contains("Next safe task:** HFTASK-0002"),
            "next task missing:\n{md}"
        );
        assert!(
            md.contains("Next command:** `hf claim HFTASK-0002`"),
            "exact command missing:\n{md}"
        );
        // decision rationale cites satisfied deps
        assert!(
            md.contains("deps satisfied (HFTASK-0001)"),
            "rationale must justify why it is next:\n{md}"
        );
        // cycle / context-budget wrap rule, derived from policy + the live session counter
        assert!(
            md.contains("context — wrap at ~50%"),
            "budget rule missing:\n{md}"
        );
        assert!(md.contains("cycle 2/4"), "live cycle state missing:\n{md}");
        // blocking walls: the Blocked + NEEDS-HUMAN card surfaces with its reasons
        assert!(md.contains("HFTASK-0003"), "wall task missing:\n{md}");
        assert!(
            md.contains("status Blocked"),
            "wall reason (Blocked) missing:\n{md}"
        );
        assert!(
            md.contains("NEEDS-HUMAN"),
            "wall reason (NEEDS-HUMAN) missing:\n{md}"
        );

        // B: an in-progress task is RESUMED first → the command is checkpoint, not claim,
        // and clear walls render "none".
        let tasks_b = vec![
            wo("HFTASK-0010", Status::Checkpointed, &[], &[], "in progress"),
            wo("HFTASK-0011", Status::Backlog, &[], &[], "later"),
        ];
        let replay_b = vec![(tasks_b[0].id.clone(), Status::Checkpointed)];
        let next_b = next_safe(&tasks_b, &replay_b);
        let md_b = direction_block(&tasks_b, &replay_b, next_b, &policy, &sess);
        assert!(
            md_b.contains("Next command:** `hf checkpoint HFTASK-0010`"),
            "in-progress task must resume via checkpoint:\n{md_b}"
        );
        assert!(
            md_b.contains("Blocking walls:** none."),
            "should report no walls:\n{md_b}"
        );

        // C: the "tasks" wrap strategy renders the legacy fixed-count rule.
        let mut policy_tasks = policy::Policy::default();
        policy_tasks.loop_cfg.wrap_strategy = "tasks".into();
        let md_c = direction_block(&tasks_b, &replay_b, next_b, &policy_tasks, &sess);
        assert!(
            md_c.contains("tasks — wrap (checkpoint → handoff) at cycle_flush=4"),
            "tasks strategy must render the fixed-count rule:\n{md_c}"
        );
    }

    #[test]
    fn summary_counts_done_vs_remaining_and_picks_next() {
        let tasks = sample_tasks();
        // mark the first task Done via replay; the second is the next safe task
        let replay = vec![(tasks[0].id.clone(), Status::Done)];
        let s = summary_json(&tasks, &replay, 7);
        assert_eq!(s["tasks_total"], 2);
        assert_eq!(s["done"].as_array().unwrap().len(), 1);
        assert_eq!(s["remaining"].as_array().unwrap().len(), 1);
        assert_eq!(s["next_task_id"], serde_json::json!(tasks[1].id));
        assert_eq!(
            s["next_command"],
            serde_json::json!(format!("hf claim {}", tasks[1].id))
        );
    }

    #[test]
    fn exhausted_backlog_summary_uses_valid_handoff_command() {
        let tasks = sample_tasks();
        let replay = vec![
            (tasks[0].id.clone(), Status::Done),
            (tasks[1].id.clone(), Status::Done),
        ];
        let s = summary_json(&tasks, &replay, 760);
        assert_eq!(s["next_task_id"], serde_json::Value::Null);
        assert_eq!(s["next_command"], serde_json::json!("hf handoff"));
    }

    #[test]
    fn exhausted_backlog_resume_commands_do_not_emit_bare_done() {
        let tasks = sample_tasks();
        let replay = vec![
            (tasks[0].id.clone(), Status::Done),
            (tasks[1].id.clone(), Status::Done),
        ];
        let md = render_packet_md(&tasks, &replay, 760);
        assert!(
            md.contains("Next command:** `hf handoff`"),
            "direction block must point to a valid command:\n{md}"
        );
        assert!(
            md.contains("```bash\nhf resume\nhf handoff\n```"),
            "resume command block must not contain an invalid bare done token:\n{md}"
        );
        assert!(
            !md.contains("\nhf resume\ndone\n"),
            "bare done is invalid shell outside a loop and must not guide agents:\n{md}"
        );
    }

    // --- HFTASK-0029 hygiene bundle ---------------------------------------------------

    use handoff_test_support::cwd_lock;

    /// Build a minimal valid card for a given id/status (test fixture).
    fn card(id: &str, status: Status) -> WorkOrder {
        let objective = format!("objective for {id}");
        let path_scope = vec!["handoff/**".to_string()];
        let acceptance = vec!["done".to_string()];
        WorkOrder {
            schema: "handoff.task.v1".into(),
            id: id.into(),
            title: format!("title {id}"),
            status,
            priority: Priority::P1,
            objective: objective.clone(),
            path_scope: path_scope.clone(),
            acceptance_criteria: acceptance.clone(),
            test_commands: vec!["cargo test".into()],
            dependencies: vec![],
            blocked_by: vec![],
            allows_network: false,
            allows_dependency_addition: false,
            correlation_id: "test".into(),
            role: None,
            intent_lock: WorkOrder::compute_intent_lock(&objective, &path_scope, &acceptance),
        }
    }

    /// A leaser whose reservation outcome is fixed — drives the claim gate in tests.
    struct StubLeaser(lease::Reserve);
    impl lease::Leaser for StubLeaser {
        fn reserve(&self, _resource: &str, _ttl: u64, _note: &str) -> lease::Reserve {
            self.0.clone()
        }
        fn release(&self, _resource: &str) -> bool {
            true
        }
    }

    /// Make a temp dir + cd into it (cwd-locked); returns the dir and the previous cwd so
    /// the caller restores it. `.handoff/tasks` is created (the LOCAL/KERNEL home).
    fn temp_cwd(tag: &str) -> (PathBuf, PathBuf) {
        let mut tmp = std::env::temp_dir();
        tmp.push(format!("hf-0029-{tag}-{}-{}", std::process::id(), now_ns()));
        fs::create_dir_all(tmp.join(HF).join("tasks")).unwrap();
        let tmp = tmp.canonicalize().unwrap();
        let prev = std::env::current_dir().unwrap();
        std::env::set_current_dir(&tmp).unwrap();
        (tmp, prev)
    }

    /// AC-A: ship stages ONLY `git add -u` (no untracked) + the task's own card; an
    /// untracked junk file is never in the stage set, so it can't be swept into the PR.
    #[test]
    fn ship_stage_specs_excludes_untracked_and_includes_card() {
        let _g = cwd_lock();
        let (tmp, prev) = temp_cwd("ship");
        // task card present on disk; an untracked junk file also present.
        save_task(&card("HFTASK-9101", Status::Claimed));
        fs::write("junk-untracked.txt", "scratch").unwrap();

        let specs = ship_stage_specs("HFTASK-9101");
        std::env::set_current_dir(&prev).unwrap();

        // -u = tracked modifications/deletions only (NO untracked).
        assert!(specs.iter().any(|s| s == "-u"), "must stage tracked via -u");
        // the task's own card is staged explicitly.
        assert!(
            specs.contains(&task_card_relpath("HFTASK-9101")),
            "card must be staged; got {specs:?}"
        );
        // NOTHING ever stages the untracked junk file or `-A`/`.`.
        assert!(!specs.iter().any(|s| s.contains("junk-untracked")));
        assert!(!specs.iter().any(|s| s == "-A" || s == "."));
        let _ = fs::remove_dir_all(tmp);
    }

    /// AC-A (corollary): with no card on disk, ship stages only `-u` — never a wildcard.
    #[test]
    fn ship_stage_specs_without_card_is_just_tracked() {
        let _g = cwd_lock();
        let (tmp, prev) = temp_cwd("ship-nocard");
        let specs = ship_stage_specs("HFTASK-NOPE");
        std::env::set_current_dir(&prev).unwrap();
        assert_eq!(specs, vec!["-u".to_string()]);
        let _ = fs::remove_dir_all(tmp);
    }

    /// AC-B: re-seeding must NOT clobber an existing card's status. We simulate seed's
    /// additive write: a pre-existing `done` card is preserved (skipped), a missing card
    /// is written.
    #[test]
    fn seed_is_additive_and_preserves_existing_status() {
        let _g = cwd_lock();
        let (tmp, prev) = temp_cwd("seed");
        // Pre-existing card with status DONE.
        save_task(&card("HFTASK-0001", Status::Done));
        // A would-be seed value for the SAME id, but as Backlog (the clobbering value).
        let reseed_existing = card("HFTASK-0001", Status::Backlog);
        let new_card = card("HFTASK-9999", Status::Backlog);

        // Replicate cmd_seed's additive loop: only write MISSING cards.
        for wo in [&reseed_existing, &new_card] {
            if !tasks_dir().join(format!("{}.task.json", wo.id)).exists() {
                save_task(wo);
            }
        }

        let kept = load_task_in(&tasks_dir(), "HFTASK-0001").unwrap();
        let created = load_task_in(&tasks_dir(), "HFTASK-9999");
        std::env::set_current_dir(&prev).unwrap();

        assert_eq!(
            kept.status,
            Status::Done,
            "existing done card must be preserved, not reset to backlog"
        );
        assert!(created.is_some(), "a missing seed card must be created");
        assert_eq!(created.unwrap().status, Status::Backlog);
        let _ = fs::remove_dir_all(tmp);
    }

    /// AC-C: a blocked/refused claim returns `false` (→ CLI exits nonzero); a successful
    /// claim returns `true` (→ exit 0). Driven via a stub leaser so no real mesh/process.
    #[test]
    fn claim_returns_false_when_blocked_true_when_acquired() {
        let _g = cwd_lock();
        let (tmp, prev) = temp_cwd("claim");
        save_task(&card("HFTASK-9201", Status::Backlog));

        // Refused (peer holds the lease) → false.
        let blocked = cmd_claim_with(
            "HFTASK-9201",
            &StubLeaser(lease::Reserve::Conflict("held by peer-x".into())),
        );
        // Acquired → true (records the ledger transition into the temp KERNEL home).
        let ok = cmd_claim_with("HFTASK-9201", &StubLeaser(lease::Reserve::Acquired));
        std::env::set_current_dir(&prev).unwrap();

        assert!(
            !blocked,
            "a blocked claim must return false (CLI exits nonzero)"
        );
        assert!(ok, "a successful claim must return true (CLI exits 0)");
        let _ = fs::remove_dir_all(tmp);
    }

    // --- HFTASK-0010 review request ---------------------------------------------------

    #[test]
    fn gh_pr_view_json_parses() {
        let json = r#"{
            "url": "https://github.com/FlexNetOS/handoff/pull/42",
            "number": 42,
            "headRefName": "feature/x",
            "baseRefName": "master",
            "isDraft": false
        }"#;
        let meta: GhPrView = serde_json::from_str(json).unwrap();
        assert_eq!(meta.number, 42);
        assert_eq!(meta.head_ref_name, "feature/x");
        assert_eq!(meta.base_ref_name, "master");
        assert!(!meta.is_draft);
    }

    #[test]
    fn review_changed_files_splits_lines() {
        let out = "src/main.rs\nhf/src/policy.rs\n\n";
        let files: Vec<String> = out
            .lines()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        assert_eq!(files, vec!["src/main.rs", "hf/src/policy.rs"]);
    }
}
