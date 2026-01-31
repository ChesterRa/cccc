import os
import subprocess
import tempfile
import unittest
from pathlib import Path


class TestGitWorktree(unittest.TestCase):
    def setUp(self) -> None:
        """Create a temporary git repository for testing."""
        self.temp_dir = tempfile.mkdtemp()
        self.repo_root = Path(self.temp_dir) / "test_repo"
        self.repo_root.mkdir()

        # Initialize git repo
        subprocess.run(["git", "init"], cwd=self.repo_root, capture_output=True)
        subprocess.run(
            ["git", "config", "user.email", "test@test.com"],
            cwd=self.repo_root,
            capture_output=True,
        )
        subprocess.run(
            ["git", "config", "user.name", "Test"],
            cwd=self.repo_root,
            capture_output=True,
        )

        # Create initial commit
        (self.repo_root / "README.md").write_text("# Test Repo")
        subprocess.run(["git", "add", "."], cwd=self.repo_root, capture_output=True)
        subprocess.run(
            ["git", "commit", "-m", "Initial commit"],
            cwd=self.repo_root,
            capture_output=True,
        )

    def tearDown(self) -> None:
        """Clean up temporary directory."""
        import shutil

        shutil.rmtree(self.temp_dir, ignore_errors=True)

    def test_git_worktree_add_creates_worktree(self) -> None:
        from cccc.kernel.git import git_worktree_add

        worktree_path = Path(self.temp_dir) / "worktree_feature"
        success, msg = git_worktree_add(
            self.repo_root, worktree_path, "feature-branch", create_branch=True
        )

        self.assertTrue(success, f"Failed to create worktree: {msg}")
        self.assertTrue(worktree_path.exists())
        self.assertTrue((worktree_path / ".git").exists())
        self.assertTrue((worktree_path / "README.md").exists())

    def test_git_worktree_add_existing_branch(self) -> None:
        from cccc.kernel.git import git_worktree_add

        # Create a branch first
        subprocess.run(
            ["git", "branch", "existing-branch"],
            cwd=self.repo_root,
            capture_output=True,
        )

        worktree_path = Path(self.temp_dir) / "worktree_existing"
        success, msg = git_worktree_add(
            self.repo_root, worktree_path, "existing-branch", create_branch=False
        )

        self.assertTrue(success, f"Failed to create worktree: {msg}")
        self.assertTrue(worktree_path.exists())

    def test_git_worktree_list_returns_worktrees(self) -> None:
        from cccc.kernel.git import git_worktree_add, git_worktree_list

        # Create a worktree first
        worktree_path = Path(self.temp_dir) / "worktree_list_test"
        git_worktree_add(self.repo_root, worktree_path, "list-test-branch")

        worktrees = git_worktree_list(self.repo_root)

        self.assertGreaterEqual(len(worktrees), 2)  # main + new worktree
        # Resolve paths to handle symlinks (e.g., /tmp -> /private/tmp on macOS)
        paths = [Path(w.get("path", "")).resolve() for w in worktrees]
        self.assertIn(worktree_path.resolve(), paths)

    def test_git_worktree_remove_deletes_worktree(self) -> None:
        from cccc.kernel.git import git_worktree_add, git_worktree_remove

        worktree_path = Path(self.temp_dir) / "worktree_to_remove"
        git_worktree_add(self.repo_root, worktree_path, "remove-test-branch")
        self.assertTrue(worktree_path.exists())

        success, msg = git_worktree_remove(self.repo_root, worktree_path)

        self.assertTrue(success, f"Failed to remove worktree: {msg}")
        self.assertFalse(worktree_path.exists())

    def test_git_is_worktree_detects_worktree(self) -> None:
        from cccc.kernel.git import git_is_worktree, git_worktree_add

        # Main repo should not be detected as a worktree (has .git directory)
        self.assertFalse(git_is_worktree(self.repo_root))

        # Create a worktree
        worktree_path = Path(self.temp_dir) / "worktree_detect"
        git_worktree_add(self.repo_root, worktree_path, "detect-branch")

        # Worktree should be detected (has .git file, not directory)
        self.assertTrue(git_is_worktree(worktree_path))


if __name__ == "__main__":
    unittest.main()
