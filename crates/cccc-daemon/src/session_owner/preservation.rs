//! Read-only compatibility and member observations. Neither path acquires an
//! owner control stream, starts a provider, or turns a missing sample into proof.

use super::browser::BrowserResourceState;
use super::controller;
use super::protocol::{ActorSnapshot, OwnerSnapshot};
use super::receipt::AgentIdentity;
use cccc_core::HomeLayout;
use cccc_core::update_capabilities::{KeepDecision, KeepRefusal, UpdateCapabilities};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::time::{Duration, Instant};

const PREFLIGHT_BUDGET: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AgentObservations {
    pub running: Vec<AgentIdentity>,
    pub unconfirmed: Vec<AgentIdentity>,
    pub unavailable_owners: Vec<String>,
}

struct OwnerRead {
    owner_id: String,
    contract: u32,
    result: io::Result<(u32, OwnerSnapshot)>,
}

async fn read_owners(home: &HomeLayout, budget: Duration) -> io::Result<Vec<OwnerRead>> {
    let deadline = Instant::now() + budget;
    let clients = controller::clients_for_observation(home)?;
    let mut owners = Vec::with_capacity(clients.len());
    for client in clients {
        owners.push(OwnerRead {
            owner_id: client.address.owner_id.clone(),
            contract: client.address.contract,
            result: client.observe(deadline).await,
        });
    }
    Ok(owners)
}

pub(super) async fn observations(
    home: &HomeLayout,
    budget: Duration,
) -> io::Result<AgentObservations> {
    let deadline = Instant::now() + budget;
    let owners = read_owners(home, budget).await?;
    Ok(observed_members(home, owners, deadline))
}

fn observed_members(
    home: &HomeLayout,
    owners: Vec<OwnerRead>,
    deadline: Instant,
) -> AgentObservations {
    // A changed or unreadable remote route cannot invalidate a local holder's
    // observation. Unreadable remote work stays unconfirmed in the stop receipt;
    // preflight and the final preservation report still require readable facts.
    let active = match super::remote::read(home, deadline, &held_members(&owners)) {
        Ok(remote) => remote.active,
        Err(error) => {
            tracing::debug!(kind = ?error.kind(), "remote member observations unavailable");
            Vec::new()
        }
    };
    observed_agents(owners, active)
}

fn held_members(owners: &[OwnerRead]) -> BTreeSet<super::remote::MemberKey> {
    owners
        .iter()
        .filter_map(|owner| owner.result.as_ref().ok())
        .flat_map(|(_, snapshot)| snapshot.actors.iter())
        .map(|actor| {
            (
                actor.actor.group_id.clone(),
                actor.actor.actor_id.clone(),
                actor.actor.actor_created_at.clone(),
            )
        })
        .collect()
}

fn observed_agents(owners: Vec<OwnerRead>, remote: Vec<AgentIdentity>) -> AgentObservations {
    let mut result = AgentObservations {
        running: Vec::new(),
        unconfirmed: Vec::new(),
        unavailable_owners: Vec::new(),
    };
    let mut members: BTreeMap<_, Vec<(AgentIdentity, bool)>> = BTreeMap::new();
    for owner in owners {
        match owner.result {
            Ok((_, snapshot)) => {
                for actor in snapshot.actors {
                    // A confirmed exit is absence, not an unknown observation.
                    // It must not make a current generation in another holder
                    // look like two live registrations of the same member.
                    if positively_exited(&actor) {
                        continue;
                    }
                    let identity = controller::identity(&actor.actor, &owner.owner_id);
                    let key = (
                        identity.group_id.clone(),
                        identity.actor_id.clone(),
                        identity.actor_created_at.clone(),
                    );
                    members.entry(key).or_default().push((
                        identity,
                        controller::running(&actor) && actor.actor.generation.is_some(),
                    ));
                }
                for browser in snapshot.browsers {
                    if browser.resource.state == BrowserResourceState::Exited {
                        continue;
                    }
                    if matches!(
                        browser.resource.state,
                        BrowserResourceState::Pending | BrowserResourceState::Unconfirmed
                    ) {
                        // Even a login-only profile can hide an expected tab
                        // when its resource observation is unavailable.
                        if !result.unavailable_owners.contains(&owner.owner_id) {
                            result.unavailable_owners.push(owner.owner_id.clone());
                        }
                    }
                    for identity in browser.identities() {
                        let key = (
                            identity.group_id.clone(),
                            identity.actor_id.clone(),
                            identity.actor_created_at.clone(),
                        );
                        members.entry(key).or_default().push((
                            identity,
                            browser.resource.state == BrowserResourceState::Ready
                                && browser.reattached,
                        ));
                    }
                }
            }
            Err(_) => result.unavailable_owners.push(owner.owner_id),
        }
    }
    for identity in remote {
        members
            .entry((
                identity.group_id.clone(),
                identity.actor_id.clone(),
                identity.actor_created_at.clone(),
            ))
            .or_default()
            .push((identity, false));
    }
    for observations in members.into_values() {
        let unique = observations.len() == 1;
        for (identity, running) in observations {
            if unique && running {
                result.running.push(identity);
            } else {
                result.unconfirmed.push(identity);
            }
        }
    }
    result
}

