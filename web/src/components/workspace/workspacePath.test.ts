import { expect, it } from "vite-plus/test";
import { workspaceAbsolutePath, workspaceRelativePath } from "./workspacePath";

it.each([
  ["src/main.rs", "/repo", "src/main.rs"],
  ["./src//main.rs", "/repo", "src/main.rs"],
  ['"/repo/a b.txt"', "/repo", "a b.txt"],
  ["/repo/foo\\bar.txt", "/repo", "foo\\bar.txt"],
  ["/src/main.rs", "/", "src/main.rs"],
  ["/repo-other/file", "/repo", null],
  ["/repo", "/repo", ""],
  ["../secrets", "/repo", null],
  ["src/../../secrets", "/repo", null],
  ["https://example.org/file", "/repo", null],
  ["C:\\repo\\src\\main.rs", "C:\\repo", "src/main.rs"],
  ["c:/REPO/src/main.rs", "C:\\repo", "src/main.rs"],
  ["src\\main.rs", "\\\\?\\C:\\repo", "src/main.rs"],
  ["\\\\?\\C:\\repo\\src\\main.rs", "C:\\repo", "src/main.rs"],
  ["\\\\server\\share\\repo\\a.txt", "\\\\?\\UNC\\server\\share\\repo", "a.txt"],
  ["C:\\repo-other\\file", "C:\\repo", null],
  ["C:relative.txt", "C:\\repo", null],
  ["..\\secrets", "C:\\repo", null],
  ["", "/repo", null],
  ['""', "/repo", null],
  [".", "/repo", ""],
  ["./", "/repo", ""],
  ["/repo/", "/repo", ""],
  ["/", "/", ""],
  ["/repo/.pytest_cache/v/cache/", "/repo", ".pytest_cache/v/cache"],
  ["C:/repo/", "C:/repo", ""],
])("resolves %s within %s without changing path identity", (input, root, expected) => {
  expect(workspaceRelativePath(input, root)).toBe(expected);
});

it("copies absolute paths using the workspace platform rather than the browser platform", () => {
  expect(workspaceAbsolutePath("/repo/", "a\\b.txt")).toBe("/repo/a\\b.txt");
  expect(workspaceAbsolutePath("C:\\repo\\", "src/a.txt")).toBe("C:\\repo\\src\\a.txt");
  expect(workspaceAbsolutePath("/", "a.txt")).toBe("/a.txt");
});
