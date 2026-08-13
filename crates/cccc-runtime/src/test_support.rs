use crate::LaunchSpec;
use cccc_contracts::RunnerKind;
use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

pub(crate) fn test_guard() -> MutexGuard<'static, ()> {
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    TEST_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("test lock")
}

pub(crate) fn spec(
    temp: &tempfile::TempDir,
    group: &str,
    actor: &str,
    command: &str,
) -> LaunchSpec {
    LaunchSpec {
        group_id: group.into(),
        actor_id: actor.into(),
        runner: RunnerKind::Headless,
        command: vec!["sh".into(), "-c".into(), command.into()],
        cwd: temp.path().into(),
        env: BTreeMap::new(),
        cols: 80,
        rows: 24,
    }
}
