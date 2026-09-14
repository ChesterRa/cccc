//! Member-approved, non-transitive Group connections.
use crate::connect::ConnectInstance;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectGroupTicket {
    pub account_origin: String,
    pub account_id: String,
    pub instance_id: String,
    pub instance_name: String,
    pub device_id: String,
    pub group_id: String,
    pub group_generation: String,
    pub title: String,
    pub issued_at: String,
    pub expires_at: String,
    pub signature: String,
}

impl ConnectGroupTicket {
    pub fn signing_material(&self) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([
            "cccc.connect.group.ticket.v1",
            self.account_origin,
            self.account_id,
            self.instance_id,
            self.instance_name,
            self.device_id,
            self.group_id,
            self.group_generation,
            self.title,
            self.issued_at,
            self.expires_at
        ]))
        .expect("serializable Group ticket")
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectGroupCheck {
    pub ticket: ConnectGroupTicket,
    pub nonce: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectGroupCheckResult {
    pub ticket_sha256: String,
    pub nonce: String,
    pub title: String,
    pub expires_at: String,
    pub signature: String,
}

impl ConnectGroupCheckResult {
    pub fn signing_material(&self) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([
            "cccc.connect.group.check.v1",
            self.ticket_sha256,
            self.nonce,
            self.title,
            self.expires_at
        ]))
        .expect("serializable Group check")
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConnectGroupEndpoint {
    pub account_id: String,
    pub instance: ConnectInstance,
    pub group_id: String,
    pub group_generation: String,
    pub title: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConnectGroupLink {
    pub id: String,
    pub source: ConnectGroupEndpoint,
    pub target: ConnectGroupEndpoint,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConnectGroupLinks {
    pub protocol_version: u32,
    pub account_origin: String,
    pub account_id: String,
    pub device_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub links: Vec<ConnectGroupLink>,
}
