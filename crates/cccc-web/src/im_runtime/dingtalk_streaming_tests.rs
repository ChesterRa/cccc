use super::*;
use axum::extract::{OriginalUri, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{post, put};
use axum::{Json, Router};
use serde_json::json;
use tokio::sync::mpsc;

struct FakeToken;

#[async_trait]
impl DingTalkCardToken for FakeToken {
    async fn access_token(&self) -> Result<String, String> {
        Ok("card-token".into())
    }
}

#[derive(Debug)]
struct CapturedRequest {
    method: &'static str,
    path: String,
    headers: HeaderMap,
    body: Value,
}

#[derive(Clone)]
struct ServerState {
    requests: mpsc::UnboundedSender<CapturedRequest>,
    mode: ServerMode,
}

#[derive(Clone, Copy)]
enum ServerMode {
    Success,
    FailCreates,
    FailDirectCreate,
    FailFinalize,
}

async fn capture_post(
    State(state): State<ServerState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let fail = matches!(state.mode, ServerMode::FailCreates)
        || matches!(state.mode, ServerMode::FailDirectCreate)
            && body["openSpaceId"]
                .as_str()
                .is_some_and(|space| space.contains("IM_ROBOT"));
    state
        .requests
        .send(CapturedRequest {
            method: "POST",
            path: uri.path().into(),
            headers,
            body,
        })
        .expect("capture card create");
    if fail {
        return (
            axum::http::StatusCode::BAD_GATEWAY,
            Json(json!({"message":"card unavailable"})),
        )
            .into_response();
    }
    Json(json!({})).into_response()
}

async fn capture_put(
    State(state): State<ServerState>,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    let fail = matches!(state.mode, ServerMode::FailFinalize)
        && body["isFinalize"].as_bool() == Some(true);
    state
        .requests
        .send(CapturedRequest {
            method: "PUT",
            path: uri.path().into(),
            headers,
            body,
        })
        .expect("capture card update");
    if fail {
        return (
            axum::http::StatusCode::BAD_GATEWAY,
            Json(json!({"message":"finalize unavailable"})),
        )
            .into_response();
    }
    Json(json!({})).into_response()
}

async fn test_streamer(
    mode: ServerMode,
) -> (
    DingTalkCardStreamer,
    mpsc::UnboundedReceiver<CapturedRequest>,
) {
    let (tx, rx) = mpsc::unbounded_channel();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let address = listener.local_addr().expect("address");
    tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route(CREATE_ENDPOINT, post(capture_post))
                .route(STREAM_ENDPOINT, put(capture_put))
                .with_state(ServerState { requests: tx, mode }),
        )
        .await
        .expect("server");
    });
    (
        DingTalkCardStreamer {
            token: Arc::new(FakeToken),
            http: reqwest::Client::new(),
            openapi_base: format!("http://{address}"),
            robot_code: "fallback-robot".into(),
            active: Mutex::new(HashMap::new()),
            completed: Mutex::new(HashSet::new()),
            throttle: Duration::from_millis(300),
        },
        rx,
    )
}

fn target(
    conversation_type: &str,
    chat_id: &str,
    user_id: &str,
    robot_code: &str,
) -> DingTalkTarget {
    DingTalkTarget {
        chat_id: chat_id.into(),
        robot_code: robot_code.into(),
        conversation_type: conversation_type.into(),
        user_id: user_id.into(),
    }
}

fn stream_event(op: &str, stream_id: &str, text: &str) -> Event {
    let mut event = Event::new("chat.stream", "group");
    event.by = "assistant".into();
    event.data.insert("op".into(), json!(op));
    event.data.insert("stream_id".into(), json!(stream_id));
    event.data.insert("text".into(), json!(text));
    event
}

#[tokio::test]
async fn creates_group_and_direct_cards_with_the_expected_routes() {
    let (streamer, mut requests) = test_streamer(ServerMode::Success).await;
    streamer
        .send(
            &[
                target("2", "cid-group", "", "callback-robot"),
                target("1", "direct-conversation", "staff-1", ""),
            ],
            &stream_event("start", "stream-1", "hello"),
        )
        .await;

    let group = requests.recv().await.expect("group create");
    assert_eq!(group.method, "POST");
    assert_eq!(group.path, CREATE_ENDPOINT);
    assert_eq!(group.headers["x-acs-dingtalk-access-token"], "card-token");
    assert_eq!(group.body["cardTemplateId"], AI_CARD_TEMPLATE_ID);
    assert_eq!(group.body["callbackType"], "STREAM");
    assert_eq!(group.body["openSpaceId"], "dtv1.card//IM_GROUP.cid-group");
    assert_eq!(
        group.body["imGroupOpenDeliverModel"]["robotCode"],
        "callback-robot"
    );
    assert_eq!(
        group.body["cardData"]["cardParamMap"]["msgContent"],
        "**assistant**\n\nhello"
    );
    assert_eq!(group.body["outTrackId"].as_str().map(str::len), Some(32));

    let direct = requests.recv().await.expect("direct create");
    assert_eq!(direct.method, "POST");
    assert_eq!(direct.path, CREATE_ENDPOINT);
    assert_eq!(direct.body["openSpaceId"], "dtv1.card//IM_ROBOT.staff-1");
    assert_eq!(
        direct.body["imRobotOpenDeliverModel"]["spaceType"],
        "IM_ROBOT"
    );
}

