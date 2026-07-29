use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use aws_lc_rs::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io;

use crate::HomeLayout;
use crate::fs::{read_yaml, with_exclusive_lock, write_secret_yaml};

pub const SESSION_PROTOCOL: &str = "/cccc/group_bridge/session-ws/1.0.0";

#[derive(Clone, Debug)]
pub struct GroupBridgeIdentity {
    pub peer_id: String,
    pub public_key_b64: String,
    private_key: Vec<u8>,
}

#[derive(Default, Deserialize, Serialize)]
struct IdentityFile {
    #[serde(default)]
    private_key: String,
    #[serde(default)]
    public_key: String,
    #[serde(default)]
    peer_id: String,
}

impl GroupBridgeIdentity {
    pub fn load_or_create(home: &HomeLayout) -> io::Result<Self> {
        let path = home.root().join("group_bridge_identity_key.yaml");
        let lock = home.root().join("group_bridge_identity_key.lock");
        with_exclusive_lock(&lock, || {
            let stored = read_yaml::<IdentityFile>(&path).unwrap_or_default();
            let private_key =
                decode_private_key(&stored.private_key).unwrap_or_else(new_private_key);
            let identity = Self::from_private_key(private_key)?;
            if stored.private_key != encode(&identity.private_key)
                || stored.public_key != identity.public_key_b64
                || stored.peer_id != identity.peer_id
            {
                write_secret_yaml(
                    &path,
                    &IdentityFile {
                        private_key: encode(&identity.private_key),
                        public_key: identity.public_key_b64.clone(),
                        peer_id: identity.peer_id.clone(),
                    },
                )?;
            }
            Ok(identity)
        })
    }

    pub fn sign_session_hello(
        &self,
        target_group_id: &str,
        src_group_id: &str,
    ) -> io::Result<Value> {
        let mut hello = json!({
            "target_group_id":target_group_id.trim(),
            "src_group_id":src_group_id.trim(),
            "remote_peer_id":self.peer_id,
        });
        let material = session_hello_material(&hello)?;
        let key = Ed25519KeyPair::from_seed_unchecked(&self.private_key)
            .map_err(|error| io::Error::other(error.to_string()))?;
        hello["public_key"] = json!(self.public_key_b64);
        hello["signature"] = json!(encode(key.sign(&material).as_ref()));
        Ok(hello)
    }

    fn from_private_key(private_key: Vec<u8>) -> io::Result<Self> {
        let key = Ed25519KeyPair::from_seed_unchecked(&private_key)
            .map_err(|error| io::Error::other(error.to_string()))?;
        let public_key = key.public_key().as_ref();
        Ok(Self {
            peer_id: peer_id(public_key),
            public_key_b64: encode(public_key),
            private_key,
        })
    }
}

pub fn session_hello_material(hello: &Value) -> io::Result<Vec<u8>> {
    serde_json::to_vec(&json!({
        "protocol":SESSION_PROTOCOL,
        "remote_peer_id":hello["remote_peer_id"].as_str().unwrap_or("").trim(),
        "src_group_id":hello["src_group_id"].as_str().unwrap_or("").trim(),
        "target_group_id":hello["target_group_id"].as_str().unwrap_or("").trim(),
    }))
    .map_err(io::Error::other)
}

pub fn authenticated_session_peer_id(hello: &Value) -> Option<String> {
    let public_key = base64::engine::general_purpose::STANDARD
        .decode(hello["public_key"].as_str()?.trim())
        .ok()?;
    let signature = base64::engine::general_purpose::STANDARD
        .decode(hello["signature"].as_str()?.trim())
        .ok()?;
    let expected = hello["remote_peer_id"].as_str()?.trim();
    let actual = peer_id(&public_key);
    if actual != expected {
        return None;
    }
    let material = session_hello_material(hello).ok()?;
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(&material, &signature)
        .ok()?;
    Some(actual)
}

fn decode_private_key(value: &str) -> Option<Vec<u8>> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(value.trim())
        .ok()?;
    (raw.len() == 32).then_some(raw)
}

fn new_private_key() -> Vec<u8> {
    let mut key = vec![0; 32];
    SystemRandom::new()
        .fill(&mut key)
        .expect("system random source unavailable");
    key
}

fn encode(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn peer_id(public_key: &[u8]) -> String {
    let mut protobuf = vec![0x08, 0x01, 0x12, public_key.len() as u8];
    protobuf.extend_from_slice(public_key);
    let mut multihash = vec![0x00, protobuf.len() as u8];
    multihash.extend_from_slice(&protobuf);
    base58(&multihash)
}

fn base58(raw: &[u8]) -> String {
    const ALPHABET: &[u8] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
    let zeros = raw.iter().take_while(|byte| **byte == 0).count();
    let mut number = raw.to_vec();
    let mut encoded = Vec::new();
    while number.iter().any(|byte| *byte != 0) {
        let mut remainder = 0u16;
        for byte in &mut number {
            let value = (remainder << 8) | u16::from(*byte);
            *byte = (value / 58) as u8;
            remainder = value % 58;
        }
        encoded.push(ALPHABET[remainder as usize]);
    }
    encoded.extend(std::iter::repeat_n(ALPHABET[0], zeros));
    encoded.reverse();
    String::from_utf8(encoded).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_stable_and_builds_signed_python_hello() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let first = GroupBridgeIdentity::load_or_create(&home).expect("identity");
        let second = GroupBridgeIdentity::load_or_create(&home).expect("identity");
        assert_eq!(first.peer_id, second.peer_id);
        assert_eq!(first.public_key_b64, second.public_key_b64);
        let hello = first
            .sign_session_hello("g_remote", "g_local")
            .expect("hello");
        assert_eq!(hello["remote_peer_id"], first.peer_id);
        assert!(!hello["signature"].as_str().unwrap_or("").is_empty());
    }
}
