//! The validation report types. These carry the serde derives (the edge); the
//! pure model never does. The JSON shape mirrors the oracle fixtures
//! (`04`/`05`): `{ items:[{id,type,valid,issues:[{level,path,message}],
//! durationMs}], summary:{totals,byType}, version }`.

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum IssueLevel {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Issue {
    pub level: IssueLevel,
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Item {
    pub id: String,
    #[serde(rename = "type")]
    pub item_type: String,
    pub valid: bool,
    pub issues: Vec<Issue>,
    /// Nulled out in tests (the oracle's timing is not reproducible). Always
    /// emitted so the shape matches the fixtures.
    #[serde(rename = "durationMs")]
    pub duration_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub items: u64,
    pub passed: u64,
    pub failed: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Summary {
    pub totals: Counts,
    #[serde(rename = "byType")]
    pub by_type: std::collections::BTreeMap<String, Counts>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Report {
    pub items: Vec<Item>,
    pub summary: Summary,
    pub version: String,
}

impl Report {
    /// Build a single-item report (the common case: validate one spec) and
    /// compute the summary from it.
    pub fn single(item: Item) -> Self {
        let passed = u64::from(item.valid);
        let failed = u64::from(!item.valid);
        let counts = Counts {
            items: 1,
            passed,
            failed,
        };
        let mut by_type = std::collections::BTreeMap::new();
        by_type.insert(item.item_type.clone(), counts.clone());
        Report {
            items: vec![item],
            summary: Summary {
                totals: counts,
                by_type,
            },
            version: "1.0".to_string(),
        }
    }
}
