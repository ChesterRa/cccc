use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use aws_lc_rs::signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::io;

use crate::HomeLayout;
use crate::fs::{read_yaml, with_exclusive_lock, write_secret_yaml};

#[derive(Clone, Debug)]
pub struct InstanceIdentity {
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

impl InstanceIdentity {
    /// Read the existing identity without creating files or repairing metadata.
    pub fn load(home: &HomeLayout) -> io::Result<Self> {
        let stored: IdentityFile = read_yaml(&home.root().join("group_bridge_identity_key.yaml"))?;
        let private_key = decode_private_key(&stored.private_key).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid persisted instance private key",
            )
        })?;
        Self::from_private_key(private_key)
    }

    // Preserve this persisted key name: changing it would create a new device
    // identity and invalidate existing Connect bindings.
    pub fn load_or_create(home: &HomeLayout) -> io::Result<Self> {
        let path = home.root().join("group_bridge_identity_key.yaml");
        let lock = home.root().join("group_bridge_identity_key.lock");
        with_exclusive_lock(&lock, || {
            let stored = match read_yaml::<IdentityFile>(&path) {
                Ok(stored) => Some(stored),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            let private_key = match &stored {
                Some(stored) => decode_private_key(&stored.private_key).ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid persisted instance private key",
                    )
                })?,
                None => new_private_key(),
            };
            let identity = Self::from_private_key(private_key)?;
            let stored = stored.unwrap_or_default();
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

    pub fn sign(&self, material: &[u8]) -> io::Result<String> {
        let key = Ed25519KeyPair::from_seed_unchecked(&self.private_key)
            .map_err(|error| io::Error::other(error.to_string()))?;
        Ok(encode(key.sign(material).as_ref()))
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

pub fn peer_id_from_public_key(public_key_b64: &str) -> Option<String> {
    let key = base64::engine::general_purpose::STANDARD
        .decode(public_key_b64)
        .ok()?;
    (key.len() == 32).then(|| peer_id(&key))
}

pub fn verify_signature(
    instance_id: &str,
    public_key: &str,
    signature: &str,
    material: &[u8],
) -> bool {
    let Ok(key) = base64::engine::general_purpose::STANDARD.decode(public_key) else {
        return false;
    };
    let Ok(signature) = base64::engine::general_purpose::STANDARD.decode(signature) else {
        return false;
    };
    key.len() == 32
        && peer_id(&key) == instance_id
        && UnparsedPublicKey::new(&ED25519, key)
            .verify(material, &signature)
            .is_ok()
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
#[path = "instance_identity_tests.rs"]
mod tests;
