use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::{GroupStore, HomeLayout, ledger};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

#[tokio::test]
async fn group_copy_export_preview_and_staged_import_work_without_python() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let store = GroupStore::new(home.clone()).expect("store");
    let group = store.create("Web Copy", "").expect("group");
    let app = cccc_web::app(home);
    let exported = app
        .clone()
        .oneshot(
            Request::get(format!("/api/v1/groups/{}/copy/export", group.group_id))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("export");
    assert_eq!(exported.status(), StatusCode::OK);
    assert_eq!(exported.headers()[header::CONTENT_TYPE], "application/zip");
    let package = exported
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();

    let boundary = "copy-boundary";
    let mut body = format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"group.zip\"\r\nContent-Type: application/zip\r\n\r\n").into_bytes();
    body.extend_from_slice(&package);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    let preview = app
        .clone()
        .oneshot(
            Request::post("/api/v1/groups/copy/preview_import")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={boundary}"),
                )
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("preview");
    assert_eq!(preview.status(), StatusCode::OK);
    let preview: Value = serde_json::from_slice(
        &preview
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json");
    let upload_id = preview["result"]["upload_id"].as_str().expect("upload id");
    assert_eq!(
        preview["result"]["preview"]["source_group_id"],
        group.group_id
    );

    let import_boundary = "import-boundary";
    let import_body = format!(
        "--{import_boundary}\r\nContent-Disposition: form-data; name=\"upload_id\"\r\n\r\n{upload_id}\r\n--{import_boundary}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nImported Web Copy\r\n--{import_boundary}--\r\n"
    );
    let imported = app
        .oneshot(
            Request::post("/api/v1/groups/copy/import")
                .header(
                    header::CONTENT_TYPE,
                    format!("multipart/form-data; boundary={import_boundary}"),
                )
                .body(Body::from(import_body))
                .expect("request"),
        )
        .await
        .expect("import");
    assert_eq!(imported.status(), StatusCode::OK);
    let imported: Value = serde_json::from_slice(
        &imported
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
    )
    .expect("json");
    assert_ne!(imported["result"]["group_id"], group.group_id);
    assert_eq!(imported["result"]["group_id_conflict"], true);
    let imported_group_id = imported["result"]["group_id"]
        .as_str()
        .expect("imported group id");
    let events = ledger::tail(
        &store
            .ledger_path(imported_group_id)
            .expect("imported ledger"),
        1,
    )
    .expect("import lifecycle event");
    assert_eq!(events[0].kind, "group.create");
    assert_eq!(events[0].data["imported"], true);
}
