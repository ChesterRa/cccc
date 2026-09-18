#[path = "../build_support/source.rs"]
mod source;

#[test]
fn source_identity_tracks_content_and_new_files_without_git_or_build_outputs() {
    let dir = tempfile::tempdir().expect("temporary source root");
    let root = dir.path();
    std::fs::create_dir_all(root.join("crates/example/src")).expect("prepare source fixture");
    std::fs::create_dir_all(root.join("resources")).expect("prepare source fixture");
    for path in [
        "Cargo.toml",
        "Cargo.lock",
        "crates/example/src/lib.rs",
        "resources/mcp_tools.json",
        "crates/cccc-daemon/resources/capability-allowlist.default.yaml",
        "crates/cccc-web/resources/voice-models.default.json",
        "resources/nested/help.md",
    ] {
        std::fs::create_dir_all(root.join(path).parent().expect("parent"))
            .expect("resource directory");
        std::fs::write(root.join(path), path).expect("prepare source fixture");
    }
    let id = || {
        source::fingerprint(root, &source::inputs(root).expect("source inputs"))
            .expect("source fingerprint")
    };
    let original = id();
    assert_eq!(
        source::inputs(root).expect("source inputs").resource_dirs,
        [
            "crates/cccc-daemon/resources",
            "crates/cccc-web/resources",
            "resources",
        ]
        .map(std::path::PathBuf::from)
    );
    for resource in [
        "crates/cccc-daemon/resources/capability-allowlist.default.yaml",
        "crates/cccc-web/resources/voice-models.default.json",
        "resources/nested/help.md",
    ] {
        std::fs::write(root.join(resource), "changed embedded resource").expect("change resource");
        assert_ne!(
            original,
            id(),
            "resource change must affect identity: {resource}"
        );
        std::fs::write(root.join(resource), resource).expect("restore fixture resource");
        assert_eq!(original, id());
    }
    std::fs::create_dir_all(root.join("crates/example/assets")).expect("prepare source fixture");
    std::fs::write(root.join("crates/example/assets/output.rs"), "generated")
        .expect("prepare source fixture");
    assert_eq!(original, id());
    std::fs::write(root.join("crates/example/src/new.rs"), "new").expect("prepare source fixture");
    assert_ne!(original, id());
    std::fs::remove_file(root.join("crates/example/src/new.rs")).expect("prepare source fixture");
    assert_eq!(original, id());
    std::fs::write(root.join("resources/mcp_tools.json"), "new contract")
        .expect("prepare source fixture");
    assert_ne!(original, id());
    assert_eq!(
        cccc_core::build_info::current()["source_id"]
            .as_str()
            .expect("compiled source id")
            .len(),
        64
    );
}