fn positively_exited(actor: &ActorSnapshot) -> bool {
    actor.observation_error.is_none()
        && (actor.provider_exited
            || actor
                .owner
                .as_ref()
                .is_some_and(|owner| owner.state == crate::ops::actor_runtime::OwnerState::Exited))
}

pub(super) async fn check(home: &HomeLayout, candidate: &UpdateCapabilities) -> KeepDecision {
    if !super::enabled(home) || !candidate.preserve_agents {
        return refused(refusal(
            None,
            None,
            "keep_not_supported",
            "This host or candidate cannot keep agents running through a restart.",
        ));
    }
    let deadline = Instant::now() + PREFLIGHT_BUDGET;
    match read_owners(home, PREFLIGHT_BUDGET).await {
        Ok(owners) => complete_decision(home, candidate, owners, deadline),
        Err(_) => refused(refusal(
            None,
            None,
            "owner_unreachable",
            "CCCC could not read the current session holders; preservation is unconfirmed.",
        )),
    }
}

fn complete_decision(
    home: &HomeLayout,
    candidate: &UpdateCapabilities,
    owners: Vec<OwnerRead>,
    deadline: Instant,
) -> KeepDecision {
    // Include confirmed exits too: their absence from the retained cohort is
    // positive evidence, unlike a configured member with no holder sample.
    let held = held_members(&owners);
    let mut covered = held.clone();
    let mut occupied = BTreeSet::new();
    for (_, snapshot) in owners.iter().filter_map(|owner| owner.result.as_ref().ok()) {
        for actor in &snapshot.actors {
            let member = &actor.actor;
            if !positively_exited(actor) {
                occupied.insert((member.group_id.clone(), member.actor_id.clone()));
            }
        }
        for browser in &snapshot.browsers {
            for member in browser.identities() {
                covered.insert((
                    member.group_id.clone(),
                    member.actor_id.clone(),
                    member.actor_created_at,
                ));
                if browser.resource.state != BrowserResourceState::Exited {
                    occupied.insert((member.group_id, member.actor_id));
                }
            }
        }
    }
    let decision = decide(candidate, owners);
    if !decision.allowed {
        return decision;
    }
    let remote = match super::remote::read(home, deadline, &held) {
        Ok(remote) => remote,
        Err(error) => {
            return refused(refusal(
                None,
                None,
                if error.kind() == io::ErrorKind::TimedOut {
                    "preflight_timeout"
                } else {
                    "member_state_unconfirmed"
                },
                "CCCC could not read the complete saved member resources; inspect their connections and current work before retrying.",
            ));
        }
    };
    if !remote.refusals.is_empty() {
        return KeepDecision {
            allowed: false,
            refusals: remote.refusals,
        };
    }
    covered.extend(remote.covered);
    match uncovered_members(&remote.groups, &covered, &occupied, deadline) {
        Ok(refusals) => KeepDecision {
            allowed: refusals.is_empty(),
            refusals,
        },
        Err(error) if error.kind() == io::ErrorKind::TimedOut => refused(refusal(
            None,
            None,
            "preflight_timeout",
            "The member check did not finish within its deadline; try again later.",
        )),
        Err(_) => refused(refusal(
            None,
            None,
            "member_state_unconfirmed",
            "CCCC could not read the complete registered member list; preservation is unconfirmed.",
        )),
    }
}

fn uncovered_members(
    groups: &[cccc_core::GroupDoc],
    covered: &BTreeSet<(String, String, String)>,
    occupied: &BTreeSet<(String, String)>,
    deadline: Instant,
) -> io::Result<Vec<KeepRefusal>> {
    let within_deadline = || {
        if Instant::now() >= deadline {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "member check deadline elapsed",
            ))
        } else {
            Ok(())
        }
    };
    within_deadline()?;
    let mut refusals = Vec::new();
    for group in groups {
        within_deadline()?;
        let id = &group.group_id;
        for actor in &group.actors {
            if !actor.enabled
                || !actor.external_session_id.is_empty()
                || actor.id == "voice-secretary"
                || actor.internal_kind.as_deref() == Some("voice_secretary")
                || covered.contains(&(id.clone(), actor.id.clone(), actor.created_at.clone()))
            {
                continue;
            }
            // On an enabled holder platform, shared browser launches must go
            // through these holders. decide() already rejected unreadable or
            // pending resources, so no live mapping means no browser to keep.
            // A mapping for an older creation of this member is not absence.
            if actor.runtime.is_browser_web_model()
                && !occupied.contains(&(id.clone(), actor.id.clone()))
            {
                continue;
            }
            refusals.push(KeepRefusal {
                code: "member_state_unconfirmed".into(),
                group_id: Some(id.clone()),
                actor_id: Some(actor.id.clone()),
                owner_id: None,
                display_name: (!actor.title.trim().is_empty()).then(|| actor.title.trim().into()),
                message: "CCCC could not confirm this enabled member's session resources; try again once its start or stop settles, or choose restart agents.".into(),
            });
        }
    }
    within_deadline()?;
    Ok(refusals)
}

