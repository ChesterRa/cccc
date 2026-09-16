use cccc_core::workspace::{self, ListOptions, WriteOutcome};

#[path = "support/workspace_fixture.rs"]
mod workspace_fixture;
use workspace_fixture::fixture;

#[test]
fn relative_paths_that_climb_out_of_the_scope_are_rejected() {
    let fixture = fixture();
    for escape in ["../", "..", "../secrets.txt", "src/../../secrets.txt"] {
        assert!(
            workspace::safe_relative(escape).is_err(),
            "{escape} must be rejected"
        );
    }
    assert!(workspace::read_file(&fixture.group, "../secrets.txt").is_err());
    assert!(workspace::list(&fixture.group, "..", ListOptions::default()).is_err());
}

#[test]
fn absolute_paths_cannot_reach_outside_the_scope() {
    let fixture = fixture();
    assert!(workspace::safe_relative("/etc/passwd").is_err());
    assert!(workspace::read_file(&fixture.group, "/etc/passwd").is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_pointing_outside_the_scope_are_refused() {
    let fixture = fixture();
    let outside = fixture.repo.parent().expect("parent").join("outside.txt");
    std::fs::write(&outside, "secret\n").expect("outside");
    std::os::unix::fs::symlink(&outside, fixture.repo.join("leak.txt")).expect("symlink");

    // The relative path itself is clean, so only the canonicalized root check can catch this.
    let error = workspace::read_file(&fixture.group, "leak.txt").expect_err("must refuse");
    assert!(
        error.to_string().contains("active scope"),
        "unexpected error: {error}"
    );
}

#[test]
fn reading_a_text_file_reports_content_and_a_digest_that_authorizes_writes() {
    let fixture = fixture();
    let read = workspace::read_file(&fixture.group, "src/lib.rs").expect("read");
    assert_eq!(read.content, "fn main() {}\n");
    assert_eq!(read.path, "src/lib.rs");
    assert!(!read.binary);
    assert!(!read.truncated);

    let written = workspace::write_file(
        &fixture.group,
        "src/lib.rs",
        "fn main() { 1; }\n",
        &read.sha256,
    )
    .expect("write");
    let WriteOutcome::Written { created, .. } = written else {
        panic!("expected the matching digest to be accepted");
    };
    assert!(!created);
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("src/lib.rs")).expect("reread"),
        "fn main() { 1; }\n"
    );
}

#[cfg(unix)]
#[test]
fn saving_an_executable_script_keeps_its_mode() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = fixture();
    let script = fixture.repo.join("run.sh");
    std::fs::write(&script, "#!/bin/sh\necho one\n").expect("script");
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");

    let read = workspace::read_file(&fixture.group, "run.sh").expect("read");
    let outcome = workspace::write_file(
        &fixture.group,
        "run.sh",
        "#!/bin/sh\necho two\n",
        &read.sha256,
    )
    .expect("write");
    assert!(matches!(outcome, WriteOutcome::Written { .. }));

    let mode = std::fs::metadata(&script)
        .expect("metadata")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        mode, 0o755,
        "an atomic replace must not strip the executable bit"
    );
    assert_eq!(
        std::fs::read_to_string(&script).expect("reread"),
        "#!/bin/sh\necho two\n"
    );
}

#[test]
fn writing_with_a_stale_digest_conflicts_instead_of_clobbering_the_actor_edit() {
    let fixture = fixture();
    let read = workspace::read_file(&fixture.group, "src/lib.rs").expect("read");
    // An Actor rewrites the same file while the browser tab still holds the old content.
    std::fs::write(fixture.repo.join("src/lib.rs"), "fn actor() {}\n").expect("actor write");

    let outcome = workspace::write_file(
        &fixture.group,
        "src/lib.rs",
        "fn browser() {}\n",
        &read.sha256,
    )
    .expect("write");
    let WriteOutcome::Conflict { sha256 } = outcome else {
        panic!("a stale digest must conflict");
    };
    assert_ne!(sha256, read.sha256);
    assert_eq!(
        std::fs::read_to_string(fixture.repo.join("src/lib.rs")).expect("reread"),
        "fn actor() {}\n",
        "the conflicting write must not touch the file"
    );
}

