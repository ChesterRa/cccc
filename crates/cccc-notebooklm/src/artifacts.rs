use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Client, Error, Result, rpc};

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
        let status = match row.get(4).and_then(Value::as_i64) {
            Some(1) => "in_progress",
            Some(3) => "completed",
            Some(4) => "failed",
            _ => "pending",
        };
        Ok(ArtifactGeneration {
            artifact_id,
            kind: normalize_kind(kind)?.into(),
            status: status.into(),
            raw: result,
        })
    }

    pub fn download_artifact(&self, artifact: &Artifact) -> Result<Vec<u8>> {
        if let Some(content) = artifact.content.as_deref() {
            return Ok(content.as_bytes().to_vec());
        }
        let url = artifact
            .download_url
            .as_deref()
            .ok_or_else(|| Error::Refused("artifact is not ready for download".into()))?;
        let cookie = self.cookie_header()?;
        let response = crate::auth::attach_cookie(self.http.get(url), &cookie).send()?;
        if crate::transport::is_auth_redirect(&response) {
            return Err(Error::Authentication);
        }
        Ok(response.error_for_status()?.bytes()?.to_vec())
    }
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
    let status = match row.get(4).and_then(Value::as_i64).unwrap_or(0) {
        1 => "in_progress",
        2 => "pending",
        3 => "completed",
        4 => "failed",
        _ => "unknown",
    };
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

fn artifact_content(row: &[Value], type_code: i64) -> Option<String> {
    if type_code == 2 {
        return row.get(7).and_then(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .or_else(|| value.get(0).and_then(Value::as_str).map(str::to_owned))
        });
    }
    if type_code == 9 {
        return row
            .get(18)
            .and_then(|value| serde_json::to_string_pretty(value).ok());
    }
    None
}

fn artifact_url(row: &[Value], type_code: i64) -> Option<String> {
    let preferred_mime = match type_code {
        1 => Some("audio/mp4"),
        3 => Some("video/mp4"),
        _ => None,
    };
    let mut urls = Vec::new();
    collect_urls(&Value::Array(row.to_vec()), &mut urls);
    preferred_mime
        .and_then(|mime| {
            urls.iter()
                .find(|(_, candidate_mime)| candidate_mime.as_deref() == Some(mime))
                .map(|(url, _)| url.clone())
        })
        .or_else(|| urls.first().map(|(url, _)| url.clone()))
}

fn collect_urls(value: &Value, output: &mut Vec<(String, Option<String>)>) {
    if let Some(items) = value.as_array() {
        if let Some(url) = items
            .first()
            .and_then(Value::as_str)
            .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        {
            let mime = items.get(2).and_then(Value::as_str).map(str::to_owned);
            output.push((url.into(), mime));
        }
        for item in items {
            collect_urls(item, output);
        }
    }
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
}
