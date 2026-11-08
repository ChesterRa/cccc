// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useComposerStore } from "../../stores/useComposerStore";
import { usePendingPageWork } from "../../stores/usePendingPageWork";
import { useWorkspaceNavigation } from "../../stores/workspaceNavigation";
import { reloadForNewBuild, reloadPage } from "./reloadPage";

afterEach(() => {
  vi.restoreAllMocks();
  useComposerStore.setState(useComposerStore.getInitialState(), true);
  usePendingPageWork.setState({ owners: new Set() });
  useWorkspaceNavigation.setState({ owners: new Set(), pending: null });
  window.history.replaceState(null, "", "/");
});

it("reports the current unsent draft before asking the trusted entry to reopen", () => {
  const parent = { postMessage: vi.fn() } as unknown as Window;
  vi.spyOn(window, "parent", "get").mockReturnValue(parent);
  const proof = btoa(
    JSON.stringify({
      frame_id: "frame",
      target_instance_id: "target",
      target_device_id: "device",
      parent_origin: "https://entry.test",
      expires_at: "2099-01-01T00:00:00Z",
      signature: "fixture",
    }),
  );
  window.history.replaceState(null, "", `/ui/connect?proof=${encodeURIComponent(proof)}`);
  useComposerStore.setState({ composerText: "unsent" });
  reloadPage();
  expect(parent.postMessage).toHaveBeenNthCalledWith(
    1,
    expect.objectContaining({ type: "workspace_dirty", dirty: true, frame_id: "frame" }),
    "https://entry.test",
  );
  expect(parent.postMessage).toHaveBeenNthCalledWith(
    2,
    expect.objectContaining({ type: "reload", frame_id: "frame" }),
    "https://entry.test",
  );
  vi.mocked(parent.postMessage).mockClear();
  useComposerStore.setState({ activeGroupId: "a" });
  useComposerStore.getState().switchGroup("a", "b");
  reloadPage();
  expect(parent.postMessage).toHaveBeenNthCalledWith(
    1,
    expect.objectContaining({ type: "workspace_dirty", dirty: true }),
    "https://entry.test",
  );
  vi.mocked(parent.postMessage).mockClear();
  useComposerStore.getState().switchGroup("b", "a");
  useComposerStore.getState().setComposerText("");
  // The saved copy for the active Group can be older than the cleared editor.
  reloadPage();
  expect(parent.postMessage).toHaveBeenNthCalledWith(
    1,
    expect.objectContaining({ type: "workspace_dirty", dirty: false }),
    "https://entry.test",
  );
});

it("reloads for a newer build only while the page holds no unsaved work, once per cooldown", () => {
  const parent = { postMessage: vi.fn() } as unknown as Window;
  vi.spyOn(window, "parent", "get").mockReturnValue(parent);
  const proof = btoa(
    JSON.stringify({
      frame_id: "frame",
      target_instance_id: "target",
      target_device_id: "device",
      parent_origin: "https://entry.test",
      expires_at: "2099-01-01T00:00:00Z",
      signature: "fixture",
    }),
  );
  window.history.replaceState(null, "", `/ui/connect?proof=${encodeURIComponent(proof)}`);
  const values = new Map<string, string>();
  const storage = {
    getItem: (key: string) => values.get(key) ?? null,
    setItem: (key: string, value: string) => void values.set(key, value),
  } as Storage;
  const reloads = () =>
    vi.mocked(parent.postMessage).mock.calls.filter(([message]) => message.type === "reload")
      .length;

  // An unsent draft, unsaved workspace edits or other page work hold the reload back.
  useComposerStore.setState({ composerText: "unsent" });
  expect(reloadForNewBuild(storage, 1_000)).toBe(false);
  useComposerStore.setState({ composerText: "" });
  useWorkspaceNavigation.setState({ owners: new Set([Symbol("draft")]) });
  reloadForNewBuild(storage, 2_000);
  useWorkspaceNavigation.setState({ owners: new Set() });
  usePendingPageWork.setState({ owners: new Set([Symbol("recording")]) });
  reloadForNewBuild(storage, 3_000);
  expect(reloads()).toBe(0);

  // A held-back reload claims no cooldown, so the first reload once the work is gone happens.
  usePendingPageWork.setState({ owners: new Set() });
  expect(reloadForNewBuild(storage, 4_000)).toBe(true);
  expect(reloads()).toBe(1);
  expect(reloadForNewBuild(storage, 5_000)).toBe(false);
  expect(reloads()).toBe(1);
});
