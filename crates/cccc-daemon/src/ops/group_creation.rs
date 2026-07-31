use cccc_contracts::DaemonRequest;
use cccc_core::path_input::{PreparedDirectory, ensure_exact_directory, remove_if_created_empty};
use cccc_core::{GroupDoc, GroupStore, HomeLayout, Registry, Scope, active, group_scope, scope};
use serde_json::{Value, json};
use std::io;

use crate::dispatch::{OpError, OpResult, object, required_arg, store, string_arg};

pub fn handle(home: &HomeLayout, request: &DaemonRequest) -> Option<OpResult> {
    (request.op == "group_create_with_scope").then(|| create(home, request))
}

fn create(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    create_using(home, request, &RealCreationSteps)
}

trait CreationSteps {
    fn create(&self, store: &GroupStore, title: &str, topic: &str) -> io::Result<GroupDoc>;
    fn attach(&self, store: &GroupStore, group_id: &str, scope: Scope) -> io::Result<GroupDoc>;
    fn append(
        &self,
        home: &HomeLayout,
        request: &DaemonRequest,
        group: &GroupDoc,
    ) -> Result<(), OpError>;
    fn activate(&self, home: &HomeLayout, group_id: &str) -> io::Result<()>;
    fn rollback(&self, store: &GroupStore, group_id: &str) -> io::Result<bool>;
}

struct RealCreationSteps;

impl CreationSteps for RealCreationSteps {
    fn create(&self, store: &GroupStore, title: &str, topic: &str) -> io::Result<GroupDoc> {
        store.create(title, topic)
    }

    fn attach(&self, store: &GroupStore, group_id: &str, scope: Scope) -> io::Result<GroupDoc> {
        group_scope::attach(store, group_id, scope)
    }

    fn append(
        &self,
        home: &HomeLayout,
        request: &DaemonRequest,
        group: &GroupDoc,
    ) -> Result<(), OpError> {
        super::groups::append_group_event(
            home,
            group,
            "group.create",
            request,
            json!({"title": group.title, "topic": group.topic, "scope": group.active_scope_key}),
        )
    }

    fn activate(&self, home: &HomeLayout, group_id: &str) -> io::Result<()> {
        active::set(home, group_id)
    }

    fn rollback(&self, store: &GroupStore, group_id: &str) -> io::Result<bool> {
        store.delete(group_id)
    }
}

fn create_using(
    home: &HomeLayout,
    request: &DaemonRequest,
    steps: &impl CreationSteps,
) -> OpResult {
    let raw_path = required_arg(request, "path")?;
    let directory = ensure_exact_directory(&raw_path).map_err(path_error)?;
    let detected = scope::detect(&directory.path).map_err(|error| {
        cleanup_directory(&directory, OpError::invalid(error), "scope detection")
    })?;
    let group_store = store(home)
        .map_err(|error| cleanup_directory(&directory, error, "group store initialization"))?;
    reject_duplicate_scope(home, &directory, &detected.scope_key)?;

    let title = string_arg(request, "title").unwrap_or_else(|| detected.label.clone());
    let topic = string_arg(request, "topic").unwrap_or_default();
    let created = steps
        .create(&group_store, &title, &topic)
        .map_err(|error| cleanup_directory(&directory, OpError::io(error), "group creation"))?;
    match finish_create(home, request, &group_store, created, detected, steps) {
        Ok(group) => object(json!({
            "group_id": group.group_id,
            "group": super::group_runtime::group(group),
        })),
        Err(failure) => Err(rollback_create(&group_store, &directory, failure, steps)),
    }
}

fn reject_duplicate_scope(
    home: &HomeLayout,
    directory: &PreparedDirectory,
    scope_key: &str,
) -> Result<(), OpError> {
    let registry = Registry::load(home)
        .map_err(OpError::io)
        .map_err(|error| cleanup_directory(directory, error, "registry read"))?;
    let Some(group_id) = registry.defaults.get(scope_key) else {
        return Ok(());
    };
    let mut error = OpError::new(
        "scope_already_attached",
        "project directory is already attached to a group",
    );
    error
        .details
        .insert("group_id".into(), Value::String(group_id.clone()));
    Err(cleanup_directory(
        directory,
        error,
        "duplicate scope detection",
    ))
}

struct CreateFailure {
    group_id: String,
    error: OpError,
}

fn finish_create(
    home: &HomeLayout,
    request: &DaemonRequest,
    store: &GroupStore,
    created: GroupDoc,
    detected: Scope,
    steps: &impl CreationSteps,
) -> Result<GroupDoc, CreateFailure> {
    let group_id = created.group_id.clone();
    let result = (|| {
        let group = steps
            .attach(store, &group_id, detected)
            .map_err(OpError::io)?;
        steps.append(home, request, &group)?;
        steps.activate(home, &group_id).map_err(OpError::io)?;
        Ok(group)
    })();
    result.map_err(|error| CreateFailure { group_id, error })
}

fn rollback_create(
    store: &GroupStore,
    directory: &PreparedDirectory,
    failure: CreateFailure,
    steps: &impl CreationSteps,
) -> OpError {
    if let Err(rollback) = steps.rollback(store, &failure.group_id) {
        return rollback_error(failure.error, "group", rollback);
    }
    cleanup_directory(directory, failure.error, "project directory")
}

fn cleanup_directory(directory: &PreparedDirectory, original: OpError, stage: &str) -> OpError {
    match remove_if_created_empty(directory) {
        Ok(()) => original,
        Err(rollback) => rollback_error(original, stage, rollback),
    }
}

fn rollback_error(original: OpError, stage: &str, rollback: io::Error) -> OpError {
    let mut error = OpError::new(
        "rollback_failed",
        format!(
            "{}; rollback failed for {stage}: {rollback}",
            original.message
        ),
    );
    error
        .details
        .insert("original_code".into(), Value::String(original.code));
    error
}

fn path_error(error: io::Error) -> OpError {
    let code = match error.kind() {
        io::ErrorKind::NotFound => "path_not_found",
        io::ErrorKind::NotADirectory => "not_dir",
        io::ErrorKind::PermissionDenied => "permission_denied",
        _ => "invalid_path",
    };
    OpError::new(code, error.to_string())
}

#[cfg(test)]
#[path = "group_creation_tests.rs"]
mod tests;
