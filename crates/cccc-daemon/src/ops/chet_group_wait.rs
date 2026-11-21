//! The one visible fact that a Group request to Chet was never sent: the host
//! still held it when its wait expired. Chet history keeps the private fact;
//! the source Group gets a single `failed` delivery on the original message,
//! which says nothing about what Chet was doing instead.
use super::chet_history::{map_error, registration};
use super::operation::{Operation, Policy::Write};
use crate::dispatch::{OpError, OpResult, object};
use cccc_contracts::DaemonRequest;
use cccc_contracts::chet::GroupInputSender;
use cccc_core::chet_history::{self, Record, StartFailure, Store};
use cccc_core::{GroupStore, HomeLayout, actors, connect_groups};
use serde_json::{Value, json};
use std::collections::HashSet;
use uuid::Uuid;

/// The delivery reason the Web shows as "not sent to Chet; send it again".
pub(crate) const WAIT_EXPIRED_REASON: &str = "chet_wait_expired";
const TRANSPORT: &str = "chet_group";

pub(super) fn resolve_operation(request: &DaemonRequest) -> Option<Operation> {
    // A Group write: the delivery fact is ordered with the Group's other writes.
    (request.op == "chet_group_wait_expired").then(|| Operation::new(Write, wait_expired))
}

fn wait_expired(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    if request.args.get("by").and_then(Value::as_str) != Some("user")
        || request.args.contains_key("_cccc_native_binding")
        || request.args.contains_key("_cccc_native_source_binding")
        || request.args.contains_key("_cccc_web_binding")
    {
        return Err(OpError::new(
            "permission_denied",
            "user authority is required",
        ));
    }
    let group_id = request
        .args
        .get("group_id")
        .and_then(Value::as_str)
        .ok_or_else(|| OpError::invalid("group_id is required"))?;
    let input_id: Uuid = request
        .args
        .get("input_id")
        .cloned()
        .ok_or_else(|| OpError::invalid("input_id is required"))
        .and_then(|value| serde_json::from_value(value).map_err(OpError::invalid))?;
    let store = Store::new(home.root());
    let input = match store
        .find_group_inputs(&HashSet::from([input_id]))
        .map_err(map_error)?
        .into_iter()
        .next()
    {
        Some(Record::GroupInput { input, .. }) if input.group_id == group_id => input,
        _ => {
            return Err(OpError::invalid(
                "a wait expiry requires this Group's input",
            ));
        }
    };
    // The private fact comes first: once it exists no owner submits the input,
    // and a retry of this call completes the Group fact below.
    let recorded = store
        .record_not_started(
            chet_history::group_wait_expired_id(input_id),
            input_id,
            StartFailure::Cancelled,
        )
        .map_err(map_error)?;
    let groups = GroupStore::new(home.clone()).map_err(OpError::io)?;
    let group = match groups.load(group_id) {
        Ok(group) if connect_groups::generation(&group) == input.group_generation => group,
        // The message it would mark is gone with its Group.
        Ok(_) => return source_unavailable(recorded),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return source_unavailable(recorded);
        }
        Err(error) => return Err(OpError::io(error)),
    };
    let ledger_path = groups.ledger_path(group_id).map_err(OpError::io)?;
    if cccc_core::ledger::find_event(&ledger_path, &input.event_id)
        .map_err(OpError::io)?
        .is_none()
    {
        return source_unavailable(recorded);
    }
    let (delivery_id, appended) = super::runtime_delivery::record_unsent_service_delivery(
        home,
        &group,
        actors::CHET_SERVICE_PRINCIPAL,
        &input.group_generation,
        &input.event_id,
        TRANSPORT,
        WAIT_EXPIRED_REASON,
    )?;
    // Written on every call, once by its ID: a call that stopped after the
    // delivery fact completes the notice when it is retried.
    if matches!(input.sender, GroupInputSender::Actor { .. }) {
        super::actor_delivery::tell_sender_not_sent_to_chet(
            home,
            group_id,
            &input.event_id,
            &delivery_id,
        )?;
    }
    object(json!({
        "registration":registration(recorded),
        "delivery":if appended { "recorded" } else { "existing" },
    }))
}

