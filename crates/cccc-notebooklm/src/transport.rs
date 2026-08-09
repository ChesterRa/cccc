use reqwest::header::{ORIGIN, REFERER};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::{BASE_URL, BATCHEXECUTE_URL, Client, DEFAULT_BL, auth, cookies, rpc};

impl Client {
    pub(crate) fn rpc(&self, rpc_id: &str, params: Value, source_path: &str) -> Result<Value> {
        self.rpc_with_null(rpc_id, params, source_path, false)
    }

    pub(crate) fn rpc_allow_null(
        &self,
        rpc_id: &str,
        params: Value,
        source_path: &str,
    ) -> Result<Value> {
        self.rpc_with_null(rpc_id, params, source_path, true)
    }

    fn rpc_with_null(
        &self,
        rpc_id: &str,
        params: Value,
        source_path: &str,
        allow_null: bool,
    ) -> Result<Value> {
        let f_req = rpc::encode(rpc_id, params)?;
        let authuser = self.auth.authuser.to_string();
        let cookie_header = self.cookie_header()?;
        let response = auth::attach_cookie(self.http.post(BATCHEXECUTE_URL), &cookie_header)
            .header(ORIGIN, BASE_URL)
            .header(REFERER, format!("{BASE_URL}{source_path}"))
            .header("x-goog-authuser", &authuser)
            .query(&[
                ("rpcids", rpc_id),
                ("source-path", source_path),
                ("f.sid", self.auth.session_id.as_str()),
                ("bl", DEFAULT_BL),
                ("hl", "en"),
                ("_reqid", "1"),
                ("rt", "c"),
                ("authuser", authuser.as_str()),
            ])
            .form(&[("f.req", f_req), ("at", self.auth.csrf_token.clone())])
            .send()?;
        self.capture_cookies(&response)?;
        if is_auth_redirect(&response) {
            return Err(Error::Authentication);
        }
        let status = response.status();
        if matches!(status.as_u16(), 401 | 403) {
            return Err(Error::Authentication);
        }
        if status.as_u16() == 429 {
            return Err(Error::RateLimited("HTTP 429".into()));
        }
        let raw = response.error_for_status()?.text()?;
        rpc::decode(&raw, rpc_id, allow_null)
    }

    pub(crate) fn cookie_header(&self) -> Result<String> {
        let storage = self.storage_state()?;
        let raw = serde_json::to_string(&storage)
            .map_err(|error| Error::InvalidCredential(error.to_string()))?;
        auth::parse_storage(&raw).map(|(_, header, _)| header)
    }

    pub(crate) fn capture_cookies(&self, response: &reqwest::blocking::Response) -> Result<()> {
        let mut storage = self
            .storage_state
            .lock()
            .map_err(|_| Error::InvalidCredential("credential lock is poisoned".into()))?;
        cookies::merge_response(&mut storage, response)
    }
}

pub(crate) fn is_auth_redirect(response: &reqwest::blocking::Response) -> bool {
    response
        .url()
        .host_str()
        .is_some_and(|host| host == "accounts.google.com" || host.ends_with(".accounts.google.com"))
}
