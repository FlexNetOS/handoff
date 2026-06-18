//! EDGE: Architecture Decision Record parsing + the supersession-graph walk
//! (design §2, schema `adr` artifact). ADRs are immutable, append-only files at
//! the repo-level `adr/`; a later ADR supersedes an earlier one via its
//! `Status: accepted, supersedes ADR-NNNN` and/or a `Supersedes: ADR-NNNN` line.
//!
//! This module is pure: it parses `(filename, source)` pairs into [`Adr`]s and
//! computes the in-force set / next number. Reading the `adr/` directory is the
//! CLI's job (the FS edge).

/// The lifecycle status of an ADR (the leading word of its `Status:` line).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdrStatus {
    /// `proposed` — not yet in force.
    Proposed,
    /// `accepted` (optionally "accepted, supersedes ADR-NNNN").
    Accepted,
    /// Anything else (e.g. `rejected`, `deprecated`), kept verbatim.
    Other(String),
}

/// A parsed ADR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adr {
    /// The NNNN sequence number (from the filename, else the `# NNNN.` heading).
    pub number: u32,
    /// The decision title (heading text after `NNNN.`).
    pub title: String,
    pub status: AdrStatus,
    /// ADR numbers this ADR supersedes (from the Status and/or Supersedes line).
    pub supersedes: Vec<u32>,
}

impl Adr {
    /// Is this ADR accepted (a precondition for being in force)?
    pub fn is_accepted(&self) -> bool {
        self.status == AdrStatus::Accepted
    }
}

/// Parse one ADR from its filename and Markdown source. Returns `None` if no
/// sequence number can be determined (filename or heading).
pub fn parse_adr(filename: &str, src: &str) -> Option<Adr> {
    let number = number_from_filename(filename).or_else(|| number_from_heading(src))?;
    let title = title_from_heading(src).unwrap_or_default();

    let mut status = AdrStatus::Other(String::new());
    let mut supersedes: Vec<u32> = Vec::new();
    for raw in src.lines() {
        let line = raw.trim().trim_start_matches('-').trim();
        if let Some(rest) = line.strip_prefix("Status:") {
            let val = rest.trim();
            let lead = val
                .split([',', ' '])
                .next()
                .unwrap_or("")
                .to_ascii_lowercase();
            status = match lead.as_str() {
                "proposed" => AdrStatus::Proposed,
                "accepted" => AdrStatus::Accepted,
                _ => AdrStatus::Other(val.to_string()),
            };
            collect_adr_refs(val, &mut supersedes);
        } else if let Some(rest) = line.strip_prefix("Supersedes:") {
            collect_adr_refs(rest, &mut supersedes);
        }
    }
    supersedes.sort_unstable();
    supersedes.dedup();

    Some(Adr {
        number,
        title,
        status,
        supersedes,
    })
}

