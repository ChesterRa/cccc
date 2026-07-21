use super::inbound_attachments::{AttachmentSpec, download_response};
use cccc_core::HomeLayout;
use lark_channel::lark_openapi::{OpenApiClient, ReqwestOpenApiTransport};
use lark_channel::{ResourceDescriptor, ResourceType};
use serde_json::Value;

pub(super) async fn materialize_resources(
    home: &HomeLayout,
    group_id: &str,
    http: &reqwest::Client,
    openapi: &OpenApiClient<ReqwestOpenApiTransport>,
    base_url: &str,
    resources: &[ResourceDescriptor],
) -> Vec<Value> {
    if resources.is_empty() {
        return Vec::new();
    }
    let token = match openapi.tenant_access_token().await {
        Ok(token) => token,
        Err(error) => {
            tracing::warn!(%error, "failed to authorize Feishu attachment download");
            return Vec::new();
        }
    };
    let mut result = Vec::with_capacity(resources.len());
    for resource in resources {
        match materialize_resource(home, group_id, http, base_url, &token, resource).await {
            Ok(value) => result.push(value),
            Err(error) => tracing::warn!(
                %error,
                message_id = %resource.message_id,
                "failed to download Feishu attachment"
            ),
        }
    }
    result
}

async fn materialize_resource(
    home: &HomeLayout,
    group_id: &str,
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    resource: &ResourceDescriptor,
) -> Result<Value, String> {
    let (url, spec) = resource_request(base_url, resource)?;
    let response = http
        .get(url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| error.to_string())?;
    download_response(home, group_id, response, None, spec).await
}

fn resource_request(
    base_url: &str,
    resource: &ResourceDescriptor,
) -> Result<(reqwest::Url, AttachmentSpec), String> {
    let (path, spec) = match resource.resource_type {
        ResourceType::Image => {
            let key = required(resource.image_key.as_deref(), "image_key")?;
            (
                format!(
                    "/open-apis/im/v1/messages/{}/resources/{key}",
                    resource.message_id
                ),
                AttachmentSpec::new("image", "image.png", "image/png").with_source_id(key),
            )
        }
        ResourceType::File | ResourceType::Audio | ResourceType::Media => {
            let key = required(resource.file_key.as_deref(), "file_key")?;
            let title =
                resource
                    .file_name
                    .clone()
                    .unwrap_or_else(|| match resource.resource_type {
                        ResourceType::Audio => "audio.opus".into(),
                        ResourceType::Media => "video.mp4".into(),
                        _ => "file".into(),
                    });
            let mime_type = match resource.resource_type {
                ResourceType::Audio => "audio/opus",
                ResourceType::Media => "video/mp4",
                _ => "",
            };
            (
                format!(
                    "/open-apis/im/v1/messages/{}/resources/{key}",
                    resource.message_id
                ),
                AttachmentSpec::new("file", title, mime_type).with_source_id(key),
            )
        }
        ResourceType::Sticker => {
            let key = required(resource.file_key.as_deref(), "file_key")?;
            (
                format!(
                    "/open-apis/im/v1/messages/{}/resources/{key}",
                    resource.message_id
                ),
                AttachmentSpec::new("image", "sticker.png", "image/png").with_source_id(key),
            )
        }
        ResourceType::Folder | ResourceType::Unknown => {
            return Err(format!(
                "unsupported Feishu resource type: {:?}",
                resource.resource_type
            ));
        }
    };
    let mut url = reqwest::Url::parse(base_url).map_err(|error| error.to_string())?;
    url.set_path(&path);
    let resource_type = if matches!(resource.resource_type, ResourceType::Image) {
        "image"
    } else {
        "file"
    };
    url.query_pairs_mut().append_pair("type", resource_type);
    Ok((url, spec))
}

fn required<'a>(value: Option<&'a str>, name: &str) -> Result<&'a str, String> {
    value
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| format!("Feishu attachment has no {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_message_resource_download_request() {
        let resource = ResourceDescriptor {
            message_id: "om_1".into(),
            resource_type: ResourceType::File,
            file_key: Some("file_1".into()),
            image_key: None,
            file_name: Some("report.pdf".into()),
            duration_ms: None,
        };
        let (url, spec) = resource_request("https://open.feishu.cn", &resource).expect("request");
        assert_eq!(
            url.as_str(),
            "https://open.feishu.cn/open-apis/im/v1/messages/om_1/resources/file_1?type=file"
        );
        assert_eq!(spec.title, "report.pdf");
        assert_eq!(spec.mime_type, "application/pdf");
    }

    #[test]
    fn builds_image_download_request() {
        let resource = ResourceDescriptor {
            message_id: "om_1".into(),
            resource_type: ResourceType::Image,
            file_key: None,
            image_key: Some("img_1".into()),
            file_name: None,
            duration_ms: None,
        };
        let (url, spec) =
            resource_request("https://open.larksuite.com", &resource).expect("request");
        assert_eq!(
            url.as_str(),
            "https://open.larksuite.com/open-apis/im/v1/messages/om_1/resources/img_1?type=image"
        );
        assert_eq!(spec.kind, "image");
    }
}
