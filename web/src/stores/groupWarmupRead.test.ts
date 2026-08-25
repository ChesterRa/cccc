import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

vi.mock("../services/api", () => ({
  fetchGroup: vi.fn(),
  fetchLedgerTail: vi.fn(),
  fetchActors: vi.fn(),
}));

import * as api from "../services/api";
import {
  getGroupWarmupRead,
  resetGroupWarmupReadsForTests,
  startGroupWarmupRead,
} from "./groupWarmupRead";

describe("groupWarmupRead", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    resetGroupWarmupReadsForTests();
    vi.mocked(api.fetchGroup).mockResolvedValue({ ok: true, result: { group: {} } } as never);
    vi.mocked(api.fetchLedgerTail).mockResolvedValue({
      ok: true,
      result: { events: [], has_more: false },
    } as never);
    vi.mocked(api.fetchActors).mockResolvedValue({ ok: true, result: { actors: [] } } as never);
  });

  it("shares one read set while hover warmup and selection overlap", async () => {
    const first = startGroupWarmupRead("g-demo");
    const second = startGroupWarmupRead("g-demo");

    expect(second).toBe(first);
    await first;
    expect(api.fetchGroup).toHaveBeenCalledTimes(1);
    expect(api.fetchLedgerTail).toHaveBeenCalledTimes(1);
    expect(api.fetchActors).toHaveBeenCalledTimes(1);
  });

  it("reuses a just-completed hover read when selection follows", async () => {
    const warmup = startGroupWarmupRead("g-demo");
    await warmup;

    expect(getGroupWarmupRead("g-demo")).toBe(warmup);
    expect(startGroupWarmupRead("g-demo")).toBe(warmup);
    expect(api.fetchGroup).toHaveBeenCalledTimes(1);
    expect(api.fetchLedgerTail).toHaveBeenCalledTimes(1);
    expect(api.fetchActors).toHaveBeenCalledTimes(1);
  });

  it("does not reuse a completed warmup when any required response failed", async () => {
    vi.mocked(api.fetchLedgerTail).mockResolvedValueOnce({
      ok: false,
      error: { code: "unavailable", message: "temporarily unavailable" },
    } as never);

    const failedWarmup = startGroupWarmupRead("g-demo");
    await failedWarmup;

    expect(getGroupWarmupRead("g-demo")).toBeUndefined();
    const retry = startGroupWarmupRead("g-demo");
    expect(retry).not.toBe(failedWarmup);
    await retry;
    expect(api.fetchGroup).toHaveBeenCalledTimes(2);
    expect(api.fetchLedgerTail).toHaveBeenCalledTimes(2);
    expect(api.fetchActors).toHaveBeenCalledTimes(2);
  });
});
