//! EDGE: archive orchestration — validate → per-delta (parse, merge, emit) →
//! abort-all-on-any-failure → (caller moves the dir). Transactional across all
//! specs: if ANY merge errors, nothing is emitted (contract §6, design §5).
//!
//! The actual filesystem move is intentionally NOT done here (slice 7 / CLI
//! owns I/O); this module computes the merged outputs and op counts purely from
//! in-memory `(base, delta)` pairs so it is unit-testable.

use crate::model::{apply_delta, sync_delta, Delta, DeltaOp, MergeError, SpecDoc};
use crate::parse::{emit_spec, parse_delta, parse_spec};

/// Per-spec op counts (`+added / ~modified / -removed / →renamed`), mirroring
/// the CLI's per-spec summary line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpCounts {
    pub added: usize,
    pub modified: usize,
    pub removed: usize,
    pub renamed: usize,
}

impl OpCounts {
    fn of(delta: &Delta) -> Self {
        let mut c = OpCounts::default();
        for op in &delta.ops {
            match op {
                DeltaOp::Added(_) => c.added += 1,
                DeltaOp::Modified(_) => c.modified += 1,
                DeltaOp::Removed { .. } => c.removed += 1,
                DeltaOp::Renamed { .. } => c.renamed += 1,
            }
        }
        c
    }
}

/// One spec to merge during an archive: the capability id, the base spec source,
/// and the change's delta source.
pub struct SpecMerge<'a> {
    pub capability: &'a str,
    pub base_src: &'a str,
    pub delta_src: &'a str,
}

/// The merged result for one spec: the emitted Markdown and its op counts.
#[derive(Clone, Debug)]
pub struct MergedSpec {
    pub capability: String,
    pub markdown: String,
    pub counts: OpCounts,
}

/// An archive failure attributed to a specific capability.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{capability}: {source}")]
pub struct ArchiveError {
    pub capability: String,
    #[source]
    pub source: MergeError,
}

/// Merge a single `(base, delta)` pair in memory: parse both, apply the delta,
/// emit. Returns the merged Markdown + counts, or the merge error.
pub fn merge_one(
    base_src: &str,
    delta_src: &str,
) -> Result<(SpecDoc, String, OpCounts), MergeError> {
    let base = parse_spec(base_src);
    let delta = parse_delta(delta_src);
    let merged = apply_delta(&base, &delta)?;
    let markdown = emit_spec(&merged);
    Ok((merged, markdown, OpCounts::of(&delta)))
}

/// Orchestrate one sync (intelligent merge): parse → sync_delta → emit.
pub fn sync_one(
    base_src: &str,
    delta_src: &str,
) -> Result<(SpecDoc, String, OpCounts), MergeError> {
    let base = parse_spec(base_src);
    let delta = parse_delta(delta_src);
    let merged = sync_delta(&base, &delta)?;
    let markdown = emit_spec(&merged);
    Ok((merged, markdown, OpCounts::of(&delta)))
}

/// Transactional archive driver. Merges EVERY spec first; only if all succeed
/// does it return the merged outputs (the caller then writes them and moves the
/// change dir). If any spec fails, returns `Err` and the caller writes nothing
/// — reproducing the CLI's `"Aborted. No files were changed."`.
pub fn archive_specs(specs: &[SpecMerge<'_>]) -> Result<Vec<MergedSpec>, ArchiveError> {
    let mut out = Vec::with_capacity(specs.len());
    for spec in specs {
        match merge_one(spec.base_src, spec.delta_src) {
            Ok((_, markdown, counts)) => out.push(MergedSpec {
                capability: spec.capability.to_string(),
                markdown,
                counts,
            }),
            Err(source) => {
                return Err(ArchiveError {
                    capability: spec.capability.to_string(),
                    source,
                })
            }
        }
    }
    Ok(out)
}
