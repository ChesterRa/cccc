use super::*;

#[tokio::test]
async fn volcengine_does_not_wait_for_audio_dependent_ack_and_finishes_on_negative_sequence() {
    for legacy in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let endpoint = format!("ws://{}", listener.local_addr().expect("address"));
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.expect("accept");
            let mut socket = tokio_tungstenite::accept_hdr_async(
                stream,
                move |request: &Request, response: Response| {
                    assert_eq!(
                        request.headers()["x-api-resource-id"],
                        "volc.seedasr.sauc.duration"
                    );
                    if legacy {
                        assert_eq!(request.headers()["x-api-app-key"], "test-app");
                        assert_eq!(request.headers()["x-api-access-key"], "test-access");
                        assert!(!request.headers().contains_key("x-api-key"));
                    } else {
                        assert_eq!(request.headers()["x-api-key"], "test-volc-key");
                    }
                    Ok(response)
                },
            )
            .await
            .expect("upgrade");
            let init = socket
                .next()
                .await
                .expect("init")
                .expect("frame")
                .into_data();
            assert_eq!(&init[..4], &[0x11, 0x10, 0x11, 0]);
            let body: Value = serde_json::from_reader(flate2::read::GzDecoder::new(&init[8..]))
                .expect("init payload");
            assert_eq!(body["audio"]["rate"], 16000);
            assert_eq!(body["request"]["show_utterances"], true);
            assert_eq!(body["request"]["result_type"], "single");
            // Optimized streams may not acknowledge init until audio arrives.
            let audio = socket
                .next()
                .await
                .expect("audio")
                .expect("frame")
                .into_data();
            assert_eq!(&audio[..4], &[0x11, 0x20, 0x01, 0]);
            socket
                .send(volc_reply("早", 0, false, false))
                .await
                .expect("partial");
            socket
                .send(volc_reply("早上好", 0, true, false))
                .await
                .expect("stable first sentence");
            let last = socket
                .next()
                .await
                .expect("last packet")
                .expect("frame")
                .into_data();
            assert_eq!(&last[..4], &[0x11, 0x22, 0x01, 0]);
            socket
                .send(volc_reply("世界", 600, true, true))
                .await
                .expect("final");
        });
        let mut cfg = config("test-volc-key");
        if legacy {
            cfg.auth_mode = "app_token".into();
            cfg.app_id = "test-app".into();
            cfg.access_token = "test-access".into();
        }
        let opened = tokio::time::timeout(
            Duration::from_secs(2),
            connection::connect_at(Provider::Volcengine, cfg, "auto", &endpoint),
        )
        .await
        .expect("must not wait for init ack")
        .expect("connect");
        let (_temp, home) = home();
        let mut active =
            active::Active::from_opened(&home, &json!({"capture_mode":"prompt"}), opened)
                .expect("active");
        active.audio(&vec![0; 6400]).await.expect("audio");
        let partial = active.reader.next().await.expect("partial").expect("frame");
        assert_eq!(
            active.receive(partial).expect("parse")[0]["type"],
            "partial"
        );
        let first_final = active
            .reader
            .next()
            .await
            .expect("first final")
            .expect("frame");
        assert_eq!(
            active.receive(first_final).expect("parse")[0]["text"],
            "早上好"
        );
        active.stop(json!(2)).await.expect("finish");
        let final_message = active.reader.next().await.expect("final").expect("frame");
        assert_eq!(
            active.receive(final_message).expect("parse")[0]["text"],
            "世界"
        );
        assert!(active.completed);
        assert_eq!(active.transcript.text(), "早上好\n世界");
        server.await.expect("server");
    }
}

pub(super) fn volc_reply(text: &str, start: u64, finalized: bool, last: bool) -> Message {
    let payload = json!({"result":{"text":text,"utterances":[{
        "text":text,"start_time":start,"end_time":start+500,"definite":finalized
    }]}})
    .to_string();
    let mut frame = vec![0x11, if last { 0x93 } else { 0x91 }, 0x10, 0];
    frame.extend(if last { -2i32 } else { 1i32 }.to_be_bytes());
    frame.extend((payload.len() as u32).to_be_bytes());
    frame.extend(payload.as_bytes());
    Message::Binary(frame.into())
}

#[test]
fn gzip_responses_are_decoded_with_a_decompression_limit() {
    use flate2::{Compression, write::GzEncoder};
    use std::io::Write;
    let encode = |body: &[u8]| {
        let mut gzip = GzEncoder::new(Vec::new(), Compression::fast());
        gzip.write_all(body).expect("compress");
        let data = gzip.finish().expect("gzip");
        let mut frame = vec![0x11, 0x92, 0x11, 0];
        frame.extend((data.len() as u32).to_be_bytes());
        frame.extend(data);
        frame
    };
    let frame=encode(br#"{"result":{"text":"hello","utterances":[{"text":"hello","start_time":0,"end_time":10,"definite":true}]}}"#);
    let (event, done) = volcengine::parse(&frame).expect("decode");
    assert!(done);
    assert!(matches!(event, transcript::Event::Update { .. }));
    let oversized = encode(&vec![b' '; volcengine::MAX_REPLY_BYTES + 1]);
    assert!(
        volcengine::parse(&oversized).is_err(),
        "compressed data cannot bypass the reply limit"
    );
}
