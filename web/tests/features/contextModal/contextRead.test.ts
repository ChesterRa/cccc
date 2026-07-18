import { describe, expect, it, vi } from "vite-plus/test";
import {
  openContextModalData,
  reloadContextModalData,
  syncContextModalData,
  type ContextModalFetch,
} from "../../../src/features/contextModal/contextRead";

describe("contextRead", () => {
  it("opens modal data on the full-detail path without forcing fresh", async () => {
    const fetchContext = vi.fn<ContextModalFetch>().mockResolvedValue(undefined);

    await openContextModalData(fetchContext, "g-1");

    expect(fetchContext).toHaveBeenCalledWith("g-1", { detail: "full", fresh: undefined });
  });

  it("syncs modal data after writes on the full-detail path", async () => {
    const fetchContext = vi.fn<ContextModalFetch>().mockResolvedValue(undefined);

    await syncContextModalData(fetchContext, "g-1");

    expect(fetchContext).toHaveBeenCalledWith("g-1", { detail: "full", fresh: undefined });
  });

  it("reloads modal data on an explicit fresh full path", async () => {
    const fetchContext = vi.fn<ContextModalFetch>().mockResolvedValue(undefined);

    await reloadContextModalData(fetchContext, "g-1");

    expect(fetchContext).toHaveBeenCalledWith("g-1", { detail: "full", fresh: true });
  });

  it("skips empty group ids", async () => {
    const fetchContext = vi.fn<ContextModalFetch>().mockResolvedValue(undefined);

    await openContextModalData(fetchContext, "");
    await syncContextModalData(fetchContext, "   ");
    await reloadContextModalData(fetchContext, "");

    expect(fetchContext).not.toHaveBeenCalled();
  });
});
