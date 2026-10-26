// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import type { GroupCopyPreview } from "../../../services/api/groups";
import enSettings from "../../../i18n/locales/en/settings.json";
const api = vi.hoisted(() => ({
  previewGroupCopy: vi.fn(),
  fetchDirSuggestions: vi.fn(),
  fetchDirContents: vi.fn(),
  cleanupGroupCopyUpload: vi.fn(),
  exportGroupCopy: vi.fn(),
  importGroupCopy: vi.fn(),
}));
const groupStore = vi.hoisted(() => ({
  selectedGroupId: null as string | null,
  refreshGroups: vi.fn(),
  setSelectedGroupId: vi.fn(),
  loadGroup: vi.fn(),
}));
vi.mock("../../../services/api", () => api);
vi.mock("../../../stores/useGroupStore", () => ({
  useGroupStore: Object.assign(
    (selector: (state: typeof groupStore) => unknown) => selector(groupStore),
    { getState: () => groupStore },
  ),
}));
vi.mock("react-i18next", () => ({
  useTranslation: () => ({
    t: (key: string) =>
      key === "copyGroups.reconnectNativeSessions"
        ? enSettings.copyGroups.reconnectNativeSessions
        : key,
  }),
}));
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
  groupStore.selectedGroupId = groupId || null;
  groupStore.refreshGroups.mockResolvedValue(undefined);
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
  const selectGroup = async (nextGroupId: string) => {
    groupStore.selectedGroupId = nextGroupId;
    await act(async () => root.render(<CopyGroupsTab isDark={false} groupId={nextGroupId} />));
  };
  return { host, button, unmount, selectGroup };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

async function choosePackage(
  host: HTMLElement,
  file: File,
  preview: GroupCopyPreview = { source_workspace_root: "/src", source_title: "Source", actors: [] },
) {
  api.previewGroupCopy.mockResolvedValue({ ok: true, result: { upload_id: "upload-1", preview } });
  await act(async () => {
    const input = host.querySelector<HTMLInputElement>('input[type="file"]')!;
    Object.defineProperty(input, "files", { value: [file], configurable: true });
    input.dispatchEvent(new Event("change", { bubbles: true }));
  });
}

it.each([
  { name: "native Actor", nativeSession: true, requiresReconnect: false, visible: true },
  {
    name: "native reconnect summary",
    nativeSession: false,
    requiresReconnect: true,
    visible: true,
  },
  { name: "managed Codex Actor", nativeSession: false, requiresReconnect: false, visible: false },
])(
  "previews paused native access from explicit flags, not runtime ($name)",
  async ({ nativeSession, requiresReconnect, visible }) => {
    const { host, unmount } = await mountTab("source-group");
    try {
      await choosePackage(host, new File(["zip"], "same-instance.zip"), {
        source_group_id: "source-group",
        source_title: "Source",
        source_workspace_root: "/src",
        actors: [{ id: "actor", runtime: "codex", native_session: nativeSession }],
        requires_reconnect: { native_sessions: requiresReconnect },
      });
      const notices = [...host.querySelectorAll("[data-copy-groups-preview] li")].filter((node) =>
        node.textContent?.includes("Native CCCC access imports paused"),
      );
      expect(notices).toHaveLength(visible ? 1 : 0);
      if (visible) {
        expect(notices[0].textContent).toContain("even in the same instance");
        expect(notices[0].textContent).toContain(
          "Explicitly resume access or connect native sessions",
        );
        expect(notices[0].textContent).toContain("source Group is unchanged");
      }
      expect(api.importGroupCopy).not.toHaveBeenCalled();
      expect(groupStore.setSelectedGroupId).not.toHaveBeenCalled();
      expect(groupStore.selectedGroupId).toBe("source-group");
    } finally {
      await unmount();
    }
  },
);

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

