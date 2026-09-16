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
    let scope = url::form_urlencoded::byte_serialize(fixture.repo.to_string_lossy().as_bytes())
        .collect::<String>();

    let (status, listing) = json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{group}/workspace/list?scope_key=scope_repo&scope_url={scope}"
        ))
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
            "/api/v1/groups/{group}/workspace/file?scope_key=scope_repo&scope_url={scope}&path=src%2Flib.rs"
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
                serde_json::json!({"scope_key":"scope_repo", "scope_url":fixture.repo.to_string_lossy(), "path":"src/lib.rs","content":"fn main() { 2; }\n","sha256":sha})
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
    let scope = url::form_urlencoded::byte_serialize(fixture.repo.to_string_lossy().as_bytes())
        .collect::<String>();

    let (status, hidden) = json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{group}/workspace/list?scope_key=scope_repo&scope_url={scope}"
        ))
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
            "/api/v1/groups/{group}/workspace/list?scope_key=scope_repo&scope_url={scope}&show_ignored=true"
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
    let scope = url::form_urlencoded::byte_serialize(fixture.repo.to_string_lossy().as_bytes())
        .collect::<String>();

    let (_, file) = json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{group}/workspace/file?scope_key=scope_repo&scope_url={scope}&path=src%2Flib.rs"
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
                serde_json::json!({"scope_key":"scope_repo", "scope_url":fixture.repo.to_string_lossy(), "path":"src/lib.rs","content":"fn browser() {}\n","sha256":sha})
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
    let scope = url::form_urlencoded::byte_serialize(fixture.repo.to_string_lossy().as_bytes())
        .collect::<String>();
    // The decoy sits beside the scope root, reachable only by climbing out of it.
    let escape = "..%2Fsecret.txt";

    let (status, _) = json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{group}/workspace/list?scope_key=scope_repo&scope_url={scope}&path={escape}"
        ))
        .body(Body::empty())
        .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "list must refuse traversal");

    let (status, read) = json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{group}/workspace/file?scope_key=scope_repo&scope_url={scope}&path={escape}"
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
                serde_json::json!({"scope_key":"scope_repo", "scope_url":fixture.repo.to_string_lossy(), "path":"../secret.txt", "content":"owned\n", "sha256":""}).to_string(),
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
    let scope = url::form_urlencoded::byte_serialize(fixture.repo.to_string_lossy().as_bytes())
        .collect::<String>();

    for path in [
        format!("/api/v1/groups/{group}/workspace/path?scope_key=scope_repo&scope_url={scope}"),
        format!("/api/v1/groups/{group}/workspace/list?scope_key=scope_repo&scope_url={scope}"),
        format!(
            "/api/v1/groups/{group}/workspace/file?scope_key=scope_repo&scope_url={scope}&path=src%2Flib.rs"
        ),
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

#[tokio::test]
async fn whitespace_in_file_names_survives_http_read_and_save() {
    let fixture = fixture();
    let app = auth_support::authenticated_app(fixture.home.clone());
    let group = &fixture.group_id;
    let scope = url::form_urlencoded::byte_serialize(fixture.repo.to_string_lossy().as_bytes())
        .collect::<String>();
    std::fs::write(fixture.repo.join("note.txt"), "neighbor")
        .expect("workspace HTTP whitespace fixture");
    for (path, encoded) in [(" note.txt", "%20note.txt"), ("note.txt ", "note.txt%20")] {
        std::fs::write(fixture.repo.join(path), "selected")
            .expect("workspace HTTP whitespace fixture");
        let (status, file) = json(
            &app,
            Request::get(format!(
                "/api/v1/groups/{group}/workspace/file?scope_key=scope_repo&scope_url={scope}&path={encoded}"
            ))
            .body(Body::empty())
            .expect("workspace HTTP whitespace fixture"),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(file["result"]["content"], "selected");
        assert_eq!(file["result"]["path"], path);
        let (status, _) = json(&app, Request::put(format!(
            "/api/v1/groups/{group}/workspace/file"
        )).header(header::CONTENT_TYPE, "application/json").body(Body::from(
            serde_json::json!({"scope_key":"scope_repo", "scope_url":fixture.repo.to_string_lossy(), "path":path,"content":"edited","sha256":file["result"]["sha256"]}).to_string()
        )).expect("workspace HTTP whitespace fixture")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(
            std::fs::read_to_string(fixture.repo.join(path))
                .expect("workspace HTTP whitespace fixture"),
            "edited"
        );
        assert_eq!(
            std::fs::read_to_string(fixture.repo.join("note.txt"))
                .expect("workspace HTTP whitespace fixture"),
            "neighbor"
        );
    }
}

#[tokio::test]
async fn saving_an_open_file_after_group_use_does_not_write_the_new_scope() {
    use cccc_core::{GroupStore, Scope, group_scope};
    let fixture = fixture();
    let app = auth_support::authenticated_app(fixture.home.clone());
    let group = &fixture.group_id;
    let scope = url::form_urlencoded::byte_serialize(fixture.repo.to_string_lossy().as_bytes())
        .collect::<String>();
    let (_, opened) = json(&app, Request::get(format!(
        "/api/v1/groups/{group}/workspace/file?scope_key=scope_repo&scope_url={scope}&path=src%2Flib.rs"
    )).body(Body::empty()).expect("read request")).await;
    let other = tempfile::tempdir().expect("second scope");
    std::fs::create_dir(other.path().join("src")).expect("src");
    std::fs::write(other.path().join("src/lib.rs"), "fn main() {}\n").expect("identical file");
    let store = GroupStore::new(fixture.home.clone()).expect("store");
    group_scope::attach(
        &store,
        group,
        Scope {
            scope_key: "scope_other".into(),
            url: other.path().to_string_lossy().into_owned(),
            label: "other".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach other scope");
    group_scope::activate(&store, group, "scope_repo").expect("activate original");
    group_scope::activate(&store, group, "scope_other").expect("group_use other");
    let (status, result) = json(&app, Request::put(format!("/api/v1/groups/{group}/workspace/file"))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(serde_json::json!({"scope_key":"scope_repo", "scope_url":fixture.repo.to_string_lossy(), "path":"src/lib.rs", "content":"changed", "sha256":opened["result"]["sha256"]}).to_string()))
        .expect("save request")).await;
    assert_eq!(status, StatusCode::CONFLICT, "{result}");
    assert_eq!(result["error"]["code"], "workspace_scope_changed");
    for root in [&fixture.repo, &other.path().to_path_buf()] {
        assert_eq!(
            std::fs::read_to_string(root.join("src/lib.rs")).expect("unchanged file"),
            "fn main() {}\n"
        );
    }
}

#[tokio::test]
async fn scope_identity_is_required_and_cannot_follow_a_relocated_binding() {
    use cccc_core::{GroupStore, Scope, group_scope};
    let fixture = fixture();
    let app = auth_support::authenticated_app(fixture.home.clone());
    let group = &fixture.group_id;
    let scope = url::form_urlencoded::byte_serialize(fixture.repo.to_string_lossy().as_bytes())
        .collect::<String>();
    let file_url = format!("/api/v1/groups/{group}/workspace/file");
    let (status, _) = json(
        &app,
        Request::get(format!("{file_url}?path=src%2Flib.rs"))
            .body(Body::empty())
            .expect("unbound read"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (_, opened) = json(
        &app,
        Request::get(format!(
            "{file_url}?scope_key=scope_repo&scope_url={scope}&path=src%2Flib.rs"
        ))
        .body(Body::empty())
        .expect("bound read"),
    )
    .await;
    assert_eq!(opened["result"]["scope_key"], "scope_repo");
    assert_eq!(
        opened["result"]["scope_url"],
        fixture.repo.to_string_lossy().as_ref()
    );
    let body = serde_json::json!({"path":"src/lib.rs", "content":"edited", "sha256":opened["result"]["sha256"]});
    let (status, _) = json(
        &app,
        Request::put(&file_url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("unbound write"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let other = tempfile::tempdir().expect("relocated scope");
    std::fs::create_dir(other.path().join("src")).expect("src");
    std::fs::write(other.path().join("src/lib.rs"), "fn main() {}\n").expect("same content");
    let store = GroupStore::new(fixture.home.clone()).expect("store");
    group_scope::attach(
        &store,
        group,
        Scope {
            scope_key: "scope_repo".into(),
            url: other.path().to_string_lossy().into_owned(),
            label: "relocated".into(),
            git_remote: String::new(),
        },
    )
    .expect("reattach same identity at another path");
    for suffix in ["list", "file"] {
        let (status, _) = json(&app, Request::get(format!("/api/v1/groups/{group}/workspace/{suffix}?scope_key=scope_repo&scope_url={scope}&path=src"))
            .body(Body::empty()).expect("stale read")).await;
        assert_eq!(status, StatusCode::CONFLICT);
    }
    let mut body = body;
    body["scope_key"] = "scope_repo".into();
    body["scope_url"] = fixture.repo.to_string_lossy().into_owned().into();
    let (status, _) = json(
        &app,
        Request::put(&file_url)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .expect("stale write"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(
        std::fs::read_to_string(other.path().join("src/lib.rs")).expect("neighbor"),
        "fn main() {}\n"
    );
}

#[tokio::test]
async fn workspace_directory_is_not_reported_as_outside_scope() {
    let fixture = fixture();
    std::fs::create_dir_all(fixture.repo.join(".pytest_cache/v/cache")).expect("cache directory");
    let app = auth_support::authenticated_app(fixture.home.clone());
    let scope = url::form_urlencoded::byte_serialize(fixture.repo.to_string_lossy().as_bytes())
        .collect::<String>();
    let (status, payload) = json(
        &app,
        Request::get(format!(
            "/api/v1/groups/{}/workspace/file?scope_key=scope_repo&scope_url={scope}&path=.pytest_cache/v/cache",
            fixture.group_id,
        ))
        .body(Body::empty()).expect("request"),
    ).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{payload}");
    assert_eq!(payload["error"]["code"], "not_a_file");
}

#[tokio::test]
async fn path_lookup_distinguishes_files_directories_missing_and_escaped_paths() {
    let fixture = fixture();
    std::fs::create_dir_all(fixture.repo.join(".pytest_cache/v/cache")).expect("cache directory");
    std::fs::write(fixture.repo.join("literal\\name.txt"), "literal").expect("literal filename");
    std::os::unix::fs::symlink(".pytest_cache/v/cache", fixture.repo.join("alias"))
        .expect("internal symlink");
    std::os::unix::fs::symlink(
        fixture.repo.parent().expect("scope parent"),
        fixture.repo.join("outside"),
    )
    .expect("external symlink");
    let app = auth_support::authenticated_app(fixture.home.clone());
    let scope = url::form_urlencoded::byte_serialize(fixture.repo.to_string_lossy().as_bytes())
        .collect::<String>();
    let base = format!("/api/v1/groups/{}/workspace", fixture.group_id);
    for (path, canonical, is_dir) in [
        ("", "", true),
        (".", "", true),
        (".pytest_cache/v/cache", ".pytest_cache/v/cache", true),
        ("alias", ".pytest_cache/v/cache", true),
        ("src/lib.rs", "src/lib.rs", false),
        ("literal\\name.txt", "literal\\name.txt", false),
    ] {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("scope_key", "scope_repo")
            .append_pair("scope_url", &fixture.repo.to_string_lossy())
            .append_pair("path", path)
            .finish();
        let (status, payload) = json(
            &app,
            Request::get(format!("{base}/path?{query}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{path}: {payload}");
        assert_eq!(payload["result"]["path"], canonical);
        assert_eq!(payload["result"]["is_dir"], is_dir);
        assert_eq!(payload["result"]["scope_key"], "scope_repo");
        assert_eq!(
            payload["result"]["scope_url"],
            fixture.repo.to_string_lossy().as_ref()
        );
    }
    for (route, path, status, code) in [
        ("path", "missing", StatusCode::NOT_FOUND, "NOT_FOUND"),
        (
            "path",
            "../secret.txt",
            StatusCode::FORBIDDEN,
            "outside_scope",
        ),
        ("path", "outside", StatusCode::FORBIDDEN, "outside_scope"),
        (
            "list",
            "src/lib.rs",
            StatusCode::BAD_REQUEST,
            "not_a_directory",
        ),
    ] {
        let (actual, payload) = json(
            &app,
            Request::get(format!(
                "{base}/{route}?scope_key=scope_repo&scope_url={scope}&path={path}"
            ))
            .body(Body::empty())
            .expect("request"),
        )
        .await;
        assert_eq!(actual, status, "{payload}");
        assert_eq!(payload["error"]["code"], code);
    }
    let (status, payload) = json(
        &app,
        Request::get(format!(
            "{base}/path?scope_key=old_scope&scope_url={scope}&path=src"
        ))
        .body(Body::empty())
        .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(payload["error"]["code"], "workspace_scope_changed");

    let tokens =
        cccc_core::access_tokens::AccessTokenStore::new(fixture.home.clone()).expect("token store");
    tokens
        .create(
            "other group",
            vec!["g_other".into()],
            false,
            Some("path-test-restricted"),
        )
        .expect("restricted token");
    let (status, _) = json(
        &app,
        Request::get(format!(
            "{base}/path?scope_key=scope_repo&scope_url={scope}&path=src"
        ))
        .header(header::AUTHORIZATION, "Bearer path-test-restricted")
        .body(Body::empty())
        .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}
