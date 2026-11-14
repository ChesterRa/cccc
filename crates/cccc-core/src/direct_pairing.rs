//! Legacy pairing and compatible invitation import.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

pub fn invite(home: &HomeLayout, group: &str) -> io::Result<String> {
    invite_checked(home, group, None)
}
pub fn invite_checked(
    home: &HomeLayout,
    group: &str,
    expected: Option<&DirectListener>,
) -> io::Result<String> {
    let local = endpoint(home, group)?;
    update(home, |store| {
        if store.relations.len() >= 64 {
            return Err(invalid(
                "Remove unused connections before creating another invitation",
            ));
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
        let invitation = new_invitation(local.clone(), listener, 1, chrono::Duration::minutes(30));
        store.relations.push(DirectRelation {
            id: invitation.id.clone(),
            local,
            remote: None,
            state: DirectState::Invited,
            created_at: cccc_contracts::utc_now(),
            expires_at: invitation.expires_at.clone(),
            secret_sha256: hash(&invitation.secret),
            invitation: None,
        });
        encode_invitation(&invitation)
    })
}
pub fn join(home: &HomeLayout, group: &str, text: &str) -> io::Result<String> {
    if text.len() > 8192 {
        return Err(invalid("Invitation is too large"));
    }
    let raw = URL_SAFE_NO_PAD
        .decode(
            text.trim()
                .strip_prefix("cccc-direct:")
                .ok_or_else(|| invalid("Invalid direct invitation"))?,
        )
        .map_err(|_| invalid("Invalid direct invitation"))?;
    let invitation: DirectInvitation =
        serde_json::from_slice(&raw).map_err(|_| invalid("Invalid direct invitation"))?;
    validate_endpoint(&invitation.host)?;
    address(&invitation.address)?;
    let local = endpoint(home, group)?;
    if !matches!(invitation.v, 1..=3)
        || (invitation.v == 3) != (invitation.expires_at == DIRECT_PERMANENT_EXPIRY)
        || !is_direct(&invitation.id)
        || !unexpired(&invitation.expires_at)
        || invitation.secret.len() != 64
        || local.instance_id == invitation.host.instance_id
    {
        return Err(invalid("Invitation expired or belongs to this instance"));
    }
    let connection_id = if invitation.reusable() {
        shared::connection_id(&invitation.id, &local)
    } else {
        invitation.id.clone()
    };
    update(home, |store| {
        if store.retired.contains(&connection_id) {
            return Err(invalid(
                "This invitation was already retired; request a new invitation",
            ));
        }
        if let Some(existing) = store.relations.iter().find(|r| r.id == connection_id) {
            // Names and software versions are snapshots; reusable grants bind the key and generation.
            let same_local = existing.local == local
                || (invitation.reusable()
                    && existing.local.group_id == local.group_id
                    && current(home, &existing.local)?);
            if same_local
                && existing
                    .invitation
                    .as_ref()
                    .is_some_and(|i| i.secret == invitation.secret)
                && !matches!(existing.state, DirectState::Revoked | DirectState::Expired)
            {
                return Ok(existing.id.clone());
            }
            return Err(invalid("Invitation was already used"));
        }
        // Importing a fresh key is an explicit new admission attempt. Retire
        // definitive old rejections for this pair, never an active/unknown grant.
        let mut removed = Vec::new();
        store.relations.retain(|relation| {
            let closed = relation.local.group_id == local.group_id
                && relation.remote.as_ref().is_some_and(|remote| {
                    remote.instance_id == invitation.host.instance_id
                        && remote.group_id == invitation.host.group_id
                })
                && matches!(relation.state, DirectState::Revoked | DirectState::Expired);
            if closed {
                removed.push(relation.id.clone());
            }
            !closed
        });
        store.retired.extend(removed);
        if store.relations.len() >= 64
            || store.relations.iter().any(|r| {
                r.local.group_id == local.group_id
                    && r.remote.as_ref().is_some_and(|remote| {
                        remote.instance_id == invitation.host.instance_id
                            && remote.group_id == invitation.host.group_id
                    })
            })
        {
            return Err(invalid(
                "A direct connection for this Group pair already exists",
            ));
        }
        let id = connection_id.clone();
        store.relations.push(DirectRelation {
            id: id.clone(),
            local,
            remote: Some(invitation.host.clone()),
            state: DirectState::Pending,
            created_at: cccc_contracts::utc_now(),
            expires_at: invitation.expires_at.clone(),
            secret_sha256: String::new(),
            invitation: Some(invitation),
        });
        Ok(id)
    })
}
/// Called only after mutual TLS; the submitted key must equal the TLS identity.
pub fn hello(
    home: &HomeLayout,
    id: &str,
    secret: &str,
    remote: &DirectEndpoint,
    tls_key: &str,
) -> io::Result<DirectState> {
    validate_endpoint(remote)?;
    if remote.public_key != tls_key {
        return Err(invalid("Peer identity mismatch"));
    }
    update(home, |store| {
        hello_existing(home, store, id, secret, remote)
    })
}

pub(super) fn hello_existing(
    home: &HomeLayout,
    store: &mut DirectStore,
    id: &str,
    secret: &str,
    remote: &DirectEndpoint,
) -> io::Result<DirectState> {
    let Some(position) = store
        .relations
        .iter()
        .position(|r| r.id == id && r.invitation.is_none())
    else {
        // A pinned receiver can definitively refuse an unknown or retired ID.
        // This negative answer grants nothing and discloses no stored content.
        return Ok(DirectState::Revoked);
    };
    if store.relations.iter().enumerate().any(|(i, r)| {
        i != position
            && r.local.group_id == store.relations[position].local.group_id
            && r.remote.as_ref().is_some_and(|other| {
                other.instance_id == remote.instance_id && other.group_id == remote.group_id
            })
    }) {
        return Err(invalid(
            "A direct connection for this Group pair already exists",
        ));
    }
    let relation = &mut store.relations[position];
    if relation.secret_sha256 != hash(secret)
        || relation.remote.as_ref().is_some_and(|r| r != remote)
        || !current(home, &relation.local)?
    {
        return Err(invalid("Invitation no longer matches this Group pair"));
    }
    if relation.state == DirectState::Revoked {
        return Ok(DirectState::Revoked);
    }
    if relation.state == DirectState::Expired
        || (relation.state != DirectState::Active && !unexpired(&relation.expires_at))
    {
        relation.state = DirectState::Expired;
        return Ok(DirectState::Expired);
    }
    if relation.remote.is_none() {
        relation.remote = Some(remote.clone());
        relation.state = DirectState::Pending;
    }
    Ok(relation.state.clone())
}

pub(super) fn encode_invitation(invitation: &DirectInvitation) -> io::Result<String> {
    Ok(format!(
        "cccc-direct:{}",
        URL_SAFE_NO_PAD.encode(serde_json::to_vec(invitation).map_err(io::Error::other)?)
    ))
}
