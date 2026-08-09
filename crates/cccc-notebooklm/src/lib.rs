mod artifacts;
mod auth;
mod chat;
mod cookies;
mod error;
mod models;
mod rpc;
mod transport;

pub use artifacts::{Artifact, ArtifactGeneration};
pub use error::{Error, Result};
pub use models::{Notebook, QueryResult, Reference, Source};

use std::sync::Mutex;
use std::time::Duration;

use reqwest::blocking::Client as HttpClient;
use reqwest::header::{ORIGIN, REFERER};
use serde_json::{Value, json};

use auth::AuthState;

pub(crate) const BASE_URL: &str = "https://notebooklm.google.com";
pub(crate) const BATCHEXECUTE_URL: &str =
    "https://notebooklm.google.com/_/LabsTailwindUi/data/batchexecute";
const QUERY_URL: &str = "https://notebooklm.google.com/_/LabsTailwindUi/data/google.internal.labs.tailwind.orchestration.v1.LabsTailwindOrchestrationService/GenerateFreeFormStreamed";
pub(crate) const DEFAULT_BL: &str = "boq_labs-tailwind-frontend_20260301.03_p0";

pub struct Client {
    http: HttpClient,
    auth: AuthState,
    storage_state: Mutex<Value>,
}

impl Client {
    pub fn from_storage_state(raw: &str) -> Result<Self> {
        let (mut storage_state, cookie_header, authuser) = auth::parse_storage(raw)?;
        let http = HttpClient::builder()
            .timeout(Duration::from_secs(180))
            .connect_timeout(Duration::from_secs(15))
            .user_agent("Mozilla/5.0 CCCC NotebookLM Rust adapter")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;
        let response = http
            .get(format!("{BASE_URL}/"))
            .header(reqwest::header::COOKIE, &cookie_header)
            .send()?;
        cookies::merge_response(&mut storage_state, &response)?;
        if response.status() == reqwest::StatusCode::UNAUTHORIZED
            || response.status() == reqwest::StatusCode::FORBIDDEN
            || response
                .url()
                .host_str()
                .is_some_and(|host| host == "accounts.google.com")
        {
            return Err(Error::Authentication);
        }
        let html = response.error_for_status()?.text()?;
        let (csrf_token, session_id) = auth::extract_tokens(&html)?;
        Ok(Self {
            http,
            auth: AuthState {
                csrf_token,
                session_id,
                authuser,
            },
            storage_state: Mutex::new(storage_state),
        })
    }

    pub fn storage_state(&self) -> Result<Value> {
        self.storage_state
            .lock()
            .map(|value| value.clone())
            .map_err(|_| Error::InvalidCredential("credential lock is poisoned".into()))
    }

    pub fn health_check(&self) -> Result<()> {
        self.list_notebooks().map(|_| ())
    }

    pub fn list_notebooks(&self) -> Result<Vec<Notebook>> {
        let result = self.rpc(rpc::LIST_NOTEBOOKS, json!([null, 1, null, [2]]), "/")?;
        let Some(rows) = result
            .as_array()
            .and_then(|value| value.first())
            .and_then(Value::as_array)
        else {
            if result.is_null() || result.as_array().is_some_and(Vec::is_empty) {
                return Ok(Vec::new());
            }
            return Err(Error::drift("notebook list", "expected [[notebook rows]]"));
        };
        rows.iter().map(Notebook::parse).collect()
    }

    pub fn create_notebook(&self, title: &str) -> Result<Notebook> {
        let result = self.rpc(
            rpc::CREATE_NOTEBOOK,
            json!([title, null, null, template_block()]),
            "/",
        )?;
        Notebook::parse(&result)
    }

