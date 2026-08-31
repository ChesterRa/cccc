use base64::Engine;
use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn sign(proof_key: &str, challenge: &str) -> Option<String> {
    if proof_key.is_empty() || challenge.is_empty() {
        return None;
    }
    let mut mac = HmacSha256::new_from_slice(proof_key.as_bytes()).ok()?;
    mac.update(challenge.as_bytes());
    Some(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
}

pub fn verify(proof_key: &str, challenge: &str, proof: &str) -> bool {
    let Ok(proof) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(proof) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(proof_key.as_bytes()) else {
        return false;
    };
    mac.update(challenge.as_bytes());
    mac.verify_slice(&proof).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_is_bound_to_both_the_secret_and_challenge() {
        let proof = sign("secret-a", "challenge-a").expect("proof");
        assert!(verify("secret-a", "challenge-a", &proof));
        assert!(!verify("secret-b", "challenge-a", &proof));
        assert!(!verify("secret-a", "challenge-b", &proof));
    }
}
