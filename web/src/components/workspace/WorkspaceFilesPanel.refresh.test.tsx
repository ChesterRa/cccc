// @vitest-environment happy-dom
import { act } from "react";
import { describe, expect, it } from "vite-plus/test";
import {
  ROOT_ITEMS,
  listing,
  mount,
  panel,
  mainArea,
  rowByName,
  saveButton,
  edit,
  click,
  host,
  fetchWorkspaceListing,
  fetchWorkspaceFile,
  saveWorkspaceFile,
} from "./workspaceFilesTestSupport";

describe("workspace files surfaces", () => {
  it("keeps saving available on the next file when an earlier save lands late", async () => {
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
      listing(path, [...ROOT_ITEMS, { name: "LICENSE", path: "LICENSE", is_dir: false }]),
    );
    fetchWorkspaceFile.mockImplementation(async (_group: string, path: string) => ({
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
    }));
    let releaseSave: (() => void) | null = null;
    saveWorkspaceFile.mockImplementation(
      () =>
        new Promise((resolve) => {
          releaseSave = () => resolve({ ok: true, result: { sha256: "saved", created: false } });
        }),
    );
    await mount();

    await click(rowByName("README.txt"));
    await edit("# edited\n");
    await click(saveButton());

    // The operator moves to another file while the first save is still in flight.
    await click(rowByName("LICENSE"));
    await act(async () => {
      releaseSave?.();
      await Promise.resolve();
    });

    // The retired save must not leave the new file stuck behind a disabled Save button.
    await edit("another edit\n");
    expect(saveButton().disabled).toBe(false);
  });

  it("keeps an unsaved draft when the ignored-file filter is toggled", async () => {
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
        sha256: "abc",
      },
    });
    await mount();

    await click(rowByName("README.txt"));
    await edit("# unsaved work\n");

    await click(panel().querySelector<HTMLElement>('[title="File browser options"]')!);
    await click(document.querySelector<HTMLElement>('[role="menuitemcheckbox"]')!);

    // The filter decides which rows the tree lists; it has no claim on the editor.
    expect(mainArea().querySelector("textarea")?.value).toBe("# unsaved work\n");
    expect(fetchWorkspaceListing.mock.calls.at(-1)?.[2]).toEqual({
      showIgnored: false,
      scopeKey: "scope-a",
      scopeUrl: "/repo",
    });
  });

  it("drops a listing that arrives after a refresh replaced it", async () => {
    fetchWorkspaceListing.mockImplementation(async (_group: string, path: string) =>
      listing(path, ROOT_ITEMS),
    );
    await mount();
    expect(panel().textContent).not.toContain("NEW.md");

    const pending: Array<(items: unknown[]) => void> = [];
    fetchWorkspaceListing.mockImplementation(
      (_group: string, path: string) =>
        new Promise((resolve) => {
          pending.push((items) => resolve(listing(path, items)));
        }),
    );
    const refresh = host.querySelector<HTMLElement>('[data-testid="refresh"]')!;
    await click(refresh);
    await click(refresh);
    expect(pending).toHaveLength(2);

    // The newest reload lands first and shows the file that was just created.
    await act(async () => {
      pending[1]?.([...ROOT_ITEMS, { name: "NEW.md", path: "NEW.md", is_dir: false }]);
      await Promise.resolve();
    });
    expect(panel().textContent).toContain("NEW.md");

    // The listing that refresh replaced arrives late; it must not roll the tree back.
    await act(async () => {
      pending[0]?.(ROOT_ITEMS);
      await Promise.resolve();
    });
    expect(panel().textContent).toContain("NEW.md");
  });
});
