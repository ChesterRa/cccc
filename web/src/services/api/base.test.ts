import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import {
  apiJson,
  isAuthRequiredErrorCode,
  normalizePresentationBrowserSurfaceState,
  onAuthRequired,
  removeAuthTokenFromUrl,
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

  it("recognizes the supported authentication error codes", () => {
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

  it("notifies the auth gate for auth_required responses", async () => {
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

  it("does not place long-lived access tokens in URLs", () => {
    vi.stubGlobal("window", {
      location: { origin: "http://localhost:5555", href: "http://localhost:5555/ui/", search: "" },
    });
    vi.stubGlobal("sessionStorage", { setItem: vi.fn() });
    setAuthToken("current-token");

    expect(withAuthToken("/api/v1/events/stream")).toBe("/api/v1/events/stream");
    expect(withAuthToken("http://172.19.79.11:8848/ui/?view=1")).toBe(
      "http://172.19.79.11:8848/ui/?view=1",
    );
  });

  it("never injects the owner token into a cross-origin URL", () => {
    vi.stubGlobal("window", {
      location: { origin: "http://localhost:5555", href: "http://localhost:5555/ui/", search: "" },
    });
    vi.stubGlobal("sessionStorage", { setItem: vi.fn() });
    setAuthToken("current-token");

    expect(refreshAuthTokenInUrl("https://example.com/page")).toBe("https://example.com/page");
    expect(refreshAuthTokenInUrl("https://evil.example/page?token=1")).toBe(
      "https://evil.example/page?token=1",
    );
    expect(refreshAuthTokenInUrl("//evil.example/page?token=1")).toBe(
      "//evil.example/page?token=1",
    );
    expect(refreshAuthTokenInUrl("http://localhost:5556/page?token=1")).toBe(
      "http://localhost:5556/page?token=1",
    );
    expect(refreshAuthTokenInUrl("https://localhost:5555/page?token=1")).toBe(
      "https://localhost:5555/page?token=1",
    );
  });

  it("strips stale query tokens from same-origin presentation URLs", () => {
    vi.stubGlobal("window", {
      location: { origin: "http://localhost:5555", href: "http://localhost:5555/ui/", search: "" },
    });
    vi.stubGlobal("sessionStorage", { setItem: vi.fn() });
    setAuthToken("current-token");

    expect(refreshAuthTokenInUrl("/preview?token=expired&view=1")).toBe("/preview?view=1");
    expect(refreshAuthTokenInUrl("http://localhost:5555/preview?token=expired")).toBe(
      "http://localhost:5555/preview",
    );
  });

  it("removes the consumed token from browser history without dropping other URL state", () => {
    const replaceState = vi.fn();
    vi.stubGlobal("window", {
      location: { href: "https://d-1.cccc.foo/ui/?token=acc_secret&view=group#actor" },
      history: { state: { preserved: true }, replaceState },
    });

    removeAuthTokenFromUrl();

    expect(replaceState).toHaveBeenCalledWith({ preserved: true }, "", "/ui/?view=group#actor");
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
