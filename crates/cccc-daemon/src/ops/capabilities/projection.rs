use cccc_contracts::Actor;
use cccc_core::capabilities::{Capability, CapabilityStore};
use cccc_core::{GroupDoc, HomeLayout};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

use crate::dispatch::OpError;

struct Member {
    actor: Option<Actor>,
    enabled: BTreeSet<String>,
    blocked: BTreeSet<String>,
}

/// One authority for member suitability across search, bootstrap and the Web.
pub(super) struct Projection {
    home: HomeLayout,
    group: Option<GroupDoc>,
    group_id: String,
    members: BTreeMap<String, Member>,
    global_blocked: BTreeSet<String>,
    controls: Value,
    runtime: Value,
    records: BTreeMap<String, Value>,
}

impl Projection {
    pub(super) fn new(
        home: &HomeLayout,
        store: &CapabilityStore,
        group_id: &str,
        group: Option<&GroupDoc>,
    ) -> Result<Self, OpError> {
        let native = store.load().map_err(OpError::io)?;
        let path = home.root().join("state/capabilities/state.json");
        let controls = if path.exists() {
            cccc_core::fs::read_json(&path).map_err(OpError::io)?
        } else {
            json!({})
        };
        let path = home.root().join("state/capabilities/runtime.json");
        let runtime = if path.exists() {
            cccc_core::fs::read_json(&path).map_err(OpError::io)?
        } else {
            json!({})
        };
        let actors = group.map(|group| group.actors.as_slice()).unwrap_or(&[]);
        let mut members = BTreeMap::new();
        for (id, actor) in actors
            .iter()
            .map(|actor| (actor.id.as_str(), Some(actor.clone())))
            .chain(std::iter::once(("user", None)))
        {
            let effective =
                super::effective_state::load(home, group_id, id, &native).map_err(OpError::io)?;
            members.insert(
                id.into(),
                Member {
                    actor,
                    enabled: effective.enabled,
                    blocked: effective.blocked,
                },
            );
        }
        let records = store.resolved_records().map_err(OpError::io)?;
        Ok(Self {
            home: home.clone(),
            group: group.cloned(),
            group_id: group_id.into(),
            members,
            global_blocked: native.blocked,
            controls,
            runtime,
            records,
        })
    }

    pub(super) fn summary(&self, capability: &Capability, actor_id: &str, locale: &str) -> Value {
        let localized = cccc_core::capability_recommendations::localized(capability, locale);
        let mut summary = json!({
            "id":localized.id,"name":localized.name,"kind":localized.kind,
            "description":localized.description,"source":localized.source,
            "source_uri":localized.source_uri,"tags":localized.tags,
            "sync_state":localized.sync_state,"tool_names":localized.tool_names,
            "enable_supported":localized.enable_supported,
            "qualification_status":localized.qualification_status,
            "content_origin":content_origin(capability),
            "read_only":capability.is_release_recommendation(),
            "for_you":self.for_you(capability, actor_id),
        });
        for key in ["recommendation", "requirements_summary", "verification"] {
            summary[key] = localized.metadata.get(key).cloned().unwrap_or_else(|| {
                if key == "requirements_summary" {
                    json!("")
                } else {
                    json!([])
                }
            });
        }
        summary["use_when"] = json!(
            localized
                .metadata
                .get("use_when")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .take(1)
                .cloned()
                .collect::<Vec<_>>()
        );
        if capability.is_release_recommendation() {
            summary["avoid_when"] = localized
                .metadata
                .get("avoid_when")
                .cloned()
                .unwrap_or_else(|| json!([]));
        }
        summary
    }

