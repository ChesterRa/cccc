//! Terminal pairing exercises the real native ticket/Hello and saved grant in isolated Homes.
#[path = "session_node_join/failures.rs"]
mod failures;
#[path = "session_node_join/homes.rs"]
mod homes;
use cccc_core::session_nodes;
use homes::{Homes, connection, output, paste, progress, stop};

const PROMPT: &str = "Paste the hub's session-node ticket";

async fn ticket(f: &Homes) -> (String, String) {
    let mut ticket = cccc_session_node::create_invitation(&f.hub, 0)
        .await
        .expect("isolated hub invitation");
    ticket.addresses.retain(|address| {
        address
            .parse::<std::net::SocketAddr>()
            .is_ok_and(|address| address.ip().is_loopback())
    });
    let wire = cccc_session_node::encode_ticket(&ticket).expect("encode fixture invitation");
    (wire, ticket.secret)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn ticket_connect_and_saved_resume_stay_in_the_terminal() {
    let mut f = Homes::new();
    f.start().await;
    let (wire, secret) = ticket(&f).await;
    let forbidden = [&wire[..], &secret[..]];
    #[cfg(unix)]
    let initial = {
        f.install_fake_codex();
        &["connect", "--runtime", "codex", "--ticket-stdin"][..]
    };
    #[cfg(not(unix))]
    let initial = &["connect", "--ticket-stdin"][..];
    let mut first = f.command(initial);
    progress(&mut first, PROMPT, &forbidden).await;
    assert!(
        session_nodes::peers(&f.hub)
            .expect("unpaired hub")
            .is_empty()
    );
    paste(&mut first, &wire).await;
    let paired = connection(&f.node, &mut first, &forbidden).await;
    let peers = session_nodes::peers(&f.hub).expect("paired hub");
    assert_eq!(peers.len(), 1);
    #[cfg(unix)]
    let hostname = nix::unistd::gethostname()
        .expect("local hostname")
        .to_string_lossy()
        .into_owned();
    #[cfg(windows)]
    let hostname = String::from("Fixture-PC");
    assert_eq!(peers[0].name, hostname.trim());
    assert_eq!(peers[0].grant_id, paired.grant_id);
    #[cfg(unix)]
    assert!(f.native_calls().contains("codex:mcp"));
    stop(first, &forbidden).await;

    // A revoked device enrolls again only with a fresh ticket, keeping its device key.
    cccc_session_node::revoke(&f.hub, &paired.node_id)
        .await
        .expect("revoke the paired device");
    let (manual, manual_secret) = ticket(&f).await;
    let forbidden = [&manual[..], &manual_secret[..]];
    let mut again = f.command(&["connect", "--ticket-stdin", "--name", "Named laptop"]);
    progress(&mut again, PROMPT, &forbidden).await;
    paste(&mut again, &manual).await;
    let offline = connection(&f.node, &mut again, &forbidden).await;
    assert_eq!(offline.node_id, paired.node_id);
    assert_ne!(offline.grant_id, paired.grant_id);
    assert_eq!(
        session_nodes::peers(&f.hub).expect("paired hub")[0].name,
        "Named laptop"
    );
    stop(again, &forbidden).await;

    let mut standard = f.command(&["connect"]);
    let restored = connection(&f.node, &mut standard, &[PROMPT]).await;
    assert_eq!(restored.grant_id, offline.grant_id);
    assert_eq!(restored.node_id, paired.node_id);
    stop(standard, &forbidden).await;

    // An invalid fresh ticket leaves the saved pairing intact.
    let saved = f.descriptor_bytes();
    let mut child = f.command(&["connect", "--ticket-stdin"]);
    progress(&mut child, PROMPT, &[]).await;
    paste(&mut child, "private-invalid-ticket").await;
    let (ok, text) = output(child, &["private-invalid-ticket"]).await;
    assert!(!ok && text.contains("Invalid or missing session-node pairing ticket"));
    assert!(f.descriptor_bytes() == saved);

    cccc_session_node::revoke(&f.hub, &paired.node_id)
        .await
        .expect("revoke fixture device");
    let (ok, text) = output(f.command(&["connect"]), &[PROMPT, &wire, &secret]).await;
    assert!(!ok && text.contains("rejected") && text.contains("pairing ticket"));
    assert!(
        f.descriptor_bytes() == saved,
        "a rejected Hello must preserve the prior grant"
    );
    f.close().await;
}