#[tokio::test]
async fn updates_are_throttled_but_finalize_is_always_sent_and_deduplicated_once() {
    let (streamer, mut requests) = test_streamer(ServerMode::Success).await;
    let targets = [target("2", "cid-group", "", "")];
    streamer
        .send(&targets, &stream_event("start", "stream-1", "a"))
        .await;
    let create = requests.recv().await.expect("create");
    let out_track_id = create.body["outTrackId"]
        .as_str()
        .expect("outTrackId")
        .to_owned();

    streamer
        .send(&targets, &stream_event("update", "stream-1", "ab"))
        .await;
    let update = requests.recv().await.expect("first update");
    assert_eq!(update.method, "PUT");
    assert_eq!(update.path, STREAM_ENDPOINT);
    assert_eq!(update.body["outTrackId"], out_track_id);
    assert_eq!(update.body["content"], "**assistant**\n\nab");
    assert_eq!(update.body["isFull"], true);
    assert_eq!(update.body["isFinalize"], false);

    streamer
        .send(&targets, &stream_event("update", "stream-1", "abc"))
        .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(30), requests.recv())
            .await
            .is_err(),
        "rapid intermediate snapshots should be dropped"
    );

    streamer
        .send(&targets, &stream_event("end", "stream-1", "final"))
        .await;
    let end = requests.recv().await.expect("finalize");
    assert_eq!(end.method, "PUT");
    assert_eq!(end.body["outTrackId"], out_track_id);
    assert_eq!(end.body["content"], "**assistant**\n\nfinal");
    assert_eq!(end.body["isFinalize"], true);

    assert_eq!(
        streamer.take_completed_targets("stream-1"),
        HashSet::from(["cid-group".to_owned()])
    );
    assert!(streamer.take_completed_targets("stream-1").is_empty());
}

#[tokio::test]
async fn failed_create_leaves_final_message_fallback_enabled() {
    let (streamer, mut requests) = test_streamer(ServerMode::FailCreates).await;
    let targets = [target("2", "cid-group", "", "callback-robot")];
    streamer
        .send(&targets, &stream_event("start", "stream-1", "a"))
        .await;
    assert_eq!(requests.recv().await.expect("failed create").method, "POST");

    streamer
        .send(&targets, &stream_event("update", "stream-1", "ab"))
        .await;
    streamer
        .send(&targets, &stream_event("end", "stream-1", "final"))
        .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(30), requests.recv())
            .await
            .is_err()
    );
    assert!(streamer.take_completed_targets("stream-1").is_empty());
}

#[tokio::test]
async fn one_failed_target_does_not_disable_other_cards_or_its_own_fallback() {
    let (streamer, mut requests) = test_streamer(ServerMode::FailDirectCreate).await;
    let targets = [
        target("2", "cid-group", "", "callback-robot"),
        target("1", "direct-chat", "staff-1", ""),
    ];
    streamer
        .send(&targets, &stream_event("start", "stream-1", "a"))
        .await;
    assert_eq!(requests.recv().await.expect("group create").method, "POST");
    assert_eq!(requests.recv().await.expect("direct create").method, "POST");

    streamer
        .send(&targets, &stream_event("end", "stream-1", "final"))
        .await;
    let finalize = requests.recv().await.expect("group finalize");
    assert_eq!(finalize.method, "PUT");
    assert_eq!(
        streamer.take_completed_targets("stream-1"),
        HashSet::from(["cid-group".to_owned()])
    );
}

#[tokio::test]
async fn failed_finalize_keeps_the_completed_message_fallback_enabled() {
    let (streamer, mut requests) = test_streamer(ServerMode::FailFinalize).await;
    let targets = [target("2", "cid-group", "", "callback-robot")];
    streamer
        .send(&targets, &stream_event("start", "stream-1", "a"))
        .await;
    assert_eq!(requests.recv().await.expect("create").method, "POST");
    streamer
        .send(&targets, &stream_event("end", "stream-1", "final"))
        .await;
    assert_eq!(
        requests.recv().await.expect("failed finalize").method,
        "PUT"
    );
    assert!(streamer.take_completed_targets("stream-1").is_empty());
}

#[tokio::test]
async fn normalized_card_text_keeps_the_exact_final_message_fallback_enabled() {
    let (streamer, mut requests) = test_streamer(ServerMode::Success).await;
    let targets = [target("2", "cid-group", "", "callback-robot")];
    streamer
        .send(&targets, &stream_event("start", "stream-1", "a"))
        .await;
    assert_eq!(requests.recv().await.expect("create").method, "POST");

    streamer
        .send(
            &targets,
            &stream_event("end", "stream-1", "first\tline\r\nlast"),
        )
        .await;
    let finalize = requests.recv().await.expect("finalize");
    assert_eq!(
        finalize.body["content"],
        "**assistant**\n\nfirst  line\nlast"
    );
    assert!(streamer.take_completed_targets("stream-1").is_empty());
}

#[test]
fn card_text_is_normalized_and_utf8_safe() {
    assert_eq!(
        prepare_stream_text("\r\nfirst\tline\r\n\r\n\r\nlast\r\n"),
        "first  line\n\nlast"
    );
    let long = "你".repeat(5_000);
    let prepared = prepare_stream_text(&long);
    assert_eq!(prepared.chars().count(), 4_096);
    assert!(prepared.ends_with('…'));
}
