//! HFTASK-0013: envctl secrets-engine seam (experimental, feature-gated).
//!
//! This module provides the deterministic merge-gate decision helper that will eventually
//! sit beneath the §5b AI gatekeeper. It is intentionally lightweight for the first slice:
//! it uses the secrets-engine `broker::decide` logic directly without requiring a live vault
//! or relay bearer lifecycle.

use envctl_secrets_engine::broker::{
    decide::{decide, CanonRequest, RelayDecision},
    policy::{Method, Provider, RelayKind, RelayPolicy, SwapMode},
    VerifiedBearer,
};

/// Build the default GitHub merge-gate relay policy.
///
/// Default-deny except for `api.github.com` requests under `/repos/**` using GET or POST.
fn github_merge_policy() -> RelayPolicy {
    RelayPolicy {
        relay_id: "hf-merge-gate".into(),
        kind: RelayKind::Named,
        provider: Provider::Github,
        secret_name: "github".into(),
        swap: SwapMode::BaseUrlRepoint {
            upstream_base: "https://api.github.com".into(),
        },
        host_allow: vec!["api.github.com".into()],
        path_allow: vec!["/repos/".into()],
        method_allow: vec![Method::Get, Method::Post],
        policy_ttl_secs: 24 * 60 * 60,
        rate_per_min: None,
        quota_total_requests: None,
        quota_total_bytes: None,
        enabled: true,
        revoked: false,
    }
}

/// A synthetic, never-expired local bearer for the decision helper.
///
/// This is a test/dev stub: in production the bearer is minted from the live vault DEK,
/// bound to the peer uid/pid, and clamped to <=24h.
fn synthetic_bearer() -> VerifiedBearer {
    VerifiedBearer {
        policy_id: 1,
        token_id: "hf-merge-gate-stub".into(),
        expires_at_ms: i64::MAX,
        issued_at_ms: 0,
        issued_boottime_ms: 0,
        client_uid: None,
        client_pid: None,
        client_id: None,
        dpop_jkt: None,
        revoked: false,
    }
}

/// Parse an HTTP method string into the secrets-engine `Method` enum.
fn parse_method(method: &str) -> Option<Method> {
    match method.to_uppercase().as_str() {
        "GET" => Some(Method::Get),
        "HEAD" => Some(Method::Head),
        "POST" => Some(Method::Post),
        "PUT" => Some(Method::Put),
        "PATCH" => Some(Method::Patch),
        "DELETE" => Some(Method::Delete),
        "CONNECT" => Some(Method::Connect),
        "OPTIONS" => Some(Method::Options),
        _ => None,
    }
}

/// Deterministic merge-gate decision for GitHub API egress.
///
/// Returns `Ok(true)` if the request is allowed by the default GitHub merge-gate policy,
/// `Ok(false)` if denied, and `Err(...)` if the method/path are malformed.
///
/// This is a dev/stub seam: it supplies a synthetic bearer and zero usage budgets so the
/// decision depends only on host/path/method allowlists and policy state.
pub fn github_merge_gate(method: &str, host: &str, path: &str) -> Result<bool, String> {
    let method = parse_method(method).ok_or_else(|| format!("unknown HTTP method: {method}"))?;
    if !path.starts_with('/') {
        return Err(format!("path must start with '/': {path}"));
    }
    let req = CanonRequest {
        method,
        host: host.to_lowercase(),
        sni: None,
        path: path.into(),
        bytes_out: 0,
        peer_uid: None,
        peer_pid: None,
        usage_requests: 1,
        usage_bytes: 0,
        rate_in_window: 1,
        remote: None,
    };
    let policy = github_merge_policy();
    let bearer = synthetic_bearer();
    let decision = decide(
        &policy, &bearer, &req, 1,    // now_ms
        1,    // boottime_now_ms
        None, // gate_absent_since_ms
        0,    // issuance_floor_ms
    );
    Ok(matches!(decision, RelayDecision::Allow))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_api_repos_get_is_allowed() {
        assert!(github_merge_gate("GET", "api.github.com", "/repos/FlexNetOS/handoff").unwrap());
    }

    #[test]
    fn github_api_repos_post_is_allowed() {
        assert!(
            github_merge_gate("POST", "api.github.com", "/repos/FlexNetOS/handoff/issues").unwrap()
        );
    }

    #[test]
    fn unknown_host_is_denied() {
        assert!(!github_merge_gate("GET", "evil.com", "/repos/FlexNetOS/handoff").unwrap());
    }

    #[test]
    fn github_api_admin_endpoint_is_denied() {
        assert!(!github_merge_gate("GET", "api.github.com", "/admin/users").unwrap());
    }

    #[test]
    fn github_delete_is_denied() {
        assert!(
            !github_merge_gate("DELETE", "api.github.com", "/repos/FlexNetOS/handoff").unwrap()
        );
    }

    #[test]
    fn malformed_method_errors() {
        assert!(github_merge_gate("FOO", "api.github.com", "/repos/x").is_err());
    }

    #[test]
    fn relative_path_errors() {
        assert!(github_merge_gate("GET", "api.github.com", "repos/x").is_err());
    }
}
