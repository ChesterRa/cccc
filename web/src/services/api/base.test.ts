import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import {
  apiJson,
  isAuthRequiredErrorCode,
  normalizePresentationBrowserSurfaceState,
  onAuthRequired,
  refreshAuthTokenInUrl,
  setAuthToken,
  withAuthToken,
} from "./base";

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

describe("normalizePresentationBrowserSurfaceState", () => {
  it("preserves projected display ownership for the browser status UI", () => {
    const state = normalizePresentationBrowserSurfaceState({
      active: true,
      state: "ready",
      metadata: { display: ":123", display_owned: true, display_owner: "cccc_xvfb", adopted: true },
    });

    expect(state.metadata).toEqual({
      display: ":123",
      display_owned: true,
      display_owner: "cccc_xvfb",
      adopted: true,
    });
  });
});

describe("authenticated URLs", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("replaces an expired token instead of appending a duplicate", () => {
    vi.stubGlobal("window", {
      location: { origin: "http://localhost:5555", href: "http://localhost:5555/ui/", search: "" },
    });
    vi.stubGlobal("sessionStorage", { setItem: vi.fn() });
    setAuthToken("current-token");

    expect(withAuthToken("http://172.19.79.11:8848/ui/?token=expired&view=1")).toBe(
      "http://172.19.79.11:8848/ui/?token=current-token&view=1",
    );
  });

  it("only refreshes arbitrary page URLs that already carry a CCCC token", () => {
    vi.stubGlobal("window", {
      location: { origin: "http://localhost:5555", href: "http://localhost:5555/ui/", search: "" },
    });
    vi.stubGlobal("sessionStorage", { setItem: vi.fn() });
    setAuthToken("current-token");

    expect(refreshAuthTokenInUrl("https://example.com/page")).toBe("https://example.com/page");
    expect(refreshAuthTokenInUrl("http://127.0.0.1:8848/ui/?token=expired")).toBe(
      "http://127.0.0.1:8848/ui/?token=current-token",
    );
  });
});

describe("normalizePresentationBrowserSurfaceState", () => {
  it("preserves projected display ownership for the browser status UI", () => {
    const state = normalizePresentationBrowserSurfaceState({
      active: true,
      state: "ready",
      metadata: { display: ":123", display_owned: true, display_owner: "cccc_xvfb", adopted: true },
    });

    expect(state.metadata).toEqual({
      display: ":123",
      display_owned: true,
      display_owner: "cccc_xvfb",
      adopted: true,
    });
  });
});
