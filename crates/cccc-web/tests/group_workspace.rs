#![cfg(unix)]
mod auth_support;
mod workspace_support;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use workspace_support::{fixture, json, listed};

#[tokio::test]
async fn workspace_routes_list_read_and_write_within_the_active_scope() {
    let fixture = fixture();
    let app = auth_support::authenticated_app(fixture.home.clone());
    let group = &fixture.group_id;

    let (status, listing) = json(
        &app,
        Request::get(format!("/api/v1/groups/{group}/workspace/list"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let names: Vec<&str> = listing["result"]["items"]
        .as_array()
        .expect("items")
        .iter()
        .filter_map(|item| item["name"].as_str())
        .collect();
    assert!(names.contains(&"src"), "{names:?}");
    assert_eq!(
        names.first(),
        Some(&"src"),
        "directories must sort ahead of files"
    );

    let (status, file) = json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{group}/workspace/file?path=src%2Flib.rs"
        ))
        .body(Body::empty())
        .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(file["result"]["content"], "fn main() {}\n");
    let sha = file["result"]["sha256"].as_str().expect("sha").to_owned();

    let (status, written) = json(
        &app,
        Request::put(format!("/api/v1/groups/{group}/workspace/file"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({"path":"src/lib.rs","content":"fn main() { 2; }\n","sha256":sha})
                    .to_string(),
            ))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(written["result"]["created"], false);
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("src/lib.rs")).expect("reread"),
        "fn main() { 2; }\n"
    );
}

#[tokio::test]
async fn the_show_ignored_flag_is_accepted_and_reveals_ignored_entries() {
    let fixture = fixture();
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.email=c@e.io",
            "-c",
            "user.name=c",
            "commit",
            "-qm",
            "base",
        ],
    ] {
        std::process::Command::new("git")
            .args(&args)
            .current_dir(&fixture.repo)
            .output()
            .expect("git");
    }
    std::fs::write(fixture.repo.join(".gitignore"), "build/\n").expect("gitignore");
    std::fs::create_dir_all(fixture.repo.join("build")).expect("build");
    std::fs::write(fixture.repo.join("build/out.o"), "obj").expect("obj");

    let app = auth_support::authenticated_app(fixture.home.clone());
    let group = &fixture.group_id;

    let (status, hidden) = json(
        &app,
        Request::get(format!("/api/v1/groups/{group}/workspace/list"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!listed(&hidden).contains(&"build".to_owned()));

    // The browser sends `true`; a Rust bool query field rejects anything else with a 400,
    // which turned the "show ignored" toggle into an error state.
    let (status, shown) = json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{group}/workspace/list?show_ignored=true"
        ))
        .body(Body::empty())
        .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "show_ignored=true must be accepted");
    assert!(
        listed(&shown).contains(&"build".to_owned()),
        "ignored entries must appear once asked for: {:?}",
        listed(&shown)
    );
}

#[tokio::test]
async fn a_stale_digest_is_rejected_over_http_instead_of_overwriting() {
    let fixture = fixture();
    let app = auth_support::authenticated_app(fixture.home.clone());
    let group = &fixture.group_id;

    let (_, file) = json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{group}/workspace/file?path=src%2Flib.rs"
        ))
        .body(Body::empty())
        .expect("request"),
    )
    .await;
    let sha = file["result"]["sha256"].as_str().expect("sha").to_owned();
    std::fs::write(fixture.repo.join("src/lib.rs"), "fn actor() {}\n").expect("actor write");

    let (status, conflict) = json(
        &app,
        Request::put(format!("/api/v1/groups/{group}/workspace/file"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::json!({"path":"src/lib.rs","content":"fn browser() {}\n","sha256":sha})
                    .to_string(),
            ))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(conflict["error"]["code"], "workspace_write_conflict");
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("src/lib.rs")).expect("reread"),
        "fn actor() {}\n",
        "the Actor's edit must survive a conflicting browser save"
    );
}

#[tokio::test]
async fn every_workspace_entrypoint_refuses_paths_outside_the_scope() {
    let fixture = fixture();
    let app = auth_support::authenticated_app(fixture.home.clone());
    let group = &fixture.group_id;
    // The decoy sits beside the scope root, reachable only by climbing out of it.
    let escape = "..%2Fsecret.txt";

    let (status, _) = json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{group}/workspace/list?path={escape}"
        ))
        .body(Body::empty())
        .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "list must refuse traversal");

    let (status, read) = json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{group}/workspace/file?path={escape}"
        ))
        .body(Body::empty())
        .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "read must refuse traversal");
    assert_eq!(read["error"]["code"], "outside_scope");

    let (status, _) = json(
        &app,
        Request::put(format!("/api/v1/groups/{group}/workspace/file"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                r#"{"path":"../secret.txt","content":"owned\n","sha256":""}"#,
            ))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "write must refuse traversal");
    assert_eq!(
        std::fs::read_to_string(fixture.repo.parent().expect("parent").join("secret.txt"))
            .unwrap_or_default(),
        "",
        "no file may be created outside the scope root"
    );
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("secret.txt")).expect("decoy"),
        "not in the repo\n"
    );
}

#[tokio::test]
async fn exhibit_mode_hides_workspace_files_including_reads() {
    let fixture = fixture();
    let app =
        auth_support::authenticated_app_with_mode(fixture.home.clone(), cccc_web::WebMode::Exhibit);
    let group = &fixture.group_id;

    for path in [
        format!("/api/v1/groups/{group}/workspace/list"),
        format!("/api/v1/groups/{group}/workspace/file?path=src%2Flib.rs"),
    ] {
        let (status, payload) = json(
            &app,
            Request::get(&path).body(Body::empty()).expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path} must be blocked");
        assert_eq!(payload["error"]["code"], "read_only", "{path}");
    }
}
