// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

const state = vi.hoisted(() => ({
  admin: false,
  signedIn: false,
  bootstrap: false,
  groups: vi.fn(),
  clear: vi.fn(),
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: (key: string) => key }) }));
vi.mock("./auth/AuthTokenLoginForm", () => ({
  AuthTokenLoginForm: ({
    error,
    onSubmit,
  }: {
    error: string;
    onSubmit: (token: string) => void;
  }) => <button onClick={() => onSubmit("target-secret")}>{error || "login"}</button>,
}));
vi.mock("../services/api", () => ({
  shouldForceTokenLogin: () => false,
  clearAuthToken: state.clear,
  setAuthToken: vi.fn(),
  clearForceTokenLogin: vi.fn(),
  isAuthRequiredErrorCode: (code: string) => code === "unauthorized",
  onAuthRequired: () => () => {},
  fetchWebAccessSession: async () => ({
    ok: true,
    result: {
      web_access_session: {
        is_admin: state.admin,
        current_browser_signed_in: state.signedIn,
        bootstrap_required: state.bootstrap,
      },
    },
  }),
  fetchGroups: state.groups,
}));
import { AuthGate } from "./AuthGate";

describe("Connect administrator login gate", () => {
  let host: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.useFakeTimers();
    state.admin = false;
    state.signedIn = true;
    state.bootstrap = false;
    state.groups.mockReset().mockResolvedValue({ ok: true, result: { groups: [] } });
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
  });
  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.useRealTimers();
  });
  const mount = () =>
    act(async () =>
      root.render(
        <AuthGate requireAdmin>
          <div data-workbench>target workbench</div>
        </AuthGate>,
      ),
    );
  it("never reads target groups or mounts the workbench with a restricted token", async () => {
    await mount();
    expect(host.textContent).toContain("connect.adminRequired");
    expect(state.groups).not.toHaveBeenCalled();
    await act(async () => host.querySelector("button")?.click());
    expect(host.querySelector("[data-workbench]")).toBeNull();
    expect(state.groups).not.toHaveBeenCalled();
  });
  it("does not bootstrap an unconfigured target through an embedded view", async () => {
    state.bootstrap = true;
    state.signedIn = false;
    await mount();
    expect(host.querySelector("[data-workbench]")).toBeNull();
    expect(state.groups).not.toHaveBeenCalled();
  });
  it("uses the target login and unmounts its live workbench after administrator revocation", async () => {
    state.admin = true;
    await mount();
    expect(host.querySelector("[data-workbench]")).not.toBeNull();
    state.admin = false;
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(host.querySelector("[data-workbench]")).toBeNull();
    expect(host.textContent).toContain("connect.adminRequired");
  });
});
