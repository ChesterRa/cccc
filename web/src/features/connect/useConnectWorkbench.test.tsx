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
  it("retains the account label offline and clears it when Connect confirms device rejection", async () => {
    mocks.request.mockResolvedValue({
      ok: true,
      result: { ...snapshot().result, account_label: "owner@example.test" },
    });
    await act(async () => root.render(<Probe />));
    expect(state.accountLabel).toBe("owner@example.test");
    mocks.request.mockResolvedValue({
      ok: false,
      error: { code: "network_error", message: "Offline" },
    });
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(state.accountLabel).toBe("owner@example.test");
    mocks.request.mockResolvedValue({
      ok: true,
      result: {
        connect: {
          ...snapshot().result.connect,
          directory: null,
          error_code: "membership_disabled",
        },
        account_label: null,
      },
    });
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(state.accountLabel).toBeNull();
    expect(state.instances).toEqual([]);
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
  it("retains orientation through a failed read without extending the directory grant, then clears revoked access", async () => {
    await act(async () => root.render(<Probe />));
    await act(async () => {
      state.select("b", "group-b");
      state.remember(state.instances[0], [{ group_id: "group-b", title: "B", running: true }]);
    });
    mocks.request.mockResolvedValue({
      ok: false,
      error: { code: "network_error", message: "Offline" },
    });
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(state.failed).toBe(true);
    expect(state.activeInstance?.instance_id).toBe("b");
    expect(state.listings.b.groups).toHaveLength(1);
    await act(async () => vi.advanceTimersByTimeAsync(120000));
    expect(state.instances).toHaveLength(1);
    expect(state.available).toBe(false);
    expect(state.activeInstance).toBeNull();
    expect(state.selected?.groupId).toBe("group-b");
    mocks.access.mockResolvedValue(false);
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(state.instances).toEqual([]);
    expect(state.listings).toEqual({});
    expect(state.selected).toBeNull();
    expect(state.accountLabel).toBeNull();
  });
  it("does not mistake an unavailable entry check for a revoked session", async () => {
    await act(async () => root.render(<Probe />));
    mocks.access.mockResolvedValue(null);
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(state.instances).toHaveLength(1);
    expect(state.failed).toBe(true);
    expect(mocks.request).toHaveBeenCalledTimes(1);
  });
  it("keeps expired labels for orientation and clears them on a confirmed unlink", async () => {
    await act(async () => root.render(<Probe />));
    const expired = snapshot();
    Object.assign(expired.result.connect, {
      directory: null,
      error_code: "connect_directory_expired",
    });
    mocks.request.mockResolvedValue(expired);
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(state.instances).toHaveLength(1);
    expect(state.available).toBe(false);
    mocks.request.mockResolvedValue({ ok: true, result: { connect: null, account_label: null } });
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(state.instances).toHaveLength(0);
  });
});
