use cccc_core::HomeLayout;
use cccc_core::capabilities::CapabilityState;
use std::collections::BTreeSet;
use std::io;

pub(super) struct EffectiveCapabilityState {
    pub enabled: BTreeSet<String>,
    pub blocked: BTreeSet<String>,
    pub hidden: BTreeSet<String>,
}

pub(super) fn load(
    home: &HomeLayout,
    group_id: &str,
    actor_id: &str,
    native: &CapabilityState,
) -> io::Result<EffectiveCapabilityState> {
    let legacy = cccc_core::capability_legacy::scope(home, group_id, actor_id)?;
    Ok(EffectiveCapabilityState {
        enabled: merged(&legacy.enabled, &native.enabled, &native.disabled),
        blocked: merged(&legacy.blocked, &native.blocked, &native.unblocked),
        hidden: merged(&legacy.hidden, &native.hidden, &native.visible),
    })
}

fn merged(
    legacy: &BTreeSet<String>,
    added: &BTreeSet<String>,
    removed: &BTreeSet<String>,
) -> BTreeSet<String> {
    legacy
        .union(added)
        .filter(|id| !removed.contains(*id))
        .cloned()
        .collect()
}
