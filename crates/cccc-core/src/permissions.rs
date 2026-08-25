use cccc_contracts::ActorRole;
use std::io;

use crate::GroupDoc;
use crate::actors::effective_role;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorAction {
    List,
    Add,
    Remove,
    Update,
    Start,
    Stop,
    Restart,
}

pub fn require_actor(
    group: &GroupDoc,
    by: &str,
    action: ActorAction,
    target: &str,
) -> io::Result<()> {
    let who = by.trim();
    if who.is_empty() || who == "user" || who == "system" {
        return Ok(());
    }
    match (effective_role(group, who), action) {
        (Some(_), ActorAction::List) => Ok(()),
        (
            Some(ActorRole::Foreman),
            ActorAction::Add | ActorAction::Start | ActorAction::Stop | ActorAction::Restart,
        ) => Ok(()),
        (Some(ActorRole::Foreman), ActorAction::Remove)
            if target == who || effective_role(group, target) == Some(ActorRole::Peer) =>
        {
            Ok(())
        }
        (Some(ActorRole::Peer), ActorAction::Restart) => Ok(()),
        (Some(ActorRole::Peer), ActorAction::Stop | ActorAction::Remove) if target == who => Ok(()),
        (Some(_), ActorAction::Update) => Err(io::Error::other(
            "actor.update is restricted to user-facing ports",
        )),
        (Some(_), _) => Err(io::Error::other(format!("permission denied: {who}"))),
        (None, _) => Err(io::Error::other(format!("unknown actor: {who}"))),
    }
}

pub fn require_group(group: &GroupDoc, by: &str) -> io::Result<()> {
    let who = by.trim();
    if who.is_empty() || who == "user" || who == "system" {
        return Ok(());
    }
    match effective_role(group, who) {
        Some(ActorRole::Foreman) => Ok(()),
        Some(ActorRole::Peer) => Err(io::Error::other(format!("permission denied: {who}"))),
        None => Err(io::Error::other(format!("unknown actor: {who}"))),
    }
}

pub fn require_group_member(group: &GroupDoc, by: &str) -> io::Result<()> {
    let who = by.trim();
    if who.is_empty() || who == "user" || who == "system" {
        return Ok(());
    }
    effective_role(group, who)
        .map(|_| ())
        .ok_or_else(|| io::Error::other(format!("unknown actor: {who}")))
}

pub fn require_inbox(group: &GroupDoc, by: &str, target: &str) -> io::Result<()> {
    let who = by.trim();
    if who.is_empty() || who == "user" || who == "system" {
        return Ok(());
    }
    match effective_role(group, who) {
        Some(ActorRole::Foreman) => Ok(()),
        Some(ActorRole::Peer) if who == target => Ok(()),
        Some(ActorRole::Peer) => Err(io::Error::other(format!("permission denied: {who}"))),
        None => Err(io::Error::other(format!("unknown actor: {who}"))),
    }
}
