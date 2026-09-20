use base64::Engine;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::io::{Cursor, Read};
use std::path::Path;

pub(crate) const MAX_MEDIA_BYTES: usize = 20 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 5 * 1024 * 1024;
pub(crate) const PPTX_MIME: &str =
    "application/vnd.openxmlformats-officedocument.presentationml.presentation";

/// Called only after active-scope or group-blob path resolution.
pub(crate) fn read(path: &Path, args: &Map<String, Value>) -> Result<Value, String> {
    if ["region", "page", "view"]
        .iter()
        .any(|key| args.contains_key(*key))
    {
        return Err("region/page/view are not supported: read returns original files or UTF-8 text without local conversion. Refresh the connector's tool definitions".into());
    }
    let metadata = path.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() {
        return Err("read expects a regular file".into());
    }
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut prefix = Vec::new();
    (&mut file)
        .take(12)
        .read_to_end(&mut prefix)
        .map_err(|error| error.to_string())?;
    let mime = image_mime(&prefix);
    let is_pdf = prefix.starts_with(b"%PDF-");
    let is_zip = prefix.starts_with(b"PK\x03\x04");
    let limit = if mime.is_some() || is_pdf || is_zip {
        byte_limit(args, MAX_MEDIA_BYTES, MAX_MEDIA_BYTES)?
    } else {
        byte_limit(args, 200_000, MAX_TEXT_BYTES)?
    };
    let mut bytes = prefix;
    let remaining = (limit + 1).saturating_sub(bytes.len());
    file.take(remaining as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    let truncated = bytes.len() > limit;
    if is_pdf || is_zip {
        if truncated {
            return Err(format!(
                "file exceeds max_bytes ({limit}, maximum 20 MiB); original files are never truncated"
            ));
        }
        let (mime, extension) = if is_pdf {
            ("application/pdf", "pdf")
        } else if is_pptx(&bytes)? {
            (PPTX_MIME, "pptx")
        } else {
            return Err("read supports PDF and PPTX original-file resources, not other ZIP formats; use info/blob_path for metadata. No local conversion is performed".into());
        };
        // The client consumes the original file; CCCC never extracts document
        // content or renders pages/slides as a fallback.
        let uri = format!("cccc-file:///{:x}.{extension}", Sha256::digest(&bytes));
        let mut result = crate::router::tool_result(json!({
            "path":path,"bytes":bytes.len(),"mime_type":mime,"truncated":false,
            "transport":"mcp_embedded_resource",
            "notice":"Original file attached as an MCP resource. Use the client's native file reader, including native page/image viewing for image-only PDFs. Report unavailable content explicitly; no local extraction or rendering was performed."
        }));
        result["content"]
            .as_array_mut()
            .expect("content")
            .push(json!({
                "type":"resource","resource":{
                    "uri":uri,"mimeType":mime,
                    "blob":base64::engine::general_purpose::STANDARD.encode(bytes)
                }
            }));
        return Ok(result);
    }
    if let Some(mime) = mime {
        if truncated {
            return Err(format!(
                "image exceeds max_bytes ({limit}, maximum 20 MiB); images are never truncated"
            ));
        }
        let mut result = crate::router::tool_result(json!({
            "path":path,"bytes":bytes.len(),"mime_type":mime,"truncated":false
        }));
        result["content"].as_array_mut().expect("content").push(json!({
            "type":"image","mimeType":mime,"data":base64::engine::general_purpose::STANDARD.encode(bytes)
        }));
        return Ok(result);
    }
    bytes.truncate(limit);
    let text = match std::str::from_utf8(&bytes) {
        Ok(text) => text,
        Err(error) if truncated && error.error_len().is_none() => {
            std::str::from_utf8(&bytes[..error.valid_up_to()]).map_err(|error| error.to_string())?
        }
        Err(_) => return Err("read supports UTF-8 text, native PNG, JPEG or WebP images, and original PDF/PPTX resources; use info/blob_path for other files. No local conversion is performed".into()),
    };
    if text.contains('\0') {
        return Err("read supports UTF-8 text, native PNG, JPEG or WebP images, and original PDF/PPTX resources; use info/blob_path for other files. No local conversion is performed".into());
    }
    Ok(crate::router::tool_result(json!({
        "path":path,"content":text,"bytes":metadata.len(),"truncated":truncated
    })))
}

fn is_pptx(bytes: &[u8]) -> Result<bool, String> {
    // MIME sniffing only, including extensionless Group blobs. Do not trust a
    // filename or treat every ZIP as a presentation. Read just the bounded
    // format manifest, never slide content or embedded assets.
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| "cannot identify PPTX: invalid ZIP container".to_owned())?;
    if archive.index_for_name("ppt/presentation.xml").is_none()
        || archive.index_for_name("[Content_Types].xml").is_none()
    {
        return Ok(false);
    }
    const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
    let mut manifest = Vec::new();
    archive
        .by_name("[Content_Types].xml")
        .map_err(|_| "cannot read PPTX format metadata".to_owned())?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut manifest)
        .map_err(|_| "cannot read PPTX format metadata".to_owned())?;
    if manifest.len() as u64 > MAX_MANIFEST_BYTES {
        return Err("PPTX format metadata exceeds 1 MiB".into());
    }
    let marker =
        b"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml";
    Ok(manifest.windows(marker.len()).any(|part| part == marker))
}

