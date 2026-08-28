use reqwest::Url;
use reqwest::blocking::Client as HttpClient;
use reqwest::redirect::Policy;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

use crate::{Client, Error, Result, rpc};

const DOWNLOAD_REDIRECT_LIMIT: usize = 5;
const TRUSTED_DOWNLOAD_DOMAINS: &[&str] =
    &["google.com", "googleusercontent.com", "googleapis.com"];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Artifact {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: String,
    pub variant: Option<i64>,
    pub download_url: Option<String>,
    pub content: Option<String>,
    pub raw: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ArtifactGeneration {
    pub artifact_id: String,
    pub kind: String,
    pub status: String,
    pub raw: Value,
}

impl Client {
    pub fn list_artifacts(&self, notebook_id: &str) -> Result<Vec<Artifact>> {
        let result = self.rpc_allow_null(
            rpc::LIST_ARTIFACTS,
            json!([
                [2],
                notebook_id,
                "NOT artifact.status = \"ARTIFACT_STATUS_SUGGESTED\""
            ]),
            &format!("/notebook/{notebook_id}"),
        )?;
        let rows = unwrap_rows(&result);
        Ok(rows.iter().filter_map(parse_artifact).collect())
    }

    pub fn generate_artifact(
        &self,
        notebook_id: &str,
        kind: &str,
        language: &str,
        instructions: Option<&str>,
        requested_source_ids: Option<&[String]>,
    ) -> Result<ArtifactGeneration> {
        let source_ids = match requested_source_ids.filter(|items| !items.is_empty()) {
            Some(items) => items.to_vec(),
            None => self
                .list_sources(notebook_id)?
                .into_iter()
                .map(|source| source.id)
                .collect::<Vec<_>>(),
        };
        if source_ids.is_empty() {
            return Err(Error::Refused(
                "artifact generation requires at least one source".into(),
            ));
        }
        let params = generation_params(notebook_id, &source_ids, kind, language, instructions)?;
        let result = self.rpc_allow_null(
            rpc::CREATE_ARTIFACT,
            params,
            &format!("/notebook/{notebook_id}"),
        )?;
        let row = result
            .get(0)
            .and_then(Value::as_array)
            .ok_or_else(|| Error::drift("artifact.generate", "missing artifact row"))?;
        let artifact_id = row
            .first()
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| Error::drift("artifact.generate", "missing artifact id"))?
            .to_owned();
        let status = artifact_status(row.get(4).and_then(Value::as_i64));
        Ok(ArtifactGeneration {
            artifact_id,
            kind: normalize_kind(kind)?.into(),
            status: status.into(),
            raw: result,
        })
    }

    pub fn wait_for_artifact(
        &self,
        notebook_id: &str,
        artifact_id: &str,
        timeout: Duration,
        initial_interval: Duration,
        max_interval: Duration,
    ) -> Result<Artifact> {
        let deadline = Instant::now() + timeout;
        let mut interval = initial_interval.min(max_interval);
        loop {
            if let Some(artifact) = self
                .list_artifacts(notebook_id)?
                .into_iter()
                .find(|artifact| artifact.id == artifact_id)
            {
                match artifact.status.as_str() {
                    "completed" if artifact_ready(&artifact) => return Ok(artifact),
                    "failed" => return Ok(artifact),
                    _ => {}
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(Error::Timeout(format!(
                    "artifact {artifact_id} did not complete within {} seconds",
                    timeout.as_secs_f64()
                )));
            }
            std::thread::sleep(interval.min(deadline.saturating_duration_since(now)));
            interval = interval.saturating_mul(2).min(max_interval);
        }
    }

    pub fn download_artifact(
        &self,
        artifact: &Artifact,
        output_format: Option<&str>,
    ) -> Result<Vec<u8>> {
        if artifact.kind == "report" {
            return artifact
                .content
                .as_deref()
                .map(|content| content.as_bytes().to_vec())
                .ok_or_else(|| Error::Refused("report content is not ready for download".into()));
        }
        let url = match artifact.kind.as_str() {
            "audio" | "video" | "infographic" => artifact.download_url.as_deref(),
            "slide_deck" => slide_deck_url(
                artifact
                    .raw
                    .as_array()
                    .map(Vec::as_slice)
                    .unwrap_or_default(),
                output_format.unwrap_or("pdf"),
            ),
            other => {
                return Err(Error::Refused(format!(
                    "native artifact download is unavailable for kind={other}"
                )));
            }
        }
        .ok_or_else(|| Error::Refused("artifact is not ready for download".into()))?;
        let url = parse_trusted_download_url(url)?;
        let client = download_http_client()?;
        let mut request = client.get(url.clone());
        if let Some(host) = url.host_str().filter(|host| google_cookie_host(Some(host)))
            && let Some(cookie_header) = self.cookie_header_for(host, url.path())?
        {
            request = crate::auth::attach_cookie(request, &cookie_header);
        }
        let response = request.send()?;
        if crate::transport::is_auth_redirect(&response) {
            return Err(Error::Authentication);
        }
        let bytes = response.error_for_status()?.bytes()?.to_vec();
        if bytes.is_empty() {
            return Err(Error::Refused(
                "artifact download returned an empty response".into(),
            ));
        }
        Ok(bytes)
    }
}

fn download_http_client() -> Result<HttpClient> {
    HttpClient::builder()
        .timeout(Duration::from_secs(180))
        .connect_timeout(Duration::from_secs(15))
        .user_agent("Mozilla/5.0 CCCC NotebookLM Rust artifact downloader")
        .redirect(Policy::custom(|attempt| {
            if attempt.previous().len() > DOWNLOAD_REDIRECT_LIMIT {
                return attempt.error(std::io::Error::other(
                    "NotebookLM artifact download exceeded the redirect limit",
                ));
            }
            match trusted_download_url(attempt.url()) {
                Ok(()) => attempt.follow(),
                Err(message) => attempt.error(std::io::Error::other(message)),
            }
        }))
        .build()
        .map_err(Error::from)
}

fn parse_trusted_download_url(raw: &str) -> Result<Url> {
    let url = Url::parse(raw)
        .map_err(|error| Error::Refused(format!("invalid artifact download URL: {error}")))?;
    trusted_download_url(&url).map_err(Error::Refused)?;
    Ok(url)
}

fn trusted_download_url(url: &Url) -> std::result::Result<(), String> {
    let host = url
        .host_str()
        .ok_or_else(|| "artifact download URL is missing a host".to_owned())?;
    if url.scheme() != "https" {
        return Err(format!("artifact download URL must use HTTPS: {host}"));
    }
    if !trusted_download_host(host) {
        return Err(format!("untrusted artifact download host: {host}"));
    }
    Ok(())
}

fn trusted_download_host(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    !host
        .bytes()
        .any(|value| matches!(value, b'%' | b'\\' | b'/'))
        && TRUSTED_DOWNLOAD_DOMAINS
            .iter()
            .any(|domain| host == *domain || host.ends_with(&format!(".{domain}")))
}

fn google_cookie_host(host: Option<&str>) -> bool {
    host.is_some_and(|host| {
        let host = host.to_ascii_lowercase();
        host == "google.com" || host.ends_with(".google.com")
    })
}

fn artifact_ready(artifact: &Artifact) -> bool {
    !matches!(
        artifact.kind.as_str(),
        "audio" | "video" | "infographic" | "slide_deck"
    ) || artifact.download_url.is_some()
}

fn generation_params(
    notebook_id: &str,
    source_ids: &[String],
    kind: &str,
    language: &str,
    instructions: Option<&str>,
) -> Result<Value> {
    let kind = normalize_kind(kind)?;
    let triple = source_ids
        .iter()
        .map(|id| json!([[id]]))
        .collect::<Vec<_>>();
    let double = source_ids.iter().map(|id| json!([id])).collect::<Vec<_>>();
    let client = json!([
        2,
        null,
        null,
        [1, null, null, null, null, null, null, null, null, null, [1]],
        [[1, 4, 8, 2, 3, 6]]
    ]);
    let descriptor = match kind {
        "audio" => json!([
            null,
            null,
            1,
            triple,
            null,
            null,
            [null, [instructions, 2, null, double, language, null, 1]]
        ]),
        "video" => json!([
            null,
            null,
            3,
            triple,
            null,
            null,
            null,
            null,
            [null, null, [double, language, instructions, null, 1, 1]]
        ]),
        "report" | "study_guide" => {
            let study = kind == "study_guide";
            let title = if study { "Study Guide" } else { "Briefing Doc" };
            let description = if study {
                "Short-answer quiz, essay questions, glossary"
            } else {
                "Key insights and important quotes"
            };
            let prompt = instructions.unwrap_or(if study {
                "Create a comprehensive study guide with key concepts, practice questions, essay prompts, and a glossary."
            } else {
                "Create a comprehensive briefing document with an executive summary, key themes, important quotes, and actionable insights."
            });
            json!([
                null,
                null,
                2,
                triple,
                null,
                null,
                null,
                [
                    null,
                    [
                        title,
                        description,
                        null,
                        double,
                        language,
                        prompt,
                        null,
                        true
                    ]
                ]
            ])
        }
        "quiz" => json!([
            null,
            null,
            4,
            triple,
            null,
            null,
            null,
            null,
            null,
            [
                null,
                [2, null, instructions, null, null, null, null, [2, 2]]
            ]
        ]),
        "flashcards" => json!([
            null,
            null,
            4,
            triple,
            null,
            null,
            null,
            null,
            null,
            [null, [1, null, instructions, null, null, null, [2, 2]]]
        ]),
        "mind_map" => json!([
            null,
            null,
            4,
            triple,
            null,
            null,
            null,
            null,
            null,
            [null, [4]]
        ]),
        "infographic" => json!([
            null,
            null,
            7,
            triple,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            [[instructions, language, null, 1, 2, 1]]
        ]),
        "slide_deck" => json!([
            null,
            null,
            8,
            triple,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            [[instructions, language, 1, 1]]
        ]),
        "data_table" => json!([
            null,
            null,
            9,
            triple,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            null,
            [null, [instructions, language]]
        ]),
        _ => unreachable!(),
    };
    Ok(json!([client, notebook_id, descriptor]))
}

fn normalize_kind(kind: &str) -> Result<&'static str> {
    match kind {
        "audio" => Ok("audio"),
        "video" => Ok("video"),
        "report" => Ok("report"),
        "study_guide" | "study" | "studyguide" => Ok("study_guide"),
        "quiz" => Ok("quiz"),
        "flashcards" => Ok("flashcards"),
        "mind_map" | "mindmap" => Ok("mind_map"),
        "infographic" => Ok("infographic"),
        "slide_deck" | "slides" | "deck" => Ok("slide_deck"),
        "data_table" | "table" => Ok("data_table"),
        _ => Err(Error::Refused(format!("unsupported artifact kind: {kind}"))),
    }
}

