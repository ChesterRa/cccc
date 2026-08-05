use dingtalk_stream::DingTalkStreamClient;

#[async_trait::async_trait]
pub(super) trait AttachmentMedia: Send + Sync {
    async fn upload(
        &self,
        raw: &[u8],
        file_type: &str,
        filename: &str,
        mime: &str,
    ) -> Result<String, String>;
    async fn access_token(&self) -> Result<String, String>;
}

#[async_trait::async_trait]
impl AttachmentMedia for DingTalkStreamClient {
    async fn upload(
        &self,
        raw: &[u8],
        file_type: &str,
        filename: &str,
        mime: &str,
    ) -> Result<String, String> {
        self.upload_to_dingtalk(raw, file_type, filename, mime)
            .await
            .map_err(|error| error.to_string())
    }

    async fn access_token(&self) -> Result<String, String> {
        self.get_access_token()
            .await
            .map_err(|error| error.to_string())
    }
}
