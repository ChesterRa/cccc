import { afterEach, describe, expect, it, vi } from "vite-plus/test";

import {
  apiJson,
  filenameFromContentDisposition,
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

  it("returns a network error when an admitted response body disconnects", async () => {
    vi.stubGlobal("window", { location: { search: "" } });
    const response = new Response("partial");
    vi.spyOn(response, "text").mockRejectedValue(new DOMException("timed out", "TimeoutError"));
    vi.spyOn(globalThis, "fetch").mockResolvedValue(response);
    expect(await apiJson("/api/v1/connect")).toMatchObject({
      ok: false,
      error: { code: "NETWORK_ERROR" },
    });
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

describe("filenameFromContentDisposition", () => {
  it("decodes a Chinese group package from the RFC 5987 parameter", () => {
    const header =
      'attachment; filename="cccc-group--______--g_0e88539bb583.zip"; ' +
      "filename*=UTF-8''cccc%2Dgroup%2D%2D%E5%AE%89%E5%8D%93%E6%96%B0%E8%AE%BE%E5%A4%87%2D%2Dg%5F0e88539bb583%2Ezip";
    expect(filenameFromContentDisposition(header, "fallback.zip")).toBe(
      "cccc-group--安卓新设备--g_0e88539bb583.zip",
    );
  });

  it("never prefers the ASCII fallback when an extended name is present", () => {
    const header = "attachment; filename=\"__.txt\"; filename*=UTF-8''%E6%8A%A5%E5%91%8A.txt";
    expect(filenameFromContentDisposition(header, "fallback.txt")).toBe("报告.txt");
  });

  it("falls back to the plain parameter and then to the caller default", () => {
    expect(filenameFromContentDisposition('attachment; filename="notes.txt"', "fallback.txt")).toBe(
      "notes.txt",
    );
    expect(filenameFromContentDisposition("attachment", "fallback.txt")).toBe("fallback.txt");
    expect(filenameFromContentDisposition("", "fallback.txt")).toBe("fallback.txt");
  });

  it("survives a malformed escape by using the ASCII parameter", () => {
    const header = "attachment; filename=\"notes.txt\"; filename*=UTF-8''%E4%";
    expect(filenameFromContentDisposition(header, "fallback.txt")).toBe("notes.txt");
  });
});
