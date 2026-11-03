// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useFormStore } from "../../stores";
import { CreateGroupDirectoryBrowser } from "./CreateGroupDirectoryBrowser";
import { useCreateGroupDirectoryBrowser } from "./useCreateGroupDirectoryBrowser";
import { fetchDirContents } from "../../services/api/system";

const api = vi.hoisted(() => ({ createDirectory: vi.fn(), fetchDirContents: vi.fn() }));
vi.mock("../../services/api", () => api);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { path?: string }) => (values?.path ? `${key}: ${values.path}` : key),
  }),
}));

function DirectoryBrowser() {
  const browser = useCreateGroupDirectoryBrowser();
  const dirItems = useFormStore((state) => state.dirItems);
  const currentDir = useFormStore((state) => state.currentDir);
  const parentDir = useFormStore((state) => state.parentDir);
  return (
    <CreateGroupDirectoryBrowser
      dirItems={dirItems}
      currentDir={currentDir}
      parentDir={parentDir}
      driveLocations={[]}
      error={browser.error}
      createError={browser.createError}
      onClearCreateError={browser.clearCreateError}
      onRetry={browser.retryPath ? () => void browser.retryCreatedDirectory() : undefined}
      creatingDirectory={browser.creating}
      onSelect={() => {}}
      onFetch={(path) => void browser.fetchContents(path)}
      onCreateDirectory={browser.createDirectory}
    />
  );
}

describe("create-group directory browser", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const button = (label: string) =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (node) => node.textContent === label,
    );
  const enterName = async (name: string) => {
    const input = host.querySelector<HTMLInputElement>(
      'input[aria-label="createGroup.folderName"]',
    )!;
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, name);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };
  beforeEach(async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.clearAllMocks();
    useFormStore.setState({
      currentDir: "/src",
      parentDir: "/",
      dirItems: [],
      createGroupPath: "",
    });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root.render(<DirectoryBrowser />));
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.restoreAllMocks();
  });

  it("keeps a rejected folder name editable and selects a successful retry", async () => {
    api.createDirectory
      .mockResolvedValueOnce({
        ok: false,
        error: { code: "ALREADY_EXISTS", message: "Path already exists" },
      })
      .mockResolvedValueOnce({ ok: true, result: { path: "/src/fresh" } });
    api.fetchDirContents.mockResolvedValue({
      ok: true,
      result: { path: "/src/fresh", parent: "/src", items: [] },
    });
    await act(async () => button("createGroup.newFolder")!.click());
    await enterName("existing");
    await act(async () => button("createGroup.createFolder")!.click());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(
      "modals:createGroup.folderAlreadyExists",
    );
    expect(
      host.querySelector<HTMLInputElement>('input[aria-label="createGroup.folderName"]')?.value,
    ).toBe("existing");
    await enterName("fresh");
    await act(async () => button("createGroup.createFolder")!.click());
    expect(api.createDirectory).toHaveBeenLastCalledWith("/src", "fresh");
    expect(useFormStore.getState().createGroupPath).toBe("/src/fresh");
    expect(host.querySelector('[role="alert"]')).toBeNull();
  });

  it("keeps a folder it may not list open, empty, with the way back up", async () => {
    api.fetchDirContents.mockImplementation(fetchDirContents);
    vi.spyOn(globalThis, "fetch").mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          ok: true,
          result: { path: "/src/other", parent: "/src", items: [], readable: false },
        }),
        { headers: { "content-type": "application/json" } },
      ),
    );
    await act(async () =>
      useFormStore.setState({ dirItems: [{ name: "other", path: "/src/other", is_dir: true }] }),
    );
    await act(async () => button("other")!.click());
    expect(useFormStore.getState().currentDir).toBe("/src/other");
    expect(useFormStore.getState().parentDir).toBe("/src");
    expect(host.textContent).toContain("createGroup.noSubdirectories");
    expect(host.querySelector('[role="alert"]')).toBeNull();
  });

  it("retries opening an already created folder without creating it again", async () => {
    api.createDirectory.mockResolvedValue({ ok: true, result: { path: "/src/fresh" } });
    api.fetchDirContents.mockImplementation(fetchDirContents);
    const fetch = vi.spyOn(globalThis, "fetch");
    for (const listed of [false, false, true]) {
      fetch.mockResolvedValueOnce(
        new Response(
          JSON.stringify(
            listed
              ? { ok: true, result: { path: "/src/fresh", parent: "/src", items: [] } }
              : { ok: false, error: { code: "io_error", message: "Fixture listing failure" } },
          ),
          { headers: { "content-type": "application/json" } },
        ),
      );
    }
    await act(async () => button("createGroup.newFolder")!.click());
    await enterName("fresh");
    await act(async () => button("createGroup.createFolder")!.click());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(
      "createGroup.createdFolderUnreadable: /src/fresh",
    );
    expect(host.textContent).not.toContain("createGroup.noSubdirectories");
    await act(async () => button("common:retry")!.click());
    expect(api.fetchDirContents).toHaveBeenLastCalledWith("/src/fresh");
    expect(api.createDirectory).toHaveBeenCalledTimes(1);
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(
      "createGroup.createdFolderUnreadable: /src/fresh",
    );
    await act(async () => button("common:retry")!.click());
    expect(host.querySelector('[role="alert"]')).toBeNull();
  });
});