it("delivers an accepted export after the settings page closes", async () => {
  const exported = deferred<{ ok: true; result: { filename: string; blob: Blob } }>();
  const blob = new Blob(["synthetic ZIP"]);
  const createUrl = vi.spyOn(URL, "createObjectURL").mockReturnValue("blob:synthetic-export");
  const revokeUrl = vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
  const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
  const { button, unmount } = await mountTab("g1");
  api.exportGroupCopy.mockReturnValue(exported.promise);
  try {
    await act(async () => button("copyGroups.exportButton")!.click());
    await unmount();
    await act(async () =>
      exported.resolve({ ok: true, result: { filename: "synthetic.zip", blob } }),
    );
    expect(createUrl).toHaveBeenCalledWith(blob);
    expect(click).toHaveBeenCalledTimes(1);
    expect((click.mock.instances[0] as HTMLAnchorElement).download).toBe("synthetic.zip");
    expect(revokeUrl).toHaveBeenCalledWith("blob:synthetic-export");
  } finally {
    vi.restoreAllMocks();
  }
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

it("keeps a package preview pending across Group selection changes", async () => {
  const { host, button, selectGroup, unmount } = await mountTab("g1");
  const preview = deferred<{
    ok: true;
    result: { upload_id: string; preview: { source_title: string; actors: [] } };
  }>();
  api.previewGroupCopy.mockReturnValue(preview.promise);
  try {
    await act(async () => {
      const input = host.querySelector<HTMLInputElement>('input[type="file"]')!;
      Object.defineProperty(input, "files", { value: [new File(["zip"], "pack.zip")] });
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await selectGroup("g2");
    expect(host.querySelector<HTMLInputElement>('input[type="file"]')!.disabled).toBe(true);
    await act(async () => {
      preview.resolve({
        ok: true,
        result: { upload_id: "preview-upload", preview: { source_title: "Source", actors: [] } },
      });
    });
    expect(button("copyGroups.importButton")!.disabled).toBe(false);
    expect(api.cleanupGroupCopyUpload).not.toHaveBeenCalled();
  } finally {
    await unmount();
  }
});

it("finishes one accepted import after switching Groups without redirecting the new selection", async () => {
  const { host, button, selectGroup, unmount } = await mountTab("g1");
  const imported = deferred<{ ok: true; result: { group_id: string } }>();
  api.importGroupCopy.mockReturnValue(imported.promise);
  vi.stubGlobal("confirm", () => true);
  try {
    await choosePackage(host, new File(["zip"], "pack.zip"));
    await act(async () => button("copyGroups.importButton")!.click());
    await selectGroup("g2");
    expect(button("copyGroups.importButton")!.disabled).toBe(true);
    await act(async () => button("copyGroups.importButton")!.click());
    expect(api.importGroupCopy).toHaveBeenCalledTimes(1);
    await act(async () => imported.resolve({ ok: true, result: { group_id: "imported" } }));
    expect(groupStore.refreshGroups).toHaveBeenCalledTimes(1);
    expect(groupStore.setSelectedGroupId).not.toHaveBeenCalled();
    expect(groupStore.loadGroup).not.toHaveBeenCalled();
    expect(host.textContent).toContain("copyGroups.imported");
    expect(button("copyGroups.importButton")!.disabled).toBe(true);
    expect(button("common:reset")).toBeUndefined();
    expect(host.querySelector<HTMLInputElement>('input[type="file"]')!.disabled).toBe(false);
  } finally {
    vi.unstubAllGlobals();
    await unmount();
  }
});

it("opens the imported Group after a successful import while the selection is unchanged", async () => {
  const { host, button, unmount } = await mountTab("g1");
  api.importGroupCopy.mockResolvedValue({ ok: true, result: { group_id: "imported" } });
  vi.stubGlobal("confirm", () => true);
  try {
    await choosePackage(host, new File(["zip"], "pack.zip"));
    await act(async () => button("copyGroups.importButton")!.click());
    expect(groupStore.refreshGroups).toHaveBeenCalledTimes(1);
    expect(groupStore.setSelectedGroupId).toHaveBeenCalledWith("imported");
    expect(groupStore.loadGroup).toHaveBeenCalledWith("imported");
  } finally {
    vi.unstubAllGlobals();
    await unmount();
  }
});

it("does not redirect when the selection changes during the post-import refresh", async () => {
  const { host, button, selectGroup, unmount } = await mountTab("g1");
  const refresh = deferred<void>();
  groupStore.refreshGroups.mockReturnValue(refresh.promise);
  api.importGroupCopy.mockResolvedValue({ ok: true, result: { group_id: "imported" } });
  vi.stubGlobal("confirm", () => true);
  try {
    await choosePackage(host, new File(["zip"], "pack.zip"));
    await act(async () => button("copyGroups.importButton")!.click());
    expect(groupStore.refreshGroups).toHaveBeenCalledTimes(1);
    await selectGroup("g2");
    await act(async () => refresh.resolve());
    expect(groupStore.setSelectedGroupId).not.toHaveBeenCalled();
    expect(groupStore.loadGroup).not.toHaveBeenCalled();
  } finally {
    vi.unstubAllGlobals();
    await unmount();
  }
});

it("refreshes Groups after an accepted import completes with its settings page closed", async () => {
  const { host, button, unmount } = await mountTab("g1");
  const imported = deferred<{ ok: true; result: { group_id: string } }>();
  api.importGroupCopy.mockReturnValue(imported.promise);
  vi.stubGlobal("confirm", () => true);
  try {
    await choosePackage(host, new File(["zip"], "pack.zip"));
    await act(async () => button("copyGroups.importButton")!.click());
  } finally {
    vi.unstubAllGlobals();
    await unmount();
  }
  await act(async () => imported.resolve({ ok: true, result: { group_id: "imported" } }));
  expect(groupStore.refreshGroups).toHaveBeenCalledTimes(1);
  expect(groupStore.setSelectedGroupId).not.toHaveBeenCalled();
  expect(groupStore.loadGroup).not.toHaveBeenCalled();
});
