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
