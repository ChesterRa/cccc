use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;

fn call(root: &Path, action: &str, args: Value) -> Value {
    crate::repo::call(root, action, args.as_object().expect("arguments")).expect("inspection")
}
fn write(root: &Path, path: &str, text: impl AsRef<[u8]>) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, text).expect("file");
}
fn error(root: &Path, action: &str, args: Value) -> String {
    crate::repo::call(root, action, args.as_object().expect("arguments")).expect_err("reject")
}

#[test]
fn scoped_search_filters_case_regex_and_context_match_the_catalog() {
    let t = tempfile::tempdir().expect("workspace");
    let root = t.path();
    write(root, "src/a.rs", "before\nNeedle ONE\nafter\n");
    write(root, "src/b.ts", "Needle TWO\n");
    write(root, "docs/a.rs", "Needle THREE\n");
    write(root, "src/.hidden.rs", "Needle hidden\n");
    let a = call(
        root,
        "search",
        json!({"query":"needle","path":"src","include_globs":["**/*.rs"],"context_lines":1}),
    );
    assert_eq!(a["hits"].as_array().expect("hits").len(), 1);
    assert_eq!(
        a["hits"][0],
        json!({"path":"src/a.rs","line":2,"text":"Needle ONE","before":["before"],"after":["after"]})
    );
    assert_eq!(a["truncated"], false);
    assert_eq!(a["incomplete"], false);
    let b = call(
        root,
        "search",
        json!({"query":"Needle","include_globs":["src/**"],"exclude_globs":["**/*.ts"],"include_hidden":true}),
    );
    assert_eq!(b["hits"].as_array().expect("hits").len(), 2);
    let c = call(
        root,
        "search",
        json!({"query":"needle","case_sensitive":true}),
    );
    assert_eq!(c["hits"], json!([]));
    let d = call(
        root,
        "search",
        json!({"query":"needle [a-z]+","regex":true,"file_path":"src/a.rs"}),
    );
    assert_eq!(d["hits"].as_array().expect("hits").len(), 1);
    assert!(error(root, "search", json!({"query":"[","regex":true})).contains("regex"));
}

#[test]
fn globs_are_workspace_relative_with_portable_separator_and_literal_case() {
    let t = tempfile::tempdir().expect("workspace");
    let r = t.path();
    write(r, "src/top.rs", "x");
    write(r, "src/sub/deep.rs", "x");
    write(r, "src/UPPER.RS", "x");
    let a = call(
        r,
        "search",
        json!({"query":"x","path":"src","include_globs":["src/*.rs"]}),
    );
    assert_eq!(a["hits"].as_array().expect("hits").len(), 1);
    let b = call(
        r,
        "search",
        json!({"query":"x","path":"src","include_globs":["**/*.{rs,RS}"],"exclude_globs":["src/sub/**"]}),
    );
    assert_eq!(b["hits"].as_array().expect("hits").len(), 2);
    assert!(
        error(r, "search", json!({"query":"x","include_globs":["[bad"]}))
            .contains("invalid include_globs")
    );
}

#[test]
fn search_limits_report_actual_truncation_and_never_claim_skipped_text_absent() {
    let t = tempfile::tempdir().expect("workspace");
    let r = t.path();
    write(r, "a", "needle\nneedle\n");
    write(r, "b", "needle\n");
    let a = call(r, "search", json!({"query":"needle","limit":1}));
    assert_eq!(a["hits"].as_array().expect("hits").len(), 1);
    assert_eq!(a["truncated_reason"], "limit");
    assert_eq!(a["incomplete"], true);
    let exact = call(r, "search", json!({"query":"needle","path":"b","limit":1}));
    assert_eq!(exact["truncated"], false);
    let one_size = serde_json::to_vec(&a["hits"][0]).expect("hit").len();
    let b = call(r, "search", json!({"query":"needle","max_bytes":one_size}));
    assert_eq!(b["hits"].as_array().expect("hits").len(), 1);
    assert_eq!(b["truncated_reason"], "max_bytes");
    write(r, "oversize", "needle".repeat(10));
    write(r, "binary", [0_u8, 1, 2]);
    write(r, "bad-utf8", [255_u8]);
    let c = call(r, "search", json!({"query":"missing","max_file_bytes":16}));
    assert_eq!(c["hits"], json!([]));
    assert_eq!(c["incomplete"], true);
    assert_eq!(c["skipped_files"]["oversized"], 1);
    assert_eq!(c["skipped_files"]["non_text"], 2);
    assert!(error(r, "search", json!({"query":"needle","max_bytes":1})).contains("increase it"));
}

