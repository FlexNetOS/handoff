//! EDGE: artifact scaffolding via `minijinja` (design §1/§2). Renders stub
//! `proposal` / `design` / `tasks` / `spec` / `adr` artifacts from embedded
//! templates, injecting a lifecycle context (change name, ADR number/title/date).
//!
//! The templates mirror `intent-driven-template/.../templates/*.md` but are
//! embedded so the scaffolder works in any project without those files present
//! (the same reason the Node OpenSpec CLI embeds its templates). Writing the
//! rendered stubs to disk is the CLI's job (the FS edge).

use minijinja::{context, Environment};

/// Context substituted into a template. Defaults are the canonical placeholder
/// literals, so an un-supplied field renders as a human "fill me in" marker.
#[derive(Debug, Clone)]
pub struct ScaffoldContext {
    /// The change name (proposal/design/tasks title).
    pub change: String,
    /// ADR date (`YYYY-MM-DD` placeholder by default).
    pub date: String,
    /// ADR sequence number (`NNNN` placeholder by default).
    pub number: String,
    /// ADR decision title.
    pub title: String,
}

impl Default for ScaffoldContext {
    fn default() -> Self {
        ScaffoldContext {
            change: "<change-name>".to_string(),
            date: "YYYY-MM-DD".to_string(),
            number: "NNNN".to_string(),
            title: "<Decision title>".to_string(),
        }
    }
}

impl ScaffoldContext {
    /// A context for a named change (proposal/design/tasks).
    pub fn for_change(change: impl Into<String>) -> Self {
        ScaffoldContext {
            change: change.into(),
            ..Default::default()
        }
    }
}

/// Errors rendering a scaffold template.
#[derive(Debug, thiserror::Error)]
pub enum ScaffoldError {
    /// No template for the requested artifact name.
    #[error("unknown artifact {0:?} (expected one of: proposal, spec, design, adr, tasks)")]
    UnknownArtifact(String),
    /// minijinja failed to render.
    #[error("template render failed: {0}")]
    Render(String),
}

const PROPOSAL: &str = "# {{ change }}\n\n\
## Why\n\n\
<!-- Explain the motivation for this change. What problem does this solve? Why now? -->\n\n\
## What Changes\n\n\
<!-- Describe what will change. Be specific about new capabilities, modifications, or removals. -->\n\n\
## Capabilities\n\n\
### New Capabilities\n\
<!-- Capabilities being introduced. Replace <name> with kebab-case identifier (e.g., user-auth, data-export, api-rate-limiting). Each creates specs/<name>/spec.md with OpenSpec delta headers and Gherkin-style scenarios. -->\n\
- `<name>`: <brief description of what this capability covers>\n\n\
### Modified Capabilities\n\
<!-- Existing capabilities whose behaviour is changing (not just implementation).\n\
     Only list here if spec-level behaviour changes. Each needs a delta spec.md file.\n\
     Use existing spec names from openspec/specs/. Leave empty if no requirement changes. -->\n\
- `<existing-name>`: <what behaviour is changing>\n\n\
## Impact\n\n\
<!-- Affected code, APIs, dependencies, systems -->\n";

const DESIGN: &str = "# {{ change }} — Design\n\n\
## Context\n\n\
<!-- Background and current state -->\n\n\
## Goals / Non-Goals\n\n\
**Goals:**\n\
<!-- What this design aims to achieve -->\n\n\
**Non-Goals:**\n\
<!-- What is explicitly out of scope -->\n\n\
## Decisions\n\n\
<!-- Key design decisions and rationale -->\n\n\
## Risks / Trade-offs\n\n\
<!-- Known risks and trade-offs -->\n\n\
## Migration Plan\n\n\
<!-- Deployment, migration, and rollback notes if applicable -->\n\n\
## Open Questions\n\n\
<!-- Outstanding decisions, including any in-force ADRs that need supersession -->\n";

const TASKS: &str = "# {{ change }} — Tasks\n\n\
## 1. <!-- Task Group Name -->\n\n\
- [ ] 1.1 <!-- Task description -->\n\
- [ ] 1.2 <!-- Task description -->\n\n\
## 2. <!-- Task Group Name -->\n\n\
- [ ] 2.1 <!-- Task description -->\n\
- [ ] 2.2 <!-- Task description -->\n";

