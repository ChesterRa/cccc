//! A Hello candidate replaces reconnect state only if the observed state is unchanged.
use anyhow::{Result, ensure};
use cccc_core::{
    HomeLayout, fs,
    session_nodes::{self, ClientDescriptor},
};

#[cfg(test)]
pub(super) type WriteFault = Box<dyn FnOnce() -> Box<dyn Send> + Send>;

#[cfg(test)]
tokio::task_local! {
    pub(super) static BEFORE_WRITE: std::cell::RefCell<Option<WriteFault>>;
}

pub(super) fn commit(
    home: &HomeLayout,
    expected: Option<&ClientDescriptor>,
    candidate: &ClientDescriptor,
) -> Result<()> {
    let expected = serde_json::to_value(expected)?;
    let candidate = serde_json::to_value(candidate)?;
    let directory = home.root().join("state/session_nodes");
    // Share the lock used by set_client/update_memberships/clear_client. Calling
    // those APIs inside this closure would reacquire the same lock.
    fs::with_exclusive_lock(&directory.join("state.lock"), || {
        let current = serde_json::to_value(session_nodes::saved_client(home)?)?;
        if current != expected {
            return Err(std::io::Error::other(
                "Reconnect state changed during pairing; retry with the current configuration",
            ));
        }
        // atomic_write creates a private temporary file and renames it. Readback
        // resolves a post-rename sync error without rolling back a committed Hello.
        #[cfg(test)]
        let _fault = BEFORE_WRITE
            .try_with(|hook| hook.borrow_mut().take().map(|hook| hook()))
            .ok();
        fs::write_json_committed(&directory.join("client.json"), &candidate)
    })?;
    Ok(())
}

pub(super) fn reconnect_state(home: &HomeLayout, secret: &str) -> Result<Option<ClientDescriptor>> {
    let current = session_nodes::saved_client(home)?;
    ensure!(
        current
            .as_ref()
            .is_some_and(|current| current.secret == secret),
        "Reconnect state changed while connected; preserving the current configuration"
    );
    Ok(current)
}
