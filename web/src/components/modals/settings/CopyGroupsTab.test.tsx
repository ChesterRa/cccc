// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
const api = vi.hoisted(() => ({
  previewGroupCopy: vi.fn(),
  fetchDirSuggestions: vi.fn(),
  fetchDirContents: vi.fn(),
  cleanupGroupCopyUpload: vi.fn(),
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
