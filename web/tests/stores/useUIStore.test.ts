import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

function makeStorage() {
  const data = new Map<string, string>();
  return {
    getItem: vi.fn((key: string) => data.get(key) ?? null),
    setItem: vi.fn((key: string, value: string) => {
      data.set(key, String(value));
    }),
    removeItem: vi.fn((key: string) => {
      data.delete(key);
    }),
    clear: vi.fn(() => {
      data.clear();
    }),
  };
}

const localStorageMock = makeStorage();
vi.stubGlobal("localStorage", localStorageMock);
vi.stubGlobal("window", { setTimeout, clearTimeout });

describe("useUIStore sidebar width", () => {
  beforeEach(() => {
    vi.resetModules();
    localStorageMock.clear();
  });

  it("clamps persisted sidebar width through the public setter", async () => {
    const mod = await import("../../src/stores/useUIStore");
    mod.useUIStore.setState({ sidebarWidth: mod.SIDEBAR_DEFAULT_WIDTH });

    mod.useUIStore.getState().setSidebarWidth(999);
    expect(mod.useUIStore.getState().sidebarWidth).toBe(mod.SIDEBAR_MAX_WIDTH);

    mod.useUIStore.getState().setSidebarWidth(120);
    expect(mod.useUIStore.getState().sidebarWidth).toBe(mod.SIDEBAR_MIN_WIDTH);
  });

  it("exports a stable clamp helper for desktop resize math", async () => {
    const mod = await import("../../src/stores/useUIStore");
    expect(mod.clampSidebarWidth(NaN)).toBe(mod.SIDEBAR_DEFAULT_WIDTH);
    expect(mod.clampSidebarWidth(281.7)).toBe(282);
    expect(mod.SIDEBAR_MAX_WIDTH).toBe(360);
    expect(mod.getSidebarWidthCssValue(999)).toBe("clamp(248px, 360px, min(360px, 34vw))");
  });

  it("tracks presentation dock open state per group", async () => {
    const mod = await import("../../src/stores/useUIStore");
    mod.useUIStore.getState().setChatPresentationDockOpen("g-demo", true);
    expect(
      mod.getChatSession("g-demo", mod.useUIStore.getState().chatSessions).presentationDockOpen,
    ).toBe(true);

    mod.useUIStore.getState().setChatPresentationDockOpen("g-demo", false);
    expect(
      mod.getChatSession("g-demo", mod.useUIStore.getState().chatSessions).presentationDockOpen,
    ).toBe(false);
  });

  it("does not publish duplicate chat scroll state updates", async () => {
    const mod = await import("../../src/stores/useUIStore");
    const listener = vi.fn();
    const unsubscribe = mod.useUIStore.subscribe(listener);

    mod.useUIStore.getState().setShowScrollButton("g-demo", true);
    mod.useUIStore.getState().setShowScrollButton("g-demo", true);
    mod.useUIStore.getState().setChatUnreadCount("g-demo", 3);
    mod.useUIStore.getState().setChatUnreadCount("g-demo", 3);

    expect(listener).toHaveBeenCalledTimes(2);
    unsubscribe();
  });

  it("does not publish the same chat scroll snapshot twice", async () => {
    const mod = await import("../../src/stores/useUIStore");
    const listener = vi.fn();
    const unsubscribe = mod.useUIStore.subscribe(listener);
    const snapshot = {
      mode: "detached" as const,
      anchorId: "event-42",
      offsetPx: 80,
      updatedAt: 123,
    };

    mod.useUIStore.getState().setChatScrollSnapshot("g-demo", snapshot);
    mod.useUIStore.getState().setChatScrollSnapshot("g-demo", snapshot);

    expect(listener).toHaveBeenCalledTimes(1);
    unsubscribe();
  });

  it("keeps detached scroll snapshots only for the current page lifetime", async () => {
    let mod = await import("../../src/stores/useUIStore");
    mod.useUIStore
      .getState()
      .setChatScrollSnapshot("g-demo", {
        mode: "detached",
        anchorId: "evt-42",
        offsetPx: 96,
        updatedAt: 123456,
      });

    expect(
      mod.getChatSession("g-demo", mod.useUIStore.getState().chatSessions).scrollSnapshot,
    ).toEqual({ mode: "detached", anchorId: "evt-42", offsetPx: 96, updatedAt: 123456 });

    vi.resetModules();
    mod = await import("../../src/stores/useUIStore");
    expect(
      mod.getChatSession("g-demo", mod.useUIStore.getState().chatSessions).scrollSnapshot,
    ).toBeNull();
  });

  it("ignores legacy persisted scroll snapshots on reload", async () => {
    localStorageMock.setItem(
      "cccc-chat-sessions",
      JSON.stringify({
        "g-demo": {
          chatFilter: "all",
          scrollSnapshot: { mode: "follow", anchorId: "evt-stale", offsetPx: 40, updatedAt: 200 },
        },
      }),
    );

    const mod = await import("../../src/stores/useUIStore");
    expect(
      mod.getChatSession("g-demo", mod.useUIStore.getState().chatSessions).scrollSnapshot,
    ).toBeNull();
  });

  it("ignores obsolete runtime dock payloads on reload", async () => {
    localStorageMock.setItem(
      "cccc-chat-sessions",
      JSON.stringify({
        "g-demo": { runtimeDockExpanded: 1, runtimeDockFocusedActorId: { actor: "coder" } },
      }),
    );

    const mod = await import("../../src/stores/useUIStore");
    expect(mod.getChatSession("g-demo", mod.useUIStore.getState().chatSessions)).toMatchObject({
      presentationDockOpen: false,
      presentationDisplayMode: "modal",
    });
  });
});