    pub fn list_sources(&self, notebook_id: &str) -> Result<Vec<Source>> {
        let result = self.get_notebook_raw(notebook_id)?;
        let notebook = result
            .as_array()
            .and_then(|value| value.first())
            .and_then(Value::as_array)
            .ok_or_else(|| Error::drift("notebook detail", "expected [notebook row]"))?;
        match notebook.get(1) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(rows)) => rows.iter().map(Source::parse_entry).collect(),
            _ => Err(Error::drift(
                "notebook sources",
                "source slot was not an array",
            )),
        }
    }

    pub fn add_text_source(&self, notebook_id: &str, title: &str, content: &str) -> Result<Source> {
        let params = json!([
            [[
                null,
                [title, content],
                null,
                2,
                null,
                null,
                null,
                null,
                null,
                null,
                1
            ]],
            notebook_id,
            template_block()
        ]);
        Source::parse_unknown(&self.rpc(
            rpc::ADD_SOURCE,
            params,
            &format!("/notebook/{notebook_id}"),
        )?)
    }

    pub fn delete_source(&self, notebook_id: &str, source_id: &str) -> Result<()> {
        self.rpc_allow_null(
            rpc::DELETE_SOURCE,
            json!([[[source_id]]]),
            &format!("/notebook/{notebook_id}"),
        )?;
        Ok(())
    }

    pub fn refresh_source(&self, notebook_id: &str, source_id: &str) -> Result<()> {
        self.rpc_allow_null(
            rpc::REFRESH_SOURCE,
            refresh_source_params(source_id),
            &format!("/notebook/{notebook_id}"),
        )?;
        Ok(())
    }

    pub fn rename_source(&self, notebook_id: &str, source_id: &str, title: &str) -> Result<()> {
        self.rpc_allow_null(
            rpc::UPDATE_SOURCE,
            json!([null, [source_id], [[[title]]]]),
            &format!("/notebook/{notebook_id}"),
        )?;
        Ok(())
    }

    pub fn query(&self, notebook_id: &str, question: &str) -> Result<QueryResult> {
        self.query_scoped(notebook_id, question, None)
    }

    pub fn query_scoped(
        &self,
        notebook_id: &str,
        question: &str,
        requested_source_ids: Option<&[String]>,
    ) -> Result<QueryResult> {
        let source_ids = match requested_source_ids {
            Some(source_ids) => source_ids.to_vec(),
            None => self
                .list_sources(notebook_id)?
                .into_iter()
                .map(|source| source.id)
                .collect(),
        };
        let source_ids = source_ids
            .into_iter()
            .map(|source_id| json!([[source_id]]))
            .collect::<Vec<_>>();
        let params = json!([
            source_ids,
            question,
            null,
            [2, null, [1], [1]],
            null,
            null,
            null,
            notebook_id,
            1
        ]);
        let f_req = serde_json::to_string(&json!([
            null,
            serde_json::to_string(&params)
                .map_err(|error| Error::drift("chat request", error.to_string()))?
        ]))
        .map_err(|error| Error::drift("chat request", error.to_string()))?;
        let cookie_header = self.cookie_header()?;
        let response = auth::attach_cookie(self.http.post(QUERY_URL), &cookie_header)
            .header(ORIGIN, BASE_URL)
            .header(REFERER, format!("{BASE_URL}/notebook/{notebook_id}"))
            .header("x-goog-authuser", self.auth.authuser.to_string())
            .query(&[
                ("bl", DEFAULT_BL),
                ("hl", "en"),
                ("_reqid", "1"),
                ("rt", "c"),
                ("f.sid", self.auth.session_id.as_str()),
                ("authuser", &self.auth.authuser.to_string()),
            ])
            .form(&[("f.req", f_req), ("at", self.auth.csrf_token.clone())])
            .send()?;
        self.capture_cookies(&response)?;
        if transport::is_auth_redirect(&response) {
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
        chat::decode(&raw)
    }

    fn get_notebook_raw(&self, notebook_id: &str) -> Result<Value> {
        self.rpc(
            rpc::GET_NOTEBOOK,
            json!([notebook_id, null, template_block(), null, 0]),
            &format!("/notebook/{notebook_id}"),
        )
    }
}

fn template_block() -> Value {
    json!([
        2,
        null,
        null,
        [1, null, null, null, null, null, null, null, null, null, [1]]
    ])
}

fn refresh_source_params(source_id: &str) -> Value {
    json!([null, [source_id], [2]])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_source_uses_v080_wire_contract() {
        assert_eq!(
            refresh_source_params("source-1"),
            json!([null, ["source-1"], [2]])
        );
        assert_eq!(rpc::REFRESH_SOURCE, "FLmJqe");
    }
}
