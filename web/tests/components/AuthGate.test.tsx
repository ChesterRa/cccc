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
});
