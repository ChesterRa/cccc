//! Device-authenticated Connect directory. Web administration uses separate target credentials.
use serde::{Deserialize, Serialize};

pub const CONNECT_PROTOCOL_VERSION: u32 = 1;
pub const CONNECT_REGISTRATION_DOMAIN: &str = "cccc.connect.register.v1";
pub const MEMBERSHIP_PRODUCT_VERSION_HEADER: &str = "CCCC-Client-Version";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectRegistration {
    pub instance_id: String,
    pub public_key: String,
    pub client_version: String,
    pub public_origin: Option<String>,
    pub issued_at: String,
    pub signature: String,
}

impl ConnectRegistration {
    /// A fixed JSON array avoids object key ordering differences between Rust and JS.
    pub fn signing_material(&self, account_origin: &str, device_id: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([
            CONNECT_REGISTRATION_DOMAIN,
            account_origin,
            device_id,
            self.instance_id,
            self.public_key,
            self.client_version,
            self.public_origin,
            self.issued_at,
        ]))
        .expect("string-only registration material")
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConnectInstance {
    pub instance_id: String,
    /// A new device binding is a new authorization generation, even with the same instance key.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device_id: String,
    pub public_key: String,
    pub client_version: String,
    pub public_origin: Option<String>,
    pub display_name: String,
    pub registered_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConnectDirectory {
    pub protocol_version: u32,
    pub account_id: String,
    pub device_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub instances: Vec<ConnectInstance>,
}

/// A short-lived permission to embed one target page in one entry origin.
/// This is not a Web login and contains no human or device credential.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectFrameProof {
    pub account_origin: String,
    pub source_instance_id: String,
    pub source_device_id: String,
    pub target_instance_id: String,
    pub target_device_id: String,
    pub parent_origin: String,
    pub frame_id: String,
    pub nonce: String,
    pub issued_at: String,
    pub expires_at: String,
    pub signature: String,
}

impl ConnectFrameProof {
    pub fn signing_material(&self) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([
            "cccc.connect.frame.v1",
            self.account_origin,
            self.source_instance_id,
            self.source_device_id,
            self.target_instance_id,
            self.target_device_id,
            self.parent_origin,
            self.frame_id,
            self.nonce,
            self.issued_at,
            self.expires_at,
        ]))
        .expect("string-only frame proof")
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectIdentityProof {
    pub instance_id: String,
    pub device_id: String,
    pub public_origin: Option<String>,
    pub client_version: String,
    pub nonce: String,
    pub expires_at: String,
    pub signature: String,
}

impl ConnectIdentityProof {
    pub fn signing_material(&self) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([
            "cccc.connect.identity.v1",
            self.instance_id,
            self.device_id,
            self.public_origin,
            self.client_version,
            self.nonce,
            self.expires_at,
        ]))
        .expect("string-only identity proof")
    }
}

/// Instance-to-instance operations never carry a human Web token or an arbitrary MCP tool.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConnectPeerOperation {
    Cancel {
        cancellation: Box<crate::connect_message::ConnectCancellation>,
    },
    Deliver {
        message: Box<crate::connect_message::ConnectMessage>,
        blobs: Vec<crate::connect_message::ConnectBlobPayload>,
    },
    Receipt {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        connection_id: Option<String>,
        source_group_id: String,
        target_group_id: String,
        delivery_id: String,
        message_sha256: String,
    },
    Catalog {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        connection_id: Option<String>,
        source_group_id: String,
        #[serde(default)]
        target_group_id: Option<String>,
        #[serde(default)]
        after: Option<String>,
    },
}

impl ConnectPeerOperation {
    pub fn connection_id(&self) -> Option<&str> {
        match self {
            Self::Deliver { message, .. } => message.connection_id.as_deref(),
            Self::Cancel { cancellation } => cancellation.connection_id.as_deref(),
            Self::Catalog { connection_id, .. } | Self::Receipt { connection_id, .. } => {
                connection_id.as_deref()
            }
        }
    }
    pub fn source_group_id(&self) -> &str {
        match self {
            Self::Deliver { message, .. } => &message.source.group_id,
            Self::Cancel { cancellation } => &cancellation.source.group_id,
            Self::Catalog {
                source_group_id, ..
            }
            | Self::Receipt {
                source_group_id, ..
            } => source_group_id,
        }
    }
    /// Used by thin HTTP ports to select the same Group lock as the typed handler.
    pub fn target_group_id(&self) -> Option<&str> {
        match self {
            Self::Catalog {
                target_group_id, ..
            } => target_group_id.as_deref(),
            Self::Deliver { message, .. } => Some(&message.target.group_id),
            Self::Cancel { cancellation } => Some(&cancellation.target.group_id),
            Self::Receipt {
                target_group_id, ..
            } => Some(target_group_id),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ConnectActor {
    pub id: String,
    pub title: String,
    pub generation: String,
    pub enabled: bool,
    pub role: Option<crate::ActorRole>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConnectGroup {
    pub group_id: String,
    pub title: String,
    pub actors: Vec<ConnectActor>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConnectCatalogPage {
    pub groups: Vec<ConnectGroup>,
    pub next: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectPeerAuthorization {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_origin: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub account_id: String,
    pub source_instance_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_device_id: String,
    pub target_instance_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target_device_id: String,
    pub request_id: String,
    pub issued_at: String,
    pub expires_at: String,
    pub operation_sha256: String,
    pub signature: String,
}

impl ConnectPeerAuthorization {
    pub fn signing_material(&self) -> Vec<u8> {
        let mut material = serde_json::json!([
            "cccc.connect.peer.request.v1",
            self.account_origin,
            self.account_id,
            self.source_instance_id,
            self.source_device_id,
            self.target_instance_id,
            self.target_device_id,
            self.request_id,
            self.issued_at,
            self.expires_at,
            self.operation_sha256,
        ]);
        if let Some(id) = &self.connection_id {
            let fields = material.as_array_mut().expect("array material");
            fields[0] = serde_json::json!(if crate::direct::is_direct(id) {
                "cccc.connect.peer.direct.request.v1"
            } else {
                "cccc.connect.peer.group.request.v1"
            });
            fields.push(serde_json::json!(id));
        }
        serde_json::to_vec(&material).expect("serializable Connect peer request")
    }
}

pub const CONNECT_PROOF_HEADER: &str = "CCCC-Connect-Proof";

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectPeerRequest {
    pub proof: ConnectPeerAuthorization,
    pub operation: ConnectPeerOperation,
}

impl ConnectPeerRequest {
    pub fn signing_material(&self) -> Vec<u8> {
        self.proof.signing_material()
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectPeerResponse {
    pub request_id: String,
    pub request_sha256: String,
    pub result: serde_json::Value,
    pub signature: String,
}

impl ConnectPeerResponse {
    pub fn signing_material(&self) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!([
            "cccc.connect.peer.response.v1",
            self.request_id,
            self.request_sha256,
            self.result,
        ]))
        .expect("serializable Connect peer response")
    }
}
