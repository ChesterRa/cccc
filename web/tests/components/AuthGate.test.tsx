// @vitest-environment happy-dom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT =
  true;

const { api, calls } = vi.hoisted(() => {
  const calls: string[] = [];
  return {
    calls,
    api: {
      shouldForceTokenLogin: vi.fn(() => false),
      clearAuthToken: vi.fn(),
      fetchWebAccessSession: vi.fn(async () => {
        calls.push("session");
        return {
          ok: true as const,
          result: { web_access_session: { current_browser_signed_in: true } },
        };
      }),
      fetchGroups: vi.fn(async () => {
        calls.push("groups");
        return { ok: true as const, result: { groups: [] } };
      }),
      onAuthRequired: vi.fn(),
      isAuthRequiredErrorCode: vi.fn(() => false),
      setAuthToken: vi.fn(),
      clearForceTokenLogin: vi.fn(),
    },
  };
});

vi.mock("../../src/services/api", () => api);
vi.mock("../../src/hooks/useTheme", () => ({ useTheme: () => ({ isDark: false }) }));
vi.mock("../../src/stores", () => ({
  useBrandingStore: (selector: (state: { branding: Record<string, unknown> }) => unknown) =>
    selector({ branding: {} }),
}));

import { AuthGate } from "../../src/components/AuthGate";

describe("AuthGate startup authentication", () => {
  beforeEach(() => {
    calls.length = 0;
    vi.clearAllMocks();
    api.shouldForceTokenLogin.mockReturnValue(false);
  });

  it("establishes the session cookie before probing protected group APIs", async () => {
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => {
      root.render(
        <AuthGate>
          <div data-testid="authenticated-child">ready</div>
        </AuthGate>,
      );
    });

    await vi.waitFor(() => expect(host.textContent).toContain("ready"));
    expect(calls).toEqual(["session", "groups"]);
    await act(async () => root.unmount());
  });

  it("drops the transient bearer after the server establishes a durable cookie", async () => {
    api.shouldForceTokenLogin.mockReturnValue(true);
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => {
      root.render(
        <AuthGate>
          <div>ready</div>
        </AuthGate>,
      );
    });
    const clearCallsBeforeSubmit = api.clearAuthToken.mock.calls.length;
    const recoveryButton = Array.from(host.querySelectorAll("button")).find(
      (button) => button.textContent === "forgotTokenCta",
    );
    await act(async () => recoveryButton?.click());
    const scrollContainer = host.querySelector('[data-testid="auth-login-scroll"]');
    expect(scrollContainer?.className).toContain("overflow-y-auto");
    expect(scrollContainer?.textContent).toContain("recoverySecurityNote");
    const input = host.querySelector<HTMLInputElement>('input[name="cccc-access-token"]');
    expect(input?.autocomplete).toBe("current-password");
    await act(async () => {
      if (!input) throw new Error("token input missing");
      const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
      setValue?.call(input, "mobile-token");
      input.dispatchEvent(new Event("input", { bubbles: true }));
      input.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await act(async () => {
      host.querySelector("form")?.dispatchEvent(new Event("submit", { bubbles: true }));
    });

    await vi.waitFor(() => expect(host.textContent).toContain("ready"));
    expect(api.setAuthToken).toHaveBeenCalledWith("mobile-token");
    expect(api.clearForceTokenLogin).toHaveBeenCalled();
    expect(api.clearAuthToken.mock.calls.length).toBe(clearCallsBeforeSubmit + 1);
    await act(async () => root.unmount());
  });

  it("restores the bearer when cookie-only verification is rejected", async () => {
    api.shouldForceTokenLogin.mockReturnValue(true);
    api.fetchGroups.mockResolvedValueOnce({
      ok: false as const,
      error: { code: "authentication_required", message: "cookie missing" },
    });
    api.isAuthRequiredErrorCode.mockImplementation(
      (code: string | undefined) => code === "authentication_required",
    );
    const host = document.createElement("div");
    const root = createRoot(host);
    await act(async () => {
      root.render(
        <AuthGate>
          <div>ready</div>
        </AuthGate>,
      );
    });
    const input = host.querySelector<HTMLInputElement>('input[name="cccc-access-token"]');
    await act(async () => {
      if (!input) throw new Error("token input missing");
      const setValue = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")?.set;
      setValue?.call(input, "mobile-token");
      input.dispatchEvent(new Event("input", { bubbles: true }));
      input.dispatchEvent(new Event("change", { bubbles: true }));
      host.querySelector("form")?.dispatchEvent(new Event("submit", { bubbles: true }));
    });

    await vi.waitFor(() => expect(host.textContent).toContain("connectionFailed"));
    expect(api.clearAuthToken).toHaveBeenCalled();
    expect(api.setAuthToken).toHaveBeenNthCalledWith(1, "mobile-token");
    expect(api.setAuthToken).toHaveBeenNthCalledWith(2, "mobile-token");
    expect(api.clearForceTokenLogin).not.toHaveBeenCalled();
    expect(host.textContent).not.toContain("ready");
    await act(async () => root.unmount());
  });
});
