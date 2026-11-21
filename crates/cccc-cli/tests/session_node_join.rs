//! Both command spellings exercise the real native ticket/Hello and saved grant in isolated Homes.
#[path = "session_node_join/api.rs"]
mod api;
#[path = "session_node_join/failures.rs"]
mod failures;
#[path = "session_node_join/homes.rs"]
mod homes;
use api::LocalOrigin;
use cccc_core::session_nodes;
use homes::{Homes, connection, output, paste, progress, stop};

const PROMPT: &str = "Paste the hub's session-node ticket";

async fn ticket(f: &Homes) -> (String, String) {
    let mut ticket = cccc_web::session_node::create_invitation(&f.hub, 0)
        .await
        .expect("isolated hub invitation");
    ticket.addresses.retain(|address| {
        address
            .parse::<std::net::SocketAddr>()
            .is_ok_and(|address| address.ip().is_loopback())
    });
    let wire = cccc_web::session_node::encode_ticket(&ticket).expect("encode fixture invitation");
    (wire, ticket.secret)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn ticket_connect_and_legacy_resume_stay_in_the_terminal() {
    let mut f = Homes::new();
    f.start().await;
    let origin = LocalOrigin::new().await;
    origin
        .forbid_requests(async {
            let (wire, secret) = ticket(&f).await;
            let forbidden = [&wire[..], &secret[..], &origin.url[..]];
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
            assert!(paired.join_binding.is_none());
            #[cfg(unix)]
            assert!(f.native_calls().contains("codex:mcp"));
            stop(first, &forbidden).await;

            // Even a valid manual ticket for this same hub cannot establish trust in the old URL argument.
            cccc_web::session_node::revoke(&f.hub, &paired.node_id)
                .await
                .expect("re-enroll the previously revoked device");
            let (manual, manual_secret) = ticket(&f).await;
            let forbidden = [&manual[..], &manual_secret[..], &origin.url[..]];
            let mut legacy = f.command(&["join", &origin.url, "--name", "Named laptop"]);
            progress(&mut legacy, PROMPT, &forbidden).await;
            paste(&mut legacy, &manual).await;
            let offline = connection(&f.node, &mut legacy, &forbidden).await;
            assert!(offline.join_binding.is_none());
            assert_eq!(offline.node_id, paired.node_id);
            assert_ne!(offline.grant_id, paired.grant_id);
            assert_eq!(
                session_nodes::peers(&f.hub).expect("paired hub")[0].name,
                "Named laptop"
            );
            stop(legacy, &forbidden).await;

            let mut standard = f.command(&["connect"]);
            let restored = connection(&f.node, &mut standard, &[&origin.url, PROMPT]).await;
            assert_eq!(restored.grant_id, offline.grant_id);
            assert_eq!(restored.node_id, paired.node_id);
            stop(standard, &forbidden).await;

            // Model an installation with a verified origin persisted by the previous release.
            let mut prior = restored;
            prior.join_binding = Some(session_nodes::JoinBinding {
                origin: format!("{}/", origin.url),
                hub_id: prior.endpoint.endpoint_id.clone(),
            });
            session_nodes::set_client(&f.node, &prior).expect("pre-upgrade verified binding");
            let mut legacy = f.command(&["join", &format!("{}/", origin.url), "--no-browser"]);
            let resumed = connection(&f.node, &mut legacy, &[&origin.url, PROMPT]).await;
            assert_eq!(resumed.grant_id, prior.grant_id);
            assert_eq!(
                resumed
                    .join_binding
                    .as_ref()
                    .expect("retained legacy binding")
                    .origin,
                format!("{}/", origin.url)
            );
            stop(legacy, &forbidden).await;

            let saved = f.descriptor_bytes();
            // An explicit new ticket and an unknown origin both leave the old binding intact on invalid input.
            let other = LocalOrigin::new().await;
            for args in [
                &["join", &origin.url, "--reauthorize"][..],
                &["join", &other.url][..],
            ] {
                other
                    .forbid_requests(async {
                        let mut child = f.command(args);
                        progress(&mut child, PROMPT, &[&origin.url, &other.url]).await;
                        paste(&mut child, "private-invalid-ticket").await;
                        let (ok, text) =
                            output(child, &["private-invalid-ticket", &origin.url, &other.url])
                                .await;
                        assert!(
                            !ok && text.contains("Invalid or missing session-node pairing ticket")
                        );
                        assert!(f.descriptor_bytes() == saved);
                    })
                    .await;
            }

            cccc_web::session_node::revoke(&f.hub, &paired.node_id)
                .await
                .expect("revoke fixture device");
            let (ok, text) = output(
                f.command(&["join", &origin.url]),
                &[&origin.url, PROMPT, &wire, &secret],
            )
            .await;
            assert!(!ok && text.contains("rejected") && text.contains("pairing ticket"));
            assert!(
                f.descriptor_bytes() == saved,
                "a rejected Hello must preserve the prior grant"
            );
        })
        .await;
    f.close().await;
}
