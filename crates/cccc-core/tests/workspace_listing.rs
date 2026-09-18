use cccc_core::workspace::{self, ListOptions};
use cccc_core::workspace_git::GitStatus;
use std::path::Path;
use std::process::Command;
#[path = "support/workspace_fixture.rs"]
mod workspace_fixture;
use workspace_fixture::fixture;

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repo)
        .env("GIT_AUTHOR_NAME", "cccc")
        .env("GIT_AUTHOR_EMAIL", "cccc@example.com")
        .env("GIT_COMMITTER_NAME", "cccc")
        .env("GIT_COMMITTER_EMAIL", "cccc@example.com")
        .output()
        .expect("git");
    assert!(status.status.success(), "git {args:?} failed");
}

fn names(items: &[workspace::Entry]) -> Vec<String> {
    items.iter().map(|item| item.name.clone()).collect()
}

#[test]
fn listing_hides_git_plumbing_and_ignored_entries_until_asked() {
    let fixture = fixture();
    git(&fixture.repo, &["init", "-q"]);
    std::fs::write(fixture.repo.join(".gitignore"), "build/\n*.log\n").expect("gitignore");
    std::fs::create_dir_all(fixture.repo.join("build")).expect("build");
    std::fs::write(fixture.repo.join("debug.log"), "noise\n").expect("log");

    let hidden = workspace::list(&fixture.group, "", ListOptions::default()).expect("list");
    let visible = names(&hidden.items);
    assert!(visible.contains(&"src".to_owned()));
    assert!(visible.contains(&".gitignore".to_owned()));
    assert!(!visible.contains(&".git".to_owned()), "{visible:?}");
    assert!(!visible.contains(&"build".to_owned()), "{visible:?}");
    assert!(!visible.contains(&"debug.log".to_owned()), "{visible:?}");

    let shown = workspace::list(&fixture.group, "", ListOptions { show_ignored: true })
        .expect("list ignored");
    let shown_names = names(&shown.items);
    assert!(shown_names.contains(&"build".to_owned()));
    assert!(shown_names.contains(&"debug.log".to_owned()));
    assert!(
        !shown_names.contains(&".git".to_owned()),
        "git plumbing stays hidden even with show_ignored"
    );
    assert!(
        shown
            .items
            .iter()
            .find(|item| item.name == "build")
            .expect("build")
            .ignored
    );
}

#[test]
fn listing_carries_git_status_and_rolls_it_up_to_ancestor_directories() {
    let fixture = fixture();
    git(&fixture.repo, &["init", "-q"]);
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "base"]);
    std::fs::write(
        fixture.repo.join("src/lib.rs"),
        "fn main() { /* edit */ }\n",
    )
    .expect("edit");
    std::fs::write(fixture.repo.join("fresh.txt"), "new\n").expect("fresh");

    let root = workspace::list(&fixture.group, "", ListOptions::default()).expect("list");
    let src = root
        .items
        .iter()
        .find(|item| item.name == "src")
        .expect("src");
    assert_eq!(src.git_status, None);
    assert!(
        src.git_dirty_descendant,
        "a clean directory holding a modified file must be marked"
    );
    let fresh = root
        .items
        .iter()
        .find(|item| item.name == "fresh.txt")
        .expect("fresh");
    assert_eq!(fresh.git_status, Some(GitStatus::Untracked));

    let inner = workspace::list(&fixture.group, "src", ListOptions::default()).expect("list src");
    let lib = inner
        .items
        .iter()
        .find(|item| item.name == "lib.rs")
        .expect("lib");
    assert_eq!(lib.git_status, Some(GitStatus::Modified));
    assert_eq!(inner.path, "src");
    assert_eq!(
        inner.parent.as_deref(),
        Some(""),
        "a first-level directory must be able to navigate back to the root"
    );
}

#[test]
fn a_nested_listing_still_rolls_up_changes_from_deeper_in_its_own_subtree() {
    // The status walk is scoped to the directory being listed, so a rollup two levels down
    // is the case that breaks first if the pathspec is ever narrowed too far.
    let fixture = fixture();
    std::fs::create_dir_all(fixture.repo.join("src/deep/deeper")).expect("deep");
    std::fs::write(fixture.repo.join("src/deep/deeper/inner.rs"), "fn a() {}\n").expect("inner");
    git(&fixture.repo, &["init", "-q"]);
    git(&fixture.repo, &["add", "."]);
    git(&fixture.repo, &["commit", "-qm", "base"]);
    std::fs::write(fixture.repo.join("src/deep/deeper/inner.rs"), "fn b() {}\n").expect("edit");

    let listing = workspace::list(&fixture.group, "src", ListOptions::default()).expect("list src");
    let deep = listing
        .items
        .iter()
        .find(|item| item.name == "deep")
        .expect("deep");
    assert_eq!(deep.git_status, None);
    assert!(
        deep.git_dirty_descendant,
        "listing src must still see the edit under src/deep/deeper"
    );

    // The sibling that holds no changes must stay unmarked.
    let root = workspace::list(&fixture.group, "", ListOptions::default()).expect("list root");
    let src = root
        .items
        .iter()
        .find(|item| item.name == "src")
        .expect("src");
    assert!(src.git_dirty_descendant);
}

