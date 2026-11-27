use super::store;
use crate::{HomeLayout, fs, local_sessions::NativeHost};
use cccc_contracts::session_node::{SessionNodeEndpoint, SessionNodeMembership};
use serde::{Deserialize, Serialize};
use std::{io, net::SocketAddr};

/// Local-only descriptor. The broker secret never crosses the iroh connection.
#[derive(Clone, Serialize, Deserialize)]
pub struct ClientDescriptor {
    pub endpoint: SessionNodeEndpoint,
    pub node_id: String,
    pub grant_id: String,
    pub owner: NativeHost,
    pub address: SocketAddr,
    pub secret: String,
    pub connected: bool,
    pub memberships: Vec<SessionNodeMembership>,
}

pub fn saved_client(home: &HomeLayout) -> io::Result<Option<ClientDescriptor>> {
    store::read_file(home, "client.json")
}

pub fn client(home: &HomeLayout) -> io::Result<Option<ClientDescriptor>> {
    Ok(saved_client(home)?.filter(|client| {
        client.owner.alive() && client.connected && client.address.ip().is_loopback()
    }))
}

pub fn set_client(home: &HomeLayout, descriptor: &ClientDescriptor) -> io::Result<()> {
    if !descriptor.address.ip().is_loopback() {
        return Err(store::denied());
    }
    store::update(home, |_| {
        if saved_client(home)?.is_some_and(|old| old.owner.alive() && old.owner != descriptor.owner)
        {
            return Err(io::Error::new(
                io::ErrorKind::AddrInUse,
                "Another remote-session connector owns this Home",
            ));
        }
        fs::write_secret_json(&store::directory(home).join("client.json"), descriptor)
    })
}

pub fn replace_memberships(
    home: &HomeLayout,
    secret: &str,
    memberships: Vec<SessionNodeMembership>,
) -> io::Result<()> {
    update_memberships(home, secret, |current| {
        if memberships
            .iter()
            .any(|membership| membership.hub_id != current.endpoint.endpoint_id)
        {
            return Err(store::denied());
        }
        current.memberships = memberships;
        Ok(())
    })
}

pub fn upsert_membership(
    home: &HomeLayout,
    secret: &str,
    membership: SessionNodeMembership,
) -> io::Result<()> {
    update_memberships(home, secret, |current| {
        if membership.hub_id != current.endpoint.endpoint_id || !membership.enabled {
            return Err(store::denied());
        }
        current.memberships.retain(|old| {
            old.source_id != membership.source_id || old.group_id != membership.group_id
        });
        current.memberships.push(membership);
        Ok(())
    })
}

fn update_memberships(
    home: &HomeLayout,
    secret: &str,
    update: impl FnOnce(&mut ClientDescriptor) -> io::Result<()>,
) -> io::Result<()> {
    store::update(home, |_| {
        let mut current = client(home)?
            .filter(|client| client.secret == secret)
            .ok_or_else(store::denied)?;
        update(&mut current)?;
        if current.memberships.len() > 2048 {
            return Err(io::Error::other("Too many remote memberships"));
        }
        fs::write_secret_json(&store::directory(home).join("client.json"), &current)
    })
}

pub fn disconnect_client(home: &HomeLayout, secret: &str) -> io::Result<()> {
    store::update(home, |_| {
        let Some(mut current) = saved_client(home)? else {
            return Ok(());
        };
        if !current.owner.alive() || current.secret != secret {
            return Err(store::denied());
        }
        if current.connected {
            current.connected = false;
            fs::write_secret_json(&store::directory(home).join("client.json"), &current)?;
        }
        Ok(())
    })
}

pub fn clear_client(home: &HomeLayout, secret: &str) -> io::Result<()> {
    store::update(home, |_| {
        if saved_client(home)?.is_some_and(|client| client.secret == secret) {
            std::fs::remove_file(store::directory(home).join("client.json"))?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_admissions_preserve_each_member_and_disconnect_keeps_the_snapshot() {
        let temp = tempfile::tempdir().expect("isolated Home");
        let home = HomeLayout::from_path(temp.path()).expect("Home");
        let secret = "fixture";
        set_client(
            &home,
            &ClientDescriptor {
                endpoint: SessionNodeEndpoint {
                    endpoint_id: "fixture-hub".into(),
                    addresses: vec![],
                    relay_url: None,
                },
                node_id: "fixture-node".into(),
                grant_id: "fixture-grant".into(),
                owner: NativeHost::capture(std::process::id()).expect("fixture owner"),
                address: "127.0.0.1:1".parse().expect("loopback address"),
                secret: secret.into(),
                connected: true,
                memberships: vec![],
            },
        )
        .expect("save connector");

        let member = |group: &str| SessionNodeMembership {
            hub_id: "fixture-hub".into(),
            source_id: "fixture-source".into(),
            source_revision: "revision".into(),
            group_id: group.into(),
            group_title: group.into(),
            actor_id: "worker".into(),
            actor_generation: "generation".into(),
            role: "peer".into(),
            scope_key: String::new(),
            enabled: true,
        };
        let first = member("group-one");
        let second = member("group-two");
        let start = std::sync::Barrier::new(2);
        let admit = |member: SessionNodeMembership| {
            start.wait();
            upsert_membership(&home, secret, member).expect("admit member");
        };
        std::thread::scope(|scope| {
            let left = scope.spawn(|| admit(first.clone()));
            let right = scope.spawn(|| admit(second.clone()));
            left.join().expect("first admission");
            right.join().expect("second admission");
        });
        let memberships = client(&home)
            .expect("read state")
            .expect("live")
            .memberships;
        assert_eq!(memberships.len(), 2);
        assert!(memberships.contains(&first) && memberships.contains(&second));

        disconnect_client(&home, secret).expect("mark offline");
        assert!(client(&home).expect("read state").is_none());
        assert!(upsert_membership(&home, secret, member("late")).is_err());
        assert!(replace_memberships(&home, secret, vec![]).is_err());
        assert!(
            saved_client(&home)
                .expect("read state")
                .expect("saved")
                .memberships
                == memberships
        );
    }
}