fn byte_limit(args: &Map<String, Value>, default: usize, maximum: usize) -> Result<usize, String> {
    match args.get("max_bytes") {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .filter(|value| (1..=maximum as u64).contains(value))
            .map(|v| v as usize)
            .ok_or_else(|| format!("max_bytes must be an integer between 1 and {maximum}")),
    }
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/lWQAAAAASUVORK5CYII=";

    #[test]
    fn image_reads_return_native_content_without_base64_in_metadata() {
        let temp = tempfile::tempdir().expect("fixture");
        let path = temp.path().join("extensionless-blob");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(PNG)
            .expect("png");
        std::fs::write(&path, &bytes).expect("fixture image");
        let result = read(&path, &Map::new()).expect("read image");
        assert_eq!(result["content"][1]["type"], "image");
        assert_eq!(result["content"][1]["mimeType"], "image/png");
        assert_eq!(result["content"][1]["data"], PNG);
        assert_eq!(result["structuredContent"]["bytes"], bytes.len());
        assert!(!result["structuredContent"].to_string().contains(PNG));
        assert!(
            !result["content"][0]["text"]
                .as_str()
                .expect("text")
                .contains(PNG)
        );
        assert_eq!(image_mime(b"\xff\xd8\xff\xe0abc"), Some("image/jpeg"));
        assert_eq!(image_mime(b"RIFF1234WEBP"), Some("image/webp"));
        assert_eq!(image_mime(b"RIFF1234WAVE"), None);
    }

    #[test]
    fn image_size_limits_fail_without_returning_a_partial_image() {
        let temp = tempfile::tempdir().expect("fixture");
        let path = temp.path().join("large.png");
        std::fs::write(&path, b"\x89PNG\r\n\x1a\n").expect("header");
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("file")
            .set_len(MAX_MEDIA_BYTES as u64 + 1)
            .expect("large fixture");
        assert!(
            read(&path, &Map::new())
                .expect_err("oversized")
                .contains("never truncated")
        );
        assert!(read(&path, json!({"max_bytes":1}).as_object().expect("args")).is_err());
        for value in [
            json!(0),
            json!(-1),
            json!(true),
            json!("100"),
            json!(MAX_MEDIA_BYTES + 1),
        ] {
            assert!(read(&path, json!({"max_bytes":value}).as_object().expect("args")).is_err());
        }
        assert!(
            read(temp.path(), &Map::new())
                .expect_err("directory")
                .contains("regular file")
        );
    }

    #[test]
    fn pdf_handoff_preserves_original_bytes_without_extracting_content() {
        let temp = tempfile::tempdir().expect("fixture");
        let path = temp.path().join("extensionless");
        // Even a UTF-8 PDF must travel as an original binary resource, never as
        // purported extracted text. Extensionless Group blobs use this path too.
        let pdf = b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\n%%EOF\n";
        std::fs::write(&path, pdf).expect("pdf");
        let result = read(&path, &Map::new()).expect("PDF handoff");
        let resource = &result["content"][1]["resource"];
        assert_eq!(result["content"][1]["type"], "resource");
        assert_eq!(resource["mimeType"], "application/pdf");
        assert_eq!(
            resource["blob"],
            base64::engine::general_purpose::STANDARD.encode(pdf)
        );
        assert!(
            resource["uri"]
                .as_str()
                .expect("URI")
                .starts_with("cccc-file:///")
        );
        assert_eq!(result["structuredContent"]["bytes"], pdf.len());
        assert!(result["structuredContent"]["content"].is_null());
        assert!(
            !result["content"][0]["text"]
                .as_str()
                .expect("metadata")
                .contains("/Catalog")
        );
        assert!(
            read(&path, json!({"max_bytes":1}).as_object().expect("args"))
                .expect_err("no partial file")
                .contains("never truncated")
        );
        assert_eq!(std::fs::read(&path).expect("unchanged PDF"), pdf);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .expect("PDF")
            .set_len(MAX_MEDIA_BYTES as u64 + 1)
            .expect("oversized PDF");
        assert!(
            read(&path, &Map::new())
                .expect_err("bounded PDF")
                .contains("never truncated")
        );
    }

    fn zip_fixture(entries: &[(&str, &[u8])]) -> Vec<u8> {
        use std::io::Write;
        let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in entries {
            archive
                .start_file(*name, zip::write::SimpleFileOptions::default())
                .expect("ZIP entry");
            archive.write_all(content).expect("ZIP bytes");
        }
        archive.finish().expect("ZIP fixture").into_inner()
    }

    #[test]
    fn pptx_handoff_sniffs_extensionless_files_and_preserves_the_complete_archive() {
        let temp = tempfile::tempdir().expect("fixture");
        let path = temp.path().join("extensionless");
        let manifest = br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/></Types>"#;
        let bytes = zip_fixture(&[
            ("[Content_Types].xml", manifest),
            ("ppt/presentation.xml", b"<p:presentation/>"),
            ("ppt/slides/slide1.xml", b"<p:sld>Unextracted text</p:sld>"),
        ]);
        std::fs::write(&path, &bytes).expect("PPTX");
        let result = read(&path, &Map::new()).expect("PPTX handoff");
        let resource = &result["content"][1]["resource"];
        assert_eq!(resource["mimeType"], PPTX_MIME);
        assert_eq!(
            resource["blob"],
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        );
        assert_eq!(
            resource["uri"],
            format!("cccc-file:///{:x}.pptx", Sha256::digest(&bytes))
        );
        assert_eq!(result["structuredContent"]["bytes"], bytes.len());
        assert!(
            !result["structuredContent"]
                .to_string()
                .contains("Unextracted")
        );
        assert!(
            read(
                &path,
                json!({"max_bytes":bytes.len() - 1})
                    .as_object()
                    .expect("args")
            )
            .expect_err("whole file required")
            .contains("never truncated")
        );
        assert_eq!(std::fs::read(&path).expect("source"), bytes);

        // Extension alone is not evidence: DOCX, macro-enabled presentations,
        // ordinary ZIPs and corrupt containers must not become PPTX resources.
        let path = temp.path().join("misnamed.pptx");
        for archive in [
            zip_fixture(&[("notes.txt", b"ordinary zip")]),
            zip_fixture(&[
                ("[Content_Types].xml", manifest),
                ("word/document.xml", b"<w:document/>"),
            ]),
            zip_fixture(&[
                (
                    "[Content_Types].xml",
                    b"application/vnd.ms-powerpoint.presentation.macroEnabled.main+xml",
                ),
                ("ppt/presentation.xml", b"<p:presentation/>"),
            ]),
            b"PK\x03\x04broken".to_vec(),
        ] {
            std::fs::write(&path, archive).expect("unsupported file");
            assert!(read(&path, &Map::new()).is_err());
        }
    }

    #[test]
    fn pptx_format_sniffing_bounds_decompressed_metadata() {
        let bytes = zip_fixture(&[
            ("[Content_Types].xml", &vec![b' '; 1024 * 1024 + 1]),
            ("ppt/presentation.xml", b"<p:presentation/>"),
        ]);
        assert!(
            is_pptx(&bytes)
                .expect_err("bounded manifest")
                .contains("exceeds 1 MiB")
        );
    }

    #[test]
    fn conversion_requests_fail_without_changing_the_file() {
        let temp = tempfile::tempdir().expect("fixture");
        let path = temp.path().join("image.png");
        let png = base64::engine::general_purpose::STANDARD
            .decode(PNG)
            .expect("png");
        std::fs::write(&path, &png).expect("image");
        for args in [
            json!({"region":{"x":0,"y":0,"width":1,"height":1}}),
            json!({"page":1}),
            json!({"view":"image"}),
        ] {
            let error = read(&path, args.as_object().expect("args")).expect_err("no conversion");
            assert!(error.contains("not supported"), "{error}");
        }
        assert_eq!(std::fs::read(&path).expect("unchanged image"), png);
    }

    #[test]
    fn text_reads_respect_byte_budget_without_splitting_utf8() {
        let temp = tempfile::tempdir().expect("fixture");
        let path = temp.path().join("text.png");
        std::fs::write(&path, "日本語\n").expect("text fixture");
        let result =
            read(&path, json!({"max_bytes":5}).as_object().expect("args")).expect("bounded read");
        assert_eq!(result["structuredContent"]["content"], "日");
        assert_eq!(result["structuredContent"]["truncated"], true);
        assert_eq!(result["structuredContent"]["bytes"], 10);
        assert_eq!(result["content"].as_array().expect("content").len(), 1);
        let full = read(&path, &Map::new()).expect("full text");
        assert_eq!(full["structuredContent"]["content"], "日本語\n");
        assert_eq!(full["structuredContent"]["truncated"], false);
        for binary in [b"RIFF1234WAVE\0\0".as_slice(), b"\xff\xfe\x01"] {
            std::fs::write(&path, binary).expect("binary fixture");
            assert!(
                read(&path, &Map::new())
                    .expect_err("unsupported binary")
                    .contains("PNG, JPEG or WebP")
            );
        }
    }
}
