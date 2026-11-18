use super::super::{mcp, protocol::ActorRef};
use super::*;
use crate::ops::actor_runtime::{OwnerIdentity, OwnerState, RegisteredOwner};
use cccc_core::local_sessions::NativeHost;
use cccc_core::owned_mcp::{Binding, Registration};

fn capabilities() -> UpdateCapabilities {
    UpdateCapabilities {
        preserve_agents: true,
        owner_contracts: vec![1],
        mcp_registration_apis: vec![1],
        mcp_daemon_contracts: vec![1],
        ..Default::default()
    }
}

fn member(owner: &str) -> ActorSnapshot {
    let binding = Binding {
        owner_id: owner.into(),
        instance: uuid::Uuid::new_v4().simple().to_string(),
        launch_id: uuid::Uuid::new_v4().simple().to_string(),
    };
    ActorSnapshot {
        actor: ActorRef {
            group_id: "g_preservation".into(),
            actor_id: "worker".into(),
            actor_created_at: "created".into(),
            generation: Some("current".into()),
        },
        mcp: Some(mcp::Snapshot {
            initialized: true,
            source_controlled: Some(true),
            processes: vec![mcp::ObservedProcess {
                native: NativeHost::capture(std::process::id()).expect("native test process"),
                observable: true,
                report: Registration {
                    registration: 1,
                    instance: binding.instance.clone(),
                    launch_id: binding.launch_id.clone(),
                    version: "0.4.58".into(),
                    source_id: "a".repeat(64),
                    mcp_contract: 1,
                },
            }],
            binding,
        }),
        observation_error: None,
        owner: Some(RegisteredOwner {
            identity: OwnerIdentity::Managed {
                generation: Some("current".into()),
            },
            state: OwnerState::Running,
        }),
        terminal: None,
        terminal_launch: None,
        managed_status: None,
        provider_exited: false,
        workspace: None,
        structured: None,
        ready_for_mail_notice: false,
        waiting_for_workspace_trust: false,
        confirmation_terminal: false,
        confirmation_pending: false,
        confirmation_paused: false,
    }
}

fn decision(candidate: &UpdateCapabilities, actor: ActorSnapshot) -> KeepDecision {
    decide(
        candidate,
        vec![OwnerRead {
            owner_id: "owner".into(),
            contract: 1,
            result: Ok(sample(vec![actor])),
        }],
    )
}

#[test]
fn tool_compatibility_preserves_the_complete_acceptance_set_across_updates() {
    let mut caps = capabilities();
    caps.mcp_daemon_contracts = vec![1, 2];
    assert!(tool_contract_refusal(&caps, &[1]).is_none());
    assert!(tool_contract_refusal(&caps, &[1, 2]).is_none());
    // A later update must retain earlier contracts, not just the newest one.
    caps.mcp_daemon_contracts = vec![2, 3];
    assert_eq!(
        tool_contract_refusal(&caps, &[1, 2])
            .expect("lost old contract")
            .code,
        "mcp_contract_mismatch"
    );
    caps.mcp_daemon_contracts.clear();
    assert_eq!(
        decide(&caps, Vec::new()).refusals[0].code,
        "mcp_contract_mismatch"
    );
}

#[test]
fn manual_maintenance_still_has_only_the_original_tool_contract() {
    // Manual Stop/Start has no candidate-set handoff. Changing this assumption
    // requires its migration check, not just changing a declaration.
    assert_eq!(
        cccc_contracts::MCP_DAEMON_ACCEPTED_CONTRACTS,
        &[1],
        "Changing accepted contracts also requires the manual maintenance migration boundary in CCCC_DAEMON_IPC_V1.md, Automatic update preservation."
    );
    assert!(
        cccc_contracts::MCP_DAEMON_ACCEPTED_CONTRACTS
            .contains(&cccc_contracts::MCP_DAEMON_CONTRACT)
    );
}

#[test]
fn tool_registration_is_diagnostic_not_a_runtime_allowlist() {
    let mut actor = member("owner");
    actor.mcp.as_mut().expect("MCP").source_controlled = None;
    assert!(decision(&capabilities(), actor.clone()).allowed);
    actor.mcp.as_mut().expect("MCP").source_controlled = Some(false);
    assert!(decision(&capabilities(), actor.clone()).allowed);
    actor.mcp = None;
    assert!(decision(&capabilities(), actor).allowed);
}

