// @vitest-environment happy-dom
import { act } from "react";
import { expect, it, vi } from "vite-plus/test";
import {
  click,
  edit,
  resolveWorkspacePath,
  fetchWorkspaceFile,
  fetchWorkspaceListing,
  listing,
  mainArea,
  mount,
  panel,
  render,
  rowByName,
  saveButton,
  saveWorkspaceFile,
} from "./workspaceFilesTestSupport";

function filesFixture() {
  resolveWorkspacePath.mockImplementation(async (_group: string, path: string) => ({
    ok: true,
    result: { path, is_dir: path === "" || path === "build" || path === "src" },
  }));
  fetchWorkspaceListing.mockImplementation(
    async (_group: string, path: string, options: { showIgnored: boolean }) =>
      listing(
        path,
        path
          ? [{ name: "notes.txt", path: "build/notes.txt", is_dir: false, ignored: true }]
          : [
              { name: "src", path: "src", is_dir: true },
              ...(options.showIgnored
                ? [{ name: "build", path: "build", is_dir: true, ignored: true }]
                : []),
              { name: "README.txt", path: "README.txt", is_dir: false },
            ],
      ),
  );
  fetchWorkspaceFile.mockImplementation(async (_group: string, path: string) => ({
    ok: true,
    result: {
      scope_key: "scope-a",
      scope_url: "/repo",
      path,
      content: "disk",
      bytes: 4,
      mime_type: "text/plain",
      binary: false,
      truncated: false,
      sha256: "original",
    },
  }));
}
async function optionsMenu() {
  await click(panel().querySelector<HTMLElement>('[title="File browser options"]')!);
}
async function menuAction(label: string) {
  const item = [...document.querySelectorAll<HTMLElement>('[role^="menuitem"]')].find(
    (node) => node.textContent === label,
  )!;
  await click(item);
}
async function enterPath(path: string) {
  await act(async () => {
    const input = panel().querySelector<HTMLInputElement>('input[type="text"]')!;
    input.focus();
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, path);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () =>
    panel()
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
}

it("shows ignored entries by default and remembers an explicit filter without discarding drafts or expansion", async () => {
  filesFixture();
  await mount();
  expect(rowByName("build").className).not.toContain("opacity-45");
  expect(fetchWorkspaceListing.mock.calls[0]?.[2].showIgnored).toBe(true);
  await click(rowByName("src"));
  await click(rowByName("README.txt"));
  await edit("keep draft");
  await optionsMenu();
  await menuAction("Hide Git-ignored files");
  expect(localStorage.getItem("cccc-workspace-show-ignored")).toBe("false");
  expect(rowByName("src").getAttribute("aria-expanded")).toBe("true");
  expect(mainArea().querySelector("textarea")?.value).toBe("keep draft");
  expect(panel().querySelector('[title="build"]')).toBeNull();
});

it("opens an absolute path and reveals it through the filter, then collapses and reveals without rereading the file", async () => {
  localStorage.setItem("cccc-workspace-show-ignored", "false");
  filesFixture();
  await mount();
  await enterPath("/repo/build/notes.txt");
  expect(fetchWorkspaceFile).toHaveBeenCalledWith("group-1", "build/notes.txt", "scope-a", "/repo");
  expect(rowByName("build").getAttribute("aria-expanded")).toBe("true");
  expect(document.activeElement).toBe(rowByName("notes.txt"));
  await optionsMenu();
  await menuAction("Collapse all folders");
  expect(rowByName("build").getAttribute("aria-expanded")).toBe("false");
  await optionsMenu();
  await menuAction("Reveal current file");
  expect(document.activeElement).toBe(rowByName("notes.txt"));
  expect(fetchWorkspaceFile).toHaveBeenCalledTimes(1);

  // A later directory refresh must not replay an earlier reveal and steal focus.
  const input = panel().querySelector<HTMLInputElement>('input[type="text"]')!;
  input.focus();
  await click(panel().querySelector<HTMLElement>('[title="Refresh directory"]')!);
  expect(document.activeElement).toBe(input);
});

it("rejects outside paths before reading and retires input and menus on a scope change", async () => {
  filesFixture();
  await mount();
  await enterPath("/repo-other/private.txt");
  expect(fetchWorkspaceFile).not.toHaveBeenCalled();
  expect(panel().querySelector('[role="alert"]')?.textContent).toContain("inside this workspace");
  await optionsMenu();
  await render({ scopeKey: "scope-b", scopeUrl: "/other" });
  expect(document.querySelector('[role="menu"]')).toBeNull();
  expect(panel().querySelector<HTMLInputElement>('input[type="text"]')?.value).toBe("");
  expect(panel().querySelector('[role="alert"]')).toBeNull();
});

it("offers the same scoped row actions from the visible button and keyboard without opening the file", async () => {
  filesFixture();
  const attach = vi.fn();
  await mount({ onAttachPath: attach });
  const row = rowByName("README.txt");
  await click(row.querySelector("button")!);
  expect(fetchWorkspaceFile).not.toHaveBeenCalled();
  const link = document.querySelector<HTMLAnchorElement>('[role="menuitem"][download]')!;
  const query = new URL(link.href).searchParams;
  expect(query.get("path")).toBe("README.txt");
  expect(query.get("scope_key")).toBe("scope-a");
  expect(query.get("scope_url")).toBe("/repo");
  expect(query.get("download")).toBe("true");
  await menuAction("Attach as context");
  expect(attach).toHaveBeenCalledWith("README.txt");
  await act(async () =>
    row.dispatchEvent(new KeyboardEvent("keydown", { key: "F10", shiftKey: true, bubbles: true })),
  );
  expect(document.querySelector('[role="menu"]')).not.toBeNull();
  await act(async () =>
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "End", bubbles: true }),
    ),
  );
  expect(document.activeElement?.textContent).toBe("workspaceManage.delete");
  await act(async () =>
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
    ),
  );
  expect(document.activeElement).toBe(row);
});

