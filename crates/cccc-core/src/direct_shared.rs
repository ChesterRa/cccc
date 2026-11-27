//! Reusable invitation keys mint a separate approved grant for each joining Group.
use super::*;
#[cfg(test)]
#[path = "direct_shared_tests.rs"]
mod tests;

const MAX_PENDING_PER_PERMANENT_INVITATION: usize = 8;

pub fn create(
    home: &HomeLayout,
    group: &str,
    expected: Option<&DirectListener>,
) -> io::Result<String> {
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
        let invitation = new_invitation(local, listener, 3, DIRECT_PERMANENT_EXPIRY.into());
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
        || !link.invitation.permanent()
    {
        return Err(invalid(
            "This invitation is unavailable or no longer belongs to this Group",
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
        let secret_sha256 = hash(secret);
        if secret_sha256 != hash(&link.invitation.secret) || !current(home, &link.invitation.host)?
        {
            return Err(invalid("Invitation no longer matches this Group"));
        }
        // Saved pairs use their own grant above. Only permanent keys can admit a new pair.
        if !link.invitation.permanent() {
            return Ok(DirectState::Revoked);
        }
        if store
            .relations
            .iter()
            .filter(|relation| {
                relation.state == DirectState::Pending
                    && relation.local.group_id == link.invitation.host.group_id
                    && relation.invitation.is_none()
                    && relation.secret_sha256 == secret_sha256
            })
            .count()
            >= MAX_PENDING_PER_PERMANENT_INVITATION
        {
            return Err(invalid(
                "This invitation has too many pending requests. Ask the receiving administrator to review them or use another invitation",
            ));
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
            secret_sha256,
        });
        store.shared_invitations[index].uses =
            store.shared_invitations[index].uses.saturating_add(1);
        Ok(DirectState::Pending)
    })
}
