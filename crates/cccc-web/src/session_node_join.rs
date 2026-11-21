//! Bounded, ephemeral enrollment state. Only a native Hello can create a grant.
mod limit;
mod views;
use crate::api::ApiError;
use cccc_contracts::session_node_join::*;
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

struct Entry {
    request: PendingRequest,
    credential_hash: String,
    generation: String,
    deadline: Instant,
    status: JoinStatus,
    ticket: Option<String>,
}

#[derive(Default)]
pub(crate) struct Queue {
    entries: BTreeMap<String, Entry>,
    starts: limit::Limit,
    polls: limit::Limit,
}

fn authority_unavailable(_: std::io::Error) -> ApiError {
    ApiError::unavailable(
        "session_node_unavailable",
        "Device enrollment authority is unavailable",
    )
}
fn invalid() -> ApiError {
    ApiError::bad("Invalid device enrollment request")
}
fn denied() -> ApiError {
    ApiError::forbidden("Device enrollment credential is unavailable")
}
fn hash(code: &str) -> Result<String, ApiError> {
    if code.len() != 64 || !code.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    Ok(format!("{:x}", Sha256::digest(code.as_bytes())))
}

impl Queue {
    fn purge(&mut self, now: Instant) {
        self.entries.retain(|_, e| e.deadline > now);
    }

    pub fn create(
        &mut self,
        input: CreateRequest,
        home: &cccc_core::HomeLayout,
    ) -> Result<CreatedRequest, ApiError> {
        let now = Instant::now();
        self.purge(now);
        let credential_hash = hash(&input.device_code)?;
        let node = input
            .node_id
            .parse::<iroh::EndpointId>()
            .map_err(|_| invalid())?;
        if node.to_string() != input.node_id
            || input.name.trim().is_empty()
            || input.name.len() > 160
            || input.name.chars().any(char::is_control)
        {
            return Err(invalid());
        }
        if let Some(entry) = self
            .entries
            .values()
            .find(|e| e.credential_hash == credential_hash)
        {
            if entry.request.node_id != input.node_id {
                return Err(denied());
            }
            return Ok(created(&entry.request));
        }
        if !self.starts.allow(now, 16, Duration::from_secs(60)) {
            return Err(ApiError::rate_limited(
                "session_node_join_rate_limited",
                "Too many device enrollment requests; retry later",
            ));
        }
        if self.entries.len() >= 64 {
            return Err(ApiError::unavailable(
                "session_node_join_full",
                "Device enrollment queue is full; retry later",
            ));
        }
        let user_code = loop {
            let value = uuid::Uuid::new_v4().simple().to_string()[..8].to_ascii_uppercase();
            if self.entries.values().all(|e| e.request.user_code != value) {
                break value;
            }
        };
        let request = PendingRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            node_id: input.node_id,
            name: input.name.trim().into(),
            user_code,
            expires_at: (Utc::now() + chrono::Duration::seconds(JOIN_TTL_SECONDS as i64))
                .to_rfc3339(),
        };
        let generation = cccc_core::session_nodes::capture_join_generation(
            home,
            &request.node_id,
            &request.expires_at,
        )
        .map_err(authority_unavailable)?;
        let response = created(&request);
        self.entries.insert(
            request.request_id.clone(),
            Entry {
                request,
                credential_hash,
                generation,
                deadline: now + Duration::from_secs(JOIN_TTL_SECONDS),
                status: JoinStatus::Pending,
                ticket: None,
            },
        );
        Ok(response)
    }

    pub fn poll(
        &mut self,
        input: PollRequest,
        home: &cccc_core::HomeLayout,
    ) -> Result<PollResponse, ApiError> {
        let now = Instant::now();
        self.purge(now);
        let credential_hash = hash(&input.device_code)?;
        if uuid::Uuid::parse_str(&input.request_id).is_err() {
            return Err(invalid());
        }
        if !self.polls.allow(now, 128, Duration::from_secs(1)) {
            return Err(ApiError::rate_limited(
                "session_node_join_rate_limited",
                "Device enrollment polling is too frequent",
            ));
        }
        self.refresh(&input.request_id, home)?;
        let Some(entry) = self.entries.get(&input.request_id) else {
            return Ok(PollResponse {
                status: JoinStatus::Expired,
                ticket: None,
            });
        };
        if entry.credential_hash != credential_hash {
            return Err(denied());
        }
        Ok(PollResponse {
            status: entry.status,
            ticket: entry.ticket.clone(),
        })
    }

    pub fn approve(
        &mut self,
        id: &str,
        ticket: String,
        home: &cccc_core::HomeLayout,
    ) -> Result<(), ApiError> {
        if self.approval(id, home)?.is_none() {
            return Ok(());
        }
        let entry = self
            .entries
            .get_mut(id)
            .ok_or_else(|| ApiError::not_found("Device enrollment request is unavailable"))?;
        entry.status = JoinStatus::Approved;
        entry.ticket = Some(ticket);
        Ok(())
    }

    pub fn deny(&mut self, id: &str, home: &cccc_core::HomeLayout) -> Result<(), ApiError> {
        let detail = self.detail(id, home)?;
        if !matches!(detail.status, JoinStatus::Pending | JoinStatus::Denied) {
            return Err(ApiError::conflict(
                "session_node_join_closed",
                "Approved devices must be revoked using device controls",
                serde_json::json!({}),
            ));
        }
        if let Some(entry) = self.entries.get_mut(id) {
            entry.status = JoinStatus::Denied;
            entry.ticket = None;
        }
        Ok(())
    }

    pub fn revoke(&mut self, node_id: &str) {
        for entry in self
            .entries
            .values_mut()
            .filter(|e| e.request.node_id == node_id)
        {
            entry.status = JoinStatus::Denied;
            entry.ticket = None;
        }
    }
}

fn created(request: &PendingRequest) -> CreatedRequest {
    CreatedRequest {
        request_id: request.request_id.clone(),
        user_code: request.user_code.clone(),
        expires_at: request.expires_at.clone(),
    }
}

#[cfg(test)]
mod tests;
