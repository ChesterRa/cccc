use super::wecom_client::{
    UPLOAD_MEDIA_CHUNK, UPLOAD_MEDIA_FINISH, UPLOAD_MEDIA_INIT, WecomClient,
};
use aes::Aes256;
use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::NoPadding};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use md5::{Digest, Md5};
use percent_encoding::percent_decode_str;
use serde_json::{Value, json};
use std::time::Duration;

const CHUNK_SIZE: usize = 512 * 1024;
const MAX_CHUNKS: usize = 100;
const MAX_DOWNLOAD_BYTES: usize = 50 * 1024 * 1024;
const MEDIA_TIMEOUT: Duration = Duration::from_secs(60);

pub(super) struct DownloadedMedia {
    pub bytes: Vec<u8>,
    pub filename: String,
}

impl WecomClient {
    pub(super) async fn upload_media(
        &self,
        bytes: &[u8],
        media_type: &str,
        filename: &str,
    ) -> Result<String, String> {
        if bytes.is_empty() {
            return Err("WeCom media payload is empty".into());
        }
        let total_chunks = bytes.len().div_ceil(CHUNK_SIZE);
        if total_chunks > MAX_CHUNKS {
            return Err(format!(
                "WeCom media exceeds the {} MiB upload limit",
                CHUNK_SIZE * MAX_CHUNKS / 1024 / 1024
            ));
        }
        let filename = safe_filename(filename);
        let md5 = format!("{:x}", Md5::digest(bytes));
        let init = self
            .send_command(
                UPLOAD_MEDIA_INIT,
                None,
                json!({
                    "type":media_type,
                    "filename":filename,
                    "total_size":bytes.len(),
                    "total_chunks":total_chunks,
                    "md5":md5
                }),
                MEDIA_TIMEOUT,
            )
            .await?;
        let upload_id = init
            .pointer("/body/upload_id")
            .and_then(Value::as_str)
            .ok_or_else(|| "WeCom media upload init returned no upload_id".to_owned())?;

        for (chunk_index, chunk) in bytes.chunks(CHUNK_SIZE).enumerate() {
            let body = json!({
                "upload_id":upload_id,
                "chunk_index":chunk_index,
                "base64_data":STANDARD.encode(chunk)
            });
            let mut last_error = String::new();
            for attempt in 0..=2 {
                match self
                    .send_command(UPLOAD_MEDIA_CHUNK, None, body.clone(), MEDIA_TIMEOUT)
                    .await
                {
                    Ok(_) => {
                        last_error.clear();
                        break;
                    }
                    Err(error) => {
                        last_error = error;
                        if attempt < 2 {
                            tokio::time::sleep(Duration::from_millis(500 * (attempt + 1))).await;
                        }
                    }
                }
            }
            if !last_error.is_empty() {
                return Err(format!(
                    "WeCom media chunk {chunk_index} failed after retries: {last_error}"
                ));
            }
        }

        let finish = self
            .send_command(
                UPLOAD_MEDIA_FINISH,
                None,
                json!({"upload_id":upload_id}),
                MEDIA_TIMEOUT,
            )
            .await?;
        finish
            .pointer("/body/media_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .ok_or_else(|| "WeCom media upload finish returned no media_id".to_owned())
    }
}

pub(super) async fn download_file(
    url: &str,
    aes_key: Option<&str>,
) -> Result<DownloadedMedia, String> {
    let parsed = reqwest::Url::parse(url).map_err(|error| error.to_string())?;
    if parsed.scheme() != "https" && !cfg!(test) {
        return Err("WeCom media download URL must use HTTPS".into());
    }
    let response = reqwest::Client::new()
        .get(parsed.clone())
        .timeout(MEDIA_TIMEOUT)
        .send()
        .await
        .map_err(|error| format!("WeCom media download failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("WeCom media download failed: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DOWNLOAD_BYTES as u64)
    {
        return Err("WeCom media download exceeds 50 MiB".into());
    }
    let header_filename = response
        .headers()
        .get(reqwest::header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .and_then(content_disposition_filename);
    let raw = response
        .bytes()
        .await
        .map_err(|error| format!("WeCom media download failed: {error}"))?;
    if raw.len() > MAX_DOWNLOAD_BYTES {
        return Err("WeCom media download exceeds 50 MiB".into());
    }
    let bytes = match aes_key.map(str::trim).filter(|value| !value.is_empty()) {
        Some(key) => decrypt_media(&raw, key)?,
        None => raw.to_vec(),
    };
    let filename = header_filename
        .or_else(|| {
            parsed
                .path_segments()
                .and_then(|mut segments| segments.next_back())
                .map(|value| percent_decode_str(value).decode_utf8_lossy().into_owned())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "file".into());
    Ok(DownloadedMedia {
        bytes,
        filename: safe_filename(&filename),
    })
}

pub(super) fn decrypt_media(encrypted: &[u8], aes_key: &str) -> Result<Vec<u8>, String> {
    if encrypted.is_empty() {
        return Err("WeCom encrypted media is empty".into());
    }
    let key = decode_aes_key(aes_key)?;
    let iv = &key[..16];
    let mut buffer = encrypted.to_vec();
    let decrypted = cbc::Decryptor::<Aes256>::new_from_slices(&key, iv)
        .map_err(|error| format!("invalid WeCom AES key: {error}"))?
        .decrypt_padded_mut::<NoPadding>(&mut buffer)
        .map_err(|error| format!("WeCom media decrypt failed: {error}"))?;
    let padding = usize::from(*decrypted.last().ok_or("WeCom decrypted media is empty")?);
    if padding == 0
        || padding > 32
        || padding > decrypted.len()
        || decrypted[decrypted.len() - padding..]
            .iter()
            .any(|byte| usize::from(*byte) != padding)
    {
        return Err("invalid WeCom media PKCS#7 padding".into());
    }
    Ok(decrypted[..decrypted.len() - padding].to_vec())
}

fn decode_aes_key(value: &str) -> Result<Vec<u8>, String> {
    let value = value.trim();
    if value.len() == 32 {
        return Ok(value.as_bytes().to_vec());
    }
    let padded = match value.len() % 4 {
        0 => value.to_owned(),
        remainder => format!("{value}{}", "=".repeat(4 - remainder)),
    };
    let decoded = STANDARD
        .decode(padded)
        .map_err(|error| format!("invalid WeCom media AES key: {error}"))?;
    if decoded.len() != 32 {
        return Err(format!(
            "invalid WeCom media AES key length: {}",
            decoded.len()
        ));
    }
    Ok(decoded)
}

fn safe_filename(value: &str) -> String {
    let value = value.trim().replace(['/', '\\'], "_");
    if value.is_empty() {
        "file".into()
    } else {
        value.chars().take(120).collect()
    }
}

fn content_disposition_filename(value: &str) -> Option<String> {
    value.split(';').skip(1).find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        if !matches!(
            key.trim().to_ascii_lowercase().as_str(),
            "filename" | "filename*"
        ) {
            return None;
        }
        let value = value
            .trim()
            .trim_matches('"')
            .split_once("''")
            .map_or(value, |(_, encoded)| encoded);
        Some(percent_decode_str(value).decode_utf8_lossy().into_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::{BlockEncryptMut, block_padding::NoPadding};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[test]
    fn decrypts_wecom_media_with_32_byte_pkcs7_padding() {
        let key = b"12345678901234567890123456789012";
        let plain = b"plain attachment bytes";
        let padding = 32 - plain.len() % 32;
        let mut padded = plain.to_vec();
        padded.extend(std::iter::repeat_n(padding as u8, padding));
        let mut encrypted = padded.clone();
        let length = encrypted.len();
        cbc::Encryptor::<Aes256>::new_from_slices(key, &key[..16])
            .expect("cipher")
            .encrypt_padded_mut::<NoPadding>(&mut encrypted, length)
            .expect("encrypt");
        assert_eq!(
            decrypt_media(&encrypted, std::str::from_utf8(key).expect("key")).expect("decrypt"),
            plain
        );
    }

    #[test]
    fn extracts_content_disposition_filenames() {
        assert_eq!(
            content_disposition_filename("attachment; filename*=UTF-8''report%20final.pdf"),
            Some("report final.pdf".into())
        );
    }

    #[tokio::test]
    async fn downloads_media_and_uses_content_disposition_filename() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("listener");
        let address = listener.local_addr().expect("address");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("accept");
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await.expect("request");
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Disposition: attachment; filename*=UTF-8''photo%20one.png\r\n\r\nimage",
                )
                .await
                .expect("response");
        });
        let media = download_file(&format!("http://{address}/opaque"), None)
            .await
            .expect("download");
        assert_eq!(media.bytes, b"image");
        assert_eq!(media.filename, "photo one.png");
        server.await.expect("server");
    }
}
