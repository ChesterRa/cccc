//! Reusable invitation keys mint a separate approved grant for each joining Group.
use super::*;
#[cfg(test)]
#[path = "direct_shared_tests.rs"]
mod tests;

pub fn create(
    home: &HomeLayout,
    group: &str,
    expected: Option<&DirectListener>,
    seconds: i64,
) -> io::Result<String> {
    if !matches!(seconds, 0 | 86_400 | 604_800 | 2_592_000) {
        return Err(invalid(
            "Choose an invitation lifetime of 24 hours, 7 days, 30 days or permanent",
        ));
    }
    let local = endpoint(home, group)?;
    update(home, |store| {
        if store.shared_invitations.len() >= 64 {
            return Err(invalid("Delete unused invitations before creating another"));
        }
        let listener = store
            .listener
            .as_ref()
            .ok_or_else(|| invalid("Enable the direct listener first"))?;
        if expected.is_some_and(|expected| expected != listener) {
            return Err(invalid(
                "Receiving settings changed; review the current settings and try again",
            ));
        }
        let mut invitation = new_invitation(
            local,
            listener,
            if seconds == 0 { 3 } else { 2 },
            chrono::Duration::seconds(seconds),
        );
        if seconds == 0 {
            invitation.expires_at = DIRECT_PERMANENT_EXPIRY.into();
        }
        let text = encode_invitation(&invitation)?;
        store.shared_invitations.push(DirectSharedInvitation {
            invitation,
            created_at: cccc_contracts::utc_now(),
            uses: 0,
        });
        Ok(text)
    })
}

/// A secret is returned only to an explicit administrator action, never to a status poll.
pub fn copy(home: &HomeLayout, group: &str, id: &str) -> io::Result<String> {
    let store = load(home)?;
    let link = store
        .shared_invitations
        .iter()
        .find(|link| link.invitation.id == id && link.invitation.host.group_id == group)
        .ok_or_else(|| invalid("Unknown invitation"))?;
    if store.retired.contains(id)
        || !current(home, &link.invitation.host)?
        || !unexpired(&link.invitation.expires_at)
    {
        return Err(invalid(
            "This invitation has expired or no longer belongs to this Group",
        ));
    }
    encode_invitation(&link.invitation)
}

/// Deleting a key retires every grant minted from it, including approved pairs.
pub fn revoke(home: &HomeLayout, group: &str, id: &str) -> io::Result<()> {
    update(home, |store| {
        let Some(index) = store
            .shared_invitations
            .iter()
            .position(|link| link.invitation.id == id)
        else {
            return Ok(());
        };
        if store.shared_invitations[index].invitation.host.group_id != group {
            return Err(invalid("Invitation belongs to another Group"));
        }
        let digest = hash(&store.shared_invitations[index].invitation.secret);
        let mut removed = Vec::new();
        store.relations.retain(|relation| {
            let belongs = relation.local.group_id == group
                && relation.invitation.is_none()
                && relation.secret_sha256 == digest;
            if belongs {
                removed.push(relation.id.clone());
            }
            !belongs
        });
        store.retired.extend(removed);
        store.shared_invitations.remove(index);
        store.retired.insert(id.into());
        Ok(())
    })
}

pub fn connection_id(invitation: &str, endpoint: &DirectEndpoint) -> String {
    let material = serde_json::to_vec(&(
        invitation,
        &endpoint.instance_id,
        &endpoint.group_id,
        &endpoint.generation,
    ))
    .expect("serializable identity");
    let digest = Sha256::digest(material);
    let bytes: [u8; 16] = digest[..16].try_into().expect("digest prefix");
    format!("direct-{}", uuid::Uuid::from_bytes(bytes))
}

/// The TLS peer identity is verified before any invitation is consumed or grant is created.
pub fn hello(
    home: &HomeLayout,
    invitation_id: &str,
    id: &str,
    secret: &str,
    remote: &DirectEndpoint,
    tls_key: &str,
) -> io::Result<DirectState> {
    validate_endpoint(remote)?;
    if remote.public_key != tls_key || id != connection_id(invitation_id, remote) {
        return Err(invalid(
            "Peer identity or invitation connection ID mismatch",
        ));
    }
    update(home, |store| {
        if store.retired.contains(id) {
            return Ok(DirectState::Revoked);
        }
        if store.relations.iter().any(|relation| relation.id == id) {
            return pairing::hello_existing(home, store, id, secret, remote);
        }
        if store.retired.contains(invitation_id) {
            return Ok(DirectState::Revoked);
        }
        let Some(index) = store
            .shared_invitations
            .iter()
            .position(|link| link.invitation.id == invitation_id)
        else {
            return Ok(DirectState::Revoked);
        };
        let link = &store.shared_invitations[index];
        if hash(secret) != hash(&link.invitation.secret) || !current(home, &link.invitation.host)? {
            return Err(invalid("Invitation no longer matches this Group"));
        }
        if !unexpired(&link.invitation.expires_at) {
            return Ok(DirectState::Expired);
        }
        if store.relations.len() >= 64
            || store.relations.iter().any(|relation| {
                relation.local.group_id == link.invitation.host.group_id
                    && relation.remote.as_ref().is_some_and(|endpoint| {
                        endpoint.instance_id == remote.instance_id
                            && endpoint.group_id == remote.group_id
                    })
            })
        {
            return Err(invalid(
                "A direct connection for this Group pair already exists or the limit was reached",
            ));
        }
        store.relations.push(DirectRelation {
            id: id.into(),
            local: link.invitation.host.clone(),
            remote: Some(remote.clone()),
            state: DirectState::Pending,
            created_at: cccc_contracts::utc_now(),
            expires_at: link.invitation.expires_at.clone(),
            invitation: None,
            secret_sha256: hash(secret),
        });
        store.shared_invitations[index].uses =
            store.shared_invitations[index].uses.saturating_add(1);
        Ok(DirectState::Pending)
    })
}
