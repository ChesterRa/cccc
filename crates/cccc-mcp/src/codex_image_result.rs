//! Preserve native images for Codex MCP clients that prefer structuredContent.
//!
//! Codex CLI 0.166.0 identified itself as `codex-mcp-client` in an isolated
//! initialize handshake. With both fields present it sent only structuredContent
//! to the model; with the same content and no structuredContent it sent the PNG
//! as input_image. This adapter is enabled only for that client identity.
use serde_json::{Value, json};

pub(crate) const CODEX_MCP_CLIENT: &str = "codex-mcp-client";

/// Convert only final tool results containing images; internal/nested results
/// and other clients retain their original structured metadata.
pub(crate) fn preserve_images(is_codex: bool, request: &Value, response: &mut Value) {
    if !is_codex || request.get("method").and_then(Value::as_str) != Some("tools/call") {
        return;
    }
    let Some(result) = response.get_mut("result").and_then(Value::as_object_mut) else {
        return;
    };
    let Some(structured) = result.get("structuredContent").cloned() else {
        return;
    };
    // Native MCP error receipts are required to retain machine-readable errors.
    if result.get("isError").and_then(Value::as_bool) == Some(true)
        && structured.get("error").is_some()
    {
        return;
    }
    let Some(content) = result.get_mut("content").and_then(Value::as_array_mut) else {
        return;
    };
    if !content
        .iter()
        .any(|item| item.get("type").and_then(Value::as_str) == Some("image"))
    {
        return;
    }
    // Most CCCC tools already include the same JSON in content[0]. Keep any
    // additional structured fields as text too, without duplicating the usual
    // case or discarding a tool's existing content.
    let has_metadata = content.iter().any(|item| {
        item.get("type").and_then(Value::as_str) == Some("text")
            && item
                .get("text")
                .and_then(Value::as_str)
                .and_then(|text| serde_json::from_str::<Value>(text).ok())
                .is_some_and(|value| value == structured)
    });
    if !has_metadata {
        let Ok(text) = serde_json::to_string(&structured) else {
            return;
        };
        content.push(json!({"type":"text", "text":text}));
    }
    result.remove("structuredContent");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call() -> Value {
        json!({"method":"tools/call", "id":1, "params":{"name":"cccc_file"}})
    }

    fn image_response() -> Value {
        let metadata = json!({"path":"fixture.png", "bytes":3, "mime_type":"image/png"});
        json!({"jsonrpc":"2.0", "id":1, "result":{
            "content":[{"type":"text", "text":metadata.to_string()},
                       {"type":"image", "mimeType":"image/png", "data":"YWJj"}],
            "structuredContent":metadata
        }})
    }

    #[test]
    fn known_codex_receives_original_image_and_metadata_as_content() {
        let original = image_response();
        let mut result = original.clone();
        preserve_images(true, &call(), &mut result);
        assert!(result["result"].get("structuredContent").is_none());
        assert_eq!(result["result"]["content"], original["result"]["content"]);
    }

    #[test]
    fn preserves_metadata_not_already_present_in_content() {
        let mut result = image_response();
        result["result"]["structuredContent"]["extra"] = json!("retained");
        let original = result["result"]["structuredContent"].clone();
        preserve_images(true, &call(), &mut result);
        let content = result["result"]["content"].as_array().expect("content");
        assert_eq!(content[1]["type"], "image");
        let retained: Value = serde_json::from_str(content[2]["text"].as_str().expect("text"))
            .expect("metadata JSON");
        assert_eq!(retained, original);
    }

    #[test]
    fn code_mode_images_keep_the_status_and_cell_id_in_text() {
        for tool in ["cccc_code_exec", "cccc_code_wait"] {
            let metadata = json!({"status":"running", "cell_id":"fixture-cell"});
            let mut response = json!({"jsonrpc":"2.0", "id":1, "result":{
                "content":[{"type":"text", "text":metadata.to_string()},
                           {"type":"image", "mimeType":"image/png", "data":"YWJj"}],
                "structuredContent":metadata
            }});
            let request = json!({"method":"tools/call", "params":{"name":tool}});
            preserve_images(true, &request, &mut response);
            assert!(response["result"].get("structuredContent").is_none());
            let preserved: Value = serde_json::from_str(
                response["result"]["content"][0]["text"]
                    .as_str()
                    .expect("metadata"),
            )
            .expect("JSON");
            assert_eq!(preserved, metadata);
            assert_eq!(response["result"]["content"][1]["type"], "image");
        }
    }

    #[tokio::test]
    async fn version_notice_stays_visible_after_image_conversion() {
        let temp = tempfile::tempdir().expect("fixture home");
        let home = cccc_core::HomeLayout::from_path(temp.path().join("home")).expect("home");
        std::fs::create_dir_all(home.daemon_dir()).expect("daemon directory");
        std::fs::write(cccc_client::address_path(&home), r#"{"version":"999.0.0"}"#)
            .expect("address fixture");
        let mut response = image_response();
        preserve_images(true, &call(), &mut response);
        crate::version_notice::VersionNotice::default()
            .annotate(&home, Some("codex"), &call(), &mut response)
            .await;
        assert!(response["result"].get("structuredContent").is_none());
        let content = response["result"]["content"].as_array().expect("content");
        assert_eq!(content[1]["type"], "image");
        assert!(
            content[2]["text"]
                .as_str()
                .expect("notice")
                .contains("999.0.0")
        );
    }

    #[test]
    fn other_clients_non_calls_text_and_embedded_resources_are_unchanged() {
        let original = image_response();
        let mut other_client = original.clone();
        preserve_images(false, &call(), &mut other_client);
        assert_eq!(other_client, original);
        let mut not_call = original.clone();
        preserve_images(true, &json!({"method":"tools/list"}), &mut not_call);
        assert_eq!(not_call, original);
        for media in [
            json!({"type":"text", "text":"plain"}),
            json!({"type":"resource", "resource":{"mimeType":"application/pdf", "blob":"YWJj"}}),
        ] {
            let mut result = original.clone();
            result["result"]["content"][1] = media;
            let before = result.clone();
            preserve_images(true, &call(), &mut result);
            assert_eq!(result, before);
        }
    }

    #[test]
    fn does_not_remove_structured_daemon_errors_even_if_an_image_is_present() {
        let mut result = image_response();
        result["result"]["isError"] = json!(true);
        result["result"]["structuredContent"]["error"] = json!({"code":"fixture"});
        let original = result.clone();
        preserve_images(true, &call(), &mut result);
        assert_eq!(result, original);
    }
}
