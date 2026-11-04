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
  session: vi.fn(),
  translate: (key: string) => key,
}));
vi.mock("react-i18next", () => ({ useTranslation: () => ({ t: state.translate }) }));
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
  fetchWebAccessSession: state.session,
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
    state.clear.mockReset();
    state.groups.mockReset().mockResolvedValue({ ok: true, result: { groups: [] } });
    state.session
      .mockReset()
      .mockImplementation(async () => ({
        ok: true,
        result: {
          web_access_session: {
            is_admin: state.admin,
            current_browser_signed_in: state.signedIn,
            bootstrap_required: state.bootstrap,
          },
        },
      }));
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
          <div data-workbench>
            target workbench
            <textarea aria-label="draft" />
          </div>
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

  it.each([
    ["network failure", "NETWORK_ERROR", undefined],
    ["interrupted successful response", "NETWORK_ERROR", 200],
    ["server failure", "HTTP_ERROR", 503],
  ])("keeps an admitted draft after a transient %s", async (_label, code, httpStatus) => {
    state.admin = true;
    await mount();
    const draft = host.querySelector("textarea");
    expect(draft).not.toBeNull();
    if (!draft) throw new Error("missing draft");
    draft.value = "unsent work";
    state.session.mockResolvedValueOnce({
      ok: false,
      error: { code, message: "fixture", httpStatus },
    });

    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(await state.session.mock.results.at(-1)?.value).toMatchObject({
      ok: false,
      error: { code, httpStatus },
    });
    expect(host.querySelector("textarea")).toBe(draft);
    expect(draft.value).toBe("unsent work");
    expect(state.clear).not.toHaveBeenCalled();
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(host.querySelector("textarea")).toBe(draft);
    expect(draft.value).toBe("unsent work");
  });

  it.each([
    ["a forbidden response", "HTTP_ERROR", 403],
    ["an interrupted forbidden response", "NETWORK_ERROR", 403],
    ["an unauthorized response", "unauthorized", 401],
  ])("closes an admitted workbench after %s", async (_label, code, httpStatus) => {
    state.admin = true;
    await mount();
    expect(host.querySelector("[data-workbench]")).not.toBeNull();
    state.session.mockResolvedValueOnce({
      ok: false,
      error: { code, message: "fixture", httpStatus },
    });
    await act(async () => vi.advanceTimersByTimeAsync(15000));
    expect(await state.session.mock.results.at(-1)?.value).toMatchObject({
      ok: false,
      error: { code, httpStatus },
    });
    expect(host.querySelector("[data-workbench]")).toBeNull();
    expect(state.clear).toHaveBeenCalled();
  });

  it("does not admit a new workbench when its first session check fails", async () => {
    state.session.mockResolvedValueOnce({
      ok: false,
      error: { code: "NETWORK_ERROR", message: "offline" },
    });
    await mount();
    expect(host.querySelector("[data-workbench]")).toBeNull();
    expect(state.groups).not.toHaveBeenCalled();
  });
});
