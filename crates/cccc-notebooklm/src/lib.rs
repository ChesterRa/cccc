mod artifacts;
mod auth;
mod chat;
mod cookies;
mod error;
mod models;
mod rpc;
mod transport;
mod uploads;

pub use artifacts::{Artifact, ArtifactGeneration};
pub use error::{Error, Result};
pub use models::{Notebook, QueryResult, Reference, Source};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlSourceKind {
    WebPage,
    YouTube,
}

use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;

use reqwest::blocking::Client as HttpClient;
use reqwest::header::{COOKIE, LOCATION, ORIGIN, REFERER};
use serde_json::{Value, json};

use auth::AuthState;

// Wire baseline: notebooklm-py v0.8.1 (tag commit 01c419a0474e0191b88e94c572d605b4899a9c2b).
pub(crate) const BASE_URL: &str = "https://notebook.google.com";
const BASE_HOST: &str = "notebook.google.com";
const LEGACY_BASE_HOST: &str = "notebooklm.google.com";
const ACCOUNTS_HOST: &str = "accounts.google.com";
const MAX_AUTH_REDIRECTS: usize = 8;
pub(crate) const BATCHEXECUTE_URL: &str =
    "https://notebook.google.com/_/LabsTailwindUi/data/batchexecute";
const QUERY_URL: &str = "https://notebook.google.com/_/LabsTailwindUi/data/google.internal.labs.tailwind.orchestration.v1.LabsTailwindOrchestrationService/GenerateFreeFormStreamed";
pub(crate) const DEFAULT_BL: &str = "boq_labs-tailwind-frontend_20260802.02_p0";

pub struct Client {
    http: HttpClient,
    auth: AuthState,
    storage_state: Mutex<Value>,
}

impl Client {
    pub fn from_storage_state(raw: &str) -> Result<Self> {
        let (mut storage_state, _, authuser) = auth::parse_storage(raw, BASE_HOST, "/")?;
        let auth_http = HttpClient::builder()
            .timeout(Duration::from_secs(180))
            .connect_timeout(Duration::from_secs(15))
            .user_agent("Mozilla/5.0 CCCC NotebookLM Rust adapter")
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let response = fetch_auth_page(&auth_http, &mut storage_state, authuser)?;
        let http = HttpClient::builder()
            .timeout(Duration::from_secs(180))
            .connect_timeout(Duration::from_secs(15))
            .user_agent("Mozilla/5.0 CCCC NotebookLM Rust adapter")
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;
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
        match self.rpc(rpc::ADD_SOURCE, params, &format!("/notebook/{notebook_id}")) {
            Ok(value) => Source::parse_unknown(&value),
            Err(error @ (Error::Authentication | Error::InvalidCredential(_)))
            | Err(error @ Error::RateLimited(_))
            | Err(error @ Error::Refused(_)) => Err(error),
            Err(error) => Err(Error::Unresolved(format!(
                "text source create may have committed but cannot be identified safely: {error}; inspect the NotebookLM source list before retrying"
            ))),
        }
    }

    pub fn add_url_source(
        &self,
        notebook_id: &str,
        url: &str,
        title: Option<&str>,
    ) -> Result<Source> {
        let normalized_url = validated_source_url(url)?;
        let params = url_source_params(notebook_id, url)?;
        let source = self.add_source_with_recovery(
            notebook_id,
            |source| {
                source
                    .url
                    .as_deref()
                    .and_then(|value| validated_source_url(value).ok())
                    .is_some_and(|value| value == normalized_url)
            },
            || self.rpc(rpc::ADD_SOURCE, params, &format!("/notebook/{notebook_id}")),
        )?;
        Ok(self.honor_requested_title(notebook_id, source, title))
    }

    pub fn add_drive_source(
        &self,
        notebook_id: &str,
        file_id: &str,
        title: &str,
        mime_type: &str,
    ) -> Result<Source> {
        let file_id = file_id.trim();
        if file_id.is_empty() {
            return Err(Error::Refused("Drive file_id must not be empty".into()));
        }
        let title = title.trim();
        if title.is_empty() {
            return Err(Error::Refused(
                "Drive source title must not be empty".into(),
            ));
        }
        let mime_type = mime_type.trim();
        if mime_type.is_empty() {
            return Err(Error::Refused(
                "Drive source MIME type must not be empty".into(),
            ));
        }
        let params = drive_source_params(notebook_id, file_id, title, mime_type);
        let source = self.add_source_with_recovery(
            notebook_id,
            |source| source.drive_document_id.as_deref() == Some(file_id),
            || self.rpc(rpc::ADD_SOURCE, params, &format!("/notebook/{notebook_id}")),
        )?;
        Ok(self.honor_requested_title(notebook_id, source, Some(title)))
    }

