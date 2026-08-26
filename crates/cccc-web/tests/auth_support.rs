use axum::body::Body;
use axum::extract::Request;
use axum::http::header;
use axum::middleware::Next;
use axum::response::Response;
use cccc_core::HomeLayout;
use cccc_core::access_tokens::AccessTokenStore;

pub const TEST_ADMIN_TOKEN: &str = "rust-web-test-admin";

#[allow(dead_code)]
pub fn authenticated_app(home: HomeLayout) -> axum::Router {
    seed_admin(&home);
    cccc_web::app(home).layer(axum::middleware::from_fn(inject_test_admin))
}

#[allow(dead_code)]
pub fn authenticated_app_with_mode(home: HomeLayout, mode: cccc_web::WebMode) -> axum::Router {
    seed_admin(&home);
    cccc_web::app_with_mode(home, mode).layer(axum::middleware::from_fn(inject_test_admin))
}

fn seed_admin(home: &HomeLayout) {
    let store = AccessTokenStore::new(home.clone()).expect("test access token store");
    if store
        .lookup(TEST_ADMIN_TOKEN)
        .expect("test access token lookup")
        .is_none()
    {
        store
            .create(
                "rust-web-test-admin",
                Vec::new(),
                true,
                Some(TEST_ADMIN_TOKEN),
            )
            .expect("test administrator token");
    }
}

async fn inject_test_admin(mut request: Request<Body>, next: Next) -> Response {
    if (request.uri().path().starts_with("/api/v1/")
        || request
            .uri()
            .path()
            .starts_with("/api/group-bridge/pairing/"))
        && !request.headers().contains_key(header::AUTHORIZATION)
    {
        request.headers_mut().insert(
            header::AUTHORIZATION,
            format!("Bearer {TEST_ADMIN_TOKEN}")
                .parse()
                .expect("test authorization header"),
        );
    }
    next.run(request).await
}
