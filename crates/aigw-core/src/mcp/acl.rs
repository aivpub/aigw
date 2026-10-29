//! Tool-level ACL for the shared `/mcp` endpoint (Phase 55, Stage 144).
//!
//! Three scope kinds, resolved in a fixed precedence: an exact key rule beats a
//! team rule, which beats an organization rule. **A tool with no matching rule
//! is unrestricted** — ACLs are added progressively, and defaulting to "deny"
//! would mean every newly registered server needs a complete allowlist before a
//! single tool works (stage-144 §3.3).
//!
//! The rule set only ever answers the *call* question. Listing still shows an
//! ACL-denied tool on purpose: hiding it would look like the upstream never
//! offered it, when in fact an administrator forbade it. That is the opposite of
//! the `user_required` case, where hiding is right because the user can fix the
//! condition themselves.
//!
//! `scope_type = key` matches [`KeyIdentity::token_hash`] — that struct has no
//! `key_id`, and `mcp_call_log.key_id` already stores the same hash.

use serde::{Deserialize, Serialize};

use crate::middleware::KeyIdentity;

pub const SCOPE_KEY: &str = "key";
pub const SCOPE_TEAM: &str = "team";
pub const SCOPE_ORG: &str = "org";

/// One row of `mcp_tool_allowlist`, minus `server_id` (every caller is already
/// scoped to one server, and the admin response shape is exactly these four
/// fields). Doubles as the request/response element type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct ToolRule {
    pub tool_name: String,
    pub scope_type: String,
    pub scope_id: String,
    pub allowed: bool,
}

/// Who is asking. Built from [`KeyIdentity`] alone, so there is no parameter a
/// caller could use to claim another key, team or organization.
#[derive(Debug, Clone, Copy)]
pub struct Actor<'a> {
    pub token_hash: &'a str,
    pub team_id: Option<&'a str>,
    pub organization_id: Option<&'a str>,
}

impl<'a> Actor<'a> {
    pub fn from_identity(identity: &'a KeyIdentity) -> Self {
        Actor {
            token_hash: &identity.token_hash,
            team_id: identity.team_id.as_deref(),
            organization_id: identity.organization_id.as_deref(),
        }
    }

    /// The `(scope_type, scope_id)` pairs this actor holds, most specific first.
    fn scopes(&self) -> impl Iterator<Item = (&'static str, &'a str)> {
        Some((SCOPE_KEY, self.token_hash))
            .into_iter()
            .chain(self.team_id.map(|id| (SCOPE_TEAM, id)))
            .chain(self.organization_id.map(|id| (SCOPE_ORG, id)))
    }
}

pub fn is_known_scope(scope_type: &str) -> bool {
    matches!(scope_type, SCOPE_KEY | SCOPE_TEAM | SCOPE_ORG)
}

/// The call-side decision: the most specific matching rule wins, and no
/// matching rule means allowed.
pub fn is_allowed(rules: &[ToolRule], tool_name: &str, actor: &Actor<'_>) -> bool {
    for (scope_type, scope_id) in actor.scopes() {
        if let Some(rule) = rules.iter().find(|rule| {
            rule.tool_name == tool_name
                && rule.scope_type == scope_type
                && rule.scope_id == scope_id
        }) {
            return rule.allowed;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "token-hash-a";
    const OTHER_HASH: &str = "token-hash-b";

    fn rule(tool: &str, scope_type: &str, scope_id: &str, allowed: bool) -> ToolRule {
        ToolRule {
            tool_name: tool.to_string(),
            scope_type: scope_type.to_string(),
            scope_id: scope_id.to_string(),
            allowed,
        }
    }

    fn actor() -> Actor<'static> {
        Actor {
            token_hash: HASH,
            team_id: Some("team-1"),
            organization_id: Some("org-1"),
        }
    }

    #[test]
    fn no_rule_means_unrestricted() {
        assert!(is_allowed(&[], "search", &actor()));
        assert!(is_allowed(
            &[rule("other-tool", SCOPE_KEY, HASH, false)],
            "search",
            &actor()
        ));
    }

    #[test]
    fn a_denied_key_rule_blocks_the_call() {
        let rules = [rule("search", SCOPE_KEY, HASH, false)];
        assert!(!is_allowed(&rules, "search", &actor()));
        assert!(is_allowed(&rules, "other", &actor()));
    }

    #[test]
    fn a_denied_team_rule_blocks_when_the_key_has_no_rule_of_its_own() {
        let rules = [rule("search", SCOPE_TEAM, "team-1", false)];
        assert!(!is_allowed(&rules, "search", &actor()));
    }

    #[test]
    fn a_key_rule_overrides_the_team_rule() {
        let rules = [
            rule("search", SCOPE_KEY, HASH, true),
            rule("search", SCOPE_TEAM, "team-1", false),
        ];
        assert!(is_allowed(&rules, "search", &actor()));
    }

    #[test]
    fn a_denied_key_rule_overrides_an_allowing_team_rule() {
        let rules = [
            rule("search", SCOPE_KEY, HASH, false),
            rule("search", SCOPE_TEAM, "team-1", true),
        ];
        assert!(!is_allowed(&rules, "search", &actor()));
    }

    #[test]
    fn a_team_rule_overrides_the_org_rule() {
        let rules = [
            rule("search", SCOPE_TEAM, "team-1", true),
            rule("search", SCOPE_ORG, "org-1", false),
        ];
        assert!(is_allowed(&rules, "search", &actor()));
    }

    #[test]
    fn an_org_rule_applies_when_nothing_more_specific_matches() {
        let rules = [rule("search", SCOPE_ORG, "org-1", false)];
        assert!(!is_allowed(&rules, "search", &actor()));
    }

    #[test]
    fn rules_for_another_key_team_or_org_do_not_apply() {
        let rules = [
            rule("search", SCOPE_KEY, OTHER_HASH, false),
            rule("search", SCOPE_TEAM, "team-2", false),
            rule("search", SCOPE_ORG, "org-2", false),
        ];
        assert!(is_allowed(&rules, "search", &actor()));
    }

    #[test]
    fn an_actor_without_team_or_org_falls_through_to_its_key_rule() {
        let rules = [
            rule("search", SCOPE_KEY, HASH, false),
            rule("search", SCOPE_TEAM, "team-1", true),
        ];
        let key_only = Actor {
            token_hash: HASH,
            team_id: None,
            organization_id: None,
        };
        assert!(!is_allowed(&rules, "search", &key_only));
    }

    #[test]
    fn only_the_three_scope_kinds_are_accepted() {
        for scope in [SCOPE_KEY, SCOPE_TEAM, SCOPE_ORG] {
            assert!(is_known_scope(scope), "{scope} must be accepted");
        }
        for scope in ["", "user", "global", "KEY"] {
            assert!(!is_known_scope(scope), "{scope} must be rejected");
        }
    }

    #[test]
    fn the_actor_is_taken_from_the_key_identity() {
        let identity = KeyIdentity {
            token_hash: HASH.to_string(),
            key_alias: Some("k".to_string()),
            user_id: Some("u".to_string()),
            team_id: Some("team-1".to_string()),
            organization_id: Some("org-1".to_string()),
            is_master_key: false,
            user_role: None,
        };
        let rules = [rule("search", SCOPE_TEAM, "team-1", false)];
        assert!(!is_allowed(
            &rules,
            "search",
            &Actor::from_identity(&identity)
        ));
    }
}
