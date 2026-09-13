// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useConnectWorkbench } from "./useConnectWorkbench";

const mocks = vi.hoisted(() => ({ request: vi.fn(), access: vi.fn() }));
vi.mock("../../services/api/base", () => ({ apiJson: mocks.request }));

describe("Connect entry access lifecycle", () => {
  let root: ReturnType<typeof createRoot>;
  let host: HTMLDivElement;
  let state: ReturnType<typeof useConnectWorkbench>;
  function Probe({ enabled = true }) {
    state = useConnectWorkbench(enabled, mocks.access);
    return null;
  }
  const snapshot = () => ({
    ok: true,
    result: {
      connect: {
        instance_id: "a",
        directory: {
          expires_at: new Date(Date.now() + 120000).toISOString(),
          instances: [{ instance_id: "a" }, { instance_id: "b", device_id: "device-b" }],
        },
      },
    },
  });
  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.useFakeTimers();
    mocks.access.mockReset().mockResolvedValue(true);
    mocks.request.mockReset().mockImplementation(async () => snapshot());
    host = document.createElement("div");
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    vi.useRealTimers();
  });
  it("does not read Connect for a restricted entry and recovers a later admin session", async () => {
    mocks.access.mockResolvedValue(false);
    await act(async () => root.render(<Probe />));
    expect(mocks.request).not.toHaveBeenCalled();
    expect(state.instances).toEqual([]);
    mocks.access.mockResolvedValue(true);
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(state.instances.map((instance) => instance.instance_id)).toEqual(["b"]);
    await act(async () => {
      state.select("b", "group-b");
      state.remember(state.instances[0], [{ group_id: "group-b", title: "B", running: true }]);
    });
    mocks.access.mockResolvedValue(false);
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(state.selected).toBeNull();
    expect(state.listings).toEqual({});
    expect(mocks.request).toHaveBeenCalledTimes(1);
  });
  it("discards directory responses arriving after the entry was disabled", async () => {
    let complete!: (response: ReturnType<typeof snapshot>) => void;
    mocks.request.mockImplementation(
      () =>
        new Promise((resolve) => {
          complete = resolve;
        }),
    );
    await act(async () => root.render(<Probe />));
    await act(async () => root.render(<Probe enabled={false} />));
    await act(async () => complete(snapshot()));
    expect(state.instances).toEqual([]);
    await act(async () => vi.advanceTimersByTimeAsync(30000));
    expect(mocks.request).toHaveBeenCalledTimes(1);
  });
  it("preserves remote navigation across local selection and explicit collapse, without retaining it across a new binding", async () => {
    await act(async () => root.render(<Probe />));
    await act(async () => {
      state.select("b", "group-b");
      state.remember(state.instances[0], [{ group_id: "group-b", title: "B", running: true }]);
    });
    await act(async () => state.selectLocal());
    expect(state.selected).toBeNull();
    expect(state.listings.b.groups[0].group_id).toBe("group-b");
    await act(async () => state.toggleExpanded("b"));
    expect(state.collapsedInstances).toEqual(["b"]);
    await act(async () => state.select("b", "group-b"));
    expect(state.collapsedInstances).toEqual([]);
    expect(state.listings.b.groups).toHaveLength(1);
    mocks.request.mockImplementation(async () => {
      const result = snapshot();
      result.result.connect.directory.instances[1].device_id = "replacement-device";
      return result;
    });
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(state.listings).toEqual({});
  });

  it("invalidates only the locked target and forgets all cached lists after entry authorization is lost", async () => {
    mocks.request.mockImplementation(async () => {
      const result = snapshot();
      result.result.connect.directory.instances.push({ instance_id: "c", device_id: "device-c" });
      return result;
    });
    await act(async () => root.render(<Probe />));
    await act(async () => {
      for (const instance of state.instances)
        state.remember(instance, [
          { group_id: instance.instance_id, title: instance.instance_id, running: false },
        ]);
      state.selectLocal();
    });
    expect(Object.keys(state.listings).sort()).toEqual(["b", "c"]);
    await act(async () => state.remember(state.instances[0], null));
    expect(Object.keys(state.listings)).toEqual(["c"]);
    mocks.access.mockResolvedValue(false);
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(state.listings).toEqual({});
  });
});
