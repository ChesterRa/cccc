//! Web Push message encryption (RFC 8291 over RFC 8188 `aes128gcm`) and
//! VAPID authorization (RFC 8292), built on the aws-lc-rs primitives the
//! workspace already ships for TLS. Every message uses a fresh salt and a
//! fresh ephemeral ECDH key; the VAPID key is the only long-lived secret.
use aws_lc_rs::aead::{AES_128_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use aws_lc_rs::agreement::{self, ECDH_P256, PrivateKey, UnparsedPublicKey};
use aws_lc_rs::hkdf::{HKDF_SHA256, KeyType, Prk, Salt};
use aws_lc_rs::rand::{SecureRandom, SystemRandom};
use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde_json::json;

/// One record holds the whole message: RFC 8291 caps a push message at
/// 4096 octets, so the 86-octet header leaves 3993 for the plaintext after
/// the 16-octet tag and the one-octet padding delimiter.
const RECORD_SIZE: u32 = 4096;
const HEADER_LEN: usize = 16 + 4 + 1 + 65;
pub(crate) const MAX_PLAINTEXT: usize = RECORD_SIZE as usize - HEADER_LEN - 16 - 1;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum CryptoError {
    InvalidKey,
    TooLarge,
    Internal,
}

/// The browser's subscription keys, checked once when they are stored.
#[derive(Clone)]
pub(crate) struct ReceiverKeys {
    p256dh: [u8; 65],
    auth: [u8; 16],
}

impl ReceiverKeys {
    pub(crate) fn parse(p256dh: &str, auth: &str) -> Result<Self, CryptoError> {
        let p256dh: [u8; 65] = decode(p256dh)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(CryptoError::InvalidKey)?;
        let auth: [u8; 16] = decode(auth)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(CryptoError::InvalidKey)?;
        // Agreeing with a throwaway key proves the point is on the curve.
        let probe = PrivateKey::generate(&ECDH_P256).map_err(|_| CryptoError::Internal)?;
        agreement::agree(
            &probe,
            UnparsedPublicKey::new(&ECDH_P256, &p256dh),
            CryptoError::InvalidKey,
            |_| Ok(()),
        )?;
        Ok(Self { p256dh, auth })
    }

    /// Store the same validated bytes in one bounded, unpadded representation.
    pub(crate) fn encoded(&self) -> (String, String) {
        (
            URL_SAFE_NO_PAD.encode(self.p256dh),
            URL_SAFE_NO_PAD.encode(self.auth),
        )
    }
}

/// Encrypt `plaintext` for one subscription with a fresh salt and key.
pub(crate) fn encrypt(plaintext: &[u8], keys: &ReceiverKeys) -> Result<Vec<u8>, CryptoError> {
    let mut salt = [0_u8; 16];
    SystemRandom::new()
        .fill(&mut salt)
        .map_err(|_| CryptoError::Internal)?;
    let sender = PrivateKey::generate(&ECDH_P256).map_err(|_| CryptoError::Internal)?;
    encrypt_with(plaintext, keys, &sender, &salt)
}

fn encrypt_with(
    plaintext: &[u8],
    keys: &ReceiverKeys,
    sender: &PrivateKey,
    salt: &[u8; 16],
) -> Result<Vec<u8>, CryptoError> {
    if plaintext.len() > MAX_PLAINTEXT {
        return Err(CryptoError::TooLarge);
    }
    let sender_public = sender
        .compute_public_key()
        .map_err(|_| CryptoError::Internal)?;
    let sender_public = sender_public.as_ref();
    let (cek, nonce) = content_keys(
        sender,
        &keys.p256dh,
        &keys.p256dh,
        sender_public,
        &keys.auth,
        salt,
    )?;
    let mut record = Vec::with_capacity(plaintext.len() + 17);
    record.extend_from_slice(plaintext);
    record.push(2); // The last (and only) record's padding delimiter.
    let key =
        LessSafeKey::new(UnboundKey::new(&AES_128_GCM, &cek).map_err(|_| CryptoError::Internal)?);
    key.seal_in_place_append_tag(
        Nonce::assume_unique_for_key(nonce),
        Aad::empty(),
        &mut record,
    )
    .map_err(|_| CryptoError::Internal)?;
    let mut body = Vec::with_capacity(HEADER_LEN + record.len());
    body.extend_from_slice(salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(65);
    body.extend_from_slice(sender_public);
    body.extend_from_slice(&record);
    Ok(body)
}

/// RFC 8291 section 3.4: the content-encryption key and nonce for one message.
fn content_keys(
    own: &PrivateKey,
    peer_public: &[u8],
    ua_public: &[u8],
    as_public: &[u8],
    auth: &[u8; 16],
    salt: &[u8; 16],
) -> Result<([u8; 16], [u8; 12]), CryptoError> {
    let ikm = ecdh_ikm(own, peer_public, ua_public, as_public, auth)?;
    let prk = Salt::new(HKDF_SHA256, salt).extract(&ikm);
    let mut cek = [0_u8; 16];
    expand(&prk, b"Content-Encoding: aes128gcm\0", &mut cek)?;
    let mut nonce = [0_u8; 12];
    expand(&prk, b"Content-Encoding: nonce\0", &mut nonce)?;
    Ok((cek, nonce))
}

fn ecdh_ikm(
    own: &PrivateKey,
    peer_public: &[u8],
    ua_public: &[u8],
    as_public: &[u8],
    auth: &[u8; 16],
) -> Result<[u8; 32], CryptoError> {
    let shared = agreement::agree(
        own,
        UnparsedPublicKey::new(&ECDH_P256, peer_public),
        CryptoError::InvalidKey,
        |secret| Ok(secret.to_vec()),
    )?;
    let prk = Salt::new(HKDF_SHA256, auth).extract(&shared);
    let mut info = Vec::with_capacity(14 + 65 + 65);
    info.extend_from_slice(b"WebPush: info\0");
    info.extend_from_slice(ua_public);
    info.extend_from_slice(as_public);
    let mut ikm = [0_u8; 32];
    expand(&prk, &info, &mut ikm)?;
    Ok(ikm)
}

struct Len(usize);

impl KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

fn expand(prk: &Prk, info: &[u8], out: &mut [u8]) -> Result<(), CryptoError> {
    prk.expand(&[info], Len(out.len()))
        .and_then(|okm| okm.fill(out))
        .map_err(|_| CryptoError::Internal)
}

/// The application server's long-lived VAPID identity.
pub(crate) struct VapidKey {
    pair: EcdsaKeyPair,
    public_key: String,
}

impl VapidKey {
    pub(crate) fn generate_pkcs8() -> Result<Vec<u8>, CryptoError> {
        EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
            .map(|document| document.as_ref().to_vec())
            .map_err(|_| CryptoError::Internal)
    }

    pub(crate) fn from_pkcs8(pkcs8: &[u8]) -> Result<Self, CryptoError> {
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8)
            .map_err(|_| CryptoError::InvalidKey)?;
        let public_key = URL_SAFE_NO_PAD.encode(pair.public_key().as_ref());
        Ok(Self { pair, public_key })
    }

    /// The application server key a browser subscribes with.
    pub(crate) fn public_key(&self) -> &str {
        &self.public_key
    }

    /// RFC 8292 `Authorization` value for one push service origin. The JWS
    /// signature is the fixed-width `R || S` form that ES256 requires.
    pub(crate) fn authorization(
        &self,
        audience: &str,
        subject: &str,
        expires_at: u64,
    ) -> Result<String, CryptoError> {
        let header = URL_SAFE_NO_PAD.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = URL_SAFE_NO_PAD
            .encode(json!({"aud":audience,"exp":expires_at,"sub":subject}).to_string());
        let signing_input = format!("{header}.{claims}");
        let signature = self
            .pair
            .sign(&SystemRandom::new(), signing_input.as_bytes())
            .map_err(|_| CryptoError::Internal)?;
        Ok(format!(
            "vapid t={signing_input}.{}, k={}",
            URL_SAFE_NO_PAD.encode(signature.as_ref()),
            self.public_key
        ))
    }
}

/// Base64url as browsers and RFC 8291 write it; trailing padding is tolerated.
fn decode(value: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(value.trim_end_matches('=')).ok()
}

/// The receiving browser's side, for tests.
#[cfg(test)]
pub(crate) fn decrypt(
    body: &[u8],
    ua_private: &PrivateKey,
    keys: &ReceiverKeys,
) -> Option<Vec<u8>> {
    let salt: [u8; 16] = body.get(..16)?.try_into().ok()?;
    let record_size = u32::from_be_bytes(body.get(16..20)?.try_into().ok()?);
    if record_size != RECORD_SIZE || *body.get(20)? != 65 {
        return None;
    }
    let as_public = body.get(21..86)?;
    let (cek, nonce) = content_keys(
        ua_private,
        as_public,
        &keys.p256dh,
        as_public,
        &keys.auth,
        &salt,
    )
    .ok()?;
    let key = LessSafeKey::new(UnboundKey::new(&AES_128_GCM, &cek).ok()?);
    let mut record = body.get(86..)?.to_vec();
    let plain = key
        .open_in_place(
            Nonce::assume_unique_for_key(nonce),
            Aad::empty(),
            &mut record,
        )
        .ok()?;
    let delimiter = plain.iter().rposition(|byte| *byte != 0)?;
    (plain[delimiter] == 2).then(|| plain[..delimiter].to_vec())
}

#[cfg(test)]
mod tests;