    pub fn add_file_source(
        &self,
        notebook_id: &str,
        file_path: &Path,
        title: Option<&str>,
    ) -> Result<Source> {
        uploads::add_file(self, notebook_id, file_path, title)
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
        let cookie_header = self
            .cookie_header_for(
                BASE_HOST,
                "/_/LabsTailwindUi/data/google.internal.labs.tailwind.orchestration.v1.LabsTailwindOrchestrationService/GenerateFreeFormStreamed",
            )?
            .ok_or_else(|| Error::InvalidCredential("NotebookLM API cookies are missing".into()))?;
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

    fn add_source_with_recovery(
        &self,
        notebook_id: &str,
        matches: impl Fn(&Source) -> bool,
        create: impl FnOnce() -> Result<Value>,
    ) -> Result<Source> {
        let baseline = self
            .list_sources(notebook_id)
            .ok()
            .map(|sources| sources.into_iter().map(|source| source.id).collect());
        let created = create();
        if let Ok(value) = &created
            && let Ok(source) = Source::parse_unknown(value)
            && baseline
                .as_ref()
                .is_none_or(|ids: &std::collections::HashSet<String>| !ids.contains(&source.id))
        {
            return Ok(source);
        }
        if let Err(error) = &created
            && !matches!(
                error,
                Error::Transport(_)
                    | Error::Rpc { .. }
                    | Error::SchemaDrift { .. }
                    | Error::Timeout(_)
                    | Error::Unresolved(_)
            )
        {
            return created.and_then(|value| Source::parse_unknown(&value));
        }
        match self.recover_created_source(notebook_id, baseline.as_ref(), matches) {
            Ok(Some(source)) => Ok(source),
            Ok(None) => created.and_then(|value| Source::parse_unknown(&value)),
            Err(error) => Err(error),
        }
    }

    fn recover_created_source(
        &self,
        notebook_id: &str,
        baseline: Option<&std::collections::HashSet<String>>,
        matches: impl Fn(&Source) -> bool,
    ) -> Result<Option<Source>> {
        let Some(baseline) = baseline else {
            return Err(Error::Unresolved(
                "source create may have committed, but the pre-create baseline was unavailable; inspect the NotebookLM source list before retrying".into(),
            ));
        };
        let candidates = self
            .list_sources(notebook_id)
            .map_err(|error| {
                Error::Unresolved(format!(
                    "source create may have committed, but the recovery probe failed: {error}; inspect the NotebookLM source list before retrying"
                ))
            })?
            .into_iter()
            .filter(|source| !baseline.contains(&source.id) && matches(source))
            .collect::<Vec<_>>();
        match candidates.as_slice() {
            [] => Ok(None),
            [source] => Ok(Some(source.clone())),
            _ => Err(Error::Unresolved(format!(
                "source create may have committed ambiguously: recovery found {} new matching sources; inspect the NotebookLM source list before retrying",
                candidates.len()
            ))),
        }
    }

    fn honor_requested_title(
        &self,
        notebook_id: &str,
        mut source: Source,
        requested_title: Option<&str>,
    ) -> Source {
        let Some(requested_title) = requested_title
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            return source;
        };
        if source.title.as_deref() != Some(requested_title)
            && self
                .rename_source(notebook_id, &source.id, requested_title)
                .is_ok()
        {
            source.title = Some(requested_title.into());
        }
        source
    }
}

pub fn classify_url_source(raw: &str) -> Result<UrlSourceKind> {
    let url = validated_source_url(raw)?;
    Ok(if is_youtube_url(&url) {
        UrlSourceKind::YouTube
    } else {
        UrlSourceKind::WebPage
    })
}

fn fetch_auth_page(
    http: &HttpClient,
    storage_state: &mut Value,
    authuser: usize,
) -> Result<reqwest::blocking::Response> {
    let mut url = reqwest::Url::parse(&format!("{BASE_URL}/"))
        .map_err(|error| Error::InvalidCredential(error.to_string()))?;
    if authuser != 0 {
        url.query_pairs_mut()
            .append_pair("authuser", &authuser.to_string());
    }
    for _ in 0..=MAX_AUTH_REDIRECTS {
        let host = url.host_str().ok_or(Error::Authentication)?;
        if !allowed_auth_url(&url) {
            return Err(Error::Authentication);
        }
        let raw = serde_json::to_string(storage_state)
            .map_err(|error| Error::InvalidCredential(error.to_string()))?;
        let (_, cookie_header, _) = auth::parse_storage(&raw, host, url.path())?;
        let response = http.get(url.clone()).header(COOKIE, cookie_header).send()?;
        cookies::merge_response(storage_state, &response)?;
        if !response.status().is_redirection() {
            return Ok(response);
        }
        let location = response
            .headers()
            .get(LOCATION)
            .and_then(|value| value.to_str().ok())
            .ok_or(Error::Authentication)?;
        url = response
            .url()
            .join(location)
            .map_err(|_| Error::Authentication)?;
    }
    Err(Error::Authentication)
}

fn allowed_auth_url(url: &reqwest::Url) -> bool {
    url.scheme() == "https"
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && matches!(
            url.host_str(),
            Some(BASE_HOST | LEGACY_BASE_HOST | ACCOUNTS_HOST)
        )
}

fn validated_source_url(raw: &str) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw.trim())
        .map_err(|error| Error::Refused(format!("invalid NotebookLM source URL: {error}")))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(Error::Refused(
            "NotebookLM source URL must be an absolute HTTP(S) URL without credentials".into(),
        ));
    }
    Ok(url)
}

