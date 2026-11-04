//! Runtimes such as Claude Code and Codex load instruction files (AGENTS.md,
//! CLAUDE.md and their variants) when a session starts and keep that copy while
//! the files change on disk. The daemon records the files for each session it
//! delivers to: before launch where it starts the session itself, otherwise at
//! the first delivery. The next delivery after one of them changed, appeared or
//! was removed carries a one-time line asking the agent to check the current
//! instructions. A change made before the record is not seen. One record is
//! kept for each member and workspace it was observed in, in daemon memory, so
//! a daemon restart clears them and a session is recorded again at its next
//! delivery. Lines name paths only: no file contents, and no claim about which
//! instructions take precedence. Only regular files count: a path that resolves
//! to a directory or a FIFO has no instruction file, so a file replaced by one
//! is reported as removed; non-regular targets are skipped before opening.

use std::collections::{BTreeMap, HashMap};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use cccc_contracts::Actor;
use cccc_core::HomeLayout;
use sha2::{Digest, Sha256};

/// The names Claude Code and Codex read in the workspace and each directory above it.
const FILE_NAMES: [&str; 5] = [
    "AGENTS.override.md",
    "AGENTS.md",
    "CLAUDE.md",
    "CLAUDE.local.md",
    ".claude/CLAUDE.md",
];

/// The readable instruction files seen from one workspace (by its canonical
/// path), by path, with the digest of the content each path resolves to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    root: PathBuf,
    files: BTreeMap<PathBuf, [u8; 32]>,
}

impl Snapshot {
    /// Each path is kept, so removing one of two links to a file is reported; a
    /// file reached through several links is read once. `None` when a file could
    /// not be read for a reason other than its absence, such as a permission
    /// error: that is not taken as a removal.
    pub(crate) fn read(root: &Path) -> Option<Self> {
        let root = std::fs::canonicalize(root).ok()?;
        let mut by_target: HashMap<PathBuf, Option<[u8; 32]>> = HashMap::new();
        let mut files = BTreeMap::new();
        for dir in root.ancestors() {
            for name in FILE_NAMES {
                let path = dir.join(name);
                let target = match std::fs::canonicalize(&path) {
                    Ok(target) => target,
                    Err(error) if absent(&error) => continue,
                    Err(_) => return None,
                };
                let digest = match by_target.get(&target) {
                    Some(digest) => *digest,
                    None => {
                        let digest = regular_file_digest(&target).ok()?;
                        by_target.insert(target, digest);
                        digest
                    }
                };
                if let Some(digest) = digest {
                    files.insert(path, digest);
                }
            }
        }
        Some(Self { root, files })
    }

    fn changes(&self, now: &Self) -> Vec<String> {
        let mut changes = Vec::new();
        for (path, digest) in &now.files {
            match self.files.get(path) {
                None => changes.push(format!("{} was added", self.show(path))),
                Some(earlier) if earlier != digest => {
                    changes.push(format!("{} changed", self.show(path)));
                }
                Some(_) => {}
            }
        }
        for path in self.files.keys() {
            if !now.files.contains_key(path) {
                changes.push(format!("{} was removed", self.show(path)));
            }
        }
        changes
    }

    /// Paths inside the workspace are shown relative to it.
    fn show(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }
}