#[test]
fn missing_tool_observations_do_not_replace_session_identity_checks() {
    let mut actor = member("owner");
    actor.mcp.as_mut().expect("MCP").processes[0]
        .report
        .launch_id = uuid::Uuid::new_v4().simple().to_string();
    actor.mcp.as_mut().expect("MCP").processes[0].observable = false;
    assert!(decision(&capabilities(), actor.clone()).allowed);
    actor.actor.generation = None;
    assert_eq!(
        decision(&capabilities(), actor).refusals[0].code,
        "member_state_unconfirmed"
    );
}

#[test]
fn owner_contracts_and_observation_deadlines_remain_required() {
    let mut caps = capabilities();
    caps.owner_contracts.clear();
    assert_eq!(
        decision(&caps, member("owner")).refusals[0].code,
        "owner_contract_mismatch"
    );
    caps = capabilities();
    caps.mcp_registration_apis.clear();
    assert!(decision(&caps, member("owner")).allowed);
    let timed_out = decide(
        &capabilities(),
        vec![OwnerRead {
            owner_id: "owner".into(),
            contract: 1,
            result: Err(io::Error::new(io::ErrorKind::TimedOut, "fixture")),
        }],
    );
    assert_eq!(timed_out.refusals[0].code, "preflight_timeout");
}

#[test]
fn confirmed_exits_are_absent_while_unknown_observations_stay_unconfirmed() {
    let current = member("current");
    let mut old_provider = member("old");
    old_provider.provider_exited = true;
    let mut old_terminal = member("old");
    old_terminal.owner.as_mut().expect("terminal owner").state = OwnerState::Exited;
    let mut unknown = member("current");
    unknown.actor.actor_id = "unknown".into();
    unknown.provider_exited = true;
    unknown.observation_error = Some(super::super::protocol::Failure::new(
        "fixture_unavailable",
        "exit could not be confirmed",
    ));
    let mut no_generation = member("current");
    no_generation.actor.actor_id = "no_generation".into();
    no_generation.actor.generation = None;
    let observations = observed_agents(
        vec![
            OwnerRead {
                owner_id: "old".into(),
                contract: 1,
                result: Ok(sample(vec![old_provider, old_terminal])),
            },
            OwnerRead {
                owner_id: "current".into(),
                contract: 1,
                result: Ok(sample(vec![current.clone(), unknown, no_generation])),
            },
            OwnerRead {
                owner_id: "unavailable".into(),
                contract: 1,
                result: Err(io::Error::other("fixture connection failed")),
            },
        ],
        Vec::new(),
    );
    assert_eq!(
        observations.running,
        vec![controller::identity(&current.actor, "current")]
    );
    assert_eq!(observations.unavailable_owners, vec!["unavailable"]);
    assert_eq!(
        observations
            .unconfirmed
            .iter()
            .map(|identity| identity.actor_id.as_str())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["unknown", "no_generation"])
    );
}

#[test]
fn an_exited_old_registration_is_not_a_second_live_member_in_the_final_check() {
    let mut old = member("old");
    old.provider_exited = true;
    let live = member("current");
    let decision = decide(
        &capabilities(),
        vec![
            OwnerRead {
                owner_id: "old".into(),
                contract: 1,
                result: Ok(sample(vec![old])),
            },
            OwnerRead {
                owner_id: "current".into(),
                contract: 1,
                result: Ok(sample(vec![live])),
            },
        ],
    );
    assert!(decision.allowed, "{:?}", decision.refusals);
}

fn configured_home(
    runtime: cccc_contracts::ActorRuntime,
) -> (tempfile::TempDir, HomeLayout, cccc_core::GroupDoc) {
    let temp = tempfile::tempdir().expect("isolated member configuration");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let store = cccc_core::GroupStore::new(home.clone()).expect("group store");
    let mut group = store
        .create("coverage fixture", "")
        .expect("registered group");
    group.running = true;
    let mut actor = cccc_contracts::Actor::new("worker");
    actor.runtime = runtime;
    actor.title = "Fixture member".into();
    group.actors.push(actor);
    store.save(&group).expect("active member");
    (temp, home, group)
}

