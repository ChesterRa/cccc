// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";
import { useGroupStore } from "./useGroupStore";
import { fetchRuntimes } from "../services/api";
import type { RuntimeInfo } from "../types";
vi.mock("../services/api", () => ({ fetchRuntimes: vi.fn() }));

describe("Runtime detection refresh", () => {
  const native: RuntimeInfo = {
    name: "antigravity",
    display_name: "Antigravity",
    available: true,
    mode_availability: { default: true, acp: false },
  };
  beforeEach(() => {
    vi.clearAllMocks();
    useGroupStore.setState({ runtimes: [], runtimeDetectionStatus: "idle" });
  });
  afterEach(() => {
    useGroupStore.setState({ runtimes: [], runtimeDetectionStatus: "idle" });
  });
  it("shares an in-flight check between editors and refreshes a cached result", async () => {
    useGroupStore.getState().setRuntimes([native]);
    let resolve!: (response: Awaited<ReturnType<typeof fetchRuntimes>>) => void;
    vi.mocked(fetchRuntimes).mockReturnValueOnce(
      new Promise((done) => {
        resolve = done;
      }),
    );
    const first = useGroupStore.getState().refreshRuntimes();
    const second = useGroupStore.getState().refreshRuntimes();
    expect(first).toBe(second);
    expect(fetchRuntimes).toHaveBeenCalledTimes(1);
    expect(useGroupStore.getState().runtimeDetectionStatus).toBe("loading");
    const installed = {
      ...native,
      available: false,
      mode_availability: { default: false, acp: true },
    };
    resolve({ ok: true, result: { runtimes: [installed], available: [] } });
    await first;
    expect(useGroupStore.getState().runtimes).toEqual([installed]);
    expect(useGroupStore.getState().runtimeDetectionStatus).toBe("ready");
    vi.mocked(fetchRuntimes).mockResolvedValueOnce({
      ok: true,
      result: { runtimes: [native], available: [native.name] },
    });
    await useGroupStore.getState().refreshRuntimes();
    expect(fetchRuntimes).toHaveBeenCalledTimes(2);
    expect(useGroupStore.getState().runtimes).toEqual([native]);
  });
  it("marks a failed check separately while preserving existing catalog and Group state", async () => {
    useGroupStore.getState().setRuntimes([native]);
    const actors = useGroupStore.getState().actors;
    vi.mocked(fetchRuntimes).mockResolvedValueOnce({
      ok: false,
      error: { code: "offline", message: "fixture", details: {} },
    });
    await useGroupStore.getState().refreshRuntimes();
    expect(useGroupStore.getState().runtimeDetectionStatus).toBe("error");
    expect(useGroupStore.getState().runtimes).toEqual([native]);
    expect(useGroupStore.getState().actors).toBe(actors);
    vi.mocked(fetchRuntimes).mockResolvedValueOnce({
      ok: true,
      result: { runtimes: [native], available: [native.name] },
    });
    await useGroupStore.getState().refreshRuntimes();
    expect(useGroupStore.getState().runtimeDetectionStatus).toBe("ready");
  });
  it("allows an explicit retry after a network failure", async () => {
    vi.mocked(fetchRuntimes).mockRejectedValueOnce(new Error("fixture network failure"));
    await useGroupStore.getState().refreshRuntimes();
    expect(useGroupStore.getState().runtimeDetectionStatus).toBe("error");
    vi.mocked(fetchRuntimes).mockResolvedValueOnce({
      ok: true,
      result: { runtimes: [native], available: [native.name] },
    });
    await useGroupStore.getState().refreshRuntimes();
    expect(useGroupStore.getState().runtimeDetectionStatus).toBe("ready");
  });
});
