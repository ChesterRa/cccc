// @vitest-environment happy-dom
import { act } from "react";
import { describe, expect, it, vi } from "vite-plus/test";
import {
  ROOT_ITEMS,
  listing,
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
  it("drops a stale read when the user opens another file first", async () => {
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
      listing(path, ROOT_ITEMS),
    );
    const pending: Array<() => void> = [];
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
      listing(path, [...ROOT_ITEMS, { name: "LICENSE", path: "LICENSE", is_dir: false }]),
    );
    fetchWorkspaceFile.mockImplementation(
      (_group: string, path: string) =>
        new Promise((resolve) => {
          pending.push(() =>
            resolve({
              ok: true,
              result: {
                scope_key: "scope-a",
                scope_url: "/repo",
                path,
                content: `content of ${path}\n`,
                bytes: 10,
                mime_type: "text/plain",
                binary: false,
                truncated: false,
                sha256: path,
              },
            }),
          );
        }),
    );
    await mount();

    // The operator clicks README.txt, then changes their mind and clicks LICENSE.
    await click(rowByName("README.txt"));
    await click(rowByName("LICENSE"));
    expect(pending).toHaveLength(2);

    // The newest read lands first, then the superseded README.txt response arrives late.
    await act(async () => {
      pending[1]?.();
      await Promise.resolve();
    });
    expect(mainArea().querySelector("textarea")?.value).toBe("content of LICENSE\n");

    await act(async () => {
      pending[0]?.();
      await Promise.resolve();
    });

    expect(mainArea().querySelector("textarea")?.value).toBe("content of LICENSE\n");
    expect(panel().querySelector('[aria-selected="true"]')?.textContent).toContain("LICENSE");
  });

  it("keeps keystrokes typed while a save is in flight", async () => {
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
      listing(path, ROOT_ITEMS),
    );
    fetchWorkspaceFile.mockResolvedValue({
      ok: true,
      result: {
        scope_key: "scope-a",
        scope_url: "/repo",
        path: "README.txt",
        content: "one\n",
        bytes: 4,
        mime_type: "text/plain",
        binary: false,
        truncated: false,
        sha256: "v1",
      },
    });
    let finishSave: (() => void) | null = null;
    saveWorkspaceFile.mockImplementation(
      () =>
        new Promise((resolve) => {
          finishSave = () =>
            resolve({ ok: true, result: { path: "README.txt", sha256: "v2", created: false } });
        }),
    );
    await mount();
    await click(rowByName("README.txt"));

    const type = async (value: string) => {
      await act(async () => {
        const editor = mainArea().querySelector("textarea");
        Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, "value")?.set?.call(
          editor,
          value,
        );
        editor?.dispatchEvent(new Event("input", { bubbles: true }));
      });
    };

    await type("two\n");
    const save = [...mainArea().querySelectorAll("button")].find(
      (node) => node.getAttribute("title") === "Save",
    );
    await click(save as HTMLElement);
    // The request is still open; the user keeps typing.
    await type("two and three\n");
    await act(async () => {
      finishSave?.();
      await Promise.resolve();
    });

    expect(mainArea().querySelector("textarea")?.value).toBe("two and three\n");
  });

  it("shows why a file failed to open, next to the tree", async () => {
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
      listing(path, ROOT_ITEMS),
    );
    fetchWorkspaceFile.mockResolvedValue({
      ok: false,
      error: { code: "NOT_FOUND", message: "Path not found: README.txt" },
    });
    await mount();

    await click(rowByName("README.txt"));

    // The viewer never mounts on a failed read, so the panel has to carry the reason.
    expect(mainArea().textContent).toContain("chat");
    expect(panel().textContent).toContain("Path not found: README.txt");
    expect(panel().querySelectorAll('[role="treeitem"]')).toHaveLength(2);
  });

  it("hides the pin action when the panel is read-only", async () => {
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
      listing(path, ROOT_ITEMS),
    );
    await mount({ readOnly: true, onPinPath: vi.fn() });

    await act(async () => {
      rowByName("README.txt").dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, clientX: 10, clientY: 10 }),
      );
    });

    const labels = [...document.querySelectorAll('[role="menuitem"]')].map(
      (node) => node.textContent,
    );
    expect(labels).toContain("Attach as context");
    expect(labels).not.toContain("Pin to a Presentation slot");
  });
});

it("shows a failed subdirectory and retries only when asked", async () => {
  let attempts = 0;
  fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) => {
    if (!path) return listing(path, ROOT_ITEMS);
    attempts += 1;
    return attempts === 1
      ? { ok: false, error: { code: "PERMISSION", message: "Permission denied: src" } }
      : listing(path, [{ name: "recovered.rs", path: "src/recovered.rs", is_dir: false }]);
  });
  await mount();
  await click(rowByName("src"));
  expect(panel().querySelector('[role="alert"]')?.textContent).toContain("Permission denied: src");
  expect(attempts).toBe(1);
  await click(rowByName("src"));
  await click(rowByName("src"));
  expect(attempts).toBe(1);
  const retry = [...panel().querySelectorAll("button")].find(
    (button) => button.textContent === "Retry",
  )!;
  await click(retry);
  expect(attempts).toBe(2);
  expect(panel().querySelector('[role="alert"]')).toBeNull();
  expect(rowByName("recovered.rs")).toBeTruthy();
});
