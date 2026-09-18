use axum::body::Body;
use axum::http::{Request, StatusCode};
use cccc_core::{GroupStore, HomeLayout, Scope, group_scope};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

pub struct Fixture {
    _temp: tempfile::TempDir,
    pub repo: std::path::PathBuf,
    pub home: HomeLayout,
    pub group_id: String,
}

pub fn fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    // Scope URLs are compared byte-for-byte by the routes, so the fixture path
    // must already be canonical (macOS temp dirs live behind a /var symlink).
    let repo = temp
        .path()
        .canonicalize()
        .expect("canonical tempdir")
        .join("repo");
    std::fs::create_dir_all(repo.join("src")).expect("src");
    std::fs::write(repo.join("src/lib.rs"), "fn main() {}\n").expect("lib");
    std::fs::write(repo.join("secret.txt"), "not in the repo\n").expect("decoy");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let group = groups.create("workspace", "").expect("group");
    group_scope::attach(
        &groups,
        &group.group_id,
        Scope {
            scope_key: "scope_repo".into(),
            url: repo.to_string_lossy().into_owned(),
            label: "repo".into(),
            git_remote: String::new(),
        },
    )
    .expect("attach");
    Fixture {
        _temp: temp,
        repo,
        home,
        group_id: group.group_id,
    }
}

pub fn listed(payload: &Value) -> Vec<String> {
    payload["result"]["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["name"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

pub async fn json(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.expect("response");
    let status = response.status();
    let body = response
        .into_body()
        .collect()
        .await
        .expect("body")
        .to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}
