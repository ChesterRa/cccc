use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Notebook {
    pub id: String,
    pub title: String,
    pub sources_count: usize,
    pub is_owner: bool,
}

impl Notebook {
    pub(crate) fn parse(row: &Value) -> Result<Self> {
        let row = row
            .as_array()
            .ok_or_else(|| Error::drift("notebook row", "expected an array"))?;
        let title = row
            .first()
            .and_then(Value::as_str)
            .unwrap_or("")
            .replace("thought\n", "")
            .trim()
            .to_owned();
        let id = row.get(2).and_then(Value::as_str).unwrap_or("").to_owned();
        if id.is_empty() {
            return Err(Error::drift(
                "notebook row",
                "notebook id at slot 2 is absent",
            ));
        }
        let sources_count = row.get(1).and_then(Value::as_array).map_or(0, Vec::len);
        let is_owner = row
            .get(5)
            .and_then(Value::as_array)
            .and_then(|meta| meta.get(1))
            .is_none_or(|value| value == &Value::Bool(false));
        Ok(Self {
            id,
            title,
            sources_count,
            is_owner,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Source {
    pub id: String,
    pub title: Option<String>,
    pub kind: String,
    pub status: String,
    pub url: Option<String>,
    pub drive_document_id: Option<String>,
}

impl Source {
    pub(crate) fn parse_entry(row: &Value) -> Result<Self> {
        let row = row
            .as_array()
            .ok_or_else(|| Error::drift("source row", "expected an array"))?;
        let id = source_id(row.first()).unwrap_or_default();
        if id.is_empty() {
            return Err(Error::drift(
                "source row",
                "source id envelope is malformed",
            ));
        }
        let title = row.get(1).and_then(Value::as_str).map(str::to_owned);
        let metadata = row.get(2).and_then(Value::as_array);
        let type_code = metadata
            .and_then(|value| value.get(4))
            .and_then(Value::as_i64);
        let mime = metadata.and_then(|value| source_mime(value));
        let url = metadata
            .and_then(|value| nested_string(value.get(7)).or_else(|| nested_string(value.get(5))));
        let drive_document_id = metadata
            .and_then(|value| nested_string(value.first()).or_else(|| nested_string(value.get(9))));
        let status_code = row
            .get(3)
            .and_then(Value::as_array)
            .and_then(|value| value.get(1))
            .and_then(Value::as_i64);
        Ok(Self {
            id,
            title,
            kind: source_kind(type_code, mime).into(),
            status: match status_code {
                Some(1) => "processing",
                Some(2) => "ready",
                Some(3) => "error",
                Some(5) => "preparing",
                _ => "unknown",
            }
            .into(),
            url,
            drive_document_id,
        })
    }

    pub(crate) fn parse_unknown(value: &Value) -> Result<Self> {
        let data = value
            .as_array()
            .ok_or_else(|| Error::drift("source response", "expected an array"))?;
        let normalized = match data.first().and_then(Value::as_array) {
            Some(outer) if outer.first().and_then(Value::as_array).is_some() => {
                let inner = outer.first().and_then(Value::as_array).unwrap_or(outer);
                if inner.first().and_then(Value::as_array).is_some() {
                    inner
                } else {
                    outer
                }
            }
            _ => data,
        };
        Self::parse_entry(&Value::Array(normalized.clone()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Reference {
    pub citation_number: usize,
    pub source_id: String,
    pub text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryResult {
    pub answer: String,
    pub references: Vec<Reference>,
}

fn source_id(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(value) => Some(value.clone()),
        Value::Array(values) => {
            if let Some(value) = values.first().and_then(Value::as_str) {
                return Some(value.to_owned());
            }
            values
                .get(2)
                .and_then(Value::as_array)
                .and_then(|value| value.first())
                .and_then(Value::as_str)
                .map(str::to_owned)
        }
        _ => None,
    }
}

fn nested_string(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_array)
        .and_then(|value| value.first())
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn source_mime(metadata: &[Value]) -> Option<&str> {
    metadata.get(19).and_then(Value::as_str).or_else(|| {
        metadata
            .get(9)
            .and_then(Value::as_array)
            .and_then(|descriptor| descriptor.get(2))
            .and_then(Value::as_str)
    })
}

fn source_kind(code: Option<i64>, mime: Option<&str>) -> &'static str {
    if code == Some(14) && mime == Some("application/pdf") {
        return "pdf";
    }
    match code {
        Some(1) => "google_docs",
        Some(2) => "google_slides",
        Some(3) => "pdf",
        Some(4) => "pasted_text",
        Some(5) => "web_page",
        Some(8) => "markdown",
        Some(9) => "youtube",
        Some(10) => "media",
        Some(11) => "docx",
        Some(13) => "image",
        Some(14) => "google_spreadsheet",
        Some(16) => "csv",
        Some(17) => "epub",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_notebook_and_source_position_contracts() {
        let notebook = Notebook::parse(&json!([
            "thought\nDemo",
            [[["s1"]]],
            "n1",
            null,
            null,
            [null, false]
        ]))
        .expect("notebook");
        assert_eq!(
            notebook,
            Notebook {
                id: "n1".into(),
                title: "Demo".into(),
                sources_count: 1,
                is_owner: true
            }
        );
        let source = Source::parse_entry(&json!([
            ["s1"],
            "Doc",
            [
                null,
                null,
                null,
                null,
                4,
                null,
                null,
                ["https://example.test"]
            ],
            [null, 2]
        ]))
        .expect("source");
        assert_eq!(source.id, "s1");
        assert_eq!(source.status, "ready");
        assert_eq!(source.url.as_deref(), Some("https://example.test"));
        assert_eq!(source.drive_document_id, None);
    }

    #[test]
    fn exposes_drive_document_identity_for_safe_create_recovery() {
        let mut metadata = vec![Value::Null; 10];
        metadata[0] = json!(["drive-doc-1"]);
        metadata[4] = json!(1);
        let source = Source::parse_entry(&json!([
            [null, true, ["source-1"]],
            "Drive doc",
            metadata,
            [null, 2]
        ]))
        .expect("Drive source");
        assert_eq!(source.drive_document_id.as_deref(), Some("drive-doc-1"));
        assert_eq!(source.url, None);
    }

    #[test]
    fn disambiguates_drive_pdf_from_native_sheet() {
        let mut pdf_metadata = vec![Value::Null; 20];
        pdf_metadata[4] = json!(14);
        pdf_metadata[19] = json!("application/pdf");
        let pdf = Source::parse_entry(&json!([["pdf-1"], "Drive PDF", pdf_metadata, [null, 2]]))
            .expect("drive pdf");
        assert_eq!(pdf.kind, "pdf");

        let mut sheet_metadata = vec![Value::Null; 20];
        sheet_metadata[4] = json!(14);
        sheet_metadata[9] = json!(["drive-id", 1, "application/vnd.google-apps.spreadsheet", ""]);
        let sheet = Source::parse_entry(&json!([["sheet-1"], "Sheet", sheet_metadata, [null, 2]]))
            .expect("sheet");
        assert_eq!(sheet.kind, "google_spreadsheet");
    }

    #[test]
    fn unknown_source_status_does_not_masquerade_as_ready() {
        for status in [Value::Null, json!([null, 99])] {
            let source = Source::parse_entry(&json!([
                ["source-1"],
                "Unknown status",
                [null, null, null, null, 4],
                status
            ]))
            .expect("source");
            assert_eq!(source.status, "unknown");
        }
    }

    #[test]
    fn parses_v081_add_source_response_envelopes() {
        let web = Source::parse_unknown(&json!([[[
            ["web-source"],
            "Article",
            [
                null,
                31537,
                [1768312225, 274170000],
                ["owner", [1768312224, 923625000]],
                5,
                null,
                1,
                ["https://example.test/article"],
                64632
            ],
            [null, 2]
        ]]]))
        .expect("v0.8.1 URL source response");
        assert_eq!(web.id, "web-source");
        assert_eq!(web.kind, "web_page");
        assert_eq!(web.url.as_deref(), Some("https://example.test/article"));

        let drive = Source::parse_unknown(&json!([[[
            ["drive-source"],
            "Design doc",
            [
                ["drive-file", "opaque", 17],
                3737,
                [1769198541, 320332000],
                ["owner", [1769198540, 885478000]],
                1,
                null,
                1,
                null,
                7610
            ],
            [null, 2]
        ]]]))
        .expect("v0.8.1 Drive source response");
        assert_eq!(drive.id, "drive-source");
        assert_eq!(drive.kind, "google_docs");
        assert_eq!(drive.status, "ready");
    }
}