#[test]
fn creating_a_file_requires_an_empty_digest_and_an_absent_path() {
    let fixture = fixture();
    let created =
        workspace::write_file(&fixture.group, "src/new.rs", "// new\n", "").expect("create");
    let WriteOutcome::Written {
        created: is_new, ..
    } = created
    else {
        panic!("creating an absent path must succeed");
    };
    assert!(is_new);

    let clash =
        workspace::write_file(&fixture.group, "src/new.rs", "// again\n", "").expect("clash");
    assert!(
        matches!(clash, WriteOutcome::Conflict { .. }),
        "an empty digest must not overwrite an existing file"
    );
    assert!(workspace::write_file(&fixture.group, "../escape.rs", "// no\n", "").is_err());
}

#[test]
fn binary_and_oversized_files_are_flagged_rather_than_inlined() {
    let fixture = fixture();
    std::fs::write(fixture.repo.join("logo.bin"), [0x89, 0x50, 0x00, 0x1a]).expect("binary");
    let binary = workspace::read_file(&fixture.group, "logo.bin").expect("read binary");
    assert!(binary.binary);
    assert!(binary.content.is_empty());

    let oversized = vec![b'a'; (workspace::MAX_READ_BYTES + 1) as usize];
    std::fs::write(fixture.repo.join("huge.txt"), &oversized).expect("huge");
    let huge = workspace::read_file(&fixture.group, "huge.txt").expect("read huge");
    assert!(huge.truncated);
    assert!(huge.content.is_empty());
    assert_eq!(huge.bytes, workspace::MAX_READ_BYTES + 1);
}

#[test]
fn source_files_and_transport_streams_with_the_same_extension_keep_their_content_type() {
    let fixture = fixture();
    for name in [
        "module.ts",
        "types.d.ts",
        "module.mts",
        "module.cts",
        "view.tsx",
    ] {
        let source = "export const greeting: string = '\u{4f60}\u{597d}';\n";
        std::fs::write(fixture.repo.join(name), source).expect("source");
        let read = workspace::read_file(&fixture.group, name).expect("read source");
        assert!(!read.binary, "{name}");
        assert_eq!(read.content, source, "{name}");
        assert!(!read.sha256.is_empty());
    }
    std::fs::write(fixture.repo.join("stream.ts"), [0x47, 0x40, 0, 0x10, 0xff])
        .expect("transport stream");
    let stream = workspace::read_file(&fixture.group, "stream.ts").expect("read stream");
    assert!(stream.binary);
    assert!(stream.mime_type.starts_with("video/"));
    assert!(stream.content.is_empty());
}

#[test]
fn oversized_files_use_a_bounded_sample_to_distinguish_text_from_media() {
    let fixture = fixture();
    for (name, prefix, binary) in [
        ("large.ts", b"export const value = 1;".as_slice(), false),
        ("large.mts", b"// module".as_slice(), false),
        ("stream.ts", b"\x47\x40\x00\x10\xff".as_slice(), true),
        ("clip.mp4", b"\x00\x00\x00\x20ftypisom".as_slice(), true),
    ] {
        let mut bytes = prefix.to_vec();
        bytes.resize((workspace::MAX_READ_BYTES + 1) as usize, b' ');
        std::fs::write(fixture.repo.join(name), &bytes).expect("large file");
        let read = workspace::read_file(&fixture.group, name).expect("read large file");
        assert_eq!(read.binary, binary, "{name}");
        assert!(read.truncated);
        assert!(read.content.is_empty());
        assert!(read.sha256.is_empty());
    }

    // The sample boundary can split a valid UTF-8 character; this is still text.
    let text = " ".repeat(8191) + &"\u{4e2d}".repeat(workspace::MAX_READ_BYTES as usize / 2);
    std::fs::write(fixture.repo.join("unicode.ts"), &text).expect("unicode source");
    let read = workspace::read_file(&fixture.group, "unicode.ts").expect("read unicode source");
    assert!(read.truncated);
    assert!(!read.binary);
}