fn unwrap_rows(value: &Value) -> &[Value] {
    let Some(rows) = value.as_array() else {
        return &[];
    };
    if rows.len() == 1
        && let Some(inner) = rows[0].as_array()
        && (inner.is_empty() || inner[0].is_array())
    {
        return inner;
    }
    rows
}

fn parse_artifact(value: &Value) -> Option<Artifact> {
    let row = value.as_array()?;
    let id = row.first()?.as_str()?.to_owned();
    let type_code = row.get(2).and_then(Value::as_i64).unwrap_or(0);
    let variant = row
        .get(9)
        .and_then(|value| value.get(1))
        .and_then(|value| value.get(0))
        .and_then(Value::as_i64);
    let kind = match (type_code, variant) {
        (1, _) => "audio",
        (2, _) => "report",
        (3, _) => "video",
        (4, Some(1)) => "flashcards",
        (4, Some(2)) => "quiz",
        (4, Some(4)) => "mind_map",
        (5, _) => "mind_map",
        (7, _) => "infographic",
        (8, _) => "slide_deck",
        (9, _) => "data_table",
        _ => "unknown",
    };
    let status = artifact_status(row.get(4).and_then(Value::as_i64));
    Some(Artifact {
        id,
        title: row.get(1).and_then(Value::as_str).unwrap_or("").into(),
        kind: kind.into(),
        status: status.into(),
        variant,
        download_url: artifact_url(row, type_code),
        content: artifact_content(row, type_code),
        raw: value.clone(),
    })
}