#[test]
fn search_default_exclusions_and_explicit_paths_agree() {
    let t = tempfile::tempdir().expect("workspace");
    let r = t.path();
    for p in [".git/a", "target/a", "node_modules/a", ".hidden/a", "src/a"] {
        write(r, p, "needle");
    }
    let a = call(r, "search", json!({"query":"needle"}));
    assert_eq!(a["hits"].as_array().expect("hits").len(), 1);
    let b = call(r, "search", json!({"query":"needle","include_hidden":true}));
    assert_eq!(b["hits"].as_array().expect("hits").len(), 2);
    for p in ["target", ".hidden", ".git"] {
        let c = call(r, "search", json!({"query":"needle","path":p}));
        assert_eq!(c["hits"].as_array().expect("hits").len(), 1);
    }
}

#[test]
fn directory_pages_are_sorted_bounded_and_depth_is_explicit() {
    let t = tempfile::tempdir().expect("workspace");
    let r = t.path();
    for p in ["z.txt", "a.txt", "sub/b.txt", "sub/deep/c.txt", ".hidden"] {
        write(r, p, "x");
    }
    let page = call(r, "list", json!({"limit":2}));
    assert_eq!(
        page["entries"],
        json!([{"name":"a.txt","path":"a.txt","kind":"file"},{"name":"sub","path":"sub","kind":"dir"}])
    );
    assert_eq!(page["next_offset"], 3);
    assert_eq!(page["truncated"], true);
    let last = call(r, "list", json!({"limit":2,"offset":3}));
    assert_eq!(last["entries"][0]["name"], "z.txt");
    assert_eq!(last["next_offset"], Value::Null);
    let all = call(r, "list_dir", json!({"depth":2,"include_hidden":true}));
    assert_eq!(all["entries"].as_array().expect("entries").len(), 6);
    assert!(
        !all["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .any(|e| e["name"] == "c.txt")
    );
    let scoped = call(
        r,
        "list_dir",
        json!({"file_path":"sub","depth":2,"include_globs":["**/*.txt"]}),
    );
    assert_eq!(scoped["entries"].as_array().expect("entries").len(), 2);
    let size = serde_json::to_vec(&page["entries"][0])
        .expect("entry")
        .len();
    let budget = call(r, "list", json!({"max_bytes":size}));
    assert_eq!(budget["entries"].as_array().expect("entries").len(), 1);
    assert_eq!(budget["next_offset"], 2);
    assert!(error(r, "list", json!({"max_bytes":1})).contains("directory entry"));
}

#[test]
fn reads_hash_the_whole_file_and_explain_partial_lines_without_skipping_them() {
    let t = tempfile::tempdir().expect("workspace");
    let r = t.path();
    let source = "first\r\n日本語abcdef\r\nlast";
    write(r, "a", source);
    let full = call(
        r,
        "read",
        json!({"file_path":"a","start_line":2,"end_line":2}),
    );
    assert_eq!(full["content"], "日本語abcdef");
    assert_eq!(full["end_line"], 2);
    assert_eq!(full["total_lines"], 3);
    assert_eq!(
        full["sha256"],
        format!("{:x}", Sha256::digest(source.as_bytes()))
    );
    let clipped = call(r, "read", json!({"path":"a","start_line":2,"max_bytes":8}));
    assert_eq!(clipped["content"], "日本");
    assert_eq!(clipped["truncated"], true);
    assert_eq!(clipped["next_start_line"], 2);
    assert_eq!(clipped["partial_last_line"], true);
    let boundary = call(r, "read", json!({"path":"a","max_bytes":7}));
    assert_eq!(boundary["content"], "first");
    assert_eq!(boundary["next_start_line"], 2);
    assert_eq!(boundary["partial_last_line"], false);
    assert_eq!(boundary["sha256"], full["sha256"]);
    let end = call(r, "read", json!({"path":"a","start_line":99}));
    assert_eq!(end["content"], "");
    assert_eq!(end["truncated"], false);
    assert!(
        error(r, "read", json!({"path":"a","start_line":2,"max_bytes":1}))
            .contains("UTF-8 character")
    );
    write(r, "empty", "");
    assert_eq!(call(r, "read", json!({"path":"empty"}))["total_lines"], 0);
    write(r, "binary", [0_u8]);
    assert!(error(r, "read", json!({"path":"binary"})).contains("binary"));
    assert!(error(r, "read", json!({"path":"."})).contains("regular file"));
}

#[test]
fn invalid_inspection_parameters_fail_instead_of_silently_changing_the_query() {
    let t = tempfile::tempdir().expect("workspace");
    let r = t.path();
    write(r, "a", "text");
    for (action, args) in [
        ("read", json!({"path":"a","file_path":"b"})),
        ("read", json!({"path":"a","max_bytes":0})),
        ("read", json!({"path":"a","start_line":3,"end_line":2})),
        ("read", json!({"path":false})),
        ("list_dir", json!({"depth":9})),
        ("list", json!({"offset":0})),
        ("search", json!({"query":"t","case_sensitive":"false"})),
        ("search", json!({"query":"t","limit":501})),
        ("search", json!({"query":"t","include_globs":"*.rs"})),
        ("search", json!({"query":"t","path":"../"})),
        ("search", json!({"query":"t","path":r.to_string_lossy()})),
    ] {
        assert!(!error(r, action, args).is_empty());
    }
}

#[cfg(unix)]
#[test]
fn traversal_does_not_follow_external_links_or_cycles_but_explicit_internal_paths_work() {
    use std::os::unix::fs::symlink;
    let t = tempfile::tempdir().expect("workspace");
    let outside = tempfile::tempdir().expect("outside");
    let r = t.path();
    write(r, "sub/a", "needle");
    write(outside.path(), "secret", "outside-marker");
    symlink(outside.path(), r.join("external")).expect("link");
    symlink(outside.path().join("secret"), r.join("external-file")).expect("link");
    symlink(r, r.join("sub/cycle")).expect("cycle");
    symlink(r.join("sub/a"), r.join("internal-file")).expect("link");
    let search = call(r, "search", json!({"query":"marker"}));
    assert_eq!(search["hits"], json!([]));
    assert_eq!(search["incomplete"], false);
    let internal = call(
        r,
        "search",
        json!({"query":"needle","path":"internal-file"}),
    );
    assert_eq!(internal["hits"].as_array().expect("hits").len(), 1);
    assert!(error(r, "read", json!({"path":"external/secret"})).contains("escapes"));
    assert!(error(r, "search", json!({"query":"marker","path":"external"})).contains("escapes"));
    let listed = call(r, "list_dir", json!({"depth":8}));
    assert!(
        listed["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .any(|e| e["name"] == "external" && e["kind"] == "symlink")
    );
    assert_eq!(listed["scan_truncated"], false);
}

#[test]
fn large_reads_keep_output_bounded_and_directory_scan_limits_are_visible() {
    let t = tempfile::tempdir().expect("workspace");
    let r = t.path();
    write(r, "large", "x".repeat(2_000_000));
    let read = call(r, "read", json!({"path":"large","max_bytes":8}));
    assert_eq!(read["content"], "xxxxxxxx");
    assert_eq!(read["truncated"], true);
    assert_eq!(read["next_start_line"], 1);
    for i in 0..=super::SCAN_ENTRIES {
        write(r, &format!("many/{i:05}"), "");
    }
    let listed = call(r, "list", json!({"path":"many","limit":500}));
    assert_eq!(listed["scan_truncated"], true);
    assert_eq!(listed["next_offset"], Value::Null);
    assert_eq!(listed["incomplete"], true);
    let searched = call(r, "search", json!({"path":"many","query":"absent"}));
    assert_eq!(searched["truncated_reason"], "scan_entries");
    assert_eq!(searched["incomplete"], true);
}

#[test]
fn aggregate_search_work_is_bounded_and_narrowing_recovers_complete_results() {
    let t = tempfile::tempdir().expect("workspace");
    let r = t.path();
    let text = format!("{}\n", "a".repeat(999_998));
    for i in 0..70 {
        write(r, &format!("{i:03}.txt"), &text);
    }
    let search = call(
        r,
        "search",
        json!({"query":"absent","max_file_bytes":1_000_000}),
    );
    assert_eq!(search["hits"], json!([]));
    assert_eq!(search["truncated_reason"], "scan_bytes");
    assert_eq!(search["incomplete"], true);
    assert!(search["scanned_bytes"].as_u64().expect("bytes") <= super::SEARCH_BYTES as u64);
    let single = call(
        r,
        "search",
        json!({"path":"069.txt","query":"absent","max_file_bytes":1_000_000}),
    );
    assert_eq!(single["incomplete"], false);
}

#[cfg(unix)]
#[test]
fn special_files_are_not_opened_as_text() {
    let t = tempfile::tempdir().expect("workspace");
    let status = std::process::Command::new("mkfifo")
        .arg(t.path().join("pipe"))
        .status()
        .expect("mkfifo");
    assert!(status.success());
    assert!(error(t.path(), "read", json!({"path":"pipe"})).contains("regular file"));
    let search = call(t.path(), "search", json!({"query":"anything"}));
    assert_eq!(search["hits"], json!([]));
    assert_eq!(search["incomplete"], false);
}