fn decide(candidate: &UpdateCapabilities, owners: Vec<OwnerRead>) -> KeepDecision {
    if let Some(reason) =
        tool_contract_refusal(candidate, cccc_contracts::MCP_DAEMON_ACCEPTED_CONTRACTS)
    {
        return refused(reason);
    }
    let mut refusals = Vec::new();
    let mut members = BTreeSet::new();
    for owner in owners {
        if !candidate.owner_contracts.contains(&owner.contract) {
            refusals.push(refusal(
                Some(&owner.owner_id),
                None,
                "owner_contract_mismatch",
                "The session holder protocol is outside this candidate's supported range.",
            ));
            continue;
        }
        let (_, snapshot) = match owner.result {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::TimedOut => {
                refusals.push(refusal(
                    None,
                    None,
                    "preflight_timeout",
                    "The member check did not finish within its deadline; try again later.",
                ));
                break;
            }
            Err(_) => {
                refusals.push(refusal(
                    Some(&owner.owner_id),
                    None,
                    "owner_unreachable",
                    "The session holder did not provide a usable current observation.",
                ));
                continue;
            }
        };
        for actor in snapshot.actors {
            if positively_exited(&actor) {
                continue;
            }
            let key = (
                actor.actor.group_id.clone(),
                actor.actor.actor_id.clone(),
                actor.actor.actor_created_at.clone(),
            );
            if !members.insert(key) {
                refusals.push(refusal(Some(&owner.owner_id), Some(&actor), "member_state_unconfirmed", "This member appears in more than one session holder; its current identity is unconfirmed."));
                continue;
            }
            if let Some(reason) = member_refusal(&owner.owner_id, &actor) {
                refusals.push(reason);
            }
        }
        for browser in snapshot.browsers {
            if browser.resource.state == BrowserResourceState::Exited {
                continue;
            }
            let usable = browser.resource.state == BrowserResourceState::Ready;
            let identities = browser.identities().collect::<Vec<_>>();
            // An empty login profile still owns resources. Missing actor
            // mappings do not turn an unknown browser into an empty cohort.
            if !usable && identities.is_empty() {
                refusals.push(refusal(Some(&owner.owner_id), None, "member_state_unconfirmed",
                    "The browser holder could not confirm a retained browser resource; preservation is unconfirmed."));
            }
            for identity in identities {
                let unique = members.insert((
                    identity.group_id.clone(),
                    identity.actor_id.clone(),
                    identity.actor_created_at.clone(),
                ));
                if !usable || !unique {
                    refusals.push(KeepRefusal {
                        code: "member_state_unconfirmed".into(),
                        group_id: Some(identity.group_id),
                        actor_id: Some(identity.actor_id),
                        owner_id: Some(owner.owner_id.clone()),
                        display_name: None,
                        message: if usable {
                            "This member appears in more than one held session; its current identity is unconfirmed."
                        } else {
                            "This member's browser resources could not be confirmed; preservation is unconfirmed."
                        }.into(),
                    });
                }
            }
        }
    }
    KeepDecision {
        allowed: refusals.is_empty(),
        refusals,
    }
}

fn tool_contract_refusal(candidate: &UpdateCapabilities, current: &[u32]) -> Option<KeepRefusal> {
    let missing: Vec<_> = current
        .iter()
        .filter(|contract| !candidate.mcp_daemon_contracts.contains(contract))
        .collect();
    (!missing.is_empty()).then(|| {
        refusal(
            None,
            None,
            "mcp_contract_mismatch",
            &format!(
                "The candidate does not accept all CCCC tool contracts accepted by the running daemon (missing {missing:?})."
            ),
        )
    })
}

fn member_refusal(owner: &str, actor: &ActorSnapshot) -> Option<KeepRefusal> {
    // A positively exited member is not in the retained cohort. An absent or
    // unreadable owner, a starting member, and an unknown exit are different.
    if positively_exited(actor) {
        return None;
    }
    let reason = |code, message| Some(refusal(Some(owner), Some(actor), code, message));
    if !controller::running(actor) {
        return reason(
            "member_state_unconfirmed",
            "This member's current running state could not be confirmed; wait for it to settle and try again.",
        );
    }
    if actor
        .actor
        .generation
        .as_ref()
        .is_none_or(|generation| generation.is_empty())
    {
        return reason(
            "member_state_unconfirmed",
            "This member's current launch identity could not be confirmed.",
        );
    }
    None
}

pub(super) fn refusal(
    owner: Option<&str>,
    actor: Option<&ActorSnapshot>,
    code: &str,
    message: &str,
) -> KeepRefusal {
    KeepRefusal {
        code: code.into(),
        group_id: actor.map(|actor| actor.actor.group_id.clone()),
        actor_id: actor.map(|actor| actor.actor.actor_id.clone()),
        owner_id: owner.map(str::to_owned),
        display_name: None,
        message: message.into(),
    }
}

pub(super) fn refused(reason: KeepRefusal) -> KeepDecision {
    KeepDecision {
        allowed: false,
        refusals: vec![reason],
    }
}

#[cfg(test)]
#[path = "preservation_tests.rs"]
mod tests;
