use cccc_core::{GroupStore, HomeLayout};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Debug, Serialize)]
pub(super) struct DeliveryFailure {
    pub(super) stage: &'static str,
    pub(super) error: String,
}

#[derive(Debug, Default, Serialize)]
pub(super) struct AttachmentDeliveryReport {
    pub(super) delivered_targets: usize,
    pub(super) delivered_chat_ids: HashSet<String>,
    pub(super) failed_chat_ids: HashSet<String>,
    pub(super) failures: Vec<DeliveryFailure>,
}

impl AttachmentDeliveryReport {
    pub(super) fn fail(&mut self, stage: &'static str, error: String) {
        self.failures.push(DeliveryFailure { stage, error });
    }
}

pub(super) fn persist_failures(
    home: &HomeLayout,
    group_id: &str,
    report: &AttachmentDeliveryReport,
) {
    if report.failures.is_empty() {
        return;
    }
    let Ok(store) = GroupStore::new(home.clone()) else {
        return;
    };
    let error = format!("DingTalk attachment delivery failed: {}", json!(report));
    let _ = cccc_core::im_state::update(&store, group_id, |value| {
        if !value.is_object() {
            *value = Value::Object(Default::default());
        }
        value["attachment_delivery"] = json!(report);
        value["last_error"] = json!(error);
        Ok(())
    });
}
