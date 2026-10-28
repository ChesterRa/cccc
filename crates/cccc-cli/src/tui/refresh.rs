//! Reading what the screen shows: the session list, the group list and the
//! selected group's members. Each is read only while a screen shows it, one
//! request at a time, and each keeps its own last good answer, so one failed
//! read leaves the others as they were.

use super::groups::{self, GroupList, MemberList};
use super::sessions::{self, SessionList};
use super::tasks::{self, Request as TaskRequest, Snapshot as TaskSnapshot};
use cccc_client::DaemonClient;
use cccc_core::HomeLayout;
use std::time::{Duration, Instant};
use tokio::sync::watch;

const INTERVAL: Duration = Duration::from_secs(3);
// One read, not a verdict: a slow answer leaves the last list on screen and
// the next round asks again.
const READ_TIMEOUT: Duration = Duration::from_secs(5);

/// What the interface has on screen and so wants read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Want {
    pub(super) sessions: bool,
    pub(super) groups: bool,
    /// The group whose members are on screen.
    pub(super) members: Option<String>,
    pub(super) tasks: Option<TaskRequest>,
    /// Raised to read again now instead of at the next interval.
    pub(super) nonce: u64,
}

impl Want {
    fn any(&self) -> bool {
        self.sessions || self.groups || self.members.is_some() || self.tasks.is_some()
    }
}

pub(super) struct Lists {
    pub(super) sessions: watch::Sender<SessionList>,
    pub(super) groups: watch::Sender<GroupList>,
    pub(super) members: watch::Sender<MemberList>,
    pub(super) tasks: watch::Sender<TaskSnapshot>,
}

/// Read what `want` asks for, then again every few seconds or as soon as it
/// changes. Ends once the interface drops its side of the channels.
///
/// The reads run one after another, so an answer for a group never overtakes
/// a later one; the member list carries the group it was read for, and the
/// interface shows it only while that group is still selected.
pub(super) async fn run(home: HomeLayout, mut want: watch::Receiver<Want>, lists: Lists) {
    let client = DaemonClient::new(home).with_timeout(READ_TIMEOUT);
    loop {
        if want.wait_for(Want::any).await.is_err() {
            return;
        }
        let wanted = want.borrow_and_update().clone();
        if wanted.sessions {
            let read = sessions::read(&client).await;
            lists
                .sessions
                .send_modify(|list| list.record(read, Instant::now()));
        }
        if wanted.groups {
            let read = groups::read_groups(&client).await;
            lists
                .groups
                .send_modify(|list| list.record(read, Instant::now()));
        }
        if let Some(group_id) = &wanted.members {
            let read = groups::read_members(&client, group_id).await;
            lists
                .members
                .send_modify(|list| list.record(group_id, read, Instant::now()));
        }
        if let Some(request) = &wanted.tasks {
            let read = tasks::read(&client, request).await;
            lists
                .tasks
                .send_modify(|snapshot| snapshot.record(request, read, Instant::now()));
        }
        if lists.sessions.is_closed() {
            return;
        }
        tokio::select! {
            () = tokio::time::sleep(INTERVAL) => {}
            changed = want.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }
    }
}
