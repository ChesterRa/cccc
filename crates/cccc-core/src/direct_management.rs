//! Approval, revocation and Group retirement.
use super::*;

pub fn set_state(home: &HomeLayout, group: &str, id: &str, approve: bool) -> io::Result<()> {
    update(home, |store| {
        let relation = store
            .relations
            .iter_mut()
            .find(|r| r.id == id && r.local.group_id == group)
            .ok_or_else(|| invalid("Unknown connection"))?;
        if approve {
            if relation.invitation.is_some()
                || relation.remote.is_none()
                || !matches!(relation.state, DirectState::Pending | DirectState::Active)
                || (relation.state != DirectState::Active && !unexpired(&relation.expires_at))
                || !current(home, &relation.local)?
            {
                return Err(invalid("Connection cannot be approved"));
            }
            relation.state = DirectState::Active;
        } else {
            relation.state = DirectState::Revoked;
        }
        Ok(())
    })
}
pub fn remove(home: &HomeLayout, group: &str, id: &str) -> io::Result<()> {
    if !is_direct(id) {
        return Err(invalid("Invalid direct connection ID"));
    }
    update(home, |store| {
        if store
            .relations
            .iter()
            .any(|r| r.id == id && r.local.group_id != group)
        {
            return Err(invalid("Connection belongs to another Group"));
        }
        if store.relations.iter().any(|r| {
            r.id == id
                && r.local.group_id == group
                && !matches!(r.state, DirectState::Revoked | DirectState::Expired)
                && (r.state == DirectState::Active
                    || r.invitation.is_some()
                    || unexpired(&r.expires_at))
        }) {
            return Err(invalid("Disconnect before removing this connection"));
        }
        let existed = store.relations.iter().any(|r| r.id == id);
        store
            .relations
            .retain(|r| !(r.id == id && r.local.group_id == group));
        if existed {
            store.retired.insert(id.into());
        }
        Ok(())
    })?;
    remove_catalog(home, id)
}

/// Called after Group deletion commits. No grants, secrets or quota survive it.
pub fn retire_group(home: &HomeLayout, group: &str) -> io::Result<()> {
    update(home, |store| {
        let ids = store
            .relations
            .iter()
            .filter(|r| r.local.group_id == group)
            .map(|r| r.id.clone())
            .collect::<Vec<_>>();
        // Remove disposable caches before retiring the records, so a filesystem
        // failure leaves IDs available for an explicit cleanup retry.
        for id in &ids {
            remove_catalog(home, id)?;
        }
        store
            .shared_invitations
            .retain(|link| link.invitation.host.group_id != group);
        store.relations.retain(|r| r.local.group_id != group);
        store.retired.extend(ids);
        Ok(())
    })
}

fn remove_catalog(home: &HomeLayout, id: &str) -> io::Result<()> {
    if !is_direct(id) {
        return Err(invalid("Invalid direct connection ID"));
    }
    match std::fs::remove_file(
        home.root()
            .join("state/connect/catalog")
            .join(format!("{id}.json")),
    ) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