fn is_youtube_url(url: &reqwest::Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let youtube_host =
        host == "youtu.be" || host == "youtube.com" || host.ends_with(".youtube.com");
    if !youtube_host {
        return false;
    }
    let candidate = if host == "youtu.be" {
        url.path_segments()
            .and_then(|mut parts| parts.next().map(str::to_owned))
    } else {
        let segments = url.path_segments().map(|parts| parts.collect::<Vec<_>>());
        segments.as_ref().and_then(|parts| match parts.as_slice() {
            [kind, id, ..] if matches!(*kind, "shorts" | "embed" | "live" | "v") => {
                Some((*id).to_owned())
            }
            _ => url
                .query_pairs()
                .find_map(|(key, value)| (key == "v").then(|| value.into_owned())),
        })
    };
    candidate.as_deref().is_some_and(|value| {
        !value.is_empty()
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    })
}

fn template_block() -> Value {
    json!([
        2,
        null,
        null,
        [1, null, null, null, null, null, null, null, null, null, [1]]
    ])
}

fn url_source_params(notebook_id: &str, raw: &str) -> Result<Value> {
    let raw = raw.trim();
    let url = validated_source_url(raw)?;
    let source = if is_youtube_url(&url) {
        json!([
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            [raw],
            null,
            null,
            1
        ])
    } else {
        json!([
            null,
            null,
            [raw],
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            1
        ])
    };
    Ok(json!([[source], notebook_id, template_block()]))
}

fn drive_source_params(notebook_id: &str, file_id: &str, title: &str, mime_type: &str) -> Value {
    let source = json!([
        [file_id, mime_type, 1, title],
        null,
        null,
        null,
        null,
        null,
        null,
        null,
        null,
        null,
        1
    ]);
    json!([
        [source],
        notebook_id,
        [2],
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

    #[test]
    fn auth_redirects_are_limited_to_exact_google_hosts() {
        for raw in [
            "https://notebooklm.google.com/",
            "https://notebook.google.com/",
            "https://accounts.google.com/",
        ] {
            assert!(allowed_auth_url(&reqwest::Url::parse(raw).expect("url")));
        }
        for raw in [
            "http://notebook.google.com/",
            "https://notebook.google.com:444/",
            "https://accounts.google.com.evil.example/",
            "https://user@accounts.google.com/",
        ] {
            assert!(!allowed_auth_url(&reqwest::Url::parse(raw).expect("url")));
        }
    }

    #[test]
    fn v081_defaults_use_the_current_personal_app_host() {
        assert_eq!(BASE_URL, "https://notebook.google.com");
        assert_eq!(BASE_HOST, "notebook.google.com");
        assert_eq!(LEGACY_BASE_HOST, "notebooklm.google.com");
        assert!(BATCHEXECUTE_URL.starts_with(BASE_URL));
        assert!(QUERY_URL.starts_with(BASE_URL));
        assert_eq!(DEFAULT_BL, "boq_labs-tailwind-frontend_20260802.02_p0");
    }

    #[test]
    fn v081_url_and_drive_source_payloads_match_the_upstream_wire() {
        let web_params =
            url_source_params("notebook-1", " https://example.test ").expect("web params");
        assert_eq!(web_params[0][0][2], json!(["https://example.test"]));
        assert_eq!(web_params[0][0][10], 1);
        assert_eq!(web_params[2], template_block());

        let youtube_params = url_source_params("notebook-1", "https://youtu.be/abc_123-def")
            .expect("YouTube params");
        assert_eq!(
            youtube_params[0][0][7],
            json!(["https://youtu.be/abc_123-def"])
        );
        assert_eq!(youtube_params[0][0][10], 1);

        assert_eq!(
            drive_source_params(
                "notebook-1",
                "drive-1",
                "Design",
                "application/vnd.google-apps.document"
            ),
            json!([
                [[
                    [
                        "drive-1",
                        "application/vnd.google-apps.document",
                        1,
                        "Design"
                    ],
                    null,
                    null,
                    null,
                    null,
                    null,
                    null,
                    null,
                    null,
                    null,
                    1
                ]],
                "notebook-1",
                [2],
                [1, null, null, null, null, null, null, null, null, null, [1]]
            ])
        );
    }

    #[test]
    fn source_urls_reject_non_http_and_embedded_credentials() {
        for raw in [
            "file:///tmp/secret",
            "https://user:secret@example.test/",
            "not a URL",
        ] {
            assert!(validated_source_url(raw).is_err(), "{raw}");
        }
        let malformed_video =
            validated_source_url("https://youtube.com/watch?v=bad%20id").expect("absolute URL");
        assert!(!is_youtube_url(&malformed_video));
        assert_eq!(
            classify_url_source("https://www.youtube.com/watch?v=abc_123-def")
                .expect("YouTube URL"),
            UrlSourceKind::YouTube
        );
        assert_eq!(
            classify_url_source("https://example.test/article").expect("Web URL"),
            UrlSourceKind::WebPage
        );
    }
}
