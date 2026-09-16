import { describe, expect, it } from "vite-plus/test";

import type { WorkspaceEntry } from "../../types";
import {
  ROOT_PATH,
  ancestorsOf,
  emptyTreeState,
  expandPaths,
  flattenTree,
  invalidateDirectories,
  pendingDirectories,
  setDirectory,
  toggleExpanded,
} from "./workspaceTreeModel";

function dir(path: string): WorkspaceEntry {
  return { name: path.split("/").pop() || path, path, is_dir: true };
}

function file(path: string): WorkspaceEntry {
  return { name: path.split("/").pop() || path, path, is_dir: false };
}

describe("workspaceTreeModel", () => {
  it("hides cached descendants when an expanded link becomes unavailable", () => {
    let state = setDirectory(emptyTreeState(), ROOT_PATH, {
      items: [{ ...dir("link"), is_symlink: true, unavailable: "missing" }],
    });
    state = setDirectory(state, "link", { items: [file("link/old.txt")] });
    state = toggleExpanded(state, "link");
    expect(flattenTree(state).map((node) => [node.entry.path, node.expanded])).toEqual([
      ["link", false],
    ]);
  });
  it("reveals only the directories the user opened", () => {
    let state = emptyTreeState();
    state = setDirectory(state, ROOT_PATH, { items: [dir("src"), file("README.md")] });
    state = setDirectory(state, "src", { items: [file("src/lib.rs")] });

    expect(flattenTree(state).map((node) => node.entry.path)).toEqual(["src", "README.md"]);

    state = toggleExpanded(state, "src");
    expect(flattenTree(state).map((node) => node.entry.path)).toEqual([
      "src",
      "src/lib.rs",
      "README.md",
    ]);
    expect(flattenTree(state).map((node) => node.depth)).toEqual([0, 1, 0]);

    state = toggleExpanded(state, "src");
    expect(flattenTree(state).map((node) => node.entry.path)).toEqual(["src", "README.md"]);
  });

  it("marks an expanded directory as loading until its listing arrives", () => {
    let state = emptyTreeState();
    state = setDirectory(state, ROOT_PATH, { items: [dir("src")] });
    state = toggleExpanded(state, "src");

    const [node] = flattenTree(state);
    expect(node.expanded).toBe(true);
    expect(node.loading).toBe(true);
    expect(flattenTree(state)).toHaveLength(1);

    state = setDirectory(state, "src", { items: [file("src/lib.rs")], loading: false });
    expect(flattenTree(state).map((node) => node.entry.path)).toEqual(["src", "src/lib.rs"]);
  });

  it("lists the ancestors that must open to reveal a nested file", () => {
    expect(ancestorsOf("crates/web/src/lib.rs")).toEqual([
      ROOT_PATH,
      "crates",
      "crates/web",
      "crates/web/src",
    ]);
    expect(ancestorsOf("README.md")).toEqual([ROOT_PATH]);
  });

  it("reports directories that still need fetching, without duplicates", () => {
    let state = emptyTreeState();
    expect(pendingDirectories(state)).toEqual([ROOT_PATH]);

    state = setDirectory(state, ROOT_PATH, { items: [dir("src")] });
    state = expandPaths(state, ["src", "src"]);
    expect(state.expanded).toEqual(["src"]);
    expect(pendingDirectories(state)).toEqual(["src"]);

    state = setDirectory(state, "src", { items: [] });
    expect(pendingDirectories(state)).toEqual([]);
  });

  it("stops at a directory that lists itself instead of recursing forever", () => {
    // A self-referential symlink (`a/b -> a`) makes the listing for `a/b` contain `a/b` again.
    let state = emptyTreeState();
    state = setDirectory(state, ROOT_PATH, { items: [dir("a")] });
    state = setDirectory(state, "a", { items: [dir("a/b")] });
    state = setDirectory(state, "a/b", { items: [dir("a/b"), file("a/b/leaf.txt")] });
    state = expandPaths(state, ["a", "a/b"]);

    const rows = flattenTree(state);
    expect(new Set(rows.map((node) => node.key)).size).toBe(rows.length);
    expect(rows.map((node) => node.entry.path)).toEqual(["a", "a/b", "a/b", "a/b/leaf.txt"]);
    const looped = rows.filter((node) => node.entry.path === "a/b");
    expect(looped[0].expanded).toBe(true);
    expect(looped[1].expanded).toBe(false);
  });

  it("keeps the tree open across a refresh so a save does not collapse it", () => {
    let state = emptyTreeState();
    state = setDirectory(state, ROOT_PATH, { items: [dir("src")] });
    state = toggleExpanded(state, "src");
    state = setDirectory(state, "src", { items: [file("src/lib.rs")] });

    const refreshed = invalidateDirectories(state);
    expect(refreshed.expanded).toEqual(["src"]);
    expect(pendingDirectories(refreshed)).toEqual([ROOT_PATH, "src"]);
  });
});

it("gives shared symlink targets stable keys in each displayed branch", () => {
  let state = setDirectory(emptyTreeState(), "", { items: [dir("alias"), dir("real")] });
  for (const path of ["alias", "real"]) {
    state = setDirectory(state, path, { items: [dir("real/nested")] });
  }
  state = setDirectory(state, "real/nested", { items: [file("real/nested/file.txt")] });
  state = expandPaths(state, ["alias", "real", "real/nested"]);
  const rows = flattenTree(state);
  expect(new Set(rows.map((node) => node.key)).size).toBe(rows.length);
  expect(rows.filter((node) => !node.entry.is_dir).map((node) => node.key)).toEqual([
    "alias/nested/file.txt",
    "real/nested/file.txt",
  ]);
  expect(rows.filter((node) => !node.entry.is_dir).map((node) => node.entry.path)).toEqual([
    "real/nested/file.txt",
    "real/nested/file.txt",
  ]);
  state = toggleExpanded(state, "alias");
  expect(flattenTree(state).map((node) => node.key)).toEqual([
    "alias",
    "real",
    "real/nested",
    "real/nested/file.txt",
  ]);
  state = toggleExpanded(state, "alias");
  expect(flattenTree(state)).toEqual(rows);
});

it.each(["constructor", "toString", "__proto__"])(
  "loads and refreshes directory %s without inherited cache entries",
  (name) => {
    let state = setDirectory(emptyTreeState(), "", { items: [dir(name)] });
    state = toggleExpanded(state, name);
    expect(pendingDirectories(state)).toEqual([name]);
    expect(flattenTree(state)[0].loading).toBe(true);
    state = setDirectory(state, name, { loading: true });
    expect(flattenTree(state)).toHaveLength(1);
    state = setDirectory(state, name, { items: [file(`${name}/child.txt`)], loading: false });
    expect(flattenTree(state).map((row) => row.entry.path)).toEqual([name, `${name}/child.txt`]);
    state = invalidateDirectories(state);
    expect(pendingDirectories(state)).toEqual(["", name]);
    expect(flattenTree(state)).toEqual([]);
  },
);
