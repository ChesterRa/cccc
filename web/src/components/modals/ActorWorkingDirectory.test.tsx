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
  // A listing that fails; a folder the server may not list still lists, empty.
  const listing = (path: string, listed: boolean) => {
    vi.mocked(globalThis.fetch).mockResolvedValueOnce(
      new Response(
        JSON.stringify(
          listed
            ? {
                ok: true,
                result: { path, parent: path.slice(0, path.lastIndexOf("/")) || "/", items: [] },
              }
            : { ok: false, error: { code: "io_error", message: "Fixture listing failure" } },
        ),
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

  describe("as one summary line", () => {
    const renderCollapsible = async (
      scopes: { scope_key: string; url: string }[],
      value: string,
      onChange = vi.fn(),
    ) =>
      act(async () =>
        root.render(
          <ActorWorkingDirectory
            key="collapsible"
            collapsible
            groupId="g1"
            scopes={scopes}
            activeScopeKey="s1"
            value={value}
            onChange={onChange}
            onScopeAttached={vi.fn()}
            onBusyChange={vi.fn()}
            disabled={false}
          />,
        ),
      );
    const scopes = [
      { scope_key: "s1", url: "/src" },
      { scope_key: "s2", url: "/other" },
    ];

    it("shows where the Agent will start and hides the controls until asked", async () => {
      await renderCollapsible(scopes, "");

      expect(host.textContent).toContain("followGroupDirectory · /src");
      expect(host.querySelector('[role="combobox"]')).toBeNull();
      expect(button("workingDirectoryAdd")).toBeUndefined();

      await act(async () => button("workingDirectoryChange").click());

      expect(host.querySelector('[role="combobox"]')).not.toBeNull();
      expect(button("workingDirectoryAdd")).toBeDefined();
      expect(button("workingDirectoryChange")).toBeUndefined();
    });

    it("summarises a fixed directory by its own path", async () => {
      await renderCollapsible(scopes, "s2");

      expect(host.textContent).toContain("fixedDirectory · /other");
    });

    it("stays open when the chosen directory is no longer attached", async () => {
      await renderCollapsible(scopes, "gone");

      expect(button("workingDirectoryChange")).toBeUndefined();
      expect(host.querySelector('[role="combobox"]')).not.toBeNull();
      expect(host.textContent).toContain("workingDirectoryUnavailable");
    });
  });
});
