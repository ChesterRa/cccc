// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useComposerStore } from "../../stores/useComposerStore";
import { usePendingPageWork } from "../../stores/usePendingPageWork";
import { useWorkspaceNavigation } from "../../stores/workspaceNavigation";
import { reloadPage } from "./reloadPage";

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
});
