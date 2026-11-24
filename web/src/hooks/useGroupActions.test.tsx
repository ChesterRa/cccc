// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import i18n, { i18nReady } from "../i18n";
import { useGroupActions } from "./useGroupActions";
import { useGroupStore, useUIStore } from "../stores";
import * as api from "../services/api";

vi.mock("../services/api", () => ({
  startGroup: vi.fn(),
  stopGroup: vi.fn(),
  setGroupState: vi.fn(),
  deleteGroup: vi.fn(),
}));
Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
const initialGroup = useGroupStore.getState();
const initialUI = useUIStore.getState();
let root: Root;
let host: HTMLDivElement;
let result: ReturnType<typeof useGroupActions>;
const refreshGroups = vi.fn(async () => {});
const refreshActors = vi.fn(async () => {});
const showError = vi.fn();
function Probe() {
  result = useGroupActions();
  return null;
}
function success(groupId: string, running = true) {
  return { ok: true as const, result: { group: { group_id: groupId, running } } };
}

describe("Group run actions", () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    useGroupStore.setState({
      groups: [
        { group_id: "g1", title: "Current" },
        { group_id: "g2", title: "Background" },
      ],
      selectedGroupId: "g1",
      groupDoc: { group_id: "g1", state: "active" },
      refreshGroups,
      refreshActors,
    });
    useUIStore.setState({ busy: "", showError });
    vi.mocked(api.startGroup).mockResolvedValue(success("g2"));
    vi.mocked(api.stopGroup).mockResolvedValue(success("g2", false));
    vi.mocked(api.setGroupState).mockResolvedValue(success("g2"));
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root.render(<Probe />));
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    useGroupStore.setState(initialGroup, true);
    useUIStore.setState(initialUI, true);
  });

  it("targets the clicked background Group, refreshes it, and leaves the selected document alone", async () => {
    await act(async () => result.handleGroupControl("g2", "pause"));
    expect(api.setGroupState).toHaveBeenCalledExactlyOnceWith("g2", "paused");
    expect(refreshActors).toHaveBeenCalledExactlyOnceWith("g2");
    expect(refreshGroups).toHaveBeenCalledOnce();
    expect(useGroupStore.getState().selectedGroupId).toBe("g1");
    expect(useGroupStore.getState().groupDoc).toEqual({ group_id: "g1", state: "active" });
    expect(useUIStore.getState().busy).toBe("");
  });

  it.each([true, false])(
    "resumes according to the target's authoritative runtime response: running=%s",
    async (running) => {
      vi.mocked(api.setGroupState).mockResolvedValue({
        ok: true,
        result: {
          group: {
            group_id: "g2",
            running: !running,
            runtime_status: {
              lifecycle_state: "active",
              runtime_running: running,
              running_actor_count: running ? 1 : 0,
              has_running_foreman: running,
            },
          },
        },
      });
      await act(async () => result.handleGroupControl("g2", "activate"));
      expect(api.setGroupState).toHaveBeenCalledExactlyOnceWith("g2", "active");
      expect(api.startGroup).toHaveBeenCalledTimes(running ? 0 : 1);
      if (!running) expect(api.startGroup).toHaveBeenCalledWith("g2");
    },
  );

  it("does not start agents when a successful configuration response has no runtime observation", async () => {
    vi.mocked(api.setGroupState).mockResolvedValue({
      ok: true,
      result: { group: { group_id: "g2", state: "active" } },
    });
    await act(async () => result.handleGroupControl("g2", "activate"));
    expect(api.setGroupState).toHaveBeenCalledExactlyOnceWith("g2", "active");
    expect(api.startGroup).not.toHaveBeenCalled();
    expect(refreshActors).toHaveBeenCalledExactlyOnceWith("g2");
    expect(showError).not.toHaveBeenCalled();
  });

  it("does not start agents when the daemon could not observe the Group's runtime", async () => {
    vi.mocked(api.setGroupState).mockResolvedValue({
      ok: true,
      result: {
        group: {
          group_id: "g2",
          state: "active",
          running: false,
          runtime_status: {
            lifecycle_state: "active",
            available: false,
            runtime_running: false,
            running_actor_count: 0,
            has_running_foreman: false,
          },
        },
      },
    });
    await act(async () => result.handleGroupControl("g2", "activate"));
    expect(api.setGroupState).toHaveBeenCalledExactlyOnceWith("g2", "active");
    expect(api.startGroup).not.toHaveBeenCalled();
    expect(showError).not.toHaveBeenCalled();
  });

  it("holds the target through navigation and prevents duplicate submissions from another control", async () => {
    let resolve!: (value: Awaited<ReturnType<typeof api.startGroup>>) => void;
    vi.mocked(api.startGroup).mockReturnValue(
      new Promise((r) => {
        resolve = r;
      }),
    );
    let task!: Promise<void>;
    await act(async () => {
      task = result.handleGroupControl("g2", "launch");
    });
    useGroupStore.setState({ selectedGroupId: "g3", groupDoc: { group_id: "g3" } });
    await act(async () => result.handleGroupControl("g1", "stop"));
    expect(api.stopGroup).not.toHaveBeenCalled();
    await act(async () => {
      resolve(success("g2"));
      await task;
    });
    expect(refreshActors).toHaveBeenCalledWith("g2");
    expect(useGroupStore.getState().groupDoc?.group_id).toBe("g3");
  });

  it.each(["pause", "delete"] as const)(
    "does not apply a late %s result to another Group",
    async (action) => {
      let resolve!: (value: Awaited<ReturnType<typeof api.setGroupState>>) => void;
      const pending = new Promise<Awaited<ReturnType<typeof api.setGroupState>>>((r) => {
        resolve = r;
      });
      vi.stubGlobal(
        "confirm",
        vi.fn(() => true),
      );
      vi.mocked(api.setGroupState).mockReturnValue(pending);
      vi.mocked(api.deleteGroup).mockReturnValue(pending as ReturnType<typeof api.deleteGroup>);
      let task!: Promise<void>;
      await act(async () => {
        task =
          action === "pause"
            ? result.handleGroupControl("g1", "pause")
            : result.handleDeleteGroup("g1");
      });
      await act(async () => {
        useGroupStore.setState({
          selectedGroupId: "g2",
          groupDoc: { group_id: "g2", state: "active" },
        });
      });
      await act(async () => {
        resolve(success("g1"));
        await task;
      });
      expect(useGroupStore.getState().selectedGroupId).toBe("g2");
      expect(useGroupStore.getState().groupDoc).toEqual({ group_id: "g2", state: "active" });
      vi.unstubAllGlobals();
    },
  );

  it("reports the background target on failure, unlocks controls, and does not start after a rejected resume", async () => {
    vi.mocked(api.setGroupState).mockResolvedValue({
      ok: false,
      error: { code: "forbidden", message: "Access denied" },
    });
    await act(async () => result.handleGroupControl("g2", "activate"));
    expect(api.startGroup).not.toHaveBeenCalled();
    expect(showError).toHaveBeenCalledExactlyOnceWith("Background: forbidden: Access denied");
    vi.mocked(api.stopGroup).mockRejectedValueOnce(new Error("Network disconnected"));
    await act(async () => result.handleGroupControl("g2", "stop"));
    expect(showError).toHaveBeenCalledTimes(2);
    expect(useUIStore.getState().busy).toBe("");
    await act(async () => result.handleGroupControl("g2", "launch"));
    expect(api.startGroup).toHaveBeenCalledWith("g2");
  });

  it("formats mapped daemon errors with the Group title prefix across launch and delete", async () => {
    await i18nReady;
    await i18n.changeLanguage("zh");
    try {
      vi.mocked(api.startGroup).mockResolvedValueOnce({
        ok: false,
        error: { code: "invalid_project_root", message: "project root path does not exist: /gone" },
      });
      await act(async () => result.handleGroupControl("g2", "launch"));
      expect(showError).toHaveBeenCalledTimes(1);
      const launchMessage = showError.mock.calls[0]?.[0] as string;
      expect(launchMessage.startsWith("Background: ")).toBe(true);
      expect(launchMessage).not.toContain("project root path does not exist");
      expect(launchMessage).toContain("（invalid_project_root）");

      vi.stubGlobal(
        "confirm",
        vi.fn(() => true),
      );
      vi.mocked(api.deleteGroup).mockResolvedValueOnce({
        ok: false,
        error: { code: "actor_browser_busy", message: "delivering to browser" },
      });
      await act(async () => result.handleDeleteGroup("g2"));
      expect(showError).toHaveBeenCalledTimes(2);
      const deleteMessage = showError.mock.calls[1]?.[0] as string;
      expect(deleteMessage.startsWith("Background: ")).toBe(true);
      expect(deleteMessage).not.toContain("delivering to browser");
      expect(deleteMessage).toContain("（actor_browser_busy）");
    } finally {
      vi.unstubAllGlobals();
      await i18n.changeLanguage("en");
    }
  });
});