fn configured_decision(home: &HomeLayout, owners: Vec<OwnerRead>) -> KeepDecision {
    complete_decision(
        home,
        &capabilities(),
        owners,
        Instant::now() + PREFLIGHT_BUDGET,
    )
}

#[test]
fn a_closed_browser_is_absence_only_after_all_holders_are_confirmed() {
    let (_temp, home, group) = configured_home(cccc_contracts::ActorRuntime::WebModel);
    let path = home.groups_dir().join(&group.group_id).join("group.yaml");
    let before = std::fs::read(&path).expect("saved group");
    assert!(configured_decision(&home, Vec::new()).allowed);
    assert_eq!(std::fs::read(path).expect("read unchanged group"), before);
    assert!(!home.daemon_dir().join("session-owners").exists());

    let unreachable = configured_decision(
        &home,
        vec![OwnerRead {
            owner_id: "missing".into(),
            contract: 1,
            result: Err(io::Error::new(io::ErrorKind::BrokenPipe, "fixture")),
        }],
    );
    assert!(!unreachable.allowed);
    assert_eq!(unreachable.refusals[0].code, "owner_unreachable");

    let mut browser = browser_sample(&group);
    browser.resource.sessions.clear();
    browser.resource.state = BrowserResourceState::Pending;
    assert!(!configured_decision(&home, browser_owner(browser)).allowed);
}

#[test]
fn remote_members_need_saved_resource_facts_instead_of_a_process_holder() {
    for runtime in [
        cccc_contracts::ActorRuntime::ChatgptDots,
        cccc_contracts::ActorRuntime::GrokWebModel,
    ] {
        let (_temp, home, group) = configured_home(runtime);
        assert!(
            configured_decision(&home, Vec::new()).allowed,
            "an unconfigured idle member is empty"
        );
        let groups = cccc_core::GroupStore::new(home.clone()).expect("groups");
        groups
            .mutate(&group.group_id, |group| {
                group.extra.insert(
                    "runtime_states".into(),
                    serde_json::json!({"worker":{
                "active_turn_id":"unfinished","active_event_ids":["original"]}}),
                );
                Ok(())
            })
            .expect("accepted remote work without its route");
        let missing = configured_decision(&home, Vec::new());
        assert!(!missing.allowed);
        assert_eq!(missing.refusals[0].code, "remote_resource_unconfirmed");
        assert!(!home.daemon_dir().join("session-owners").exists());
    }
}

#[test]
fn unavailable_remote_facts_do_not_erase_verified_local_observations() {
    let (_temp, home, group) = configured_home(cccc_contracts::ActorRuntime::GrokWebModel);
    cccc_core::GroupStore::new(home.clone())
        .expect("groups")
        .mutate(&group.group_id, |group| {
            group.extra.insert(
                "runtime_states".into(),
                serde_json::json!({"worker":{"active_turn_id":"unfinished"}}),
            );
            Ok(())
        })
        .expect("remote work without its route");
    let remote =
        super::super::remote::read(&home, Instant::now() + PREFLIGHT_BUDGET, &BTreeSet::new())
            .expect("read conflicting remote facts");
    assert!(!remote.refusals.is_empty());
    let current = member("local");
    let owners = || {
        vec![OwnerRead {
            owner_id: "local".into(),
            contract: 1,
            result: Ok(sample(vec![current.clone()])),
        }]
    };
    let assert_local = || {
        let observed = observed_members(&home, owners(), Instant::now() + PREFLIGHT_BUDGET);
        assert_eq!(
            observed.running,
            vec![controller::identity(&current.actor, "local")]
        );
        assert!(observed.unconfirmed.is_empty());
        assert!(observed.unavailable_owners.is_empty());
    };
    assert_local();
    std::fs::write(home.registry_path(), b"{").expect("unreadable remote registry");
    assert_local();
    assert_eq!(
        std::fs::read(home.registry_path()).expect("unchanged registry"),
        b"{"
    );
}

