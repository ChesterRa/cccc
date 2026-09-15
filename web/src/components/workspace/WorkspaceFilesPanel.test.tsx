// @vitest-environment happy-dom
import { act } from "react";
import { describe, expect, it } from "vite-plus/test";
import {
  ROOT_ITEMS,
  listing,
  render,
  mount,
  panel,
  mainArea,
  rowByName,
  click,
  fetchWorkspaceListing,
  fetchWorkspaceFile,
  saveWorkspaceFile,
} from "./workspaceFilesTestSupport";

describe("workspace files surfaces", () => {
  it("removes only the collapsed symlink branch without accumulating shared file rows", async () => {
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
      listing(
        path,
        path
          ? [{ name: "file.txt", path: "real/file.txt", is_dir: false }]
          : [
              { name: "alias", path: "alias", is_dir: true },
              { name: "real", path: "real", is_dir: true },
            ],
      ),
    );
    await mount();
    await click(rowByName("real"));
    const rows = () =>
      [...panel().querySelectorAll('[role="treeitem"]')].map((row) => row.textContent);
    for (let i = 0; i < 3; i++) {
      await click(rowByName("alias"));
      expect(rows()).toEqual(["alias", "file.txt", "real", "file.txt"]);
      await click(rowByName("alias"));
      expect(rows()).toEqual(["alias", "real", "file.txt"]);
    }
    await click(rowByName("real"));
    expect(rows()).toEqual(["alias", "real"]);
  });
  it("fetches a directory only when the user expands it", async () => {
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
      listing(
        path,
        path === "src" ? [{ name: "lib.rs", path: "src/lib.rs", is_dir: false }] : ROOT_ITEMS,
      ),
    );
    await mount();

    expect(fetchWorkspaceListing).toHaveBeenCalledTimes(1);
    expect(fetchWorkspaceListing.mock.calls[0][1]).toBe("");
    expect(panel().textContent).toContain("README.txt");
    expect(panel().textContent).not.toContain("lib.rs");

    await click(rowByName("src"));

    expect(fetchWorkspaceListing).toHaveBeenCalledTimes(2);
    expect(fetchWorkspaceListing.mock.calls[1][1]).toBe("src");
    expect(panel().textContent).toContain("lib.rs");
  });

  it("opens the file in the main area and keeps the tree visible beside it", async () => {
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
      listing(path, ROOT_ITEMS),
    );
    fetchWorkspaceFile.mockResolvedValue({
      ok: true,
      result: {
        scope_key: "scope-a",
        scope_url: "/repo",
        path: "README.txt",
        content: "# hello\n",
        bytes: 8,
        mime_type: "text/plain",
        binary: false,
        truncated: false,
        sha256: "abc",
      },
    });
    await mount();
    expect(mainArea().textContent).toContain("chat");

    await click(rowByName("README.txt"));

    expect(mainArea().querySelector("textarea")?.value).toBe("# hello\n");
    // The tree must survive: the panel no longer swaps itself out for the viewer.
    expect(panel().querySelectorAll('[role="treeitem"]')).toHaveLength(2);
    expect(panel().querySelector('[aria-selected="true"]')?.textContent).toContain("README.txt");
    expect(panel().querySelector("textarea")).toBeNull();

    const close = [...mainArea().querySelectorAll("button")].find(
      (node) => node.getAttribute("title") === "Close file",
    );
    await click(close as HTMLElement);

    expect(mainArea().textContent).toContain("chat");
    expect(panel().querySelectorAll('[role="treeitem"]')).toHaveLength(2);
  });

  it("keeps an Actor's edit and offers a reload when a save conflicts", async () => {
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
      listing(path, ROOT_ITEMS),
    );
    fetchWorkspaceFile.mockResolvedValue({
      ok: true,
      result: {
        scope_key: "scope-a",
        scope_url: "/repo",
        path: "README.txt",
        content: "# old\n",
        bytes: 6,
        mime_type: "text/plain",
        binary: false,
        truncated: false,
        sha256: "stale",
      },
    });
    saveWorkspaceFile.mockResolvedValue({
      ok: false,
      error: { code: "workspace_write_conflict", message: "changed on disk" },
    });
    await mount();

    await click(rowByName("README.txt"));
    const editor = mainArea().querySelector("textarea");
    expect(editor?.value).toBe("# old\n");

    await act(async () => {
      Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")?.set?.call(
        editor,
        "# edited\n",
      );
      editor?.dispatchEvent(new Event("input", { bubbles: true }));
    });

    const save = [...mainArea().querySelectorAll("button")].find(
      (node) => node.getAttribute("title") === "Save",
    );
    expect(save?.disabled).toBe(false);
    await click(save as HTMLElement);

    expect(saveWorkspaceFile).toHaveBeenCalledWith(
      "group-1",
      "README.txt",
      "# edited\n",
      "stale",
      "scope-a",
      "/repo",
    );
    expect(mainArea().textContent).toContain("changed on disk since you opened it");
    const reload = [...mainArea().querySelectorAll("button")].find((node) =>
      node.textContent?.includes("Reload"),
    );
    expect(reload).toBeTruthy();
    // The draft must survive so the user can copy their edit before reloading.
    expect(mainArea().querySelector("textarea")?.value).toBe("# edited\n");
  });

  it("ignores a file response that arrives after the group changed", async () => {
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
      listing(path, ROOT_ITEMS),
    );
    let releaseRead: (() => void) | null = null;
    fetchWorkspaceFile.mockImplementation(
      () =>
        new Promise((resolve) => {
          releaseRead = () =>
            resolve({
              ok: true,
              result: {
                scope_key: "scope-a",
                scope_url: "/repo",
                path: "README.txt",
                content: "group A secret\n",
                bytes: 15,
                mime_type: "text/plain",
                binary: false,
                truncated: false,
                sha256: "from-a",
              },
            });
        }),
    );
    await mount({ groupId: "group-a" });

    await click(rowByName("README.txt"));
    expect(mainArea().textContent).toContain("chat");

    // The operator switches groups while group A's read is still in flight.
    await render({ groupId: "group-b" });
    await act(async () => {
      releaseRead?.();
      await Promise.resolve();
    });

    expect(mainArea().textContent).toContain("chat");
    expect(mainArea().querySelector("textarea")).toBeNull();
    expect(saveWorkspaceFile).not.toHaveBeenCalled();
  });
});
