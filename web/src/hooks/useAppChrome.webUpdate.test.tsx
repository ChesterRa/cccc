// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vite-plus/test";
import { useAppChrome } from "./useAppChrome";

const mocks = vi.hoisted(() => ({
  ping: vi.fn(),
  session: vi.fn(),
  setAccess: vi.fn(),
  setAdmin: vi.fn(),
  entry: vi.fn(() => "/ui/assets/loaded.js"),
}));
vi.mock("../services/api", () => ({ fetchPing: mocks.ping, fetchWebAccessSession: mocks.session }));
vi.mock("../utils/runtimeBuildInfo", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../utils/runtimeBuildInfo")>()),
  loadedWebEntry: mocks.entry,
}));
vi.mock("../stores", () => ({
  useUIStore: (select: (value: unknown) => unknown) =>
    select({
      canAccessGlobalSettings: null,
      setCanAccessGlobalSettings: mocks.setAccess,
      setIsWebAdmin: mocks.setAdmin,
    }),
  useGroupStore: { getState: () => ({ runtimeDetectionStatus: "loaded" }) },
  useObservabilityStore: { getState: () => ({ setRuntimeVisibilityFromSession: () => {} }) },
}));

afterEach(() => {
  vi.restoreAllMocks();
  mocks.entry.mockReturnValue("/ui/assets/loaded.js");
});

it("notices a changed served entry on focus and reconnect without losing known state on failure", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.ping.mockReset();
  mocks.session.mockResolvedValue({ ok: true, result: { web_access_session: null } });
  const ping = (entry?: string) => ({
    ok: true,
    result: { web: { read_only: false, entry_script: entry, assets_id: "assets-a" } },
  });
  mocks.ping.mockResolvedValue(ping("/ui/assets/loaded.js"));
  const refreshGroups = vi.fn();
  let current: ReturnType<typeof useAppChrome> | undefined;
  const noop = () => {};
  function Harness({ status }: { status: "connected" | "disconnected" }) {
    current = useAppChrome({
      parseUrlDeepLink: noop,
      refreshGroups,
      setWebReadOnly: noop,
      setSmallScreen: noop,
      showError: noop,
      setDirSuggestions: noop,
      groupEditOpen: false,
      addActorOpen: false,
      editingActor: null,
      sseStatus: status,
    });
    return null;
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    await act(async () => root.render(<Harness status="disconnected" />));
    expect(current?.webUpdateAvailable).toBe(false);
    mocks.ping.mockResolvedValue(ping("/ui/assets/new.js"));
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(current?.webUpdateAvailable).toBe(true);
    mocks.ping.mockResolvedValue({ ok: false });
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(current?.webUpdateAvailable).toBe(true);
    await act(async () => current?.dismissWebUpdate());
    mocks.ping.mockResolvedValue(ping("/ui/assets/new.js"));
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(current?.webUpdateAvailable).toBe(false);
    mocks.ping.mockResolvedValue(ping("/ui/assets/newer.js"));
    await act(async () => root.render(<Harness status="connected" />));
    expect(current?.webUpdateAvailable).toBe(true);
    mocks.ping.mockResolvedValue(ping(undefined));
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(current?.webUpdateAvailable).toBe(true);
    mocks.ping.mockResolvedValue(ping("/ui/assets/loaded.js"));
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(current?.webUpdateAvailable).toBe(false);
    expect(refreshGroups).toHaveBeenCalledTimes(1);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("ignores an older ping after a newer entry has been observed", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.ping.mockReset();
  mocks.session.mockResolvedValue({ ok: true, result: { web_access_session: null } });
  const pending: Array<(response: unknown) => void> = [];
  mocks.ping.mockImplementation(() => new Promise((resolve) => pending.push(resolve)));
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
    expect(pending).toHaveLength(2);
    await act(async () =>
      pending[1]({
        ok: true,
        result: { web: { read_only: false, entry_script: "/ui/assets/new.js", assets_id: "b" } },
      }),
    );
    expect(current?.webUpdateAvailable).toBe(true);
    await act(async () =>
      pending[0]({
        ok: true,
        result: { web: { read_only: false, entry_script: "/ui/assets/loaded.js", assets_id: "a" } },
      }),
    );
    expect(current?.webUpdateAvailable).toBe(true);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("tracks later asset-only updates without treating partial or dev identities as new builds", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.ping.mockReset();
  mocks.session.mockResolvedValue({ ok: true, result: { web_access_session: null } });
  const ping = (entry: string, assetsId?: string) => ({
    ok: true,
    result: { web: { read_only: false, entry_script: entry, assets_id: assetsId } },
  });
  mocks.ping.mockResolvedValue(ping("/ui/assets/loaded.js", "assets-a"));
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
  const recheck = async (entry: string, assetsId?: string) => {
    mocks.ping.mockResolvedValue(ping(entry, assetsId));
    await act(async () => window.dispatchEvent(new Event("focus")));
  };
  try {
    await act(async () => root.render(<Harness />));
    expect(current?.webUpdateAvailable).toBe(false);
    await recheck("/ui/assets/loaded.js", "assets-b");
    expect(current?.webUpdateAvailable).toBe(true);
    await act(async () => current?.dismissWebUpdate());
    await recheck("/ui/assets/loaded.js", "assets-b");
    expect(current?.webUpdateAvailable).toBe(false);
    await recheck("/ui/assets/loaded.js");
    expect(current?.webUpdateAvailable).toBe(false);
    await recheck("/ui/assets/loaded.js", "assets-c");
    expect(current?.webUpdateAvailable).toBe(true);
    await recheck("/ui/assets/loaded.js");
    expect(current?.webUpdateAvailable).toBe(true);
    await recheck("/ui/assets/loaded.js", "assets-a");
    expect(current?.webUpdateAvailable).toBe(false);

    await recheck("/ui/assets/new.js", "new-assets-a");
    expect(current?.webUpdateAvailable).toBe(true);
    await act(async () => current?.dismissWebUpdate());
    await recheck("/ui/assets/new.js", "new-assets-b");
    expect(current?.webUpdateAvailable).toBe(true);

    mocks.entry.mockReturnValue("/ui/src/main.tsx");
    await recheck("/ui/assets/new.js", "new-assets-c");
    expect(current?.webUpdateAvailable).toBe(true);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("does not establish an asset baseline or show an update for a dev-server entry", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  mocks.ping.mockReset();
  mocks.session.mockResolvedValue({ ok: true, result: { web_access_session: null } });
  mocks.entry.mockReturnValue("/ui/src/main.tsx");
  mocks.ping.mockResolvedValue({
    ok: true,
    result: { web: { read_only: false, entry_script: "/ui/assets/server.js", assets_id: "a" } },
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
    expect(current?.webUpdateAvailable).toBe(false);
    mocks.ping.mockResolvedValue({
      ok: true,
      result: { web: { read_only: false, entry_script: "/ui/assets/server.js", assets_id: "b" } },
    });
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(current?.webUpdateAvailable).toBe(false);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