it("locates a folder and the workspace root without reading them as files or replacing a draft", async () => {
  filesFixture();
  await mount();
  await click(rowByName("README.txt"));
  await edit("unsaved work");
  await enterPath("/repo/build/");
  expect(resolveWorkspacePath).toHaveBeenCalledWith("group-1", "build", "scope-a", "/repo");
  expect(rowByName("build").getAttribute("aria-expanded")).toBe("true");
  expect(document.activeElement).toBe(rowByName("build"));
  expect(mainArea().querySelector("textarea")?.value).toBe("unsaved work");
  await enterPath("/repo");
  expect(resolveWorkspacePath).toHaveBeenLastCalledWith("group-1", "", "scope-a", "/repo");
  expect(document.activeElement).toBe(panel().querySelector('[role="treeitem"]'));
  expect(fetchWorkspaceFile).toHaveBeenCalledTimes(1);
  expect(mainArea().querySelector("textarea")?.value).toBe("unsaved work");
});

it("shows lookup errors beside the path even with an open draft, and allows retry", async () => {
  filesFixture();
  await mount();
  await click(rowByName("README.txt"));
  await edit("unsaved work");
  resolveWorkspacePath.mockResolvedValueOnce({
    ok: false,
    error: { code: "NOT_FOUND", message: "Path not found: gone" },
  });
  await enterPath("gone");
  expect(panel().querySelector('[role="alert"]')?.textContent).toContain("Path not found: gone");
  expect(mainArea().querySelector("textarea")?.value).toBe("unsaved work");
  await enterPath("build");
  expect(panel().querySelector('[role="alert"]')).toBeNull();
  expect(document.activeElement).toBe(rowByName("build"));
});

it.each(["file", "scope", "lookup"])(
  "retires a delayed path lookup after newer %s navigation",
  async (next) => {
    filesFixture();
    await mount();
    await click(rowByName("README.txt"));
    let finish!: (value: unknown) => void;
    resolveWorkspacePath.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await enterPath("build");
    if (next === "file") await click(rowByName("README.txt"));
    else if (next === "scope") await render({ scopeKey: "scope-b", scopeUrl: "/other" });
    else await enterPath("src");
    await act(async () => finish({ ok: true, result: { path: "build", is_dir: true } }));
    expect(rowByName("build").getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).not.toBe(rowByName("build"));
    expect(fetchWorkspaceFile).toHaveBeenCalledTimes(1);
  },
);

it.each([
  ["workspace_write_conflict", "This file changed on disk"],
  ["permission_denied", "Write permission denied"],
])("keeps %s save feedback after locating a directory", async (code, expected) => {
  filesFixture();
  let finish!: (value: unknown) => void;
  saveWorkspaceFile.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await mount();
  await click(rowByName("README.txt"));
  await edit("unsaved work");
  await click(saveButton());
  await enterPath("src");
  await act(async () => finish({ ok: false, error: { code, message: "Write permission denied" } }));
  expect(mainArea().textContent).toContain(expected);
  expect(mainArea().querySelector("textarea")?.value).toBe("unsaved work");
  expect(saveButton().disabled).toBe(false);
});

it.each(["stay", "file", "editor", "menu", "keyboard"])(
  "handles a delayed reveal after the user chooses %s",
  async (next) => {
    filesFixture();
    const pending: Array<(value: unknown) => void> = [];
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) => {
      if (path === "build")
        return new Promise((resolve) => {
          pending.push(resolve);
        });
      return listing(path, [
        { name: "build", path: "build", is_dir: true },
        { name: "README.txt", path: "README.txt", is_dir: false },
      ]);
    });
    await mount();
    await enterPath("build/notes.txt");
    expect(pending.length).toBeGreaterThan(0);
    expect(mainArea().querySelector("textarea")?.value).toBe("disk");
    if (next === "file") {
      await click(rowByName("README.txt"));
      await act(async () => rowByName("README.txt").focus());
    } else if (next === "editor") {
      await act(async () => mainArea().querySelector("textarea")!.focus());
    } else if (next === "menu") {
      await optionsMenu();
    } else if (next === "keyboard") {
      await act(async () => {
        const folder = rowByName("build");
        folder.focus();
        folder.dispatchEvent(new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
      });
    }
    const focused = document.activeElement;
    await act(async () => {
      for (const finish of pending)
        finish(listing("build", [{ name: "notes.txt", path: "build/notes.txt", is_dir: false }]));
    });
    expect(document.activeElement).toBe(next === "stay" ? rowByName("notes.txt") : focused);
    if (next === "file") {
      expect(rowByName("README.txt").getAttribute("aria-selected")).toBe("true");
    }
  },
);