    pub(super) fn detail(
        &self,
        capability: &Capability,
        actor_id: &str,
        locale: &str,
        known_groups: &BTreeSet<String>,
    ) -> Value {
        let localized = cccc_core::capability_recommendations::localized(capability, locale);
        let mut detail = Value::Object(localized.metadata.clone());
        detail.as_object_mut().expect("metadata").remove("l10n");
        detail["capability_id"] = json!(capability.id);
        detail["content_origin"] = json!(content_origin(capability));
        detail["read_only"] = json!(capability.is_release_recommendation());
        detail["for_you"] = self.for_you(capability, actor_id);
        detail["group_enabled"] = json!(self.group_enabled(&capability.id));
        detail["members"] = json!(
            self.members
                .iter()
                .filter(|(_, member)| member.actor.is_some())
                .map(|(id, _)| {
                    let mut row = self.for_you(capability, id);
                    row["actor_id"] = json!(id);
                    row["enabled_scope"] = self
                        .enabled_scope(&capability.id, id)
                        .map_or(Value::Null, |scope| json!(scope));
                    row["enabled_scopes"] = json!(self.enabled_scopes(&capability.id, id));
                    row["last_call"] = self.last_call(&capability.id, id);
                    row
                })
                .collect::<Vec<_>>()
        );
        if let Some(record) = self.records.get(&capability.id) {
            if capability.kind == "mcp_toolpack"
                || capability.kind == "mcp"
                || capability.id.starts_with("mcp:")
            {
                detail["update"] = super::update::info(
                    record,
                    &capability.id,
                    &self.runtime,
                    &self.controls,
                    known_groups,
                );
            }
            for key in ["install_mode", "install_spec"] {
                // Historical private recipes can contain inline credentials.
                // Only versioned public recipes are safe to reproduce here.
                if (key == "install_mode" || capability.is_release_recommendation())
                    && let Some(value) = record.get(key)
                {
                    detail[key] = value.clone();
                }
            }
        }
        detail
    }

    pub(super) fn for_you(&self, capability: &Capability, actor_id: &str) -> Value {
        let member = self.members.get(actor_id);
        let enabled = member.is_some_and(|member| member.enabled.contains(&capability.id));
        let blocked = member.is_some_and(|member| member.blocked.contains(&capability.id))
            || self.global_blocked.contains(&capability.id);
        let (availability, mut reasons) = self.availability(capability, member);
        if blocked {
            reasons.push(if self.global_blocked.contains(&capability.id) {
                "blocked_by_global_policy"
            } else {
                "blocked_by_group_policy"
            });
        }
        let mut result = json!({"availability":availability,"enabled":enabled,"blocked":blocked,"reason_codes":reasons});
        if let Some(record) = self.records.get(&capability.id) {
            let execution = super::credentials::execution_record(record, &self.runtime);
            if super::credentials::bearer(&execution)
                && let Some(env) = super::credentials::env_name(&execution)
            {
                result["credential_env"] = json!(env);
                result["activation_pending"] = json!(
                    enabled
                        && availability == "compatible"
                        && self
                            .group
                            .as_ref()
                            .is_some_and(|group| super::credentials::pending(
                                &self.home,
                                group,
                                member.and_then(|member| member.actor.as_ref()),
                                &execution
                            ))
                );
                if let Some(actor) = member.and_then(|member| member.actor.as_ref()) {
                    result["credential_source"] = json!(if actor.profile_id.is_empty() {
                        "actor"
                    } else {
                        "profile"
                    });
                }
            }
        }
        result
    }

