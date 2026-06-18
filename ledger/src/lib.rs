//! `ledger` — the .handoff operational-truth tier.
//!
//! Dual-mode event ledger supporting:
//! - **v2 (default)**: RVF vector-native store with HNSW indexing for semantic recall over session history
//! - **v1 (fallback)**: rusqlite (SQLite/WAL) + rvf-crypto witness chain
//!
//! The v2 implementation uses `rvf-runtime::RvfStore` for vector storage with progressive HNSW indexing,
//! enabling query-by-intent similarity search. When RVF is unavailable or disabled, the system falls back
//! to the proven rusqlite+v1 path.
//!
//! Validates: append work-order lifecycle events, witness each one (tamper-evidence), replay to current state.

#[cfg(feature = "v1")]
mod v1;
#[cfg(feature = "v2")]
mod v2;

#[cfg(all(feature = "v1", not(feature = "v2")))]
pub use v1::*;
#[cfg(feature = "v2")]
pub use v2::*;
