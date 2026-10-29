use super::*;
use aws_lc_rs::signature::{ECDSA_P256_SHA256_FIXED, UnparsedPublicKey as SignaturePublicKey};

// RFC 8291 section 5 and appendix A.
const PLAINTEXT: &[u8] = b"When I grow up, I want to be a watermelon";
const UA_PUBLIC: &str =
    "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
const UA_PRIVATE: &str = "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94";
const AS_PUBLIC: &str =
    "BP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A8";
const AS_PRIVATE: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";
const SALT: &str = "DGv6ra1nlYgDCS1FRnbzlw";
const AUTH: &str = "BTBZMqHH6r4Tts7J_aSIgg";
const MESSAGE: &str = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml\
mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT\
pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN";

fn bytes(value: &str) -> Vec<u8> {
    decode(value).expect("base64url fixture")
}

fn receiver() -> ReceiverKeys {
    ReceiverKeys::parse(UA_PUBLIC, AUTH).expect("RFC receiver keys")
}

fn private(value: &str) -> PrivateKey {
    PrivateKey::from_private_key(&ECDH_P256, &bytes(value)).expect("RFC private key")
}

#[test]
fn the_rfc_example_is_reproduced_byte_for_byte() {
    let keys = receiver();
    let salt: [u8; 16] = bytes(SALT).try_into().expect("salt");
    let sender = private(AS_PRIVATE);
    assert_eq!(
        sender.compute_public_key().expect("public").as_ref(),
        bytes(AS_PUBLIC)
    );
    let (cek, nonce) = content_keys(
        &sender,
        &keys.p256dh,
        &keys.p256dh,
        &bytes(AS_PUBLIC),
        &keys.auth,
        &salt,
    )
    .expect("content keys");
    assert_eq!(cek.to_vec(), bytes("oIhVW04MRdy2XN9CiKLxTg"));
    assert_eq!(nonce.to_vec(), bytes("4h_95klXJ5E_qnoN"));
    let ikm = ecdh_ikm(
        &sender,
        &keys.p256dh,
        &keys.p256dh,
        &bytes(AS_PUBLIC),
        &keys.auth,
    )
    .expect("ikm");
    assert_eq!(
        ikm.to_vec(),
        bytes("S4lYMb_L0FxCeq0WhDx813KgSYqU26kOyzWUdsXYyrg")
    );
    let body = encrypt_with(PLAINTEXT, &keys, &sender, &salt).expect("encrypt");
    assert_eq!(body, bytes(MESSAGE));
    // 86-octet header, 41-octet plaintext, delimiter and 16-octet tag. The
    // example's `Content-Length: 145` disagrees with its own body.
    assert_eq!(body.len(), 144);
}

#[test]
fn the_receiver_recovers_the_rfc_example_and_fresh_messages() {
    let keys = receiver();
    let ua_private = private(UA_PRIVATE);
    assert_eq!(
        decrypt(&bytes(MESSAGE), &ua_private, &keys).as_deref(),
        Some(PLAINTEXT)
    );
    let first = encrypt(PLAINTEXT, &keys).expect("first");
    let second = encrypt(PLAINTEXT, &keys).expect("second");
    assert_eq!(
        decrypt(&first, &ua_private, &keys).as_deref(),
        Some(PLAINTEXT)
    );
    assert_eq!(
        decrypt(&second, &ua_private, &keys).as_deref(),
        Some(PLAINTEXT)
    );
    // Every message gets its own salt and its own sender key.
    assert_ne!(first[..16], second[..16]);
    assert_ne!(first[21..86], second[21..86]);
    let mut tampered = first.clone();
    *tampered.last_mut().expect("tag") ^= 1;
    assert_eq!(decrypt(&tampered, &ua_private, &keys), None);
    let stranger = PrivateKey::generate(&ECDH_P256).expect("other key");
    assert_eq!(decrypt(&first, &stranger, &keys), None);
}

#[test]
fn a_message_must_fit_one_four_kilobyte_record() {
    let keys = receiver();
    let largest = encrypt(&vec![b'x'; MAX_PLAINTEXT], &keys).expect("largest");
    assert_eq!(largest.len(), 4096);
    assert_eq!(MAX_PLAINTEXT, 3993);
    assert_eq!(
        encrypt(&vec![b'x'; MAX_PLAINTEXT + 1], &keys).err(),
        Some(CryptoError::TooLarge)
    );
    let empty = encrypt(b"", &keys).expect("empty");
    assert_eq!(
        decrypt(&empty, &private(UA_PRIVATE), &keys).as_deref(),
        Some(&b""[..])
    );
}

#[test]
fn subscription_keys_are_checked_before_they_are_kept() {
    assert!(ReceiverKeys::parse(&format!("{UA_PUBLIC}="), &format!("{AUTH}==")).is_ok());
    let off_curve = URL_SAFE_NO_PAD.encode([&[4_u8][..], &[0_u8; 64][..]].concat());
    let compressed = URL_SAFE_NO_PAD.encode(&bytes(UA_PUBLIC)[..33]);
    for (p256dh, auth) in [
        (off_curve.as_str(), AUTH),
        (compressed.as_str(), AUTH),
        (UA_PUBLIC, "BTBZMqHH6r4Tts7J_aSI"),
        (UA_PUBLIC, "not base64!"),
        ("", AUTH),
    ] {
        assert_eq!(
            ReceiverKeys::parse(p256dh, auth).err(),
            Some(CryptoError::InvalidKey),
            "{p256dh} / {auth}"
        );
    }
}

#[test]
fn vapid_authorization_is_a_verifiable_es256_token() {
    let key = VapidKey::from_pkcs8(&VapidKey::generate_pkcs8().expect("generate")).expect("load");
    let header = key
        .authorization(
            "https://fcm.googleapis.com",
            "https://reach.example",
            1_900_000_000,
        )
        .expect("authorization");
    let (token, public) = header
        .strip_prefix("vapid t=")
        .and_then(|rest| rest.split_once(", k="))
        .expect("vapid header shape");
    assert_eq!(public, key.public_key());
    assert_eq!(bytes(public).len(), 65);
    let mut parts = token.split('.');
    let (head, claims, signature) = (
        parts.next().expect("header"),
        parts.next().expect("claims"),
        parts.next().expect("signature"),
    );
    assert_eq!(parts.next(), None);
    let head: serde_json::Value = serde_json::from_slice(&bytes(head)).expect("header json");
    assert_eq!(head, json!({"typ":"JWT","alg":"ES256"}));
    let claims_json: serde_json::Value =
        serde_json::from_slice(&bytes(claims)).expect("claims json");
    assert_eq!(
        claims_json,
        json!({"aud":"https://fcm.googleapis.com","exp":1_900_000_000_u64,"sub":"https://reach.example"})
    );
    let signature = bytes(signature);
    assert_eq!(signature.len(), 64, "JWS ES256 uses fixed-width R || S");
    SignaturePublicKey::new(&ECDSA_P256_SHA256_FIXED, bytes(public))
        .verify(
            format!("{}.{claims}", token.split('.').next().unwrap_or_default()).as_bytes(),
            &signature,
        )
        .expect("signature verifies");
    assert!(VapidKey::from_pkcs8(b"not a key").is_err());
}