it("does not turn a drag starting on the row menu button into a file move", async () => {
  filesFixture();
  await mount();
  const row = rowByName("README.txt");
  await act(async () =>
    row.querySelector("button")!.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true })),
  );
  const drag = new Event("dragstart", { bubbles: true, cancelable: true });
  await act(async () => row.dispatchEvent(drag));
  expect(drag.defaultPrevented).toBe(true);
});

it("rejects a tree move from another instance origin even when Group and scope IDs match", async () => {
  filesFixture();
  await mount();
  const fetch = vi
    .spyOn(globalThis, "fetch")
    .mockResolvedValue(
      new Response(
        JSON.stringify({ ok: true, result: { path: "README.txt", destination: "src/README.txt" } }),
      ),
    );
  const event = new Event("drop", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", {
    value: {
      types: ["application/x-cccc-workspace-entry"],
      getData: () =>
        JSON.stringify({
          origin: "https://another-instance.example",
          groupId: "group-1",
          scopeKey: "scope-a",
          scopeUrl: "/repo",
          path: "README.txt",
          name: "README.txt",
        }),
    },
  });
  await act(async () => rowByName("src").dispatchEvent(event));
  expect(fetch).not.toHaveBeenCalled();
  expect(panel().textContent).toContain("workspaceManage.sameWorkspace");
  fetch.mockRestore();
});

it("rejects forged current-origin metadata as authority for a tree move", async () => {
  filesFixture();
  await mount();
  const fetch = vi.spyOn(globalThis, "fetch");
  const event = new Event("drop", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", {
    value: {
      types: ["application/x-cccc-workspace-entry"],
      getData: () =>
        JSON.stringify({
          origin: window.location.origin,
          groupId: "group-1",
          scopeKey: "scope-a",
          scopeUrl: "/repo",
          path: "README.txt",
          name: "README.txt",
        }),
    },
  });
  await act(async () => rowByName("src").dispatchEvent(event));
  expect(fetch).not.toHaveBeenCalled();
  fetch.mockRestore();
});

function dragEvent(type: string, data: Map<string, string>) {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", {
    value: {
      get types() {
        return [...data.keys()];
      },
      getData: (key: string) => data.get(key) || "",
      setData: (key: string, value: string) => data.set(key, value),
    },
  });
  return event;
}
it("accepts a local drag once and rejects its replay or canceled token", async () => {
  filesFixture();
  await mount();
  const fetch = vi
    .spyOn(globalThis, "fetch")
    .mockImplementation(
      async () =>
        new Response(
          JSON.stringify({
            ok: true,
            result: { path: "README.txt", destination: "src/README.txt", mime_type: "text/plain" },
          }),
        ),
    );
  const data = new Map<string, string>();
  await act(async () => rowByName("README.txt").dispatchEvent(dragEvent("dragstart", data)));
  expect(data.get("application/x-cccc-workspace-entry")).toBeTruthy();
  await act(async () => rowByName("src").dispatchEvent(dragEvent("drop", data)));
  expect(fetch).toHaveBeenCalledTimes(1);
  await act(async () => rowByName("src").dispatchEvent(dragEvent("drop", data)));
  expect(fetch).toHaveBeenCalledTimes(1);
  const canceled = new Map<string, string>();
  await act(async () => rowByName("README.txt").dispatchEvent(dragEvent("dragstart", canceled)));
  await act(async () => rowByName("README.txt").dispatchEvent(dragEvent("dragend", canceled)));
  await act(async () => rowByName("src").dispatchEvent(dragEvent("drop", canceled)));
  expect(fetch).toHaveBeenCalledTimes(1);
  fetch.mockRestore();
});
it("retires a drag when switching workspace scopes", async () => {
  filesFixture();
  await mount();
  const data = new Map<string, string>();
  await act(async () => rowByName("README.txt").dispatchEvent(dragEvent("dragstart", data)));
  await render({ scopeKey: "next-scope", scopeUrl: "/next-repo" });
  const fetch = vi.spyOn(globalThis, "fetch");
  await act(async () => rowByName("src").dispatchEvent(dragEvent("drop", data)));
  expect(fetch).not.toHaveBeenCalled();
  fetch.mockRestore();
});
