//! Requirement and Scenario — the structural units of a spec.

use super::Block;

/// A `### Requirement: <name>` block: the prose body between the heading and the
/// first scenario, plus the ordered scenarios.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Requirement {
    /// Text after `### Requirement: ` (stored un-normalized; use
    /// [`super::normalize_name`] for matching).
    pub name: String,
    /// Prose blocks between the heading and the first scenario.
    pub body: Vec<Block>,
    /// Ordered scenarios (order is preserved through merge/emit).
    pub scenarios: Vec<Scenario>,
}

impl Requirement {
    pub fn new(name: impl Into<String>) -> Self {
        Requirement {
            name: name.into(),
            body: Vec::new(),
            scenarios: Vec::new(),
        }
    }

    /// Does the requirement body text contain a `SHALL` or `MUST` keyword?
    /// (Word-boundary-ish: substring match on the raw body text, matching the
    /// oracle's keyword rule.)
    pub fn has_shall_or_must(&self) -> bool {
        self.body
            .iter()
            .any(|b| b.text.contains("SHALL") || b.text.contains("MUST"))
    }
}

/// A `#### Scenario: <name>` block. `steps` are the GIVEN/WHEN/THEN bullets as
/// opaque blocks — content-lint targets, not parsed semantically (design §3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scenario {
    /// Text after `#### Scenario: `.
    pub name: String,
    pub steps: Vec<Block>,
}

impl Scenario {
    pub fn new(name: impl Into<String>) -> Self {
        Scenario {
            name: name.into(),
            steps: Vec::new(),
        }
    }
}