#[test]
fn actual_launch_evidence_outweighs_later_configuration_but_missing_evidence_refuses() {
    let (_temp, home, mut group) = configured_home(cccc_contracts::ActorRuntime::Codex);
    let decision = configured_decision(&home, Vec::new());
    assert_eq!(decision.refusals[0].code, "member_state_unconfirmed");
    let mut actual = member("owner");
    actual.actor.group_id.clone_from(&group.group_id);
    actual
        .actor
        .actor_created_at
        .clone_from(&group.actors[0].created_at);
    // Saving another runtime is a next-launch setting, not a change to the
    // already observed managed provider or its tool processes.
    group.actors[0].runtime = cccc_contracts::ActorRuntime::WebModel;
    cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .save(&group)
        .expect("change next-launch configuration");
    let decision = configured_decision(
        &home,
        vec![OwnerRead {
            owner_id: "owner".into(),
            contract: 1,
            result: Ok(sample(vec![actual.clone()])),
        }],
    );
    assert!(decision.allowed, "{:?}", decision.refusals);
    actual.provider_exited = true;
    let decision = configured_decision(
        &home,
        vec![OwnerRead {
            owner_id: "owner".into(),
            contract: 1,
            result: Ok(sample(vec![actual])),
        }],
    );
    assert!(
        decision.allowed,
        "a positively exited member needs no preservation"
    );
}

#[test]
fn external_native_and_global_secretary_members_are_not_daemon_owned_actors() {
    let (_temp, home, mut group) = configured_home(cccc_contracts::ActorRuntime::Codex);
    group.actors[0].external_session_id = "external-native-source".into();
    let mut secretary = cccc_contracts::Actor::new("secretary_alias");
    secretary.internal_kind = Some("voice_secretary".into());
    group.actors.push(secretary);
    let mut disabled = cccc_contracts::Actor::new("disabled");
    disabled.enabled = false;
    group.actors.push(disabled);
    cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .save(&group)
        .expect("configuration");
    assert!(configured_decision(&home, Vec::new()).allowed);
    group.actors[0].external_session_id.clear();
    group.running = false;
    cccc_core::GroupStore::new(home.clone())
        .expect("store")
        .save(&group)
        .expect("stopped group");
    assert!(configured_decision(&home, Vec::new()).allowed);
}

