use serde_json::{Map, Value};
use std::sync::OnceLock;

const CONTRACT: &str = include_str!("../../../resources/mcp_tools.json");

fn embedded_catalog() -> &'static Vec<Value> {
    static TOOLS: OnceLock<Vec<Value>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        serde_json::from_str(CONTRACT)
            .expect("embedded resources/mcp_tools.json must be valid JSON")
    })
}

pub fn catalog() -> Vec<Value> {
    embedded_catalog().clone()
}

pub fn contains(name: &str) -> bool {
    embedded_catalog()
        .iter()
        .any(|tool| tool["name"].as_str() == Some(name))
}

/// JSON Schema defaults are annotations: MCP clients need not send them.
/// Normalize the action before routing and message/permission classification.
pub fn apply_default_action(name: &str, args: &mut Map<String, Value>) {
    if args.contains_key("action") {
        return;
    }
    if let Some(action) = embedded_catalog()
        .iter()
        .find(|t| t["name"] == name)
        .and_then(|t| t["inputSchema"]["properties"]["action"]["default"].as_str())
    {
        args.insert("action".into(), Value::String(action.into()));
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    #[test]
    fn every_published_tool_has_an_intended_exposure_path() {
        let ordinary = cccc_core::actor_base_tool_names("peer", None);
        let secretary = cccc_core::actor_base_tool_names("voice-secretary", None);
        let temp = tempfile::tempdir().expect("temp");
        let home = cccc_core::HomeLayout::from_path(temp.path()).expect("home");
        let packs = cccc_core::capabilities::CapabilityStore::new(home)
            .catalog()
            .expect("built-in catalog")
            .into_iter()
            .flat_map(|p| p.tool_names);
        let reachable = ordinary
            .chain(secretary)
            .chain(cccc_core::web_model_tool_names())
            .chain(cccc_core::USER_CONTROL_TOOL_NAMES.iter().copied())
            .map(str::to_owned)
            .chain(packs)
            .collect::<BTreeSet<_>>();
        let published = super::catalog()
            .into_iter()
            .map(|t| t["name"].as_str().expect("name").to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            published, reachable,
            "tool definitions and their intended profiles/packs must evolve together"
        );
    }

    #[test]
    fn catalog_is_unique_and_exposes_complete_contract() {
        let catalog = super::catalog();
        let names = catalog
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<BTreeSet<_>>();

        assert_eq!(catalog.len(), 54);
        assert!(names.contains("cccc_connect"));
        assert_eq!(names.len(), catalog.len());
        assert!(names.contains("cccc_code_exec"));
        assert!(names.contains("cccc_memory_admin"));
    }

    #[test]
    fn tool_permissions_are_explicit_and_file_reads_cannot_advertise_sending() {
        let catalog = super::catalog();
        for tool in &catalog {
            for hint in [
                "readOnlyHint",
                "destructiveHint",
                "idempotentHint",
                "openWorldHint",
            ] {
                assert!(
                    tool["annotations"][hint].is_boolean(),
                    "{}: missing {hint}",
                    tool["name"]
                );
            }
            if let Some(insight) = tool["inputSchema"]["properties"].get("insight") {
                assert_eq!(
                    insight["description"],
                    cccc_core::peer_insight::PEER_INSIGHT_FIELD_DESCRIPTION
                );
            }
        }
        for name in ["cccc_file", "cccc_capability_search", "cccc_repo"] {
            let tool = catalog.iter().find(|t| t["name"] == name).expect("tool");
            assert_eq!(tool["annotations"]["readOnlyHint"], true, "{name}");
            assert_eq!(tool["annotations"]["openWorldHint"], false, "{name}");
        }
        for name in [
            "cccc_file_send",
            "cccc_message_reply",
            "cccc_inbox_read",
            "cccc_code_exec",
            "cccc_shell",
        ] {
            let tool = catalog.iter().find(|t| t["name"] == name).expect("tool");
            assert_eq!(tool["annotations"]["readOnlyHint"], false, "{name}");
        }
        let read = catalog
            .iter()
            .find(|t| t["name"] == "cccc_file")
            .expect("read");
        assert_eq!(
            read["inputSchema"]["properties"]["action"]["enum"],
            serde_json::json!(["read", "info", "blob_path"])
        );
        assert_eq!(
            read["inputSchema"]["properties"]["action"]["default"],
            "read"
        );
        assert!(read["inputSchema"]["properties"].get("to").is_none());
        let send = catalog
            .iter()
            .find(|t| t["name"] == "cccc_file_send")
            .expect("send");
        assert!(send["inputSchema"]["properties"].get("action").is_none());
        assert_eq!(send["inputSchema"]["required"], serde_json::json!(["path"]));
    }
}
