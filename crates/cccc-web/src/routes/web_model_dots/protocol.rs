//! MCP 2.0 HTTP framing, alongside the existing legacy ChatGPT connector.
use axum::http::HeaderMap;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

pub(super) const VERSION: &str = "2026-07-28";
const PREFIX: &str = "io.modelcontextprotocol/";

pub(in crate::routes) fn is_modern(request: &Value, headers: &HeaderMap) -> bool {
    matches!(request["method"].as_str(), Some("server/discover"))
        || request["method"]
            .as_str()
            .is_some_and(|m| m.starts_with("events/"))
        || request["params"]["_meta"]
            .get(format!("{PREFIX}protocolVersion"))
            .is_some()
        || headers
            .get("mcp-protocol-version")
            .is_some_and(|v| v == VERSION)
}

pub(super) fn failure(request: &Value, code: i32, message: &str) -> Value {
    let mut result = json!({"jsonrpc":"2.0","error":{"code":code,"message":message}});
    if request["id"].is_string() || request["id"].is_i64() || request["id"].is_u64() {
        result["id"] = request["id"].clone();
    }
    result
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, &'static str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next().ok_or("Required MCP header is missing")?;
    if values.next().is_some() {
        return Err("Duplicate MCP header");
    }
    value.to_str().map_err(|_| "Malformed MCP header")
}

pub(in crate::routes) fn validate(request: &Value, headers: &HeaderMap) -> Result<(), Value> {
    let reject = |code, message| Err(failure(request, code, message));
    if request["jsonrpc"] != "2.0"
        || !(request["id"].is_string() || request["id"].is_i64() || request["id"].is_u64())
        || !request["params"].is_object()
    {
        return reject(-32600, "Invalid MCP 2.0 request");
    }
    let Some(method) = request["method"].as_str().filter(|m| !m.is_empty()) else {
        return reject(-32600, "MCP method is required");
    };
    let meta = &request["params"]["_meta"];
    let version = meta[format!("{PREFIX}protocolVersion")]
        .as_str()
        .filter(|v| !v.is_empty());
    if version.is_none() || !meta[format!("{PREFIX}clientCapabilities")].is_object() {
        return reject(
            -32602,
            "MCP 2.0 protocol version and client capabilities are required",
        );
    }
    if header(headers, "mcp-protocol-version") != Ok(version.expect("checked version"))
        || header(headers, "mcp-method") != Ok(method)
    {
        return reject(-32020, "MCP headers do not match the request");
    }
    if version != Some(VERSION) {
        let mut error = failure(request, -32022, "Unsupported MCP protocol version");
        error["error"]["data"] = json!({"supported":[VERSION],"requested":version});
        return Err(error);
    }
    if !matches!(
        method,
        "server/discover"
            | "tools/list"
            | "tools/call"
            | "events/list"
            | "events/subscribe"
            | "events/unsubscribe"
    ) {
        return reject(-32601, "MCP method is not supported");
    }
    if method == "tools/call" {
        let Some(name) = request["params"]["name"].as_str().filter(|n| !n.is_empty()) else {
            return reject(-32602, "Tool name is required");
        };
        let encoded = match header(headers, "mcp-name") {
            Ok(value) => value,
            Err(_) => return reject(-32020, "Mcp-Name is required"),
        };
        let decoded = if let Some(value) = encoded
            .strip_prefix("=?base64?")
            .and_then(|s| s.strip_suffix("?="))
        {
            STANDARD
                .decode(value)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
        } else {
            Some(encoded.to_owned())
        };
        if decoded.as_deref() != Some(name) {
            return reject(-32020, "Mcp-Name does not match the requested tool");
        }
    }
    Ok(())
}

pub(in crate::routes) fn complete(mut response: Value) -> Value {
    if response["result"].is_object() {
        let result = response["result"].as_object_mut().expect("checked object");
        result.insert("resultType".into(), json!("complete"));
        let mut meta = result
            .get("_meta")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        meta.insert(
            format!("{PREFIX}serverInfo"),
            json!({"name":"cccc","version":env!("CARGO_PKG_VERSION")}),
        );
        result.insert("_meta".into(), Value::Object(meta));
        if result.contains_key("tools")
            || result.contains_key("events")
            || result.contains_key("supportedVersions")
        {
            result.insert("ttlMs".into(), json!(0));
            result.insert("cacheScope".into(), json!("private"));
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn request(method: &str) -> (Value, HeaderMap) {
        let body = json!({"jsonrpc":"2.0","id":1,"method":method,"params":{"_meta":{
            "io.modelcontextprotocol/protocolVersion":VERSION,
            "io.modelcontextprotocol/clientCapabilities":{}
        }}});
        let mut headers = HeaderMap::new();
        headers.insert("mcp-protocol-version", HeaderValue::from_static(VERSION));
        headers.insert("mcp-method", HeaderValue::from_str(method).expect("header"));
        (body, headers)
    }

    #[test]
    fn modern_requests_require_matching_http_and_body_metadata() {
        let (body, mut headers) = request("events/list");
        assert!(is_modern(&body, &headers));
        assert!(validate(&body, &headers).is_ok());
        headers.insert("mcp-method", HeaderValue::from_static("tools/list"));
        assert_eq!(
            validate(&body, &headers).expect_err("mismatched method headers must be rejected")["error"]
                ["code"],
            -32020
        );
        headers.insert("mcp-method", HeaderValue::from_static("events/list"));
        headers.append("mcp-method", HeaderValue::from_static("events/list"));
        assert!(validate(&body, &headers).is_err());
        let mut body = body;
        body["id"] = json!(true);
        assert!(validate(&body, &headers).is_err());
    }

    #[test]
    fn tool_header_accepts_encoded_name_but_rejects_a_different_tool() {
        let (mut body, mut headers) = request("tools/call");
        body["params"]["name"] = json!("cccc_help");
        headers.insert(
            "mcp-name",
            HeaderValue::from_static("=?base64?Y2NjY19oZWxw?="),
        );
        assert!(validate(&body, &headers).is_ok());
        headers.insert("mcp-name", HeaderValue::from_static("cccc_message_send"));
        assert!(validate(&body, &headers).is_err());
    }

    #[test]
    fn framing_preserves_tool_payloads_and_does_not_change_legacy_detection() {
        let original = json!({"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"result"}],"structuredContent":{"ok":true},"isError":false}});
        let framed = complete(original.clone());
        assert_eq!(framed["result"]["content"], original["result"]["content"]);
        assert_eq!(
            framed["result"]["structuredContent"],
            original["result"]["structuredContent"]
        );
        assert_eq!(framed["result"]["resultType"], "complete");
        assert!(!is_modern(
            &json!({"method":"initialize"}),
            &HeaderMap::new()
        ));
        assert!(!is_modern(
            &json!({"method":"tools/list"}),
            &HeaderMap::new()
        ));
    }
}