fn artifact_status(code: Option<i64>) -> &'static str {
    match code.unwrap_or(0) {
        1 => "pending",
        2 => "in_progress",
        3 => "completed",
        4 => "failed",
        5 => "suggested",
        6 => "pending_review",
        _ => "unknown",
    }
}

fn artifact_content(row: &[Value], type_code: i64) -> Option<String> {
    if type_code == 2 {
        return row.get(7).and_then(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.get(0).and_then(Value::as_str).map(str::to_owned))
        });
    }
    None
}

fn artifact_url(row: &[Value], type_code: i64) -> Option<String> {
    match type_code {
        1 => audio_url(row),
        3 => video_url(row),
        7 => infographic_url(row),
        8 => slide_deck_url(row, "pdf").map(str::to_owned),
        _ => None,
    }
}

fn audio_url(row: &[Value]) -> Option<String> {
    let media = row.get(6)?.get(5)?.as_array()?;
    let mut fallback = None;
    for item in media.iter().filter_map(Value::as_array) {
        let Some(url) = item.first().and_then(http_url) else {
            continue;
        };
        fallback.get_or_insert_with(|| url.to_owned());
        if item.get(2).and_then(Value::as_str) == Some("audio/mp4") {
            return Some(url.to_owned());
        }
    }
    fallback
}

