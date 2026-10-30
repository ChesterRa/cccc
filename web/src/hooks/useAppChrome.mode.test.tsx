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

afterEach(() => {
  vi.useRealTimers();
  window.localStorage.clear();
  vi.restoreAllMocks();
});

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

it("rechecks version differences on focus without replaying a dismissed pair", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.ping.mockReset();
  mocks.session.mockResolvedValue({ ok: true, result: { web_access_session: null } });
  const result = (daemon: string) => ({
    ok: true,
    result: { version: "0.4.45", daemon: { version: daemon }, web: { read_only: false } },
  });
  mocks.ping.mockResolvedValue(result("0.4.44"));
  let current: ReturnType<typeof useAppChrome> | undefined;
  const noop = () => {};
  function Harness() {
    current = useAppChrome({
      parseUrlDeepLink: noop,
      refreshGroups: noop,
      setWebReadOnly: noop,
      setSmallScreen: noop,
      showError: noop,
      setDirSuggestions: noop,
      groupEditOpen: false,
      addActorOpen: false,
      editingActor: null,
    });
    return null;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    expect(current?.daemonVersionMismatch).toEqual({
      webVersion: "0.4.45",
      daemonVersion: "0.4.44",
    });
    const firstDifference = current?.daemonVersionMismatch;
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(current?.daemonVersionMismatch).toBe(firstDifference);
    await act(async () => current?.dismissDaemonVersionMismatch());
    expect(current?.daemonVersionMismatch).toBeNull();
    expect(window.localStorage.getItem("cccc.daemonVersionMismatch.dismissed")).toBe(
      JSON.stringify(["0.4.45", "0.4.44"]),
    );
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(current?.daemonVersionMismatch).toBeNull();
    mocks.ping.mockResolvedValue(result("0.4.43"));
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(current?.daemonVersionMismatch?.daemonVersion).toBe("0.4.43");
    mocks.ping.mockResolvedValue({ ok: false });
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(current?.daemonVersionMismatch).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("remembers a dismissed version pair across page loads and other tabs", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.ping.mockReset();
  mocks.session.mockResolvedValue({ ok: true, result: { web_access_session: null } });
  mocks.ping.mockResolvedValue({
    ok: true,
    result: { version: "0.4.45", daemon: { version: "0.4.44" }, web: { read_only: false } },
  });
  const key = "cccc.daemonVersionMismatch.dismissed";
  const pair = JSON.stringify(["0.4.45", "0.4.44"]);
  window.localStorage.setItem(key, pair);
  let current: ReturnType<typeof useAppChrome> | undefined;
  const noop = () => {};
  function Harness() {
    current = useAppChrome({
      parseUrlDeepLink: noop,
      refreshGroups: noop,
      setWebReadOnly: noop,
      setSmallScreen: noop,
      showError: noop,
      setDirSuggestions: noop,
      groupEditOpen: false,
      addActorOpen: false,
      editingActor: null,
    });
    return null;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    expect(current?.daemonVersionMismatch).toBeNull();
    window.localStorage.setItem(key, JSON.stringify(["0.4.45", "0.4.43"]));
    await act(async () => window.dispatchEvent(new StorageEvent("storage", { key })));
    expect(current?.daemonVersionMismatch?.daemonVersion).toBe("0.4.44");
    window.localStorage.setItem(key, pair);
    await act(async () => window.dispatchEvent(new StorageEvent("storage", { key })));
    expect(current?.daemonVersionMismatch).toBeNull();
    window.localStorage.clear();
    await act(async () => window.dispatchEvent(new StorageEvent("storage", { key: null })));
    expect(current?.daemonVersionMismatch?.daemonVersion).toBe("0.4.44");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("keeps dismissal for the current page if browser storage is unavailable", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.ping.mockReset();
  mocks.session.mockResolvedValue({ ok: true, result: { web_access_session: null } });
  mocks.ping.mockResolvedValue({
    ok: true,
    result: { version: "0.4.45", daemon: { version: "0.4.44" }, web: { read_only: false } },
  });
  vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
    throw new Error("unavailable");
  });
  vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
    throw new Error("unavailable");
  });
  let current: ReturnType<typeof useAppChrome> | undefined;
  const noop = () => {};
  function Harness() {
    current = useAppChrome({
      parseUrlDeepLink: noop,
      refreshGroups: noop,
      setWebReadOnly: noop,
      setSmallScreen: noop,
      showError: noop,
      setDirSuggestions: noop,
      groupEditOpen: false,
      addActorOpen: false,
      editingActor: null,
    });
    return null;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    expect(current?.daemonVersionMismatch?.daemonVersion).toBe("0.4.44");
    await act(async () => current?.dismissDaemonVersionMismatch());
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(current?.daemonVersionMismatch).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("does not let an older ping overwrite a newer version result", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.ping.mockReset();
  mocks.session.mockResolvedValue({ ok: true, result: { web_access_session: null } });
  const replies: Array<(response: unknown) => void> = [];
  mocks.ping.mockImplementation(() => new Promise((resolve) => replies.push(resolve)));
  let current: ReturnType<typeof useAppChrome> | undefined;
  const noop = () => {};
  function Harness() {
    current = useAppChrome({
      parseUrlDeepLink: noop,
      refreshGroups: noop,
      setWebReadOnly: noop,
      setSmallScreen: noop,
      showError: noop,
      setDirSuggestions: noop,
      groupEditOpen: false,
      addActorOpen: false,
      editingActor: null,
    });
    return null;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness />));
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(replies).toHaveLength(2);
    await act(async () =>
      replies[1]?.({
        ok: true,
        result: { version: "0.4.45", daemon: { version: "0.4.44" }, web: { read_only: false } },
      }),
    );
    expect(current?.daemonVersionMismatch?.daemonVersion).toBe("0.4.44");
    await act(async () =>
      replies[0]?.({
        ok: true,
        result: { version: "0.4.45", daemon: { version: "0.4.45" }, web: { read_only: false } },
      }),
    );
    expect(current?.daemonVersionMismatch?.daemonVersion).toBe("0.4.44");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