const SPEC: &str = "## ADDED Requirements\n\n\
### Requirement: <!-- feature or business rule name -->\n\
<!-- Optional Gherkin-style context, for example:\n\
Feature: <capability>\n\
Rule: <business rule>\n\
-->\n\n\
#### Scenario: <!-- scenario name -->\n\
- **GIVEN** <!-- starting context -->\n\
- **WHEN** <!-- action or event -->\n\
- **THEN** <!-- observable outcome -->\n\n\
## MODIFIED Requirements\n\n\
<!-- Copy the full existing requirement block from openspec/specs/<capability>/spec.md, then edit it so it represents the full desired behaviour after the change. -->\n\n\
## REMOVED Requirements\n\n\
### Requirement: <!-- removed feature or business rule name -->\n\
**Reason**: <!-- why this behaviour is removed -->\n\n\
**Migration**: <!-- how users or systems should adapt -->\n";

const ADR: &str = "# {{ number }}. {{ title }}\n\n\
- Status: proposed | accepted | accepted, supersedes ADR-XXXX\n\
- Date: {{ date }}\n\
<!-- Supersedes: ADR-XXXX  (include this line only if this ADR replaces a prior in-force ADR; omit otherwise) -->\n\n\
## Context\n\n\
<!-- Forces at play, constraints, what's prompting this decision. If this ADR supersedes a prior one, explain here why the earlier decision is being revisited; the prior ADR's file is immutable and will not be edited. -->\n\n\
## Decision\n\n\
<!-- The choice being made, stated clearly and unambiguously. -->\n\n\
## Consequences\n\n\
<!-- Positive, negative, and neutral consequences. What becomes easier? What becomes harder? -->\n";

/// The artifact names this scaffolder knows (schema ids; `specs` ⇒ the `spec`
/// delta template).
pub fn artifact_names() -> &'static [&'static str] {
    &["proposal", "spec", "design", "adr", "tasks"]
}

/// The raw (un-rendered) minijinja template for an artifact name. Accepts the
/// schema id `specs` as an alias for `spec`.
pub fn template_for(artifact: &str) -> Option<&'static str> {
    match artifact {
        "proposal" => Some(PROPOSAL),
        "spec" | "specs" => Some(SPEC),
        "design" => Some(DESIGN),
        "adr" => Some(ADR),
        "tasks" => Some(TASKS),
        _ => None,
    }
}

/// Render an artifact stub with the given context.
pub fn render(artifact: &str, ctx: &ScaffoldContext) -> Result<String, ScaffoldError> {
    let tmpl = template_for(artifact)
        .ok_or_else(|| ScaffoldError::UnknownArtifact(artifact.to_string()))?;
    let mut env = Environment::new();
    // Keep the template's trailing newline so a scaffolded file is well-formed.
    env.set_keep_trailing_newline(true);
    env.render_str(
        tmpl,
        context! {
            change => ctx.change,
            date => ctx.date,
            number => ctx.number,
            title => ctx.title,
        },
    )
    .map_err(|e| ScaffoldError::Render(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_artifact_errors() {
        let err = render("nope", &ScaffoldContext::default()).unwrap_err();
        assert!(matches!(err, ScaffoldError::UnknownArtifact(_)));
    }

    #[test]
    fn proposal_injects_change_name() {
        let out = render("proposal", &ScaffoldContext::for_change("add-json-export")).unwrap();
        assert!(out.starts_with("# add-json-export\n"), "{out}");
        assert!(out.contains("## Why"));
        assert!(out.contains("## What Changes"));
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn adr_injects_number_title_date() {
        let ctx = ScaffoldContext {
            number: "0007".to_string(),
            title: "Adopt event sourcing".to_string(),
            date: "2026-06-04".to_string(),
            ..Default::default()
        };
        let out = render("adr", &ctx).unwrap();
        assert!(out.starts_with("# 0007. Adopt event sourcing\n"), "{out}");
        assert!(out.contains("- Date: 2026-06-04"), "{out}");
    }

    #[test]
    fn spec_alias_and_passthrough() {
        // `specs` (schema id) maps to the spec delta template.
        let a = render("specs", &ScaffoldContext::default()).unwrap();
        let b = render("spec", &ScaffoldContext::default()).unwrap();
        assert_eq!(a, b);
        assert!(a.contains("## ADDED Requirements"));
        assert!(a.contains("#### Scenario:"));
    }

    #[test]
    fn rendered_proposal_parses_as_markdown_stub() {
        // The rendered stub must be valid enough that `spec show`/parse handles it.
        let out = render("proposal", &ScaffoldContext::for_change("demo")).unwrap();
        // No leftover jinja delimiters.
        assert!(!out.contains("{{"), "no unrendered template vars: {out}");
        assert!(!out.contains("}}"));
    }
}
