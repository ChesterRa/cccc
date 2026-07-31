import { describe, expect, it, vi } from "vite-plus/test";

import { imRevokeKey, revokeIMChatAuthorization } from "./imBridgeRevoke";

describe("revokeIMChatAuthorization", () => {
  it("keys in-flight revokes by chat and thread", () => {
    const current = imRevokeKey("same-chat", 0);

    expect(current).toBe("same-chat:0");
    expect(imRevokeKey("same-chat", 0)).toBe(current);
    expect(imRevokeKey("same-chat", 1)).not.toBe(current);
  });

  it("reports a business failure without refreshing authorization state", async () => {
    const refresh = vi.fn(async () => undefined);

    const error = await revokeIMChatAuthorization({
      request: async () => ({ ok: true, result: { revoked: false } }),
      refresh,
      fallbackError: "Failed to revoke chat authorization.",
    });

    expect(error).toBe("Failed to revoke chat authorization.");
    expect(refresh).not.toHaveBeenCalled();
  });

  it("refreshes authorization state after a successful revoke", async () => {
    const refresh = vi.fn(async () => undefined);

    const error = await revokeIMChatAuthorization({
      request: async () => ({ ok: true, result: { revoked: true } }),
      refresh,
      fallbackError: "Failed to revoke chat authorization.",
    });

    expect(error).toBeNull();
    expect(refresh).toHaveBeenCalledOnce();
  });

  it("reports request failures without refreshing authorization state", async () => {
    const refresh = vi.fn(async () => undefined);

    const error = await revokeIMChatAuthorization({
      request: async () => {
        throw new Error("network failure");
      },
      refresh,
      fallbackError: "Failed to revoke chat authorization.",
    });

    expect(error).toBe("Failed to revoke chat authorization.");
    expect(refresh).not.toHaveBeenCalled();
  });
});
