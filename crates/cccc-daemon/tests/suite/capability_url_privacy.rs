use base64::{Engine, engine::general_purpose::STANDARD};
use cccc_core::{GroupStore, HomeLayout, fs};
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use super::capability_install_events_support::{call, response};

const USERNAME: &str = "fixture_username_secret";
const PASSWORD: &str = "fixture_password_secret";
const QUERY: &str = "fixture_query_secret";
const FRAGMENT: &str = "fixture_fragment_secret";

fn fixture(
    status: &'static str,
    body: &'static str,
) -> (u16, TcpListener, std::thread::JoinHandle<(bool, bool)>) {
    let held = TcpListener::bind("127.0.0.1:0").expect("fixture listener");
    let port = held.local_addr().expect("address").port();
    let listener = held.try_clone().expect("listener clone");
    listener
        .set_nonblocking(true)
        .expect("nonblocking listener");
    let task = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("fixture accept: {error}"),
            }
        };
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .expect("read timeout");
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 2048];
        while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
            let count = stream.read(&mut buffer).expect("fixture request");
            assert!(count > 0 && bytes.len() + count <= 16 * 1024);
            bytes.extend_from_slice(&buffer[..count]);
        }
        let head = String::from_utf8_lossy(&bytes);
        let expected_basic = format!(
            "Basic {}",
            STANDARD.encode(format!("{USERNAME}:{PASSWORD}"))
        );
        let had_basic = head.lines().any(|line| {
            line.split_once(':').is_some_and(|(name, value)| {
                name.eq_ignore_ascii_case("authorization") && value.trim() == expected_basic
            })
        });
        let had_query = head.lines().next().is_some_and(|line| line.contains(QUERY));
        write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .expect("fixture response");
        (had_basic, had_query)
    });
    (port, held, task)
}

fn no_private_url_parts(value: &impl serde::Serialize) {
    let encoded = serde_json::to_string(value).expect("JSON");
    assert!(
        !encoded.contains(USERNAME),
        "username appeared in public output"
    );
    assert!(
        !encoded.contains(PASSWORD),
        "password appeared in public output"
    );
    assert!(!encoded.contains(QUERY), "query appeared in public output");
    assert!(
        !encoded.contains(FRAGMENT),
        "fragment appeared in public output"
    );
}

#[test]
fn skill_url_install_and_legacy_views_hide_url_credentials() {
    let temp = tempfile::tempdir().expect("temporary home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let groups = GroupStore::new(home.clone()).expect("groups");
    let owner = groups.create("owner", "").expect("group");
    let other = groups.create("other", "").expect("group");
    let body = "---\nname: private-url-fixture\ndescription: Fixture\n---\nFixture procedure.\n";
    let (port, _held, server) = fixture("200 OK", body);
    let target =
        format!("http://{USERNAME}:{PASSWORD}@127.0.0.1:{port}/SKILL.md?token={QUERY}#{FRAGMENT}");
    let public = format!("http://127.0.0.1:{port}/SKILL.md");
    let installed = call(
        &home,
        "capability_install_target",
        json!({
            "group_id":owner.group_id,"target":target,"scope":"group","by":"user"
        }),
    );
    assert_eq!(server.join().expect("fixture server"), (true, true));
    assert_eq!(installed["target"], public);
    no_private_url_parts(&installed);
    let catalog_path = home.root().join("state/capabilities/catalog.json");
    let mut catalog: Value = fs::read_json(&catalog_path).expect("catalog");
    no_private_url_parts(&catalog);

    // An old record is left on disk, but every ordinary catalog view is safe.
    catalog["records"]["skill:url:old-fixture"] = json!({
        "capability_id":"skill:url:old-fixture","kind":"skill","name":"old-fixture",
        "source_id":"url_import","source_uri":target,"source_record_id":target,
        "capsule_text":"Old fixture text.","qualification_status":"qualified","enable_supported":true
    });
    fs::write_json(&catalog_path, &catalog).expect("write old catalog fixture");
    let args = json!({"group_id":other.group_id,"actor_id":"user","by":"user"});
    let overview = call(&home, "capability_overview", args.clone());
    no_private_url_parts(&overview);
    let row = overview["items"]
        .as_array()
        .expect("overview items")
        .iter()
        .find(|row| row["capability_id"] == "skill:url:old-fixture")
        .expect("old overview row");
    assert_eq!(row["source_uri"], public);
    let search = call(&home, "capability_search", args.clone());
    no_private_url_parts(&search);
    let row = search["capabilities"]
        .as_array()
        .expect("search results")
        .iter()
        .find(|row| row["id"] == "skill:url:old-fixture")
        .expect("old search result");
    assert_eq!(row["source_uri"], public);

    call(
        &home,
        "capability_enable",
        json!({"group_id":other.group_id,"by":"user",
        "scope":"group","capability_id":"skill:url:old-fixture"}),
    );
    let state = call(&home, "capability_state", args);
    no_private_url_parts(&state);
    let row = state["active_capsule_skills"]
        .as_array()
        .expect("active skills")
        .iter()
        .find(|row| row["capability_id"] == "skill:url:old-fixture")
        .expect("old active skill");
    assert_eq!(row["source_uri"], public);
    let still_on_disk: Value = fs::read_json(&catalog_path).expect("old catalog");
    assert!(
        serde_json::to_string(&still_on_disk)
            .expect("JSON")
            .contains(PASSWORD)
    );
}

#[test]
fn skill_import_results_and_failed_downloads_hide_url_credentials() {
    let temp = tempfile::tempdir().expect("temporary home");
    let home = HomeLayout::from_path(temp.path().join("home")).expect("home");
    let group = GroupStore::new(home.clone())
        .expect("groups")
        .create("owner", "")
        .expect("group");
    let source =
        format!("http://{USERNAME}:{PASSWORD}@example.test/SKILL.md?token={QUERY}#{FRAGMENT}");
    let record = json!({"capability_id":"skill:test:private-url","kind":"skill","name":"fixture",
        "source_uri":source,"source_record_id":source,"capsule_text":"Fixture procedure."});
    for dry_run in [true, false] {
        let result = call(
            &home,
            "capability_import",
            json!({"group_id":group.group_id,"by":"user",
            "scope":"group","record":record,"dry_run":dry_run,"probe":false}),
        );
        no_private_url_parts(&result);
        assert_eq!(
            result["record"]["source_uri"],
            "http://example.test/SKILL.md"
        );
        assert_eq!(
            result["record"]["source_record_id"],
            "http://example.test/SKILL.md"
        );
    }
    let catalog: Value =
        fs::read_json(&home.root().join("state/capabilities/catalog.json")).expect("catalog");
    no_private_url_parts(&catalog);
    assert_eq!(
        catalog["records"]["skill:test:private-url"]["source_uri"],
        "http://example.test/SKILL.md"
    );
    assert_eq!(
        catalog["records"]["skill:test:private-url"]["source_record_id"],
        "http://example.test/SKILL.md"
    );

    let (port, _held, server) = fixture("403 Forbidden", "forbidden");
    let target =
        format!("http://{USERNAME}:{PASSWORD}@127.0.0.1:{port}/SKILL.md?token={QUERY}#{FRAGMENT}");
    let failed = response(
        &home,
        "capability_install_target",
        json!({"group_id":group.group_id,
        "target":target,"scope":"group","by":"user"}),
    );
    assert!(!failed.ok, "fixture download should fail");
    assert_eq!(server.join().expect("fixture server"), (true, true));
    no_private_url_parts(&failed);
}
