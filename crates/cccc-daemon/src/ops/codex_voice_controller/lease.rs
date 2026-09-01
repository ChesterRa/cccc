use anyhow::{Result, anyhow, bail};
use cccc_core::{HomeLayout, voice_recording_lease};
use serde_json::json;
use std::sync::Mutex;

const CALL_LEASE_TTL_SECONDS: i64 = 30;

pub(super) struct CallLease {
    home: HomeLayout,
    group_id: String,
    group_title: String,
    owner_id: String,
    lease_id: Mutex<Option<String>>,
}

impl CallLease {
    pub(super) fn acquire(
        home: &HomeLayout,
        group_id: &str,
        group_title: &str,
        owner_id: &str,
    ) -> Result<Self> {
        let acquired = voice_recording_lease::update(
            home,
            group_id,
            group_title,
            &json!({
                "action":"acquire",
                "owner_id":owner_id,
                "ttl_seconds":CALL_LEASE_TTL_SECONDS,
                "capture_mode":"codex_voice",
                "recognition_backend":"codex_realtime",
                "dispatch_target":"codex_voice",
                "by":"user",
            }),
        )
        .map_err(|error| anyhow!(error))?;
        let lease_id = acquired["lease_id"]
            .as_str()
            .filter(|value| !value.is_empty())
            .ok_or_else(|| anyhow!("recording lease acquisition returned no lease id"))?
            .to_owned();
        Ok(Self {
            home: home.clone(),
            group_id: group_id.to_owned(),
            group_title: group_title.to_owned(),
            owner_id: owner_id.to_owned(),
            lease_id: Mutex::new(Some(lease_id)),
        })
    }

    pub(super) fn heartbeat(&self) -> Result<()> {
        let lease_id = self.current_id()?;
        if voice_recording_lease::renew(
            &self.home,
            &self.group_id,
            &self.group_title,
            &self.owner_id,
            &lease_id,
        )
        .map_err(|error| anyhow!(error))?
        {
            Ok(())
        } else {
            bail!("Codex Voice microphone lease was lost")
        }
    }

    pub(super) fn release(&self) -> Result<()> {
        let lease_id = self
            .lease_id
            .lock()
            .map_err(|_| anyhow!("Codex Voice lease lock poisoned"))?
            .take();
        let Some(lease_id) = lease_id else {
            return Ok(());
        };
        voice_recording_lease::release(&self.home, &self.group_id, &self.owner_id, &lease_id)
            .map_err(|error| anyhow!(error))?;
        Ok(())
    }

    fn current_id(&self) -> Result<String> {
        self.lease_id
            .lock()
            .map_err(|_| anyhow!("Codex Voice lease lock poisoned"))?
            .clone()
            .ok_or_else(|| anyhow!("Codex Voice microphone lease is no longer active"))
    }
}

impl Drop for CallLease {
    fn drop(&mut self) {
        let _ = self.release();
    }
}
