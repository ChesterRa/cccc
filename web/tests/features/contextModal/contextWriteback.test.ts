import { describe, expect, it, vi } from "vite-plus/test";
import type { ApiResponse } from "../../../src/services/api";
import { reloadContextAfterWrite } from "../../../src/features/contextModal/contextWriteback";

describe("contextWriteback", () => {
  it("reloads context after successful writes", async () => {
    const reloadContext = vi.fn().mockResolvedValue(undefined);
    const response: ApiResponse<{ updated: true }> = { ok: true, result: { updated: true } };

    const result = await reloadContextAfterWrite(response, reloadContext);

    expect(result).toBe(response);
    expect(reloadContext).toHaveBeenCalledTimes(1);
  });

  it("does not reload context after failed writes", async () => {
    const reloadContext = vi.fn().mockResolvedValue(undefined);
    const response: ApiResponse<{ updated: true }> = {
      ok: false,
      error: { code: "write_failed", message: "write failed" },
    };

    const result = await reloadContextAfterWrite(response, reloadContext);

    expect(result).toBe(response);
    expect(reloadContext).not.toHaveBeenCalled();
  });
  it("returns an explicit failure when canonical readback fails", async () => {
    const reloadContext = vi.fn().mockRejectedValue(new Error("context fetch blew up"));
    const response: ApiResponse<{ updated: true }> = { ok: true, result: { updated: true } };

    const result = await reloadContextAfterWrite(response, reloadContext);

    expect(result).toEqual({
      ok: false,
      error: { code: "context_readback_failed", message: "context fetch blew up" },
    });
    expect(reloadContext).toHaveBeenCalledTimes(1);
  });
});
