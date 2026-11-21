use super::{Queue, authority_unavailable};
use crate::api::ApiError;
use cccc_contracts::session_node_join::{JoinStatus, PendingRequest, RequestDetails};
use std::time::Instant;

impl Queue {
    pub fn pending(
        &mut self,
        home: &cccc_core::HomeLayout,
    ) -> Result<Vec<PendingRequest>, ApiError> {
        self.purge(Instant::now());
        for id in self.entries.keys().cloned().collect::<Vec<_>>() {
            self.refresh(&id, home)?;
        }
        Ok(self
            .entries
            .values()
            .filter(|e| e.status == JoinStatus::Pending)
            .map(|e| e.request.clone())
            .collect())
    }

    pub fn detail(
        &mut self,
        id: &str,
        home: &cccc_core::HomeLayout,
    ) -> Result<RequestDetails, ApiError> {
        self.purge(Instant::now());
        self.refresh(id, home)?;
        let e = self
            .entries
            .get(id)
            .ok_or_else(|| ApiError::not_found("Device enrollment request is unavailable"))?;
        Ok(RequestDetails {
            request: e.request.clone(),
            status: e.status,
        })
    }

    pub fn approval(
        &mut self,
        id: &str,
        home: &cccc_core::HomeLayout,
    ) -> Result<Option<PendingRequest>, ApiError> {
        let detail = self.detail(id, home)?;
        match detail.status {
            JoinStatus::Pending => Ok(Some(detail.request)),
            JoinStatus::Approved => Ok(None),
            _ => Err(ApiError::conflict(
                "session_node_join_closed",
                "Device enrollment request is closed",
                serde_json::json!({}),
            )),
        }
    }

    pub fn generation(&self, id: &str) -> Result<String, ApiError> {
        self.entries
            .get(id)
            .map(|entry| entry.generation.clone())
            .ok_or_else(|| ApiError::not_found("Device enrollment request is unavailable"))
    }

    pub(super) fn refresh(
        &mut self,
        id: &str,
        home: &cccc_core::HomeLayout,
    ) -> Result<(), ApiError> {
        if let Some(entry) = self.entries.get_mut(id)
            && entry.status != JoinStatus::Denied
            && !cccc_core::session_nodes::join_generation_matches(
                home,
                &entry.request.node_id,
                &entry.generation,
            )
            .map_err(authority_unavailable)?
        {
            entry.status = JoinStatus::Denied;
            entry.ticket = None;
        }
        Ok(())
    }
}
