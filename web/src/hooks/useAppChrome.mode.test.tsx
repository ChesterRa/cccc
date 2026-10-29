// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useAppChrome } from "./useAppChrome";

const mocks = vi.hoisted(() => ({ ping: vi.fn(), session: vi.fn(), setAccess: vi.fn() }));
vi.mock("../services/api", () => ({ fetchPing: mocks.ping, fetchWebAccessSession: mocks.session }));
vi.mock("../stores", () => ({
  useUIStore: (select: (value: unknown) => unknown) =>
    select({ canAccessGlobalSettings: null, setCanAccessGlobalSettings: mocks.setAccess }),
  useGroupStore: { getState: () => ({ runtimeDetectionStatus: "loaded" }) },
  useObservabilityStore: { getState: () => ({ setRuntimeVisibilityFromSession: () => {} }) },
}));

afterEach(() => vi.useRealTimers());

it("does not expose the mode until ping confirms it, and retries a failed probe", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  mocks.ping.mockReset();
  mocks.session.mockResolvedValue({ ok: true, result: { web_access_session: null } });
  mocks.ping.mockResolvedValueOnce({ ok: false });
  mocks.ping.mockResolvedValueOnce({ ok: true, result: { web: { read_only: true } } });
  const setReadOnly = vi.fn();
  let known = false;
  const noop = () => {};
  function Harness() {
    const value = useAppChrome({
      parseUrlDeepLink: noop,
      refreshGroups: noop,
      setWebReadOnly: setReadOnly,
      setSmallScreen: noop,
      showError: noop,
      setDirSuggestions: noop,
      groupEditOpen: false,
      addActorOpen: false,
      editingActor: null,
    });
    known = value.webModeKnown;
    return null;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    expect(known).toBe(false);
    expect(setReadOnly).not.toHaveBeenCalled();
    await act(async () => vi.advanceTimersByTimeAsync(5000));
    expect(setReadOnly).toHaveBeenCalledWith(true);
    expect(known).toBe(true);
    expect(mocks.ping).toHaveBeenCalledTimes(2);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
  expect(vi.getTimerCount()).toBe(0);
});
