use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

use super::Capability;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapabilityState {
    #[serde(default)]
    pub enabled: BTreeSet<String>,
    #[serde(default)]
    pub disabled: BTreeSet<String>,
    #[serde(default)]
    pub blocked: BTreeSet<String>,
    #[serde(default)]
    pub unblocked: BTreeSet<String>,
    #[serde(default)]
    pub hidden: BTreeSet<String>,
    #[serde(default)]
    pub visible: BTreeSet<String>,
    #[serde(default)]
    pub custom: BTreeMap<String, Capability>,
}
