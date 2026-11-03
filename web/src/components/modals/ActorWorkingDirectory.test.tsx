// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { fetchDirContents } from "../../services/api/system";
import { ActorWorkingDirectory } from "./ActorWorkingDirectory";

const api = vi.hoisted(() => ({
  createDirectory: vi.fn(),
  fetchDirContents: vi.fn(),
  fetchDirSuggestions: vi.fn(),
  attachScope: vi.fn(),
}));
vi.mock("../../services/api", () => api);
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string, values?: { path?: string }) => (values?.path ? `${key}: ${values.path}` : key),
  }),
}));

describe("Actor working directory", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const button = (label: string) =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (node) => node.textContent === label,
    )!;
  const fill = async (input: HTMLInputElement, value: string) => {
    await act(async () => {
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(input, value);
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };
  const listing = (path: string, readable: boolean) => {
    vi.mocked(globalThis.fetch).mockResolvedValueOnce(
      new Response(
        JSON.stringify({
          ok: true,
          result: {
            path,
            parent: path.slice(0, path.lastIndexOf("/")) || "/",
            items: [],
            readable,
          },
        }),
        { headers: { "content-type": "application/json" } },
      ),
    );
  };
  beforeEach(async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.clearAllMocks();
    api.fetchDirSuggestions.mockResolvedValue({ ok: true, result: { suggestions: [] } });
    api.fetchDirContents.mockImplementation(fetchDirContents);
    vi.spyOn(globalThis, "fetch");
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () =>
      root.render(
        <ActorWorkingDirectory
          groupId="g1"
          scopes={[{ scope_key: "s1", url: "/src" }]}
          activeScopeKey="s1"
          value=""
          onChange={vi.fn()}
          onScopeAttached={vi.fn()}
          onBusyChange={vi.fn()}
          disabled={false}
          hint=""
        />,
      ),
    );
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.restoreAllMocks();
  });

  it("does not attach a stale draft when a newly created folder cannot be read", async () => {
    listing("/src", true);
    listing("/src/fresh", false);
    listing("/src/fresh", true);
    api.createDirectory.mockResolvedValue({ ok: true, result: { path: "/src/fresh" } });
    api.attachScope.mockResolvedValue({
      ok: true,
      result: { group_id: "g1", scope_key: "s2", scope: { scope_key: "s2", url: "/src/fresh" } },
    });
    await act(async () => button("workingDirectoryAdd").click());
    await fill(
      host.querySelector<HTMLInputElement>('input[aria-label="workingDirectoryPath"]')!,
      "/other",
    );
    await act(async () => button("createGroup.newFolder").click());
    await fill(
      host.querySelector<HTMLInputElement>('input[aria-label="createGroup.folderName"]')!,
      "fresh",
    );
    await act(async () => button("createGroup.createFolder").click());
    expect(host.querySelector('[role="alert"]')?.textContent).toContain("/src/fresh");
    expect(host.textContent).not.toContain("createGroup.noSubdirectories");
    expect(button("workingDirectoryUse").disabled).toBe(true);
    await act(async () => button("workingDirectoryUse").click());
    expect(api.attachScope).not.toHaveBeenCalled();
    await act(async () => button("common:retry").click());
    expect(
      host.querySelector<HTMLInputElement>('input[aria-label="workingDirectoryPath"]')!.value,
    ).toBe("/src/fresh");
    await act(async () => button("workingDirectoryUse").click());
    expect(api.attachScope).toHaveBeenCalledWith("g1", "/src/fresh", { activate: false });
  });

  it("allows a new manual path after the old listing failed without reviving its actions", async () => {
    listing("/src", false);
    api.attachScope.mockResolvedValue({
      ok: true,
      result: { group_id: "g1", scope_key: "s2", scope: { scope_key: "s2", url: "/manual" } },
    });
    await act(async () => button("workingDirectoryAdd").click());
    expect(host.querySelector('[role="alert"]')).not.toBeNull();
    expect(button("workingDirectoryUse").disabled).toBe(true);
    await fill(
      host.querySelector<HTMLInputElement>('input[aria-label="workingDirectoryPath"]')!,
      "/manual",
    );
    expect(host.querySelector('[role="alert"]')).toBeNull();
    expect(button("workingDirectoryUse").disabled).toBe(false);
    expect(button("createGroup.newFolder")).toBeUndefined();
    expect(host.textContent).not.toContain("createGroup.noSubdirectories");
    await act(async () => button("workingDirectoryUse").click());
    expect(api.attachScope).toHaveBeenCalledExactlyOnceWith("g1", "/manual", { activate: false });
    expect(api.fetchDirContents).toHaveBeenCalledExactlyOnceWith("/src");
  });
});
