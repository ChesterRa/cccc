import { afterEach, describe, expect, it, vi } from "vitest";

import { apiJson, isAuthRequiredErrorCode, onAuthRequired } from "./base";

describe("apiJson", () => {
  afterEach(() => {
    onAuthRequired(() => undefined);
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("recognizes Python and Rust authentication error codes", () => {
    expect(isAuthRequiredErrorCode("unauthorized")).toBe(true);
    expect(isAuthRequiredErrorCode("auth_required")).toBe(true);
    expect(isAuthRequiredErrorCode("permission_denied")).toBe(false);
    expect(isAuthRequiredErrorCode("admin_required")).toBe(false);
  });

  it("does not treat a scoped-token permission denial as sign-out", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const onRequired = vi.fn();
    onAuthRequired(onRequired);
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          ok: false,
          error: { code: "permission_denied", message: "group access denied" },
        }),
        { status: 403, headers: { "content-type": "application/json" } },
      ),
    );

    const resp = await apiJson("/api/v1/groups/g_denied");

    expect(resp.ok).toBe(false);
    expect(onRequired).not.toHaveBeenCalled();
  });

  it("notifies the auth gate for Rust auth_required responses", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const onRequired = vi.fn();
    onAuthRequired(onRequired);
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          ok: false,
          error: { code: "auth_required", message: "valid access token required" },
        }),
        { status: 401, headers: { "content-type": "application/json" } },
      ),
    );

    const resp = await apiJson("/api/v1/groups");

    expect(resp.ok).toBe(false);
    expect(onRequired).toHaveBeenCalled();
  });

  it("reports non-JSON HTTP failures as HTTP errors instead of parse errors", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response("<html><head><title>504 Gateway Time-out</title></head></html>", {
        status: 504,
        statusText: "Gateway Time-out",
        headers: { "content-type": "text/html" },
      }),
    );

    const resp = await apiJson("/api/v1/groups/g1/send", { method: "POST" });

    expect(resp.ok).toBe(false);
    expect(resp.ok ? "" : resp.error.code).toBe("HTTP_ERROR");
    expect(resp.ok ? "" : resp.error.message).toContain("504 Gateway Time-out");
  });
});