/// Leading digits of a filename (`0042-use-postgres.md` → 42).
fn number_from_filename(filename: &str) -> Option<u32> {
    let stem = filename.rsplit('/').next().unwrap_or(filename);
    let digits: String = stem.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// The first `# ...` heading's leading number (`# 0042. Title` → 42).
fn number_from_heading(src: &str) -> Option<u32> {
    let h = heading(src)?;
    let digits: String = h.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// The first `# ...` heading's title text, after an optional `NNNN.` prefix.
fn title_from_heading(src: &str) -> Option<String> {
    let h = heading(src)?;
    // Strip a leading "NNNN." or "NNNN" prefix and following punctuation/space.
    let rest = h.trim_start_matches(|c: char| c.is_ascii_digit());
    let rest = rest.trim_start_matches(['.', ' ', ':', '-']).trim();
    Some(if rest.is_empty() { h } else { rest.to_string() })
}

/// The text of the first `# ` (H1) heading.
fn heading(src: &str) -> Option<String> {
    src.lines()
        .map(str::trim_start)
        .find_map(|l| l.strip_prefix("# ").map(|t| t.trim().to_string()))
}

/// Collect every `ADR-NNNN` reference in `s` into `out`.
fn collect_adr_refs(s: &str, out: &mut Vec<u32>) {
    let mut rest = s;
    while let Some(idx) = rest.find("ADR-") {
        rest = &rest[idx + 4..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = digits.parse::<u32>() {
            out.push(n);
        }
    }
}

/// A collection of ADRs supporting supersession queries.
#[derive(Debug, Clone, Default)]
pub struct AdrSet {
    pub adrs: Vec<Adr>,
}

impl AdrSet {
    pub fn new(adrs: Vec<Adr>) -> Self {
        AdrSet { adrs }
    }

    /// The set of ADR numbers that some other ADR supersedes.
    pub fn superseded_numbers(&self) -> std::collections::BTreeSet<u32> {
        self.adrs
            .iter()
            .flat_map(|a| a.supersedes.iter().copied())
            .collect()
    }

    /// Is ADR `number` superseded by any ADR in the set?
    pub fn is_superseded(&self, number: u32) -> bool {
        self.adrs.iter().any(|a| a.supersedes.contains(&number))
    }

    /// The currently in-force ADRs: accepted and not superseded, sorted by
    /// number. These are the live architectural commitments (design step `adr`).
    pub fn in_force(&self) -> Vec<&Adr> {
        let superseded = self.superseded_numbers();
        let mut v: Vec<&Adr> = self
            .adrs
            .iter()
            .filter(|a| a.is_accepted() && !superseded.contains(&a.number))
            .collect();
        v.sort_by_key(|a| a.number);
        v
    }

    /// The next sequence number (max existing + 1; 1 if empty). Monotonic across
    /// the whole repo and never reused.
    pub fn next_number(&self) -> u32 {
        self.adrs
            .iter()
            .map(|a| a.number)
            .max()
            .map_or(1, |m| m + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn adr(filename: &str, status: &str, supersedes_line: Option<&str>) -> Adr {
        let mut src = format!(
            "# {} Some Decision\n\n- Status: {status}\n- Date: 2026-01-01\n",
            filename_num(filename)
        );
        if let Some(s) = supersedes_line {
            src.push_str(&format!("- Supersedes: {s}\n"));
        }
        src.push_str("\n## Context\nstuff\n");
        parse_adr(filename, &src).unwrap()
    }

    fn filename_num(filename: &str) -> &str {
        filename.split('-').next().unwrap()
    }

    #[test]
    fn parses_number_title_status() {
        let a = parse_adr(
            "0042-use-postgres.md",
            "# 0042. Use Postgres for the catalog\n\n- Status: accepted\n- Date: 2026-01-01\n\n## Context\nx\n",
        )
        .unwrap();
        assert_eq!(a.number, 42);
        assert_eq!(a.title, "Use Postgres for the catalog");
        assert_eq!(a.status, AdrStatus::Accepted);
        assert!(a.supersedes.is_empty());
    }

    #[test]
    fn parses_supersedes_from_status_and_line() {
        let a = parse_adr(
            "0043-switch-db.md",
            "# 0043. Switch DB\n\n- Status: accepted, supersedes ADR-0042\n- Supersedes: ADR-0041\n\n## Context\n",
        )
        .unwrap();
        assert_eq!(a.number, 43);
        assert_eq!(a.status, AdrStatus::Accepted);
        assert_eq!(a.supersedes, vec![41, 42]);
    }

    #[test]
    fn in_force_excludes_superseded_and_proposed() {
        let set = AdrSet::new(vec![
            adr("0001-a.md", "accepted", None),
            adr("0002-b.md", "accepted, supersedes ADR-0001", None),
            adr("0003-c.md", "proposed", None),
        ]);
        let in_force: Vec<u32> = set.in_force().iter().map(|a| a.number).collect();
        // 0001 superseded by 0002; 0003 only proposed → only 0002 is in force.
        assert_eq!(in_force, vec![2]);
        assert!(set.is_superseded(1));
        assert!(!set.is_superseded(2));
    }

    #[test]
    fn next_number_is_max_plus_one() {
        let set = AdrSet::new(vec![
            adr("0001-a.md", "accepted", None),
            adr("0007-b.md", "accepted", None),
        ]);
        assert_eq!(set.next_number(), 8);
        assert_eq!(AdrSet::default().next_number(), 1);
    }
}