#[test]
fn concurrent_saves_sharing_one_digest_keep_exactly_one_write() {
    let fixture = fixture();
    let digest = workspace::read_file(&fixture.group, "src/lib.rs")
        .expect("read")
        .sha256;
    const WRITERS: usize = 16;
    let barrier = std::sync::Barrier::new(WRITERS);
    let outcomes = std::thread::scope(|scope| {
        let handles = (0..WRITERS)
            .map(|index| {
                let group = &fixture.group;
                let digest = digest.as_str();
                let barrier = &barrier;
                scope.spawn(move || {
                    // Every writer read the same bytes, so every writer believes it may save.
                    barrier.wait();
                    workspace::write_file(
                        group,
                        "src/lib.rs",
                        &format!("fn v{index}() {{}}\n"),
                        digest,
                    )
                    .expect("write")
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("join"))
            .collect::<Vec<_>>()
    });

    let winners = outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            WriteOutcome::Written { sha256, .. } => Some(sha256.clone()),
            WriteOutcome::Conflict { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        winners.len(),
        1,
        "exactly one save may pass the digest check, got {outcomes:?}"
    );
    // The surviving bytes must be the winner's, not a later loser's silent overwrite.
    let after = workspace::read_file(&fixture.group, "src/lib.rs").expect("reread");
    assert_eq!(after.sha256, winners[0]);
}

#[test]
#[cfg(unix)]
fn workspace_paths_preserve_leading_and_trailing_spaces() {
    let f = fixture();
    std::fs::write(f.repo.join("note.txt"), "neighbor").expect("workspace whitespace fixture");
    for name in [" note.txt", "note.txt "] {
        std::fs::write(f.repo.join(name), "selected").expect("workspace whitespace fixture");
        let file = workspace::read_file(&f.group, name).expect("workspace whitespace fixture");
        assert_eq!(
            file.content, "selected",
            "reading {name:?} must use the listed path"
        );
        workspace::write_file(&f.group, name, "updated", &file.sha256)
            .expect("workspace whitespace fixture");
        assert_eq!(
            std::fs::read_to_string(f.repo.join(name)).expect("workspace whitespace fixture"),
            "updated"
        );
        assert_eq!(
            std::fs::read_to_string(f.repo.join("note.txt")).expect("workspace whitespace fixture"),
            "neighbor"
        );
    }
}

#[test]
#[cfg(unix)]
fn posix_backslash_names_do_not_alias_nested_paths() {
    let f = fixture();
    std::fs::create_dir(f.repo.join("foo")).expect("nested directory");
    std::fs::write(f.repo.join("foo/bar.txt"), "neighbor").expect("nested file");
    std::fs::write(f.repo.join(r"foo\bar.txt"), "selected").expect("backslash name");
    let listing = workspace::list(&f.group, "", workspace::ListOptions::default()).expect("list");
    let entry = listing
        .items
        .iter()
        .find(|entry| entry.name == r"foo\bar.txt")
        .expect("entry");
    assert_eq!(entry.path, r"foo\bar.txt");
    let file = workspace::read_file(&f.group, &entry.path).expect("read selected file");
    assert_eq!(file.content, "selected");
    assert_eq!(file.path, entry.path);
    workspace::write_file(&f.group, &file.path, "edited", &file.sha256)
        .expect("write selected file");
    assert_eq!(
        std::fs::read_to_string(f.repo.join(r"foo\bar.txt")).expect("selected"),
        "edited"
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join("foo/bar.txt")).expect("neighbor"),
        "neighbor"
    );
}