fn video_url(row: &[Value]) -> Option<String> {
    let variants = row.get(8)?.as_array()?;
    let mut fallback = None;
    for media in variants.iter().filter_map(Value::as_array) {
        for item in media.iter().filter_map(Value::as_array) {
            let Some(url) = item.first().and_then(http_url) else {
                continue;
            };
            fallback.get_or_insert_with(|| url.to_owned());
            if item.get(2).and_then(Value::as_str) == Some("video/mp4") {
                if item.get(1).and_then(Value::as_i64) == Some(4) {
                    return Some(url.to_owned());
                }
                fallback = Some(url.to_owned());
            }
        }
    }
    fallback
}

fn infographic_url(row: &[Value]) -> Option<String> {
    row.iter().filter_map(Value::as_array).find_map(|item| {
        item.get(2)?
            .as_array()?
            .first()?
            .get(1)?
            .as_array()?
            .first()
            .and_then(http_url)
            .map(str::to_owned)
    })
}

fn slide_deck_url<'a>(row: &'a [Value], output_format: &str) -> Option<&'a str> {
    let index = if output_format == "pptx" { 4 } else { 3 };
    row.get(16)?.get(index).and_then(http_url)
}

fn http_url(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_payload_uses_python_source_id_nesting() {
        let value = generation_params("notebook", &["source-a".into()], "audio", "en", None)
            .expect("payload");
        assert_eq!(value[2][3], json!([[["source-a"]]]));
        assert_eq!(value[2][6][1][3], json!([["source-a"]]));
    }

    #[test]
    fn parses_python_artifact_row() {
        let row = json!([
            "artifact-1",
            "Quiz",
            4,
            null,
            3,
            null,
            null,
            null,
            null,
            [null, [2]]
        ]);
        let artifact = parse_artifact(&row).expect("artifact");
        assert_eq!(artifact.id, "artifact-1");
        assert_eq!(artifact.kind, "quiz");
        assert_eq!(artifact.status, "completed");
    }

    #[test]
    fn artifact_status_matches_the_v081_backend_enum() {
        assert_eq!(artifact_status(Some(0)), "unknown");
        assert_eq!(artifact_status(Some(1)), "pending");
        assert_eq!(artifact_status(Some(2)), "in_progress");
        assert_eq!(artifact_status(Some(3)), "completed");
        assert_eq!(artifact_status(Some(4)), "failed");
        assert_eq!(artifact_status(Some(5)), "suggested");
        assert_eq!(artifact_status(Some(6)), "pending_review");
        assert_eq!(artifact_status(Some(99)), "unknown");
        assert_eq!(artifact_status(None), "unknown");
    }

    #[test]
    fn completed_media_waits_for_its_download_url() {
        let mut artifact = Artifact {
            id: "artifact-1".into(),
            title: "Audio".into(),
            kind: "audio".into(),
            status: "completed".into(),
            variant: None,
            download_url: None,
            content: None,
            raw: Value::Null,
        };
        assert!(!artifact_ready(&artifact));
        artifact.download_url = Some("https://storage.googleapis.com/audio.mp4".into());
        assert!(artifact_ready(&artifact));

        artifact.kind = "report".into();
        artifact.download_url = None;
        assert!(artifact_ready(&artifact));
    }

    #[test]
    fn artifact_download_urls_require_https_google_hosts() {
        for raw in [
            "https://notebooklm.google.com/file",
            "https://lh3.googleusercontent.com/file",
            "https://storage.googleapis.com/file",
        ] {
            let url = Url::parse(raw).expect("trusted URL");
            assert_eq!(trusted_download_url(&url), Ok(()), "{raw}");
        }
        for raw in [
            "http://notebooklm.google.com/file",
            "https://evilgoogle.com/file",
            "https://google.com.evil.test/file",
            "https://169.254.169.254/latest/meta-data",
        ] {
            let url = Url::parse(raw).expect("untrusted URL shape");
            assert!(trusted_download_url(&url).is_err(), "{raw}");
        }
    }

    #[test]
    fn artifact_urls_use_kind_specific_positions() {
        let mut slide = vec![Value::Null; 17];
        slide[16] = json!([
            null,
            null,
            null,
            "https://storage.googleapis.com/deck.pdf",
            "https://storage.googleapis.com/deck.pptx"
        ]);
        assert_eq!(
            artifact_url(&slide, 8).as_deref(),
            Some("https://storage.googleapis.com/deck.pdf")
        );
        assert_eq!(
            slide_deck_url(&slide, "pptx"),
            Some("https://storage.googleapis.com/deck.pptx")
        );

        let mut table = vec![Value::Null; 19];
        table[18] = json!([["Heading", "Value"], ["A", 1]]);
        assert_eq!(artifact_content(&table, 9), None);
        assert_eq!(artifact_url(&table, 9), None);
    }
}