#[cfg(unix)]
#[test]
fn slow_git_hook_times_out_without_trapping_files_or_leaving_descendants() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = fixture();
    git(&fixture.repo, &["init", "-q"]);
    git(&fixture.repo, &["add", "."]);
    let hook = fixture.repo.join(".git/slow-monitor");
    let escaped = fixture.repo.to_string_lossy().replace("'", "'\"'\"'");
    std::fs::write(&hook, format!("#!/bin/sh\n(sleep 5; touch '{escaped}/leaked-hook') &\nwait\nprintf 'token\\000/\\000'\n"))
        .expect("write hook");
    std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o700))
        .expect("hook permissions");
    git(
        &fixture.repo,
        &[
            "config",
            "core.fsmonitor",
            hook.to_str().expect("hook path"),
        ],
    );
    let started = std::time::Instant::now();
    let listing =
        workspace::list(&fixture.group, "", ListOptions::default()).expect("base listing survives");
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
    assert!(names(&listing.items).contains(&"src".to_owned()));
    assert!(listing.items.iter().all(|entry| entry.git_status.is_none()));
    std::thread::sleep(std::time::Duration::from_secs(4));
    assert!(
        !fixture.repo.join("leaked-hook").exists(),
        "owned descendants must be terminated"
    );
}

#[cfg(unix)]
#[test]
fn listing_identifies_links_without_leaking_external_target_metadata() {
    use cccc_core::workspace::EntryUnavailable;
    use std::os::unix::fs::symlink;
    let fixture = fixture();
    let outside = fixture.repo.parent().expect("parent").join("external");
    std::fs::create_dir(&outside).expect("outside directory");
    let _socket = std::os::unix::net::UnixListener::bind(fixture.repo.join("socket"))
        .expect("fixture socket");
    for (target, name) in [
        (fixture.repo.join("src/lib.rs"), "file-link"),
        (fixture.repo.join("src"), "dir-link"),
        (outside.clone(), "outside-link"),
        (outside.join("missing"), "missing-link"),
        (fixture.repo.join("loop-link"), "loop-link"),
    ] {
        symlink(target, fixture.repo.join(name)).expect("link");
    }
    let listing = workspace::list(&fixture.group, "", ListOptions::default()).expect("list");
    let find = |name| {
        listing
            .items
            .iter()
            .find(|item| item.name == name)
            .expect("entry")
    };
    let file = find("file-link");
    assert!(file.is_symlink);
    assert!(!file.is_dir);
    assert_eq!(file.size, Some(13));
    assert_eq!(file.unavailable, None);
    assert!(find("dir-link").is_dir);
    assert!(find("dir-link").is_symlink);
    assert!(!find("src").is_symlink);
    assert_eq!(
        find("socket").unavailable,
        Some(EntryUnavailable::Unsupported)
    );
    assert_eq!(find("socket").size, None);
    for (name, reason) in [
        ("outside-link", EntryUnavailable::OutsideScope),
        ("missing-link", EntryUnavailable::Missing),
        ("loop-link", EntryUnavailable::Unreadable),
    ] {
        let item = find(name);
        assert!(item.is_symlink);
        assert_eq!(item.unavailable, Some(reason));
        assert!(!item.is_dir);
        assert_eq!(item.size, None);
        assert_eq!(item.mime_type, None);
    }
    std::fs::remove_file(fixture.repo.join("missing-link")).expect("remove link");
    symlink(fixture.repo.join("src"), fixture.repo.join("missing-link")).expect("repair link");
    let refreshed = workspace::list(&fixture.group, "", ListOptions::default()).expect("refresh");
    let repaired = refreshed
        .items
        .iter()
        .find(|item| item.name == "missing-link")
        .expect("repaired");
    assert!(repaired.is_dir);
    assert_eq!(repaired.unavailable, None);
}

// APFS refuses to create non-UTF-8 file names, so this can only run where the
// filesystem accepts raw bytes.
#[cfg(all(unix, not(target_os = "macos")))]
#[test]
fn non_utf8_names_and_link_targets_never_alias_another_utf8_filename() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};
    let f = fixture();
    let raw = std::ffi::OsStr::from_bytes(b"name-\xff.txt");
    std::fs::write(f.repo.join(raw), "unrepresentable").expect("raw filename");
    std::fs::write(f.repo.join("name-\u{fffd}.txt"), "different file").expect("neighbor");
    let result = workspace::list(&f.group, "", ListOptions { show_ignored: true });
    assert_eq!(
        result.expect_err("no lossy actionable paths").kind(),
        std::io::ErrorKind::InvalidData
    );
    symlink(raw, f.repo.join("alias.txt")).expect("link");
    assert!(
        workspace::read_file(&f.group, "alias.txt").is_err(),
        "canonical identity must be representable exactly"
    );
    workspace::delete_entry(&f.group, "alias.txt").expect("the link itself can still be removed");
    assert!(
        f.repo.join(raw).exists(),
        "removing a link leaves its target intact"
    );
    assert_eq!(
        std::fs::read_to_string(f.repo.join("name-\u{fffd}.txt")).expect("neighbor intact"),
        "different file"
    );
}
