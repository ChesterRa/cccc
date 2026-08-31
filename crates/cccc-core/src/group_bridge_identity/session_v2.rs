use super::*;

impl GroupBridgeIdentity {
    pub fn sign_session_hello_v2(
        &self,
        target_group_id: &str,
        src_group_id: &str,
        challenge: &Value,
    ) -> io::Result<Value> {
        let mut hello = json!({
            "target_group_id":target_group_id.trim(),
            "src_group_id":src_group_id.trim(),
            "remote_peer_id":self.peer_id,
            "message_contract_version":GROUP_BRIDGE_MESSAGE_CONTRACT_VERSION,
            "challenge_nonce":challenge["nonce"].as_str().unwrap_or("").trim(),
            "challenge_issued_at":challenge["issued_at"].as_str().unwrap_or("").trim(),
            "client_nonce":uuid::Uuid::new_v4().simple().to_string(),
        });
        let material = session_hello_v2_material(&hello)?;
        self.sign_v2(&mut hello, material)?;
        Ok(hello)
    }

    pub fn sign_session_challenge_v2(&self, challenge: &mut Value) -> io::Result<()> {
        challenge["server_peer_id"] = json!(self.peer_id);
        let material = session_challenge_v2_material(challenge)?;
        self.sign_v2(challenge, material)
    }

    pub fn sign_session_ready_v2(
        &self,
        ready: &mut Value,
        hello: &Value,
        challenge: &Value,
    ) -> io::Result<()> {
        ready["protocol"] = json!(SESSION_PROTOCOL_V2);
        ready["server_peer_id"] = json!(self.peer_id);
        let material = session_ready_v2_material(ready, hello, challenge)?;
        self.sign_v2(ready, material)
    }

    fn sign_v2(&self, value: &mut Value, material: Vec<u8>) -> io::Result<()> {
        let key = Ed25519KeyPair::from_seed_unchecked(&self.private_key)
            .map_err(|error| io::Error::other(error.to_string()))?;
        value["public_key"] = json!(self.public_key_b64);
        value["signature"] = json!(encode(key.sign(&material).as_ref()));
        Ok(())
    }
}

pub fn session_hello_v2_material(hello: &Value) -> io::Result<Vec<u8>> {
    serde_json::to_vec(&json!({
        "protocol":SESSION_PROTOCOL_V2,
        "message_contract_version":hello["message_contract_version"],
        "remote_peer_id":text(hello, "remote_peer_id"),
        "src_group_id":text(hello, "src_group_id"),
        "target_group_id":text(hello, "target_group_id"),
        "challenge_nonce":text(hello, "challenge_nonce"),
        "challenge_issued_at":text(hello, "challenge_issued_at"),
        "client_nonce":text(hello, "client_nonce"),
    }))
    .map_err(io::Error::other)
}

pub fn session_challenge_v2_material(challenge: &Value) -> io::Result<Vec<u8>> {
    serde_json::to_vec(&json!({
        "protocol":SESSION_PROTOCOL_V2,
        "message_contract_version":challenge["message_contract_version"],
        "nonce":text(challenge, "nonce"),
        "issued_at":text(challenge, "issued_at"),
        "expires_at":text(challenge, "expires_at"),
        "server_peer_id":text(challenge, "server_peer_id"),
    }))
    .map_err(io::Error::other)
}

pub fn session_ready_v2_material(
    ready: &Value,
    hello: &Value,
    challenge: &Value,
) -> io::Result<Vec<u8>> {
    serde_json::to_vec(&json!({
        "type":ready["type"],
        "protocol":ready["protocol"],
        "message_contract_version":ready["message_contract_version"],
        "server_peer_id":text(ready, "server_peer_id"),
        "remote_peer_id":text(hello, "remote_peer_id"),
        "src_group_id":text(hello, "src_group_id"),
        "target_group_id":text(hello, "target_group_id"),
        "challenge_nonce":text(challenge, "nonce"),
        "challenge_issued_at":text(challenge, "issued_at"),
        "challenge_signature":text(challenge, "signature"),
        "client_nonce":text(hello, "client_nonce"),
        "hello_signature":text(hello, "signature"),
    }))
    .map_err(io::Error::other)
}

pub fn authenticated_session_v2_peer_id(hello: &Value, challenge: &Value) -> Option<String> {
    if hello["challenge_nonce"] != challenge["nonce"]
        || hello["challenge_issued_at"] != challenge["issued_at"]
        || !nonce_valid(text(hello, "client_nonce"))
    {
        return None;
    }
    authenticated_peer_id_for_material(hello, session_hello_v2_material(hello).ok()?)
}

pub fn authenticated_session_challenge_v2_peer_id(challenge: &Value) -> Option<String> {
    authenticated_peer_id_for_signature(
        challenge,
        "signature",
        "server_peer_id",
        session_challenge_v2_material(challenge).ok()?,
    )
}

pub fn authenticated_session_ready_v2_peer_id(
    ready: &Value,
    hello: &Value,
    challenge: &Value,
) -> Option<String> {
    if ready["type"] != "ready"
        || ready["protocol"] != SESSION_PROTOCOL_V2
        || ready["server_peer_id"] != challenge["server_peer_id"]
        || !nonce_valid(text(hello, "client_nonce"))
    {
        return None;
    }
    authenticated_peer_id_for_signature(
        ready,
        "signature",
        "server_peer_id",
        session_ready_v2_material(ready, hello, challenge).ok()?,
    )
}

fn text<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field].as_str().unwrap_or("").trim()
}

fn nonce_valid(value: &str) -> bool {
    (16..=128).contains(&value.len())
}
