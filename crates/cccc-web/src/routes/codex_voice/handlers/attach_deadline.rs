use crate::AppState;
use crate::codex_voice::SessionInfo;
use std::time::Duration;

pub(super) fn spawn(state: AppState, info: SessionInfo) {
    tokio::spawn(async move {
        let deadline = tokio::time::sleep(Duration::from_secs(30));
        tokio::pin!(deadline);
        let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                _ = heartbeat.tick() => match state
                    .codex_voice
                    .heartbeat_if_unattached(&info.generation)
                    .await
                {
                    Ok(true) => {}
                    Ok(false) => return,
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            generation = %info.generation,
                            "failed to renew unattached Codex Voice recording lease"
                        );
                        break;
                    }
                },
            }
        }
        match state.codex_voice.stop_if_unattached(&info.generation).await {
            Ok(true) => tracing::info!(
                generation = %info.generation,
                "stopped unattached Codex Voice call"
            ),
            Ok(false) => {}
            Err(error) => tracing::warn!(
                %error,
                generation = %info.generation,
                "failed to stop unattached Codex Voice call"
            ),
        }
    });
}
