use super::{CreationSteps, RealCreationSteps, create_using, handle};
use crate::dispatch::{OpError, dispatch};
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupDoc, GroupStore, HomeLayout, Registry, Scope, active};
use serde_json::{Value, json};

fn request(args: Value) -> DaemonRequest {
    DaemonRequest {
        v: 1,
        op: "group_create_with_scope".into(),
        args: args.as_object().cloned().expect("args"),
    }
}

#[test]
fn creates_only_the_exact_target_and_returns_attached_group() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let target = temp.path().join("project");
    let response = dispatch(&home, &request(json!({"title":"demo","path":target})));
    assert!(response.ok, "{:?}", response.error);
    let group_id = response.result["group_id"].as_str().expect("group id");
    assert!(target.is_dir());
    assert_eq!(response.result["group"]["group_id"], group_id);
    assert_eq!(
        GroupStore::new(home)
            .expect("store")
            .list()
            .expect("groups")
            .len(),
        1
    );
}

#[test]
fn missing_parent_does_not_create_group_or_recursive_directories() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let target = temp.path().join("missing/project");
    let result = handle(&home, &request(json!({"title":"demo","path":target}))).expect("handled");
    assert!(result.is_err());
    assert!(!temp.path().join("missing").exists());
    assert!(
        GroupStore::new(home)
            .expect("store")
            .list()
            .expect("groups")
            .is_empty()
    );
}

#[derive(Clone, Copy)]
enum FailAt {
    Create,
    Attach,
    Ledger,
    Active,
}

struct FaultingSteps {
    fail_at: FailAt,
    rollback_fails: bool,
}

impl CreationSteps for FaultingSteps {
    fn create(&self, store: &GroupStore, title: &str, topic: &str) -> std::io::Result<GroupDoc> {
        if matches!(self.fail_at, FailAt::Create) {
            return Err(std::io::Error::other("create failed"));
        }
        store.create(title, topic)
    }

    fn attach(
        &self,
        store: &GroupStore,
        group_id: &str,
        scope: Scope,
    ) -> std::io::Result<GroupDoc> {
        if matches!(self.fail_at, FailAt::Attach) {
            return Err(std::io::Error::other("attach failed"));
        }
        cccc_core::group_scope::attach(store, group_id, scope)
    }

    fn append(
        &self,
        home: &HomeLayout,
        request: &DaemonRequest,
        group: &GroupDoc,
    ) -> Result<(), OpError> {
        if matches!(self.fail_at, FailAt::Ledger) {
            return Err(OpError::new("io_error", "ledger failed"));
        }
        RealCreationSteps.append(home, request, group)
    }

    fn activate(&self, home: &HomeLayout, group_id: &str) -> std::io::Result<()> {
        if matches!(self.fail_at, FailAt::Active) {
            return Err(std::io::Error::other("active failed"));
        }
        active::set(home, group_id)
    }

    fn rollback(&self, store: &GroupStore, group_id: &str) -> std::io::Result<bool> {
        if self.rollback_fails {
            return Err(std::io::Error::other("delete failed"));
        }
        store.delete(group_id)
    }
}

#[test]
fn every_failed_stage_restores_all_visible_creation_state() {
    for fail_at in [
        FailAt::Create,
        FailAt::Attach,
        FailAt::Ledger,
        FailAt::Active,
    ] {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        GroupStore::new(home.clone()).expect("initialize");
        active::set(&home, "g_previous").expect("old active");
        let target = temp.path().join("project");
        let result = create_using(
            &home,
            &request(json!({"title":"demo","path":target})),
            &FaultingSteps {
                fail_at,
                rollback_fails: false,
            },
        );
        assert!(result.is_err());
        let store = GroupStore::new(home.clone()).expect("store");
        assert!(store.list().expect("groups").is_empty());
        assert!(Registry::load(&home).expect("registry").defaults.is_empty());
        assert_eq!(
            active::get(&home).expect("active").as_deref(),
            Some("g_previous")
        );
        assert!(!target.exists());
        assert!(
            std::fs::read_dir(home.groups_dir())
                .expect("groups directory")
                .all(|entry| entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with('.'))
        );
    }
}

#[test]
fn failed_second_creation_restores_previous_scope_default() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let target = temp.path().join("project");
    let first = dispatch(&home, &request(json!({"title":"first","path":target})));
    assert!(first.ok, "{:?}", first.error);
    let first_id = first.result["group_id"]
        .as_str()
        .expect("first id")
        .to_owned();
    let store = GroupStore::new(home.clone()).expect("store");
    let first_group = store.load(&first_id).expect("first group");
    let scope_key = first_group.active_scope_key.clone();

    let failure = create_using(
        &home,
        &request(json!({"title":"second","path":target})),
        &FaultingSteps {
            fail_at: FailAt::Ledger,
            rollback_fails: false,
        },
    )
    .expect_err("second creation failure");

    assert_eq!(failure.code, "io_error");
    assert_eq!(store.list().expect("groups").len(), 1);
    assert_eq!(
        Registry::load(&home)
            .expect("registry")
            .defaults
            .get(&scope_key),
        Some(&first_id)
    );
    assert_eq!(active::get(&home).expect("active"), Some(first_id));
}

#[test]
fn rollback_failure_is_never_hidden_as_the_original_stage_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let target = temp.path().join("project");
    let error = create_using(
        &home,
        &request(json!({"title":"demo","path":target})),
        &FaultingSteps {
            fail_at: FailAt::Ledger,
            rollback_fails: true,
        },
    )
    .expect_err("failure");
    assert_eq!(error.code, "rollback_failed");
    assert_eq!(error.details["original_code"], "io_error");
}