/// The content digest of a regular file; `None` when `target` is not one.
fn regular_file_digest(target: &Path) -> std::io::Result<Option<[u8; 32]>> {
    let regular = match std::fs::metadata(target) {
        Ok(metadata) => metadata.is_file(),
        Err(error) if absent(&error) => false,
        Err(error) => return Err(error),
    };
    if !regular {
        return Ok(None);
    }
    match std::fs::read(target) {
        Ok(content) => Ok(Some(Sha256::digest(&content).into())),
        Err(error) if absent(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

/// A missing file, a path through a regular file, or a directory where a file
/// is expected: there is no instruction file at that path.
fn absent(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::NotFound | ErrorKind::NotADirectory | ErrorKind::IsADirectory
    )
}

/// Home, Group, member and the workspace's canonical path.
type Key = (PathBuf, String, String, PathBuf);

struct Recorded {
    session: String,
    snapshot: Snapshot,
}

fn table() -> MutexGuard<'static, HashMap<Key, Recorded>> {
    static TABLE: OnceLock<Mutex<HashMap<Key, Recorded>>> = OnceLock::new();
    TABLE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

fn key(home: &HomeLayout, group_id: &str, actor: &Actor, root: &Path) -> Key {
    (
        home.root().to_path_buf(),
        group_id.into(),
        actor.id.clone(),
        root.to_path_buf(),
    )
}

/// A transport's session, qualified by the actor's generation so that an actor
/// recreated under the same id starts from its own record.
fn session_of(actor: &Actor, session: &str) -> String {
    format!(
        "{}\u{0}{session}",
        cccc_core::actors::generation_identity(actor)
    )
}

/// Records what a session starts with in the snapshot's workspace, from a
/// snapshot taken before it was launched. The same session keeps its earlier
/// record, so a repeated attach does not hide a change; another session
/// replaces it.
pub(crate) fn record(
    home: &HomeLayout,
    group_id: &str,
    actor: &Actor,
    session: &str,
    snapshot: Snapshot,
) {
    let session = session_of(actor, session);
    let mut table = table();
    let key = key(home, group_id, actor, &snapshot.root);
    if table
        .get(&key)
        .is_some_and(|recorded| recorded.session == session)
    {
        return;
    }
    table.insert(key, Recorded { session, snapshot });
}

/// Instruction-file changes not yet delivered to a session.
pub(crate) struct Pending {
    key: Key,
    session: String,
    now: Snapshot,
    changes: Vec<String>,
}

/// The changes in workspace `root` since this session's record there. A session
/// seen there for the first time is recorded now and reports nothing until a
/// later change; so is one whose files cannot be read yet. Each workspace keeps
/// its own record, so a member that moves from one to another and back still
/// hears about a change made in the first while it was away.
pub(crate) fn check(
    home: &HomeLayout,
    group_id: &str,
    actor: &Actor,
    session: &str,
    root: &Path,
) -> Option<Pending> {
    let root = std::fs::canonicalize(root).ok()?;
    let key = key(home, group_id, actor, &root);
    let session = session_of(actor, session);
    let recorded = table()
        .get(&key)
        .filter(|recorded| recorded.session == session)
        .map(|recorded| recorded.snapshot.clone());
    let Some(recorded) = recorded else {
        let snapshot = Snapshot::read(&root)?;
        let mut table = table();
        if table
            .get(&key)
            .is_none_or(|recorded| recorded.session != session)
        {
            table.insert(key, Recorded { session, snapshot });
        }
        return None;
    };
    // An unreadable file leaves the record as it is until a later delivery.
    let now = Snapshot::read(&root)?;
    let changes = recorded.changes(&now);
    (!changes.is_empty()).then_some(Pending {
        key,
        session,
        now,
        changes,
    })
}

impl Pending {
    /// Appended to a delivery that carries the message text.
    pub(crate) fn delivery_line(&self) -> String {
        format!(
            "[cccc] INSTRUCTION FILES CHANGED since CCCC began monitoring them for this session: {}. Instructions loaded earlier may be out of date: check the current project instructions now and apply them according to your usual instruction precedence.",
            self.changes.join("; ")
        )
    }

    /// The short form for a notification with a small size budget. A path
    /// above the workspace can be long, so a long one is only counted.
    pub(crate) fn short_line(&self) -> String {
        match self.changes.as_slice() {
            [change] if change.len() <= 64 => format!(
                "[CCCC] {change} since CCCC began monitoring it; check current project instructions before acting."
            ),
            [_] => "[CCCC] A project instruction file changed since CCCC began monitoring it; check current project instructions before acting.".into(),
            changes => format!(
                "[CCCC] {} project instruction files changed since CCCC began monitoring them; check current project instructions before acting.",
                changes.len()
            ),
        }
    }

    /// Call once the delivery carrying a line was accepted; until then the same
    /// changes are offered again. What is kept is the observation this line
    /// reported, so a change made since is still reported later.
    pub(crate) fn delivered(self) {
        let mut table = table();
        if let Some(recorded) = table
            .get_mut(&self.key)
            .filter(|recorded| recorded.session == self.session)
        {
            recorded.snapshot = self.now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        _temp: tempfile::TempDir,
        home: HomeLayout,
        work: PathBuf,
        actor: Actor,
    }

    fn fixture() -> Fixture {
        let temp = tempfile::tempdir().expect("temp");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let work = temp.path().join("repo/app");
        std::fs::create_dir_all(&work).expect("workspace");
        let work = std::fs::canonicalize(&work).expect("canonical workspace");
        let mut actor = Actor::new("a1");
        actor.generation = "gen-1".into();
        Fixture {
            _temp: temp,
            home,
            work,
            actor,
        }
    }

    impl Fixture {
        fn check(&self, session: &str) -> Option<Pending> {
            check(&self.home, "g1", &self.actor, session, &self.work)
        }

        fn record(&self, session: &str) {
            let snapshot = Snapshot::read(&self.work).expect("readable");
            record(&self.home, "g1", &self.actor, session, snapshot);
        }
    }

    #[test]
    fn a_change_is_offered_until_delivered_then_not_again() {
        let fx = fixture();
        let agents = fx.work.join("AGENTS.md");
        std::fs::write(&agents, "commit when asked").expect("write");
        assert!(fx.check("pty:1").is_none(), "first sight only records");
        assert!(fx.check("pty:1").is_none(), "nothing changed");

        std::fs::write(&agents, "commit each milestone").expect("write");
        let pending = fx.check("pty:1").expect("change");
        let line = pending.delivery_line();
        assert!(
            line.contains("for this session: AGENTS.md changed."),
            "{line}"
        );
        assert!(
            line.contains("check the current project instructions now"),
            "{line}"
        );
        assert!(!line.contains("milestone"), "no contents: {line}");
        assert_eq!(
            pending.short_line(),
            "[CCCC] AGENTS.md changed since CCCC began monitoring it; check current project instructions before acting."
        );
        drop(pending);
        // Not delivered: offered again.
        let pending = fx.check("pty:1").expect("still pending");
        pending.delivered();
        assert!(fx.check("pty:1").is_none(), "delivered once");

        std::fs::write(&agents, "commit each milestone").expect("same content");
        assert!(fx.check("pty:1").is_none(), "same content is no change");
    }

    #[test]
    fn a_change_after_the_check_is_not_swallowed_by_its_delivery() {
        let fx = fixture();
        let agents = fx.work.join("AGENTS.md");
        std::fs::write(&agents, "a").expect("write");
        fx.check("s");
        std::fs::write(&agents, "b").expect("write");
        let pending = fx.check("s").expect("first change");
        std::fs::write(&agents, "c").expect("write");
        pending.delivered();
        assert!(
            fx.check("s").is_some(),
            "the later change is still reported"
        );
    }

    #[test]
    fn additions_removals_and_files_above_the_workspace_are_named() {
        let fx = fixture();
        let parent = fx.work.parent().expect("parent").join("CLAUDE.md");
        let local = fx.work.join(".claude/CLAUDE.md");
        std::fs::write(&parent, "a").expect("write");
        fx.check("s");
        std::fs::remove_file(&parent).expect("remove");
        std::fs::create_dir_all(local.parent().expect("dir")).expect("dir");
        std::fs::write(&local, "b").expect("write");
        let pending = fx.check("s").expect("changes");
        let line = pending.delivery_line();
        assert!(line.contains(".claude/CLAUDE.md was added"), "{line}");
        assert!(
            line.contains(&format!("{} was removed", parent.display())),
            "{line}"
        );
        assert!(
            pending
                .short_line()
                .starts_with("[CCCC] 2 project instruction files changed")
        );
    }

    #[cfg(unix)]
    #[test]
    fn removing_one_of_two_links_is_a_removal() {
        let fx = fixture();
        let agents = fx.work.join("AGENTS.md");
        let claude = fx.work.join("CLAUDE.md");
        std::fs::write(&agents, "a").expect("write");
        std::os::unix::fs::symlink(&agents, &claude).expect("link");
        fx.check("s");
        std::fs::remove_file(&claude).expect("unlink");
        let line = fx.check("s").expect("removal").delivery_line();
        assert!(line.contains(": CLAUDE.md was removed."), "{line}");
    }

    #[cfg(unix)]
    #[test]
    fn a_fifo_is_no_instruction_file_and_is_never_opened() {
        let fx = fixture();
        let agents = fx.work.join("AGENTS.md");
        std::fs::write(&agents, "a").expect("write");
        fx.check("s");
        std::fs::remove_file(&agents).expect("remove");
        let status = std::process::Command::new("mkfifo")
            .arg(&agents)
            .status()
            .expect("mkfifo");
        assert!(status.success());
        // Opening the FIFO for reading would block this thread.
        let line = fx.check("s").expect("removal").delivery_line();
        assert!(line.contains(": AGENTS.md was removed."), "{line}");
    }

    #[cfg(unix)]
    #[test]
    fn an_unreadable_file_is_neither_a_removal_nor_a_new_record() {
        use std::os::unix::fs::PermissionsExt;
        let fx = fixture();
        let agents = fx.work.join("AGENTS.md");
        std::fs::write(&agents, "a").expect("write");
        fx.check("s");
        std::fs::write(&agents, "b").expect("write");
        std::fs::set_permissions(&agents, std::fs::Permissions::from_mode(0o000)).expect("chmod");
        // Root can still read it; only assert when the permission takes effect.
        if std::fs::read(&agents).is_err() {
            assert!(fx.check("s").is_none(), "no removal reported");
        }
        std::fs::set_permissions(&agents, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        let line = fx.check("s").expect("change kept").delivery_line();
        assert!(line.contains("AGENTS.md changed"), "{line}");
    }

    #[test]
    fn a_repeated_record_keeps_the_baseline_and_a_new_session_or_actor_replaces_it() {
        let mut fx = fixture();
        let agents = fx.work.join("AGENTS.md");
        std::fs::write(&agents, "a").expect("write");
        fx.record("native:1");
        std::fs::write(&agents, "b").expect("write");
        // A heartbeat records the same session again: the earlier baseline stays.
        fx.record("native:1");
        assert!(fx.check("native:1").is_some());
        // Another session starts from what it sees now.
        assert!(fx.check("native:2").is_none());
        std::fs::write(&agents, "c").expect("write");
        let pending = fx.check("native:2").expect("change");
        // An actor recreated under the same id does not inherit it.
        fx.actor.generation = "gen-2".into();
        pending.delivered();
        assert!(fx.check("native:2").is_none(), "new generation records");
        fx.actor.generation = "gen-1".into();
        assert!(
            fx.check("native:2").is_none(),
            "replaced by the new generation"
        );
    }

    #[test]
    fn each_workspace_keeps_its_own_record() {
        let fx = fixture();
        let other = fx.work.parent().expect("parent").join("other");
        std::fs::create_dir_all(&other).expect("other workspace");
        let in_a = fx.work.join("AGENTS.md");
        let in_b = other.join("AGENTS.md");
        std::fs::write(&in_a, "a").expect("write");
        std::fs::write(&in_b, "b").expect("write");
        let at = |root: &Path| check(&fx.home, "g1", &fx.actor, "web", root);
        assert!(at(&fx.work).is_none(), "first sight of A");
        assert!(at(&other).is_none(), "first sight of B");
        // A changes while the member works in B.
        std::fs::write(&in_a, "a2").expect("write");
        std::fs::write(&in_b, "b2").expect("write");
        let back_in_a = at(&fx.work).expect("A's change is kept");
        let in_b_now = at(&other).expect("B's change");
        // Each delivery settles only its own workspace.
        back_in_a.delivered();
        assert!(at(&fx.work).is_none());
        assert!(at(&other).is_some(), "B is still pending");
        in_b_now.delivered();
        assert!(at(&other).is_none());
    }

    #[test]
    fn a_workspace_reached_by_another_spelling_is_the_same_record() {
        let fx = fixture();
        let agents = fx.work.join("AGENTS.md");
        std::fs::write(&agents, "a").expect("write");
        fx.record("pty:1");
        std::fs::write(&agents, "b").expect("write");
        let spelled = fx.work.join("..").join("app");
        assert!(
            check(&fx.home, "g1", &fx.actor, "pty:1", &spelled).is_some(),
            "the launch record is found from the session's own spelling"
        );
    }
}
