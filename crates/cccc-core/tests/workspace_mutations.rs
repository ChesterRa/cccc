use cccc_core::workspace::{self, WorkspaceUpload};
use std::{fs, io};
#[path = "support/workspace_fixture.rs"]
mod workspace_fixture;
use workspace_fixture::fixture;

#[test]
fn complete_file_and_nonempty_folder_lifecycle_preserves_collisions() {
    let f = fixture();
    workspace::create_entry(&f.group, "output", true).expect("folder");
    workspace::create_entry(&f.group, "output/notes.txt", false).expect("file");
    fs::write(f.repo.join("output/notes.txt"), "contents").expect("edit");
    fs::write(f.repo.join("occupied.txt"), "keep").expect("occupied");
    assert_eq!(
        workspace::move_entry(&f.group, "output/notes.txt", "occupied.txt")
            .expect_err("collision")
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(
        fs::read_to_string(f.repo.join("occupied.txt")).expect("read"),
        "keep"
    );
    workspace::move_entry(&f.group, "output/notes.txt", "output/renamed.txt").expect("rename");
    workspace::move_entry(&f.group, "output", "organized").expect("move nonempty folder");
    assert_eq!(
        fs::read_to_string(f.repo.join("organized/renamed.txt")).expect("read"),
        "contents"
    );
    assert!(workspace::move_entry(&f.group, "organized", "organized/child").is_err());
    workspace::delete_entry(&f.group, "organized").expect("recursive delete");
    assert!(!f.repo.join("organized").exists());
    assert!(f.repo.join("occupied.txt").exists());
}

#[test]
fn root_metadata_and_parent_escapes_cannot_be_mutated() {
    let f = fixture();
    for path in ["", ".", "..", "../outside", ".git", "./.git"] {
        assert!(
            workspace::create_entry(&f.group, path, true).is_err(),
            "{path}"
        );
        assert!(workspace::delete_entry(&f.group, path).is_err(), "{path}");
    }
    assert!(f.repo.exists());
}

#[test]
fn upload_is_complete_or_absent_and_never_overwrites_a_concurrent_file() {
    let f = fixture();
    let mut upload = WorkspaceUpload::new(&f.group, "new.txt").expect("stage");
    upload.write_chunk(b"complete").expect("chunk");
    assert!(!f.repo.join("new.txt").exists());
    upload.finish(&f.group).expect("publish");
    assert_eq!(fs::read(f.repo.join("new.txt")).expect("read"), b"complete");
    let mut upload = WorkspaceUpload::new(&f.group, "raced.txt").expect("stage");
    upload.write_chunk(b"upload").expect("chunk");
    fs::write(f.repo.join("raced.txt"), "other writer").expect("race");
    assert_eq!(
        upload.finish(&f.group).expect_err("collision").kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(
        fs::read_to_string(f.repo.join("raced.txt")).expect("read"),
        "other writer"
    );
    drop(WorkspaceUpload::new(&f.group, "cancelled.txt").expect("stage"));
    assert!(!f.repo.join("cancelled.txt").exists());
    assert!(!fs::read_dir(&f.repo).expect("list").any(|entry| {
        entry
            .expect("entry")
            .file_name()
            .to_string_lossy()
            .starts_with(".cccc-upload-")
    }));
}

#[cfg(unix)]
#[test]
fn rename_and_delete_affect_links_not_their_external_or_missing_targets() {
    let f = fixture();
    let outside = tempfile::tempdir().expect("outside");
    fs::write(outside.path().join("keep.txt"), "keep").expect("file");
    std::os::unix::fs::symlink(outside.path(), f.repo.join("external")).expect("link");
    assert!(workspace::create_entry(&f.group, "external/new.txt", false).is_err());
    workspace::move_entry(&f.group, "external", "link").expect("rename link");
    workspace::delete_entry(&f.group, "link").expect("delete link");
    assert!(outside.path().join("keep.txt").exists());
    std::os::unix::fs::symlink("missing", f.repo.join("broken")).expect("link");
    workspace::delete_entry(&f.group, "broken").expect("delete broken link");
    workspace::create_entry(&f.group, "folder", true).expect("folder");
    std::os::unix::fs::symlink(outside.path(), f.repo.join("folder/link")).expect("link");
    workspace::delete_entry(&f.group, "folder").expect("delete subtree without following links");
    assert!(outside.path().join("keep.txt").exists());
}

#[test]
fn interrupted_upload_cleanup_follows_a_moved_parent_directory() {
    for external in [false, true] {
        for finish in [false, true] {
            let f = fixture();
            fs::create_dir(f.repo.join("upload-parent")).expect("parent");
            let mut upload =
                WorkspaceUpload::new(&f.group, "upload-parent/file.txt").expect("upload");
            upload
                .write_chunk(b"private partial contents")
                .expect("write");
            if external {
                fs::rename(f.repo.join("upload-parent"), f.repo.join("moved-parent"))
                    .expect("external move");
            } else {
                workspace::move_entry(&f.group, "upload-parent", "moved-parent").expect("API move");
            }
            if finish {
                assert!(upload.finish(&f.group).is_err());
            } else {
                drop(upload);
            }
            assert_eq!(
                fs::read_dir(f.repo.join("moved-parent"))
                    .expect("entries")
                    .count(),
                0,
                "no temporary contents left after parent move (external={external}, finish={finish})"
            );
        }
    }
}

#[test]
fn moved_upload_parent_cannot_redirect_publication_or_cleanup_to_a_replacement() {
    let f = fixture();
    fs::create_dir(f.repo.join("parent")).expect("parent");
    let mut upload = WorkspaceUpload::new(&f.group, "parent/file.txt").expect("stage");
    upload.write_chunk(b"private").expect("write");
    let temp_name = fs::read_dir(f.repo.join("parent"))
        .expect("list")
        .next()
        .expect("temp")
        .expect("entry")
        .file_name();
    workspace::move_entry(&f.group, "parent", "moved").expect("move");
    fs::create_dir(f.repo.join("parent")).expect("replacement parent");
    fs::write(
        f.repo.join("parent").join(&temp_name),
        "unrelated replacement",
    )
    .expect("replacement temp name");
    assert!(upload.finish(&f.group).is_err());
    assert!(!f.repo.join("parent/file.txt").exists());
    assert_eq!(
        fs::read_dir(f.repo.join("moved"))
            .expect("list moved")
            .count(),
        0
    );
    assert_eq!(
        fs::read_to_string(f.repo.join("parent").join(temp_name)).expect("replacement intact"),
        "unrelated replacement"
    );
}
