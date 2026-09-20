// @vitest-environment happy-dom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

const mocks = vi.hoisted(() => ({
  translate: (key: string) => key,
  panel: vi.fn(),
  fetchSession: vi.fn(),
  resume: vi.fn(),
  session: {
    active: true,
    ready: false,
    login_required: true,
    verification_required: true,
  } as Record<string, unknown>,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: mocks.translate }) }));
vi.mock("../../../services/api", () => ({
  fetchGroups: async () => ({
    ok: true,
    result: { groups: [{ group_id: "g_login", title: "Login" }] },
  }),
  fetchActors: async () => ({
    ok: true,
    result: { actors: [{ id: "browser", runtime: "web_model", title: "Browser" }] },
  }),
  fetchRemoteAccessState: async () => ({ ok: true, result: {} }),
  fetchWebModelConnectors: async () => ({ ok: true, result: { connectors: [] } }),
  fetchWebModelBrowserSession: (...args: unknown[]) => mocks.fetchSession(...args),
  resumeWebModelBrowserDelivery: (...args: unknown[]) => mocks.resume(...args),
  fetchWebModelBrowserSurfaceSession: async () => ({
    ok: true,
    result: { browser_session: mocks.session },
  }),
  getWebModelBrowserSurfaceWebSocketUrl: () => "ws://localhost/fixture",
}));
vi.mock("../../browser/ProjectedBrowserSurfacePanel", () => ({
  ProjectedBrowserSurfacePanel: (props: { refreshNonce: number }) => {
    mocks.panel(props);
    return <div data-testid="browser-viewer">Browser</div>;
  },
}));
import WebModelConnectorsTab from "./WebModelConnectorsTab";

describe("Web Model manual sign-in", () => {
  let host: HTMLDivElement;
  let root: Root;
  beforeEach(() => {
    Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });
    vi.useFakeTimers();
    mocks.panel.mockClear();
    mocks.session = {
      active: true,
      ready: false,
      login_required: true,
      verification_required: true,
    };
    mocks.resume.mockReset();
    mocks.fetchSession
      .mockReset()
      .mockResolvedValue({ ok: true, result: { browser_session: mocks.session } });
    vi.spyOn(document, "hidden", "get").mockReturnValue(false);
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.clearAllTimers();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it("keeps repeated Open actions from reloading the verification page", async () => {
    await act(async () => {
      root.render(<WebModelConnectorsTab isDark={false} currentGroupId="g_login" />);
    });
    const open = Array.from(host.querySelectorAll("button")).find((button) =>
      button.textContent?.includes("webModels.chatgpt.buttons.openChatGpt"),
    );
    expect(open).toBeDefined();
    expect(host.textContent).toContain("webModels.chatgpt.browser.verificationRequired");
    expect(host.textContent).toContain("webModels.chatgpt.chatSetup.verificationHint");
    await act(async () => open!.click());
    const viewer = host.querySelector('[data-testid="browser-viewer"]');
    expect(viewer).not.toBeNull();
    const refreshNonce = mocks.panel.mock.lastCall?.[0].refreshNonce;
    await act(async () => open!.click());
    expect(host.querySelector('[data-testid="browser-viewer"]')).toBe(viewer);
    expect(mocks.panel.mock.lastCall?.[0].refreshNonce).toBe(refreshNonce);
    expect(host.textContent).not.toContain("webModels.chatgpt.browser.signedIn");
  });

  it("keeps the recovery action after a failed check and removes it after explicit resume", async () => {
    mocks.session = {
      active: true,
      ready: true,
      can_resume_delivery: true,
      last_delivery_id: "receipt-1",
      last_delivery_status: "ambiguous",
    };
    mocks.fetchSession.mockResolvedValue({ ok: true, result: { browser_session: mocks.session } });
    mocks.resume.mockResolvedValueOnce({
      ok: false,
      error: { message: "An unsent draft remains" },
    });
    await act(async () =>
      root.render(<WebModelConnectorsTab isDark={false} currentGroupId="g_login" />),
    );
    const button = () =>
      Array.from(host.querySelectorAll("button")).find((b) =>
        b.textContent?.includes("webModels.chatgpt.buttons.resumeDelivery"),
      );
    expect(host.textContent).toContain("webModels.chatgpt.target.reviewBeforeResume");
    expect(button()).toBeDefined();
    await act(async () => button()!.click());
    expect(mocks.resume).toHaveBeenCalledWith("g_login", "browser", "receipt-1");
    expect(host.textContent).toContain("An unsent draft remains");
    expect(button()).toBeDefined();
    mocks.resume.mockResolvedValueOnce({
      ok: true,
      result: {
        browser_session: {
          active: true,
          ready: true,
          can_resume_delivery: false,
          last_delivery_status: "resolved",
        },
      },
    });
    await act(async () => button()!.click());
    expect(button()).toBeUndefined();
    expect(host.textContent).toContain("webModels.chatgpt.notices.deliveryResumed");
  });

  it("does not stack slow background inspections or poll a hidden page", async () => {
    await act(async () =>
      root.render(<WebModelConnectorsTab isDark={false} currentGroupId="g_login" />),
    );
    mocks.fetchSession.mockClear();
    const pending: (() => void)[] = [];
    mocks.fetchSession.mockImplementation(
      () =>
        new Promise((resolve) => {
          pending.push(() => resolve({ ok: true, result: { browser_session: mocks.session } }));
        }),
    );
    await act(async () => {
      await vi.advanceTimersByTimeAsync(12_000);
    });
    expect(mocks.fetchSession).toHaveBeenCalledTimes(1);
    await act(async () => pending.splice(0).forEach((resolve) => resolve()));
    vi.spyOn(document, "hidden", "get").mockReturnValue(true);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(12_000);
    });
    expect(mocks.fetchSession).toHaveBeenCalledTimes(1);
    vi.spyOn(document, "hidden", "get").mockReturnValue(false);
    await act(async () => document.dispatchEvent(new Event("visibilitychange")));
    expect(mocks.fetchSession).toHaveBeenCalledTimes(2);
    await act(async () => pending.splice(0).forEach((resolve) => resolve()));
  });
});