#[test]
fn missing_registry_is_empty_without_writes_but_an_unreadable_group_is_unknown() {
    let temp = tempfile::tempdir().expect("empty home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    assert!(configured_decision(&home, Vec::new()).allowed);
    assert!(
        !home.root().exists(),
        "the read must not create a registry or home"
    );
    let (_temp, home, group) = configured_home(cccc_contracts::ActorRuntime::Codex);
    std::fs::remove_file(home.groups_dir().join(&group.group_id).join("group.yaml"))
        .expect("unreadable configured group");
    let decision = configured_decision(&home, Vec::new());
    assert!(!decision.allowed);
    assert_eq!(decision.refusals[0].code, "member_state_unconfirmed");
    let expired = complete_decision(&home, &capabilities(), Vec::new(), Instant::now());
    assert_eq!(expired.refusals[0].code, "preflight_timeout");
}

fn sample(actors: Vec<ActorSnapshot>) -> (u32, OwnerSnapshot) {
    (
        1,
        OwnerSnapshot {
            actors,
            browsers: Vec::new(),
        },
    )
}

fn browser_sample(group: &cccc_core::GroupDoc) -> super::super::protocol::BrowserSnapshot {
    use super::super::browser::*;
    super::super::protocol::BrowserSnapshot {
        resource: BrowserResource {
            resource_id: "resource".into(),
            owner_id: "owner".into(),
            instance: "instance".into(),
            spec: BrowserLaunchSpec {
                profile: "/isolated/profile".into(),
                mode: BrowserMode::Headless,
                width: 800,
                height: 600,
                proxy: None,
            },
            connection: None,
            state: BrowserResourceState::Ready,
            error: None,
            sessions: vec![BrowserPage {
                key: "member".into(),
                actor: Some(BrowserActorIdentity {
                    group_id: group.group_id.clone(),
                    actor_id: group.actors[0].id.clone(),
                    actor_created_at: group.actors[0].created_at.clone(),
                }),
                target_id: "target".into(),
                state: serde_json::Value::Null,
            }],
        },
        reattached: false,
    }
}

fn browser_owner(browser: super::super::protocol::BrowserSnapshot) -> Vec<OwnerRead> {
    vec![OwnerRead {
        owner_id: "owner".into(),
        contract: 1,
        result: Ok((
            1,
            OwnerSnapshot {
                actors: Vec::new(),
                browsers: vec![browser],
            },
        )),
    }]
}

#[test]
fn a_saved_browser_mapping_covers_the_cohort_but_needs_web_publication_to_count_as_reattached() {
    let (_temp, home, group) = configured_home(cccc_contracts::ActorRuntime::WebModel);
    let mut browser = browser_sample(&group);
    let expected = browser.identities().next().expect("actual member mapping");
    assert!(configured_decision(&home, browser_owner(browser.clone())).allowed);
    let before_web = observed_agents(browser_owner(browser.clone()), Vec::new());
    assert!(before_web.running.is_empty());
    assert_eq!(before_web.unconfirmed, vec![expected.clone()]);
    browser.reattached = true;
    let after_web = observed_agents(browser_owner(browser.clone()), Vec::new());
    assert_eq!(after_web.running, vec![expected.clone()]);
    assert!(after_web.unconfirmed.is_empty());

    browser.resource.sessions[0].target_id = "replacement-target".into();
    assert_ne!(
        browser.identities().next().expect("new target").generation,
        expected.generation
    );
    browser.resource.sessions[0]
        .actor
        .as_mut()
        .expect("actor")
        .actor_created_at = "old-member-creation".into();
    let stale = configured_decision(&home, browser_owner(browser));
    assert!(!stale.allowed, "a recreated member is not the retained tab");
    assert_eq!(stale.refusals[0].actor_id.as_deref(), Some("worker"));
}

#[test]
fn an_unknown_login_only_browser_refuses_preservation_while_a_positive_exit_is_absence() {
    let (_temp, _home, group) = configured_home(cccc_contracts::ActorRuntime::WebModel);
    let mut browser = browser_sample(&group);
    browser.resource.sessions.clear();
    for state in [
        BrowserResourceState::Pending,
        BrowserResourceState::Unconfirmed,
    ] {
        browser.resource.state = state;
        let decision = decide(&capabilities(), browser_owner(browser.clone()));
        assert!(
            !decision.allowed,
            "unknown resources cannot be forgotten: {state:?}"
        );
        assert_eq!(decision.refusals[0].owner_id.as_deref(), Some("owner"));
        assert!(decision.refusals[0].actor_id.is_none());
    }
    browser.resource.state = BrowserResourceState::Exited;
    assert!(decide(&capabilities(), browser_owner(browser)).allowed);
}

#[test]
fn browser_exit_unknown_state_and_duplicate_members_are_not_interchangeable() {
    let (_temp, _home, group) = configured_home(cccc_contracts::ActorRuntime::WebModel);
    let mut browser = browser_sample(&group);
    browser.reattached = true;
    browser.resource.state = BrowserResourceState::Exited;
    let exited = observed_agents(browser_owner(browser.clone()), Vec::new());
    assert!(exited.running.is_empty() && exited.unconfirmed.is_empty());
    browser.resource.state = BrowserResourceState::Unconfirmed;
    assert_eq!(
        observed_agents(browser_owner(browser.clone()), Vec::new())
            .unconfirmed
            .len(),
        1
    );
    browser.resource.state = BrowserResourceState::Ready;
    let mut other = member("owner");
    other.actor.group_id.clone_from(&group.group_id);
    other
        .actor
        .actor_created_at
        .clone_from(&group.actors[0].created_at);
    let owners = || {
        vec![OwnerRead {
            owner_id: "owner".into(),
            contract: 1,
            result: Ok((
                1,
                OwnerSnapshot {
                    actors: vec![other.clone()],
                    browsers: vec![browser.clone()],
                },
            )),
        }]
    };
    let duplicated = observed_agents(owners(), Vec::new());
    assert!(duplicated.running.is_empty());
    assert_eq!(duplicated.unconfirmed.len(), 2);
    assert!(!decide(&capabilities(), owners()).allowed);
}
