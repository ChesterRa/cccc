use base64::Engine;
use cccc_contracts::DaemonRequest;
use cccc_core::{GroupStore, HomeLayout};
use serde_json::{Value, json};
use std::io::{Cursor, Write};

async fn call(home: &HomeLayout, group: &str, name: &str, args: Value) -> Value {
    cccc_mcp::handle_request_for_actor(home,
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":name,"arguments":args}}),
        group,"web-peer").await["result"].clone()
}

#[tokio::test]
async fn native_media_preserves_scope_blobs_and_code_mode_output() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    home.initialize().expect("initialize");
    let root = temp.path().join("project");
    std::fs::create_dir(&root).expect("workspace");
    let png = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/lWQAAAAASUVORK5CYII=";
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png)
        .expect("png");
    std::fs::write(root.join("fixture.png"), &bytes).expect("image");
    let pdf = b"%PDF-1.4\n1 0 obj << /Type /Catalog >> endobj\n%%EOF\n";
    let pdf_base64 = base64::engine::general_purpose::STANDARD.encode(pdf);
    std::fs::write(root.join("fixture.pdf"), pdf).expect("PDF");
    let mut archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in [
        (
            "[Content_Types].xml",
            r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/></Types>"#,
        ),
        ("ppt/presentation.xml", "<p:presentation/>"),
    ] {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .expect("PPTX entry");
        archive.write_all(content.as_bytes()).expect("PPTX bytes");
    }
    let pptx = archive.finish().expect("PPTX").into_inner();
    let pptx_base64 = base64::engine::general_purpose::STANDARD.encode(&pptx);
    std::fs::write(root.join("fixture.pptx"), &pptx).expect("PPTX");
    std::fs::write(temp.path().join("outside.png"), &bytes).expect("outside image");
    let store = GroupStore::new(home.clone()).expect("store");
    let mut group = store.create("image fixture", "").expect("group");
    let mut actor = cccc_contracts::Actor::new("web-peer");
    actor.runtime = cccc_contracts::ActorRuntime::WebModel;
    actor.normalize_runtime_constraints();
    cccc_core::actors::add(&mut group, actor).expect("actor");
    store.save(&group).expect("save");
    cccc_core::group_scope::attach(
        &store,
        &group.group_id,
        cccc_core::Scope {
            scope_key: "project".into(),
            url: root.to_string_lossy().into_owned(),
            label: "project".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach");
    let blob = cccc_core::blobs::store(&home, &group.group_id, &bytes).expect("blob");
    let pdf_blob = cccc_core::blobs::store(&home, &group.group_id, pdf).expect("PDF blob");
    let pptx_blob = cccc_core::blobs::store(&home, &group.group_id, &pptx).expect("PPTX blob");
    let daemon_home = home.clone();
    let daemon = tokio::spawn(async move { cccc_daemon::run(daemon_home).await });
    let client = cccc_client::DaemonClient::new(home.clone());
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if client
                .call(&DaemonRequest {
                    v: 1,
                    op: "ping".into(),
                    args: Default::default(),
                })
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("ready");
    for args in [
        json!({"action":"read","path":"fixture.png"}),
        json!({"action":"read","rel_path":blob.path}),
    ] {
        let result = call(&home, &group.group_id, "cccc_file", args).await;
        assert_eq!(result["content"][1]["data"], png, "native image: {result}");
        assert_eq!(result["structuredContent"]["mime_type"], "image/png");
    }
    for (path, blob_path, expected, mime) in [
        (
            "fixture.pdf",
            pdf_blob.path.as_str(),
            pdf_base64.as_str(),
            "application/pdf",
        ),
        (
            "fixture.pptx",
            pptx_blob.path.as_str(),
            pptx_base64.as_str(),
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        ),
    ] {
        for args in [
            json!({"action":"read","path":path}),
            json!({"action":"read","rel_path":blob_path}),
        ] {
            let result = call(&home, &group.group_id, "cccc_file", args).await;
            assert_eq!(result["content"][1]["type"], "resource", "{result}");
            assert_eq!(result["content"][1]["resource"]["blob"], expected);
            assert_eq!(result["content"][1]["resource"]["mimeType"], mime);
        }
    }
    let denied = call(
        &home,
        &group.group_id,
        "cccc_file",
        json!({"action":"read","path":"../outside.png"}),
    )
    .await;
    assert_eq!(denied["isError"], true);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temp.path().join("outside.png"), root.join("escape.png"))
            .expect("symlink");
        let denied = call(
            &home,
            &group.group_id,
            "cccc_file",
            json!({"action":"read","path":"escape.png"}),
        )
        .await;
        assert_eq!(denied["isError"], true);
    }
    if std::process::Command::new("node")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        for (path, expected, is_resource) in [
            ("fixture.png", png, false),
            ("fixture.pdf", pdf_base64.as_str(), true),
            ("fixture.pptx", pptx_base64.as_str(), true),
        ] {
            let first = call(&home,&group.group_id,"cccc_code_exec",json!({
            "source":format!("const file = await tools.cccc_file({{action:'read',path:'{path}'}}); text(file); await yield_control(); text('after media');"),
            "yield_time_ms":5000,"max_output_tokens":1
        })).await;
            assert_eq!(first["structuredContent"]["status"], "running", "{first}");
            assert_eq!(
                if is_resource {
                    &first["content"][1]["resource"]["blob"]
                } else {
                    &first["content"][1]["data"]
                },
                expected,
                "native media survives minimal text budget: {first}"
            );
            assert!(!first["structuredContent"].to_string().contains(expected));
            let next = call(
                &home,
                &group.group_id,
                "cccc_code_wait",
                json!({
                    "cell_id":first["structuredContent"]["cell_id"],"yield_time_ms":5000
                }),
            )
            .await;
            assert_eq!(next["structuredContent"]["status"], "completed", "{next}");
            assert_eq!(
                next["content"].as_array().expect("content").len(),
                1,
                "media must not repeat"
            );
            assert_eq!(next["structuredContent"]["output"], "after media");
        }
    }
    cccc_mcp::shutdown(&home).await;
    daemon.abort();
    let _ = daemon.await;
}
