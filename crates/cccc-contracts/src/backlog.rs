//! A task's reference to a topic in a user-owned workspace document.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BacklogSource {
    pub scope_key: String,
    pub path: String,
    pub entry_id: String,
}

impl BacklogSource {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.scope_key.trim().is_empty()
            || self.path.is_empty()
            || self.path.starts_with('/')
            || self.path.contains(['\\', '\0', '\n', '\r', ':'])
            || self
                .path
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            || !valid_entry_id(&self.entry_id)
        {
            Err(
                "source requires a scope_key, normalized relative path and 12 lowercase hex entry_id",
            )
        } else {
            Ok(())
        }
    }
}

pub fn valid_entry_id(id: &str) -> bool {
    id.len() == 12
        && id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
