use cccc_core::{HomeLayout, voice_recording_lease};

pub(super) struct LeaseGuard {
    pub home: HomeLayout,
    pub group: String,
    pub owner: String,
    pub lease_id: String,
}
impl Drop for LeaseGuard {
    fn drop(&mut self) {
        if let Err(error) =
            voice_recording_lease::release(&self.home, &self.group, &self.owner, &self.lease_id)
        {
            tracing::warn!(code = error.code, "external ASR lease release failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn cancellation_releases_only_the_owned_lease() {
        let temp = tempfile::tempdir().expect("tempdir");
        let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
        let lease = voice_recording_lease::update(
            &home,
            "g",
            "Group",
            &json!({"action":"acquire","owner_id":"browser"}),
        )
        .expect("lease");
        let guard = LeaseGuard {
            home: home.clone(),
            group: "g".into(),
            owner: "browser".into(),
            lease_id: lease["lease_id"].as_str().expect("lease id").into(),
        };
        let (started, ready) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _guard = guard;
            let _ = started.send(());
            std::future::pending::<()>().await;
        });
        ready.await.expect("guard active");
        task.abort();
        let _ = task.await;
        assert_eq!(
            voice_recording_lease::current(&home).expect("state"),
            json!({})
        );

        let old = voice_recording_lease::update(
            &home,
            "g",
            "Group",
            &json!({"action":"acquire","owner_id":"browser"}),
        )
        .expect("old");
        let guard = LeaseGuard {
            home: home.clone(),
            group: "g".into(),
            owner: "browser".into(),
            lease_id: old["lease_id"].as_str().expect("old id").into(),
        };
        let new = voice_recording_lease::update(
            &home,
            "g",
            "Group",
            &json!({"action":"acquire","owner_id":"browser"}),
        )
        .expect("new");
        drop(guard);
        assert!(
            voice_recording_lease::validate(
                &home,
                "g",
                "browser",
                new["lease_id"].as_str().expect("new id")
            )
            .is_ok()
        );
        assert!(
            voice_recording_lease::validate(
                &home,
                "g",
                "browser",
                old["lease_id"].as_str().expect("old id")
            )
            .is_err()
        );
    }
}
