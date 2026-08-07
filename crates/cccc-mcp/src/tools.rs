use serde_json::Value;
use std::sync::OnceLock;

const CONTRACT: &str = include_str!("../../../src/cccc/resources/mcp_tools.json");

pub fn catalog() -> Vec<Value> {
    static TOOLS: OnceLock<Vec<Value>> = OnceLock::new();
    TOOLS
        .get_or_init(|| {
            serde_json::from_str(CONTRACT)
                .expect("embedded cccc.resources/mcp_tools.json must be valid JSON")
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    #[test]
    fn catalog_is_unique_and_exposes_complete_contract() {
        let catalog = super::catalog();
        let names = catalog
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect::<BTreeSet<_>>();

        assert_eq!(catalog.len(), 59);
        assert_eq!(names.len(), catalog.len());
        assert!(names.contains("cccc_code_exec"));
        assert!(names.contains("cccc_memory_admin"));
    }
}