fn source_unavailable(recorded: chet_history::Registration) -> OpResult {
    object(json!({"registration":registration(recorded),"delivery":"source_unavailable"}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cccc_contracts::chet::GroupInput;
    use cccc_core::ledger;

    fn call(home: &HomeLayout, op: &str, args: Value) -> cccc_contracts::DaemonResponse {
        crate::dispatch::dispatch(
            home,
            &DaemonRequest {
                v: 1,
                op: op.into(),
                args: args.as_object().expect("fixture object").clone(),
            },
        )
    }

    struct Fixture {
        _temp: tempfile::TempDir,
        home: HomeLayout,
        group_id: String,
        event_id: String,
        input_id: Uuid,
    }

    impl Fixture {
        /// A member asks Chet in a Group, and the host registers that input.
        fn new() -> Self {
            let temp = tempfile::tempdir().expect("home");
            let home = HomeLayout::from_path(temp.path()).expect("home layout");
            let groups = GroupStore::new(home.clone()).expect("groups");
            let mut group = groups.create("Chet wait", "").expect("group");
            let mut actor = cccc_contracts::Actor::new("worker");
            actor.runtime = cccc_contracts::ActorRuntime::WebModel;
            group.actors.push(actor.clone());
            groups.save(&group).expect("save");
            // The sender notice goes only to a member the ledger registered.
            let mut added = cccc_contracts::Event::new("actor.add", &group.group_id);
            added.data.insert("actor".into(), json!(actor));
            ledger::append(
                &groups.ledger_path(&group.group_id).expect("ledger"),
                &added,
            )
            .expect("actor.add");
            let sent = call(
                &home,
                "send",
                json!({"group_id":group.group_id,"by":"worker",
                "to":["@chet"],"text":"Please review","message_mode":"send"}),
            );
            assert!(sent.ok, "{sent:?}");
            let event_id = sent.result["event"]["id"]
                .as_str()
                .expect("event ID")
                .to_owned();
            let input = GroupInput {
                group_id: group.group_id.clone(),
                group_generation: connect_groups::generation(&group),
                event_id: event_id.clone(),
                sender: GroupInputSender::Actor {
                    actor_id: "worker".into(),
                    actor_generation: actors::generation_identity(&actor),
                },
                reply_to: None,
            };
            let registered = call(
                &home,
                "chet_history_group_input",
                json!({"by":"user","input":input}),
            );
            let input_id = serde_json::from_value(registered.result["id"].clone()).expect("ID");
            Self {
                _temp: temp,
                home,
                group_id: group.group_id,
                event_id,
                input_id,
            }
        }

        fn expire(&self) -> cccc_contracts::DaemonResponse {
            call(
                &self.home,
                "chet_group_wait_expired",
                json!({"by":"user","group_id":self.group_id,"input_id":self.input_id}),
            )
        }

        fn events(&self, kind: &str) -> Vec<cccc_contracts::Event> {
            let path = GroupStore::new(self.home.clone())
                .expect("groups")
                .ledger_path(&self.group_id)
                .expect("ledger");
            ledger::read_all(&path)
                .expect("events")
                .into_iter()
                .filter(|event| event.kind == kind)
                .collect()
        }

        /// Not-started facts that clients read as this input's expired wait.
        fn not_started(&self) -> usize {
            self.not_started_records()
                .iter()
                .filter(|record| record["wait_expired"] == true)
                .count()
        }

        /// As the Web shows them. The host's own read stays plain stored records.
        fn not_started_records(&self) -> Vec<Value> {
            let plain = call(&self.home, "chet_history_list", json!({"by":"user"}));
            for record in plain.result["records"].as_array().expect("records") {
                serde_json::from_value::<Record>(record.clone()).expect("a stored record");
            }
            let shown = call(
                &self.home,
                "chet_history_list",
                json!({"by":"user","resolve_sources":true}),
            );
            shown.result["records"]
                .as_array()
                .expect("records")
                .iter()
                .filter(|record| record["kind"] == "not_started")
                .cloned()
                .collect()
        }
    }

    #[test]
    fn an_expiry_marks_the_message_once_and_tells_the_member_once() {
        let fixture = Fixture::new();
        let first = fixture.expire();
        assert!(first.ok, "{first:?}");
        assert_eq!(first.result["registration"], "inserted");
        assert_eq!(first.result["delivery"], "recorded");
        let retry = fixture.expire();
        assert_eq!(retry.result["registration"], "existing");
        assert_eq!(retry.result["delivery"], "existing");

        let deliveries = fixture.events("runtime.delivery");
        assert_eq!(deliveries.len(), 1);
        let data = &deliveries[0].data;
        assert_eq!(data["actor_id"], actors::CHET_SERVICE_PRINCIPAL);
        assert_eq!(data["source_event_id"], fixture.event_id.as_str());
        assert_eq!(data["state"], "failed");
        assert_eq!(data["reason"], WAIT_EXPIRED_REASON);
        assert_eq!(data["retryable"], false);
        assert_eq!(fixture.not_started(), 1);

        let statuses = call(
            &fixture.home,
            "ledger_statuses",
            json!({"group_id":fixture.group_id,"event_ids":[fixture.event_id]}),
        );
        let chet = &statuses.result["statuses"][&fixture.event_id]["obligation_status"]
            [actors::CHET_SERVICE_PRINCIPAL];
        assert_eq!(chet["delivery_state"], "failed");
        assert_eq!(chet["delivery_reason"], WAIT_EXPIRED_REASON);

        // Every call writes the notice by its ID; the member is told once.
        assert!(fixture.expire().ok);
        let notices = fixture.events("system.notify");
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].data["to"], json!(["worker"]));
        assert_eq!(
            notices[0].data["title"],
            "Your request to Chet was not sent"
        );
    }

    #[test]
    fn a_replaced_member_keeps_the_mark_and_its_successor_is_not_told() {
        let fixture = Fixture::new();
        let groups = GroupStore::new(fixture.home.clone()).expect("groups");
        let mut group = groups.load(&fixture.group_id).expect("group");
        let mut successor = cccc_contracts::Actor::new("worker");
        successor.runtime = cccc_contracts::ActorRuntime::WebModel;
        successor.generation = "successor".into();
        group.actors = vec![successor.clone()];
        groups.save(&group).expect("save");
        let mut added = cccc_contracts::Event::new("actor.add", &group.group_id);
        added.data.insert("actor".into(), json!(successor));
        ledger::append(
            &groups.ledger_path(&group.group_id).expect("ledger"),
            &added,
        )
        .expect("actor.add");

        let response = fixture.expire();
        assert_eq!(response.result["delivery"], "recorded", "{response:?}");
        assert_eq!(fixture.events("runtime.delivery").len(), 1);
        assert!(fixture.events("system.notify").is_empty());
    }

    #[test]
    fn a_retry_after_the_history_fact_completes_only_the_group_fact() {
        let fixture = Fixture::new();
        // The previous call stopped right after writing the private fact.
        Store::new(fixture.home.root())
            .record_not_started(
                chet_history::group_wait_expired_id(fixture.input_id),
                fixture.input_id,
                StartFailure::Cancelled,
            )
            .expect("private fact");
        let retry = fixture.expire();
        assert_eq!(retry.result["registration"], "existing");
        assert_eq!(retry.result["delivery"], "recorded");
        assert_eq!(fixture.events("runtime.delivery").len(), 1);
        assert_eq!(fixture.not_started(), 1);
    }

    #[test]
    fn another_cancellation_is_not_read_as_an_expired_wait() {
        let fixture = Fixture::new();
        Store::new(fixture.home.root())
            .record_not_started(Uuid::new_v4(), fixture.input_id, StartFailure::Cancelled)
            .expect("cancellation");
        let records = fixture.not_started_records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0]["reason"], "cancelled");
        assert!(records[0].get("wait_expired").is_none(), "{records:?}");
    }

    #[test]
    fn another_outcome_is_not_an_expiry_and_leaves_the_group_alone() {
        let fixture = Fixture::new();
        Store::new(fixture.home.root())
            .record_receipt(
                Uuid::new_v4(),
                fixture.input_id,
                chet_history::Receipt {
                    generation: "generation".into(),
                    turn_id: "turn".into(),
                },
            )
            .expect("receipt");
        let response = fixture.expire();
        assert_eq!(
            response.error.expect("conflict").code,
            "chet_history_outcome_conflict"
        );
        assert!(fixture.events("runtime.delivery").is_empty());
        assert_eq!(fixture.not_started(), 0);
    }

    #[test]
    fn an_expiry_needs_the_host_and_the_input_of_this_group() {
        let fixture = Fixture::new();
        for args in [
            json!({"by":"worker","group_id":fixture.group_id,"input_id":fixture.input_id}),
            json!({"by":"user","_cccc_web_binding":{},"group_id":fixture.group_id,
                   "input_id":fixture.input_id}),
            json!({"by":"user","group_id":"g_other","input_id":fixture.input_id}),
            json!({"by":"user","group_id":fixture.group_id,"input_id":Uuid::new_v4()}),
        ] {
            assert!(!call(&fixture.home, "chet_group_wait_expired", args).ok);
        }
        assert!(fixture.events("runtime.delivery").is_empty());
        assert_eq!(fixture.not_started(), 0);
    }
}
