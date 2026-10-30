use cccc_core::HomeLayout;
use std::sync::Arc;

/// Constructed only after the normal Web host has claimed its instance lock.
pub(crate) fn new(home: &HomeLayout, daemon_owned: bool) -> cccc_web::VoiceProcessRecovery {
    #[cfg(unix)]
    {
        // A separate directory prevents either guard's watch-list cleanup from
        // matching the other owner's files.
        let ledger = home
            .root()
            .join("web-processes")
            .join("owned-process-groups.json");
        Arc::new(move || {
            let result = cccc_runtime::prepare_web_process_guard(&ledger, !daemon_owned);
            if let Err(error) = &result {
                cccc_runtime::diagnostic(format_args!(
                    "[cccc] Voice process recovery pending in CCCC_HOME/web-processes: {error}"
                ));
            }
            result
        })
    }
    #[cfg(not(unix))]
    {
        let _ = (home, daemon_owned);
        Arc::new(|| Ok(()))
    }
}
