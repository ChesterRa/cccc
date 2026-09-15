//! TLS 1.3 authenticates instance keys. Group grants authorize application traffic.
use base64::{Engine, engine::general_purpose::STANDARD};
use cccc_core::instance_identity::InstanceIdentity;
use rustls::{
    ClientConfig, DigitallySignedStruct, DistinguishedName, Error, ServerConfig,
    SignatureAlgorithm, SignatureScheme,
    client::{
        AlwaysResolvesClientRawPublicKeys,
        danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    },
    pki_types::{CertificateDer, ServerName, SubjectPublicKeyInfoDer, UnixTime},
    server::{
        AlwaysResolvesServerRawPublicKeys,
        danger::{ClientCertVerified, ClientCertVerifier},
    },
    sign::{CertifiedKey, Signer, SigningKey},
};
use std::sync::Arc;

const ED25519_SPKI: &[u8] = &[
    0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
];

pub(crate) fn public_key(certificate: &[u8]) -> Result<String, Error> {
    if certificate.len() != 44 || !certificate.starts_with(ED25519_SPKI) {
        return Err(Error::General("expected an Ed25519 raw public key".into()));
    }
    Ok(STANDARD.encode(&certificate[12..]))
}

#[derive(Clone)]
struct IdentityKey(InstanceIdentity);
impl std::fmt::Debug for IdentityKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("InstanceKey").field(&self.0.peer_id).finish()
    }
}
impl SigningKey for IdentityKey {
    fn choose_scheme(&self, offered: &[SignatureScheme]) -> Option<Box<dyn Signer>> {
        offered
            .contains(&SignatureScheme::ED25519)
            .then(|| Box::new(self.clone()) as Box<dyn Signer>)
    }
    fn algorithm(&self) -> SignatureAlgorithm {
        SignatureAlgorithm::ED25519
    }
}
impl Signer for IdentityKey {
    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, Error> {
        let signature = self
            .0
            .sign(message)
            .map_err(|_| Error::General("instance signing failed".into()))?;
        STANDARD
            .decode(signature)
            .map_err(|_| Error::General("invalid instance signature".into()))
    }
    fn scheme(&self) -> SignatureScheme {
        SignatureScheme::ED25519
    }
}
fn certified_key(identity: InstanceIdentity) -> Result<Arc<CertifiedKey>, Error> {
    let mut spki = ED25519_SPKI.to_vec();
    spki.extend(
        STANDARD
            .decode(&identity.public_key_b64)
            .map_err(|_| Error::General("invalid instance key".into()))?,
    );
    public_key(&spki)?;
    Ok(Arc::new(CertifiedKey::new(
        vec![CertificateDer::from(spki)],
        Arc::new(IdentityKey(identity)),
    )))
}

#[derive(Debug)]
struct RawKeys {
    expected: Option<String>,
}
impl RawKeys {
    fn check(
        &self,
        cert: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
    ) -> Result<(), Error> {
        let key = public_key(cert.as_ref())?;
        if !intermediates.is_empty()
            || self
                .expected
                .as_ref()
                .is_some_and(|expected| expected != &key)
        {
            return Err(Error::General(
                "peer identity does not match invitation".into(),
            ));
        }
        Ok(())
    }
    fn signature(
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        rustls::crypto::verify_tls13_signature_with_raw_key(
            message,
            &SubjectPublicKeyInfoDer::from(cert.as_ref()),
            dss,
            &rustls::crypto::aws_lc_rs::default_provider().signature_verification_algorithms,
        )
    }
}
impl ServerCertVerifier for RawKeys {
    fn verify_server_cert(
        &self,
        cert: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _: &ServerName<'_>,
        _: &[u8],
        _: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        self.check(cert, intermediates)?;
        Ok(ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("TLS 1.3 is required".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Self::signature(message, cert, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}
impl ClientCertVerifier for RawKeys {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }
    fn verify_client_cert(
        &self,
        cert: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        _: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        self.check(cert, intermediates)?;
        Ok(ClientCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        _: &[u8],
        _: &CertificateDer<'_>,
        _: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Err(Error::General("TLS 1.3 is required".into()))
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        Self::signature(message, cert, dss)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
    fn requires_raw_public_keys(&self) -> bool {
        true
    }
}
pub(crate) fn server(identity: InstanceIdentity) -> Result<Arc<ServerConfig>, Error> {
    let mut config = ServerConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])?
    .with_client_cert_verifier(Arc::new(RawKeys { expected: None }))
    .with_cert_resolver(Arc::new(AlwaysResolvesServerRawPublicKeys::new(
        certified_key(identity)?,
    )));
    config.alpn_protocols = vec![b"cccc-direct/1".to_vec()];
    Ok(Arc::new(config))
}
pub(crate) fn client(
    identity: InstanceIdentity,
    expected: String,
) -> Result<Arc<ClientConfig>, Error> {
    let mut config = ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])?
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(RawKeys {
        expected: Some(expected),
    }))
    .with_client_cert_resolver(Arc::new(AlwaysResolvesClientRawPublicKeys::new(
        certified_key(identity)?,
    )));
    config.alpn_protocols = vec![b"cccc-direct/1".to_vec()];
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_core::HomeLayout;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[tokio::test]
    async fn pinned_mutual_tls_supports_both_directions_and_rejects_wrong_key() {
        let a = tempfile::tempdir().expect("home");
        let b = tempfile::tempdir().expect("home");
        let a = InstanceIdentity::load_or_create(&HomeLayout::from_path(a.path()).expect("home"))
            .expect("identity");
        let b = InstanceIdentity::load_or_create(&HomeLayout::from_path(b.path()).expect("home"))
            .expect("identity");
        for wrong in [false, true] {
            let (left, right) = tokio::io::duplex(65536);
            let acceptor = tokio_rustls::TlsAcceptor::from(server(a.clone()).expect("server"));
            let connector = tokio_rustls::TlsConnector::from(
                client(
                    b.clone(),
                    if wrong {
                        b.public_key_b64.clone()
                    } else {
                        a.public_key_b64.clone()
                    },
                )
                .expect("client"),
            );
            let (accepted, connected) = tokio::join!(
                acceptor.accept(left),
                connector.connect(ServerName::try_from("direct.invalid").expect("name"), right)
            );
            if wrong {
                assert!(accepted.is_err());
                assert!(connected.is_err());
                continue;
            }
            let mut server = accepted.expect("accept");
            let mut client = connected.expect("connect");
            assert_eq!(
                public_key(server.get_ref().1.peer_certificates().expect("key")[0].as_ref())
                    .expect("key"),
                b.public_key_b64
            );
            server.write_all(b"reply").await.expect("write");
            let mut message = [0; 5];
            client.read_exact(&mut message).await.expect("read");
            assert_eq!(&message, b"reply");
            client.write_all(b"hello").await.expect("write");
            server.read_exact(&mut message).await.expect("read");
            assert_eq!(&message, b"hello");
        }
    }
}
