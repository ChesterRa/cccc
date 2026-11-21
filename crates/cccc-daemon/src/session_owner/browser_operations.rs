//! Local daemon ports for browser resources. No public MCP tool exposes CDP
//! addresses or lets a request supply executable arguments or an environment.

#[cfg(unix)]
use crate::dispatch::object;
use crate::dispatch::{OpError, OpResult};
use cccc_contracts::DaemonRequest;
use cccc_core::HomeLayout;
#[cfg(unix)]
use serde::Deserialize;
#[cfg(unix)]
use serde_json::json;

#[cfg(unix)]
use super::{
    browser::{BrowserLaunchSpec, BrowserPrograms},
    controller,
    protocol::{Failure, Mutation},
};

pub(crate) fn handle(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    if request.args.get("by").and_then(serde_json::Value::as_str) != Some("user")
        || request.args.contains_key("group_id")
        || request.args.contains_key("_cccc_native_binding")
        || request.args.contains_key("_cccc_native_source_binding")
        || request.args.contains_key("_cccc_web_binding")
    {
        return Err(OpError::new(
            "permission_denied",
            "browser resources require an unbound local user operation",
        ));
    }
    if !super::enabled(home) {
        return Err(OpError::new(
            "unsupported_platform",
            "this daemon has no independent browser holder",
        ));
    }
    #[cfg(unix)]
    return local(home, request);
    #[cfg(not(unix))]
    {
        let _ = request;
        Err(OpError::new(
            "unsupported_platform",
            "independent browser resources are unavailable",
        ))
    }
}

#[cfg(unix)]
fn local(home: &HomeLayout, request: &DaemonRequest) -> OpResult {
    match request.op.as_str() {
        "browser_resources" => {
            object(json!({"resources":controller::browser_resources(home).map_err(owner_error)?}))
        }
        "browser_resource_ensure" => {
            #[derive(Deserialize)]
            struct Ensure {
                #[serde(flatten)]
                spec: BrowserLaunchSpec,
                programs: Option<BrowserPrograms>,
            }
            let Ensure { mut spec, programs } =
                serde_json::from_value(json!(request.args)).map_err(OpError::invalid)?;
            if programs.as_ref().is_some_and(|programs| !programs.valid()) {
                return Err(OpError::new(
                    "invalid_args",
                    "invalid browser program selection",
                ));
            }
            if !spec.profile.is_absolute() || spec.width == 0 || spec.height == 0 {
                return Err(OpError::new(
                    "invalid_args",
                    "browser profile must be absolute and dimensions positive",
                ));
            }
            spec.profile = std::fs::canonicalize(spec.profile).map_err(OpError::io)?;
            let _permit = crate::runtime_start_gate::try_permit(home)
                .map_err(|message| OpError::new("runtime_stopping", message))?
                .ok_or_else(|| {
                    OpError::new(
                        "runtime_busy",
                        "a runtime start is in progress; retry after it settles",
                    )
                })?;
            object(
                json!({"resource":controller::ensure_browser(home, spec, programs).map_err(owner_error)?}),
            )
        }
        "browser_resource_pages_set" => {
            #[derive(Deserialize)]
            struct Pages {
                resource_id: String,
                instance: String,
                sessions: Vec<super::browser::BrowserPage>,
            }
            let pages: Pages =
                serde_json::from_value(json!(request.args)).map_err(OpError::invalid)?;
            let result = controller::mutate_browser(
                home,
                &pages.instance.clone(),
                Mutation::BrowserPagesSet {
                    resource_id: pages.resource_id,
                    instance: pages.instance,
                    sessions: pages.sessions,
                },
            )
            .map_err(owner_error)?;
            object(json!({"resource":result}))
        }
        "browser_resource_close" => {
            #[derive(Deserialize)]
            struct Close {
                resource_id: String,
                instance: String,
            }
            let close: Close =
                serde_json::from_value(json!(request.args)).map_err(OpError::invalid)?;
            let result = controller::mutate_browser(
                home,
                &close.instance.clone(),
                Mutation::BrowserClose {
                    resource_id: close.resource_id,
                    instance: close.instance,
                },
            )
            .map_err(owner_error)?;
            object(result)
        }
        _ => Err(OpError::new(
            "unknown_op",
            "unknown browser resource operation",
        )),
    }
}

#[cfg(unix)]
fn owner_error(error: std::io::Error) -> OpError {
    if let Some(failure) = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<Failure>())
    {
        OpError::new(&failure.code, &failure.message)
    } else {
        OpError::io(error)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn listing_an_empty_controller_does_not_construct_a_holder() {
        let temp = tempfile::tempdir().expect("isolated home");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        home.initialize().expect("initialize");
        crate::session_owner::initialize(&home).expect("empty controller");
        let request = serde_json::from_value::<DaemonRequest>(
            json!({"op":"browser_resources","args":{"by":"user"}}),
        )
        .expect("request");
        let result = handle(&home, &request).expect("read only list");
        assert_eq!(result["resources"], json!([]));
        assert!(
            !home.daemon_dir().join("session-owners").exists(),
            "a read cannot construct an owner or default factory"
        );
    }
}
