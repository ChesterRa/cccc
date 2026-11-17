use cccc_core::HomeLayout;
use serde::{Deserialize, Serialize};
use std::io;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShutdownMode {
    Stop,
    PreserveAgents,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub group_id: String,
    pub actor_id: String,
    pub actor_created_at: String,
    pub generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShutdownReport {
    pub v: u32,
    pub daemon_pid: u32,
    pub request_id: String,
    pub mode: ShutdownMode,
    pub confirmed_stopped: Vec<AgentIdentity>,
    pub retained: Vec<AgentIdentity>,
    pub unconfirmed: Vec<AgentIdentity>,
}

/// Read only the matching daemon's final result. An absent or stale receipt
/// is not evidence that a stop or maintenance request completed.
pub fn read_shutdown_report(
    home: &HomeLayout,
    expected_daemon_pid: u32,
    request_id: Option<&str>,
) -> io::Result<Option<ShutdownReport>> {
    let path = home.daemon_dir().join("shutdown-result.json");
    let result: ShutdownReport = match cccc_core::fs::read_json(&path) {
        Ok(result) => result,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok((result.v == 1
        && result.daemon_pid == expected_daemon_pid
        && request_id.is_none_or(|request| request == result.request_id))
    .then_some(result))
}

pub(crate) fn write_shutdown_report(home: &HomeLayout, report: &ShutdownReport) -> io::Result<()> {
    cccc_core::fs::write_json(&home.daemon_dir().join("shutdown-result.json"), report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_previous_exit_is_not_a_receipt_for_this_request() {
        let directory = tempfile::tempdir().expect("isolated test directory");
        let home =
            HomeLayout::from_path(directory.path().to_path_buf()).expect("resolve isolated home");
        let report = ShutdownReport {
            v: 1,
            daemon_pid: 123,
            request_id: "request-one".into(),
            mode: ShutdownMode::PreserveAgents,
            retained: Vec::new(),
            confirmed_stopped: Vec::new(),
            unconfirmed: Vec::new(),
        };
        write_shutdown_report(&home, &report).expect("write shutdown receipt");
        assert_eq!(
            read_shutdown_report(&home, 123, Some("request-one")).expect("read shutdown receipt"),
            Some(report)
        );
        assert!(
            read_shutdown_report(&home, 124, None)
                .expect("read shutdown receipt")
                .is_none()
        );
        assert!(
            read_shutdown_report(&home, 123, Some("request-two"))
                .expect("read shutdown receipt")
                .is_none()
        );
    }
}
