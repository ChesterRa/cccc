#[path = "support/workspace_fixture.rs"]
mod workspace_fixture;
use cccc_core::workspace_changes::{DiffSide, changes, diff};
use std::{fs, process::Command};
fn git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn init(root: &std::path::Path) {
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "fixture@example.invalid"]);
    git(root, &["config", "user.name", "Fixture"]);
}
#[test]
fn staged_worktree_untracked_deleted_and_unborn_are_read_without_index_changes() {
    let f = workspace_fixture::fixture();
    assert!(!changes(&f.group).expect("non repo").repository);
    init(&f.repo);
    git(&f.repo, &["add", "."]);
    let initial = changes(&f.group).expect("unborn");
    assert_eq!(initial.entries[0].index, 'A');
    assert!(
        diff(&f.group, "src/lib.rs", DiffSide::Staged)
            .expect("unborn patch")
            .patch
            .contains("+fn main()")
    );
    git(&f.repo, &["commit", "-qm", "initial"]);
    fs::write(f.repo.join("src/lib.rs"), "staged\n").expect("stage");
    git(&f.repo, &["add", "."]);
    fs::write(f.repo.join("src/lib.rs"), "worktree\n").expect("worktree");
    fs::write(f.repo.join("new.txt"), "new\n").expect("untracked");
    let before = fs::read(f.repo.join(".git/index")).expect("index");
    let changes = changes(&f.group).expect("changes");
    assert!(
        changes
            .entries
            .iter()
            .any(|e| e.path == "src/lib.rs" && e.index == 'M' && e.worktree == 'M')
    );
    assert!(
        changes
            .entries
            .iter()
            .any(|e| e.path == "new.txt" && e.untracked)
    );
    let staged = diff(&f.group, "src/lib.rs", DiffSide::Staged)
        .expect("staged")
        .patch;
    let working = diff(&f.group, "src/lib.rs", DiffSide::Worktree)
        .expect("working")
        .patch;
    assert!(staged.contains("+staged") && !staged.contains("+worktree"));
    assert!(working.contains("-staged") && working.contains("+worktree"));
    assert_eq!(
        fs::read(f.repo.join(".git/index")).expect("index after"),
        before
    );
    fs::remove_file(f.repo.join("src/lib.rs")).expect("delete");
    assert!(
        diff(&f.group, "src/lib.rs", DiffSide::Worktree)
            .expect("deleted")
            .patch
            .contains("-staged")
    );
    assert!(diff(&f.group, "../secret", DiffSide::Worktree).is_err());
}
#[test]
fn subdirectory_scope_and_literal_paths_exclude_other_groups_and_rename_sources() {
    let mut f = workspace_fixture::fixture();
    init(&f.repo);
    fs::write(f.repo.join("private.txt"), "PRIVATE\n").expect("private");
    git(&f.repo, &["add", "."]);
    git(&f.repo, &["commit", "-qm", "initial"]);
    fs::rename(f.repo.join("private.txt"), f.repo.join("src/imported.txt")).expect("rename");
    fs::write(f.repo.join("src/literal[1].txt"), "literal\n").expect("literal");
    fs::write(f.repo.join("outside.txt"), "other\n").expect("outside");
    git(&f.repo, &["add", "."]);
    f.group.scopes[0].url = f
        .repo
        .join("src")
        .canonicalize()
        .expect("scope")
        .to_string_lossy()
        .into_owned();
    let listing = changes(&f.group).expect("scoped");
    assert!(listing.entries.iter().all(|e| !e.path.starts_with("src/")
        && e.path != "outside.txt"
        && e.previous_path.is_none()));
    let patch = diff(&f.group, "literal[1].txt", DiffSide::Staged)
        .expect("literal")
        .patch;
    assert!(patch.contains("+literal"));
    let patch = diff(&f.group, "imported.txt", DiffSide::Staged)
        .expect("imported")
        .patch;
    assert!(!patch.contains("private.txt"));
}

#[test]
fn binary_changes_and_output_limits_do_not_attempt_decoding_or_return_partial_patches() {
    let f = workspace_fixture::fixture();
    init(&f.repo);
    fs::write(f.repo.join("binary.bin"), b"\0original").expect("binary");
    fs::write(f.repo.join("large.txt"), "original\n").expect("large");
    git(&f.repo, &["add", "."]);
    git(&f.repo, &["commit", "-qm", "initial"]);
    fs::write(f.repo.join("binary.bin"), b"\0changed").expect("binary changed");
    fs::write(f.repo.join("large.txt"), "changed\n".repeat(10_100)).expect("large changed");
    let binary = diff(&f.group, "binary.bin", DiffSide::Worktree).expect("binary diff");
    assert!(!binary.limited);
    assert!(binary.patch.contains("Binary files"));
    let large = diff(&f.group, "large.txt", DiffSide::Worktree).expect("large diff");
    assert!(large.limited);
    assert!(large.patch.is_empty());
}
