use super::AnalystLifecycleEvent;
use anyhow::{Result, bail};
use tokio::sync::broadcast;

pub(super) async fn for_settlement(
    events: &mut broadcast::Receiver<AnalystLifecycleEvent>,
    turn_id: &str,
) -> Result<()> {
    loop {
        match events.recv().await {
            Ok(AnalystLifecycleEvent::Completed {
                turn_id: completed, ..
            }) if completed == turn_id => return Ok(()),
            Ok(AnalystLifecycleEvent::Disconnected) => {
                bail!("managed Runtime disconnected while cancelling its active turn")
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => {
                bail!("managed Runtime lifecycle events were lost while cancelling")
            }
            Err(broadcast::error::RecvError::Closed) => {
                bail!("managed Runtime lifecycle closed while cancelling")
            }
        }
    }
}

pub(super) async fn until_ready(
    events: &mut broadcast::Receiver<AnalystLifecycleEvent>,
) -> Result<()> {
    loop {
        match events.recv().await {
            Ok(AnalystLifecycleEvent::Completed { .. }) => return Ok(()),
            Ok(AnalystLifecycleEvent::Disconnected) => {
                bail!("Voice Analyst disconnected while waiting for its active turn")
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => {
                bail!("Voice Analyst lifecycle events were lost while waiting")
            }
            Err(broadcast::error::RecvError::Closed) => {
                bail!("Voice Analyst lifecycle closed while waiting")
            }
        }
    }
}
