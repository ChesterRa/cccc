// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
const api = vi.hoisted(() => ({
  previewGroupCopy: vi.fn(),
  fetchDirSuggestions: vi.fn(),
  fetchDirContents: vi.fn(),
  cleanupGroupCopyUpload: vi.fn(),
  exportGroupCopy: vi.fn(),
  importGroupCopy: vi.fn(),
}));
vi.mock("../../../services/api", () => api);
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
import { CopyGroupsTab } from "./CopyGroupsTab";

it("keeps the last chosen directory when earlier navigation finishes late", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const response = (path: string) => ({
    ok: true,
    result: {
      path,
      parent: "/root",
      items:
        path === "/root"
          ? ["A", "B"].map((name) => ({ name, path: `/root/${name}`, is_dir: true }))
          : [],
    },
  });
  let resolveA!: (value: ReturnType<typeof response>) => void;
  let resolveB!: (value: ReturnType<typeof response>) => void;
  const pendingA = new Promise<ReturnType<typeof response>>((resolve) => {
    resolveA = resolve;
  });
  const pendingB = new Promise<ReturnType<typeof response>>((resolve) => {
    resolveB = resolve;
  });
  api.previewGroupCopy.mockResolvedValue({
    ok: true,
    result: {
      upload_id: "test-upload",
      preview: { source_workspace_root: "/root", source_title: "Test", actors: [] },
    },
  });
  api.fetchDirSuggestions.mockResolvedValue({ ok: true, result: { suggestions: [] } });
  api.fetchDirContents.mockImplementation((path: string) =>
    path === "/root/A" ? pendingA : path === "/root/B" ? pendingB : Promise.resolve(response(path)),
  );
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const button = (label: string) =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (node) => node.textContent === label,
    )!;
  try {
    await act(async () => root.render(<CopyGroupsTab isDark={false} groupId="test" />));
    await act(async () => {
      const input = host.querySelector<HTMLInputElement>('input[type="file"]')!;
      Object.defineProperty(input, "files", { value: [new File(["fixture"], "test.zip")] });
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await act(async () => button("copyGroups.chooseFolder").click());
    await act(async () => button("A").click());
    await act(async () => button("B").click());
    expect(button("copyGroups.useFolder").disabled).toBe(true);
    await act(async () => resolveB(response("/root/B")));
    expect(button("copyGroups.useFolder").disabled).toBe(false);
    await act(async () => resolveA(response("/root/A")));
    await act(async () => button("copyGroups.useFolder").click());
    expect(host.querySelector<HTMLInputElement>('input[placeholder="/root"]')!.value).toBe(
      "/root/B",
    );
    expect(api.fetchDirContents.mock.calls.map(([path]) => path)).toEqual([
      "/root",
      "/root/A",
      "/root/B",
    ]);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

async function mountTab(groupId?: string) {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => root.render(<CopyGroupsTab isDark={false} groupId={groupId} />));
  const button = (label: string) =>
    [...host.querySelectorAll<HTMLButtonElement>("button")].find(
      (node) => node.textContent === label,
    );
  const unmount = async () => {
    await act(async () => root.unmount());
    host.remove();
  };
  return { host, button, unmount };
}

async function choosePackage(host: HTMLElement, file: File) {
  api.previewGroupCopy.mockResolvedValue({
    ok: true,
    result: {
      upload_id: "upload-1",
      preview: { source_workspace_root: "/src", source_title: "Source", actors: [] },
    },
  });
  await act(async () => {
    const input = host.querySelector<HTMLInputElement>('input[type="file"]')!;
    Object.defineProperty(input, "files", { value: [file], configurable: true });
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
}

it("exports from the export card's actions and only inside a group", async () => {
  const outside = await mountTab();
  const card = (host: HTMLElement) =>
    [...host.querySelectorAll("[data-settings-card]")].find((node) =>
      node.textContent?.includes("copyGroups.exportHeading"),
    )!;
  const exportOutside = card(outside.host).querySelector("button")!;
  expect(exportOutside.textContent).toBe("copyGroups.exportButton");
  expect(exportOutside.disabled).toBe(true);
  expect(exportOutside.title).toBe("copyGroups.openFromGroup");
  await outside.unmount();

  const inside = await mountTab("g1");
  api.exportGroupCopy.mockResolvedValue({ ok: false, error: { message: "nope" } });
  await act(async () => card(inside.host).querySelector("button")!.click());
  expect(api.exportGroupCopy).toHaveBeenCalledWith("g1");
  expect(inside.host.querySelector('[role="alert"]')?.textContent).toBe("nope");
  await inside.unmount();
});

it("imports only a previewed package, and only after the user confirms", async () => {
  const { host, button, unmount } = await mountTab("g1");
  try {
    expect(button("copyGroups.importButton")!.disabled).toBe(true);
    expect(host.querySelectorAll("[data-settings-row]")).toHaveLength(0);

    const file = new File(["zip"], "pack.zip");
    await choosePackage(host, file);
    const rowLabels = [...host.querySelectorAll("[data-settings-row] label")].map(
      (label) => label.textContent,
    );
    expect(rowLabels).toEqual(["copyGroups.workspaceRoot", "copyGroups.groupTitle"]);
    const importButton = button("copyGroups.importButton")!;
    expect(importButton.disabled).toBe(false);

    const confirm = vi.fn().mockReturnValueOnce(false);
    vi.stubGlobal("confirm", confirm);
    await act(async () => importButton.click());
    expect(confirm).toHaveBeenCalledWith("copyGroups.importConfirm");
    expect(api.importGroupCopy).not.toHaveBeenCalled();

    confirm.mockReturnValueOnce(true);
    api.importGroupCopy.mockResolvedValue({ ok: false, error: { message: "denied" } });
    await act(async () => importButton.click());
    expect(api.importGroupCopy).toHaveBeenCalledWith(
      { file, uploadId: "upload-1" },
      "/src",
      "Source",
    );
  } finally {
    vi.unstubAllGlobals();
    await unmount();
  }
});

it("reset drops the chosen package and releases its staged upload", async () => {
  const { host, button, unmount } = await mountTab("g1");
  try {
    expect(button("common:reset")).toBeUndefined();
    await choosePackage(host, new File(["zip"], "pack.zip"));
    await act(async () => button("common:reset")!.click());
    expect(api.cleanupGroupCopyUpload).toHaveBeenCalledWith("upload-1");
    expect(host.querySelectorAll("[data-settings-row]")).toHaveLength(0);
    expect(button("copyGroups.importButton")!.disabled).toBe(true);
    expect(button("common:reset")).toBeUndefined();
  } finally {
    await unmount();
  }
});