    fn availability(
        &self,
        capability: &Capability,
        member: Option<&Member>,
    ) -> (&'static str, Vec<&'static str>) {
        if capability.is_unverified_remote_search() {
            return ("unsupported", vec!["unverified_remote_search"]);
        }
        let is_mcp = capability.id.starts_with("mcp:")
            || matches!(capability.kind.as_str(), "mcp" | "mcp_toolpack");
        if is_mcp
            && member
                .and_then(|member| member.actor.as_ref())
                .is_some_and(|actor| actor.runtime.is_web_model())
        {
            return ("unsupported", vec!["external_mcp_not_supported_by_actor"]);
        }
        if !capability.enable_supported || capability.qualification_status == "blocked" {
            return ("unsupported", vec!["delivery_form_not_supported"]);
        }
        let inferred = is_mcp
            .then(|| self.records.get(&capability.id))
            .flatten()
            .and_then(|record| match record["install_mode"].as_str() {
                Some("remote_only") => Some(json!({"form":"remote_http","auth":"none"})),
                Some("package" | "command") => Some(json!({"form":"stdio","auth":"none"})),
                _ => None,
            });
        let Some(support) = capability
            .metadata
            .get("support")
            .filter(|support| support.is_object())
            .or(inferred.as_ref())
        else {
            return if CapabilityStore::builtin(&capability.id).is_some()
                || capability.kind == "pack"
            {
                ("compatible", vec![])
            } else {
                (
                    "unknown",
                    vec![if is_mcp {
                        "delivery_form_unknown"
                    } else {
                        "actor_tool_prerequisites_unknown"
                    }],
                )
            };
        };
        match support["form"].as_str() {
            Some("text_skill") => {
                if support["prerequisites"]
                    .as_array()
                    .is_some_and(|requirements| requirements.is_empty())
                {
                    ("compatible", vec![])
                } else {
                    ("unknown", vec!["actor_tool_prerequisites_unknown"])
                }
            }
            Some("remote_http" | "stdio") => {
                let execution = self
                    .records
                    .get(&capability.id)
                    .map(|record| super::credentials::execution_record(record, &self.runtime));
                let auth = execution
                    .as_ref()
                    .and_then(|record| record.pointer("/support/auth"))
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| support["auth"].as_str().unwrap_or("none"));
                if auth != "none" {
                    return match (execution.as_ref(), self.group.as_ref()) {
                        (Some(record), Some(group)) if super::credentials::bearer(record) => {
                            super::credentials::availability(
                                &self.home,
                                group,
                                member.and_then(|member| member.actor.as_ref()),
                                record,
                            )
                        }
                        _ => ("unsupported", vec!["credential_route_not_supported"]),
                    };
                }
                if support["form"] == "stdio"
                    && let Some(record) = self.records.get(&capability.id)
                {
                    let mut reasons = Vec::new();
                    if !super::package_install::prerequisites_available(record) {
                        reasons.push("missing_daemon_executable");
                    }
                    if !super::package_install::missing_environment(record).is_empty() {
                        reasons.push("missing_daemon_environment");
                    }
                    if !reasons.is_empty() {
                        return ("needs_setup", reasons);
                    }
                }
                ("compatible", vec![])
            }
            _ => ("unsupported", vec!["delivery_form_not_supported"]),
        }
    }

    pub(super) fn enabled_scope(
        &self,
        capability_id: &str,
        actor_id: &str,
    ) -> Option<&'static str> {
        self.enabled_scopes(capability_id, actor_id)
            .into_iter()
            .next()
    }

    pub(super) fn group_enabled(&self, capability_id: &str) -> bool {
        self.controls
            .get("group_enabled")
            .and_then(|groups| groups.get(&self.group_id))
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.as_str() == Some(capability_id))
            })
    }

    pub(super) fn enabled_scopes(&self, capability_id: &str, actor_id: &str) -> Vec<&'static str> {
        let mut scopes = Vec::new();
        let contains = |items: Option<&Value>| {
            items.and_then(Value::as_array).is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.as_str() == Some(capability_id))
            })
        };
        if contains(
            self.controls
                .get("actor_enabled")
                .and_then(|groups| groups.get(&self.group_id))
                .and_then(|actors| actors.get(actor_id)),
        ) {
            scopes.push("actor");
        }
        let now = chrono::Utc::now();
        let session = self
            .controls
            .get("session_enabled")
            .and_then(|groups| groups.get(&self.group_id))
            .and_then(|actors| actors.get(actor_id))
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items.iter().any(|entry| {
                    entry["capability_id"] == capability_id
                        && entry["expires_at"]
                            .as_str()
                            .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
                            .is_some_and(|expiry| expiry > now)
                })
            });
        if session {
            scopes.insert(0, "session");
        }
        if self.group_enabled(capability_id) {
            scopes.push("group");
        }
        scopes
    }

    fn last_call(&self, capability_id: &str, actor_id: &str) -> Value {
        self.runtime
            .get("actor_instances")
            .and_then(|groups| groups.get(&self.group_id))
            .and_then(|actors| actors.get(actor_id))
            .and_then(|items| items.get(capability_id))
            .and_then(|binding| binding.get("last_call"))
            .filter(|call| call["at"].is_string() && call["ok"].is_boolean())
            .cloned()
            .unwrap_or(Value::Null)
    }
}

fn content_origin(capability: &Capability) -> &'static str {
    if capability.is_release_recommendation() {
        "release_recommendation"
    } else if CapabilityStore::builtin(&capability.id).is_some() {
        "builtin"
    } else {
        "user_catalog"
    }
}
