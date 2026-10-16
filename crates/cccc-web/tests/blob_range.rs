#![cfg(unix)]
mod auth_support;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cccc_core::access_tokens::AccessTokenStore;
use cccc_core::{GroupStore, HomeLayout};
use http_body_util::BodyExt;
use tower::ServiceExt;

// An ISO base media header followed by payload bytes.
const MP4: &[u8] = b"\0\0\0\x20ftypisom\0\0\x02\0isomiso2mp41-payload-bytes";

#[tokio::test]
async fn video_blobs_play_inline_with_byte_ranges() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("video", "")
        .expect("group");
    let blob = cccc_core::blobs::store(&home, &group.group_id, MP4).expect("blob");
    let url = format!(
        "/api/v1/groups/{}/blobs/{}?filename=clip.mp4",
        group.group_id, blob.sha256
    );
    let send = |request: axum::http::request::Builder| {
        auth_support::authenticated_app(home.clone())
            .oneshot(request.body(Body::empty()).expect("request"))
    };
    let get = |range: Option<&'static str>| {
        let mut request = Request::get(&url);
        if let Some(range) = range {
            request = request.header(header::RANGE, range);
        }
        send(request)
    };

    let full = get(None).await.expect("full response");
    assert_eq!(full.status(), StatusCode::OK);
    assert_eq!(full.headers()[header::CONTENT_TYPE], "video/mp4");
    // Access may be revoked while the bytes stay the same: always revalidate.
    assert_eq!(full.headers()[header::CACHE_CONTROL], "private, no-cache");
    assert_eq!(full.headers()[header::ACCEPT_RANGES], "bytes");
    assert!(!full.headers().contains_key(header::CONTENT_DISPOSITION));
    assert_eq!(
        full.into_body().collect().await.expect("body").to_bytes(),
        MP4
    );

    let partial = get(Some("bytes=4-11")).await.expect("partial response");
    assert_eq!(partial.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        partial.headers()[header::CONTENT_RANGE],
        format!("bytes 4-11/{}", MP4.len()).as_str()
    );
    assert_eq!(
        partial.headers()[header::CACHE_CONTROL],
        "private, no-cache"
    );
    assert_eq!(partial.headers()[header::CONTENT_LENGTH], "8");
    assert_eq!(partial.headers()[header::CONTENT_TYPE], "video/mp4");
    assert_eq!(
        partial
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
        &MP4[4..12]
    );

    let tail = get(Some("bytes=-5")).await.expect("suffix response");
    assert_eq!(tail.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        tail.into_body().collect().await.expect("body").to_bytes(),
        &MP4[MP4.len() - 5..]
    );

    // Blobs carry no strong validator, so a conditional range gets everything.
    let conditional = send(
        Request::get(&url)
            .header(header::RANGE, "bytes=4-11")
            .header(header::IF_RANGE, "\"stale\""),
    )
    .await
    .expect("conditional response");
    assert_eq!(conditional.status(), StatusCode::OK);
    assert_eq!(
        conditional
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes(),
        MP4
    );

    let head = send(Request::head(&url)).await.expect("head response");
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()[header::CONTENT_TYPE], "video/mp4");
    assert_eq!(head.headers()[header::ACCEPT_RANGES], "bytes");
    assert!(
        head.into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .is_empty()
    );

    // Revalidation answers 304 and still forbids unchecked reuse.
    let modified = send(Request::get(&url)).await.expect("validator response");
    let last_modified = modified.headers()[header::LAST_MODIFIED].clone();
    let revalidated = send(Request::get(&url).header(header::IF_MODIFIED_SINCE, last_modified))
        .await
        .expect("revalidated response");
    assert_eq!(revalidated.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        revalidated.headers()[header::CACHE_CONTROL],
        "private, no-cache"
    );

    // A suffix longer than the file asks for all of it.
    let whole = get(Some("bytes=-999999"))
        .await
        .expect("long suffix response");
    assert!(whole.status().is_success(), "{}", whole.status());
    assert_eq!(
        whole.into_body().collect().await.expect("body").to_bytes(),
        MP4
    );

    // HEAD describes the whole representation even when a range is asked for.
    let ranged_head = send(Request::head(&url).header(header::RANGE, "bytes=0-3"))
        .await
        .expect("ranged head response");
    assert_eq!(ranged_head.status(), StatusCode::OK);
    assert_eq!(
        ranged_head.headers()[header::CONTENT_LENGTH],
        MP4.len().to_string().as_str()
    );

    let beyond = get(Some("bytes=1000-"))
        .await
        .expect("unsatisfiable response");
    assert_eq!(beyond.status(), StatusCode::RANGE_NOT_SATISFIABLE);
    assert_eq!(
        beyond.headers()[header::CONTENT_RANGE],
        format!("bytes */{}", MP4.len()).as_str()
    );
}

#[tokio::test]
async fn an_empty_blob_answers_a_suffix_range_with_its_empty_body() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("empty", "")
        .expect("group");
    let blob = cccc_core::blobs::store(&home, &group.group_id, b"").expect("blob");
    let response = auth_support::authenticated_app(home.clone())
        .oneshot(
            Request::get(format!(
                "/api/v1/groups/{}/blobs/{}",
                group.group_id, blob.sha256
            ))
            .header(header::RANGE, "bytes=-1")
            .body(Body::empty())
            .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .into_body()
            .collect()
            .await
            .expect("body")
            .to_bytes()
            .is_empty()
    );
}

#[tokio::test]
async fn cached_and_partial_blob_reads_recheck_current_group_access() {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = HomeLayout::from_path(temp.path().join("rust-home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("video", "")
        .expect("group");
    let blob = cccc_core::blobs::store(&home, &group.group_id, MP4).expect("blob");
    let tokens = AccessTokenStore::new(home.clone()).expect("tokens");
    let token = tokens
        .create("viewer", vec![group.group_id.clone()], false, None)
        .expect("viewer");
    let url = format!(
        "/api/v1/groups/{}/blobs/{}?filename=clip.mp4",
        group.group_id, blob.sha256
    );
    let app = cccc_web::app(home);
    let send = |request: axum::http::request::Builder| {
        app.clone().oneshot(
            request
                .header(header::AUTHORIZATION, format!("Bearer {}", token.token))
                .body(Body::empty())
                .expect("request"),
        )
    };
    let admitted = send(Request::get(&url).header(header::RANGE, "bytes=4-11"))
        .await
        .expect("partial response");
    assert_eq!(admitted.status(), StatusCode::PARTIAL_CONTENT);
    let last_modified = admitted.headers()[header::LAST_MODIFIED].clone();
    tokens
        .update(&token.token_id(), Some(Vec::new()), None)
        .expect("revoke Group access")
        .expect("viewer remains registered");

    for request in [
        Request::get(&url).header(header::RANGE, "bytes=12-"),
        Request::get(&url).header(header::IF_MODIFIED_SINCE, last_modified),
        Request::head(&url),
    ] {
        let denied = send(request).await.expect("denied response");
        assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    }
}
