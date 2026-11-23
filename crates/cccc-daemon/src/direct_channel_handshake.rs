//! Mutual TLS and compatible v1 / reusable invitation handshakes.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    id: String,
    secret: String,
    endpoint: DirectEndpoint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    invitation_id: Option<String>,
}
pub(super) async fn accept(
    home: HomeLayout,
    locks: DispatchLocks,
    channels: Channels,
    stream: tokio::net::TcpStream,
) -> Result<(), String> {
    let identity = InstanceIdentity::load(&home).map_err(|e| e.to_string())?;
    let acceptor = tokio_rustls::TlsAcceptor::from(
        crate::direct_tls::server(identity).map_err(|e| e.to_string())?,
    );
    let (stream, hello, state) = tokio::time::timeout(Duration::from_secs(10), async {
        let mut stream = acceptor
            .accept(stream)
            .await
            .map_err(|_| "Direct TLS handshake failed")?;
        if stream.get_ref().1.alpn_protocol() != Some(b"cccc-direct/1".as_slice()) {
            return Err("Unsupported direct protocol".into());
        }
        let cert = stream
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|c| c.first())
            .ok_or("Missing peer identity")?;
        let key = crate::direct_tls::public_key(cert.as_ref()).map_err(|e| e.to_string())?;
        let hello: Hello = read(&mut stream, 8192).await?;
        let state = match hello.invitation_id.as_deref() {
            Some(invitation) => direct::shared::hello(
                &home,
                invitation,
                &hello.id,
                &hello.secret,
                &hello.endpoint,
                &key,
            ),
            None => direct::hello(&home, &hello.id, &hello.secret, &hello.endpoint, &key),
        }
        .map_err(|e| e.to_string())?;
        write(&mut stream, &state).await?;
        Ok::<_, String>((stream, hello, state))
    })
    .await
    .map_err(|_| "Direct handshake timed out")??;
    if state != DirectState::Active {
        return Ok(());
    }
    channel(
        home,
        locks,
        channels,
        hello.id,
        hello.endpoint.instance_id,
        stream,
    )
    .await
}
pub(super) async fn dial(
    home: HomeLayout,
    locks: DispatchLocks,
    channels: Channels,
    relation: DirectRelation,
    retry: &mut RetryState,
) -> Result<(), String> {
    let invitation = relation
        .invitation
        .as_ref()
        .ok_or("Missing direct invitation")?;
    let identity = InstanceIdentity::load(&home).map_err(|e| e.to_string())?;
    let connector = tokio_rustls::TlsConnector::from(
        crate::direct_tls::client(identity, invitation.host.public_key.clone())
            .map_err(|e| e.to_string())?,
    );
    let (stream, state) = tokio::time::timeout(Duration::from_secs(10), async {
        let tcp = tokio::net::TcpStream::connect(&invitation.address)
            .await
            .map_err(|_| "Direct endpoint is unreachable")?;
        let name =
            rustls::pki_types::ServerName::try_from("direct.invalid").map_err(|e| e.to_string())?;
        let mut stream = connector
            .connect(name, tcp)
            .await
            .map_err(|_| "Direct TLS identity check failed")?;
        if stream.get_ref().1.alpn_protocol() != Some(b"cccc-direct/1".as_slice()) {
            return Err("Unsupported direct protocol".into());
        }
        write(
            &mut stream,
            &Hello {
                id: relation.id.clone(),
                secret: invitation.secret.clone(),
                endpoint: relation.local.clone(),
                invitation_id: invitation.reusable().then(|| invitation.id.clone()),
            },
        )
        .await?;
        let state: DirectState = read(&mut stream, 1024).await?;
        Ok::<_, String>((stream, state))
    })
    .await
    .map_err(|_| "Direct connection timed out")??;
    direct::update(&home, |store| {
        let current = store
            .relations
            .iter_mut()
            .find(|r| r.id == relation.id)
            .ok_or_else(|| std::io::Error::other("Connection was removed"))?;
        if matches!(current.state, DirectState::Revoked | DirectState::Expired) {
            return Err(std::io::Error::other("Connection was revoked"));
        }
        if matches!(
            state,
            DirectState::Active | DirectState::Revoked | DirectState::Expired
        ) {
            current.state = state.clone();
        }
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    if state != DirectState::Active {
        return Ok(());
    }
    retry.connected();
    channel(
        home,
        locks,
        channels,
        relation.id,
        invitation.host.instance_id.clone(),
        stream,
    )
    .await
}
