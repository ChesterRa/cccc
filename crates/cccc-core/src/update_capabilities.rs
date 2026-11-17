//! Explicit capabilities of an update candidate. Missing declarations do not
//! establish compatibility with retained owners or their managed MCPs.

use serde::{Deserialize, Serialize};

/// This response is emitted only before a shutdown is selected. Other failures
/// do not authorize withdrawing an update's startup fence.
pub const PRESERVATION_REFUSED: &str = "keep_preflight_refused";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct UpdateCapabilities {
    pub local_restart: bool,
    pub preserve_agents: bool,
    pub intent_schemas: Vec<u32>,
    pub owner_contracts: Vec<u32>,
    pub mcp_registration_apis: Vec<u32>,
    pub mcp_daemon_contracts: Vec<u32>,
}

/// Keep the declaration with the image that produced it. A final stop must
/// not use another check's capabilities for the intent's selected build.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct UpdateCandidate {
    pub build: crate::attended_update::BuildIdentity,
    pub capabilities: UpdateCapabilities,
}

/// A preview is not authority to stop: the daemon repeats this decision at
/// its final shutdown boundary against the current owner observations.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeepDecision {
    pub allowed: bool,
    pub refusals: Vec<KeepRefusal>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct KeepRefusal {
    pub code: String,
    pub group_id: Option<String>,
    pub actor_id: Option<String>,
    pub owner_id: Option<String>,
    pub display_name: Option<String>,
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_declaration_leaves_every_missing_capability_unknown() {
        let partial: UpdateCapabilities = serde_json::from_value(serde_json::json!({
            "mcp_daemon_contracts": [2],
            "additional_capability": {"revision": 3}
        }))
        .expect("read only the declarations this build understands");
        assert_eq!(partial.mcp_daemon_contracts, vec![2]);
        assert!(!partial.local_restart);
        assert!(!partial.preserve_agents);
        assert!(partial.intent_schemas.is_empty());
        assert!(partial.owner_contracts.is_empty());
        assert!(partial.mcp_registration_apis.is_empty());
        let absent: UpdateCapabilities = serde_json::from_value(serde_json::json!({}))
            .expect("no declarations is not a parse error");
        assert_eq!(absent, UpdateCapabilities::default());
    }
}
