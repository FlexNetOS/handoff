// HFTASK-0080 (ADR-0019 D5 #3): error-handling deny lints allowed under test only (tests assert).
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]
//! `ledger` — the .handoff operational-truth tier.
//!
//! Pure-Rust event ledger (ADR-0017 / HFTASK-0053 — no C in the trust boundary):
//! - **redb-store (default)**: the authoritative transactional store on `redb`
//!   (pure-Rust, ACID, single-writer serializable) + the `rvf-crypto` witness chain.
//!   Provides the witnessed append-only hash-chain, replay, atomic lease CAS, and rollup
//!   provenance.
//! - **v2 (overlay, opt-in)**: layers `rvf-runtime::RvfStore` on top for vector-native
//!   semantic recall (HNSW `query_by_intent`). Every authoritative method delegates to the
//!   redb store; the RVF overlay only adds recall.
//! - **legacy-sqlite (non-default)**: a one-time read-only importer that migrates an existing
//!   bundled-C-SQLite `ledger.db` into redb, re-verifying the witness chain on the way in.
//!
//! The previous bundled C-SQLite (`rusqlite`) backend has been retired from the default graph;
//! tamper-evidence and all integrity invariants are preserved byte-for-byte on the redb store.

// The authoritative redb store lives in `v1` (module name kept for minimal churn / history).
#[cfg(feature = "redb-store")]
mod v1;
#[cfg(feature = "v2")]
mod v2;

#[cfg(feature = "legacy-sqlite")]
pub mod migrate;
#[cfg(feature = "legacy-sqlite")]
pub use migrate::migrate_sqlite_to_redb;

// ADR-0018 D1: deterministic JSONL export/import — the committed continuity truth (binary redb
// stays a local cache). Always compiled (no C dependency; pure serde over the event store).
pub mod export;
pub use export::{export_jsonl, rebuild_from_jsonl};

// Default + redb-store-only build: export the authoritative store directly.
#[cfg(all(feature = "redb-store", not(feature = "v2")))]
pub use v1::*;
// v2 build: export the overlay (which re-exports the authoritative types from v1).
#[cfg(feature = "v2")]
pub use v2::*;
