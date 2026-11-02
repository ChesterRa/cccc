// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { useGroupStore, useUIStore } from "../stores";
import { EMPTY_CHAT_BUCKET } from "../stores/groupStoreCore";
import { useAppTabState } from "./useAppTabState";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

function Harness({ activeTab, isSmallScreen }: { activeTab: string; isSmallScreen: boolean }) {
  const { composerRef } = useAppTabState({
    activeTab,
    runtimeActors: [],
    selectedGroupId: "g1",
    isSmallScreen,
    setActiveTab: vi.fn(),
    setShowScrollButton: vi.fn(),
    setChatUnreadCount: vi.fn(),
  });
  return (
    <>
      <textarea ref={composerRef} aria-label="composer" />
      <button type="button">elsewhere</button>
      <div role="dialog" aria-modal="true">
        <button type="button">in dialog</button>
      </div>
    </>
  );
}

it("focuses the composer on arriving at the messages, not when the viewport changes", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const previous = useUIStore.getState();
  const frame = () =>
    act(() => new Promise<void>((resolve) => requestAnimationFrame(() => resolve())));
  const render = async (activeTab: string, isSmallScreen: boolean) => {
    useUIStore.setState({ activeTab, isSmallScreen });
    await act(async () =>
      root.render(<Harness activeTab={activeTab} isSmallScreen={isSmallScreen} />),
    );
    await frame();
  };
  const composer = () => host.querySelector("textarea")!;
  const button = (name: string) =>
    [...host.querySelectorAll("button")].find((item) => item.textContent === name)!;
  try {
    await render("chat", false);
    expect(document.activeElement).toBe(composer());

    // Turning a phone to a wide layout leaves the focus where the user put it.
    await render("chat", true);
    button("elsewhere").focus();
    await render("chat", false);
    expect(document.activeElement).toBe(button("elsewhere"));

    // Arriving at the messages while a dialog holds the focus leaves it there.
    await render("actor-1", false);
    button("in dialog").focus();
    await render("chat", false);
    expect(document.activeElement).toBe(button("in dialog"));

    button("elsewhere").focus();
    await render("actor-1", false);
    await render("chat", false);
    expect(document.activeElement).toBe(composer());
  } finally {
    act(() => root.unmount());
    useUIStore.setState({ activeTab: previous.activeTab, isSmallScreen: previous.isSmallScreen });
    host.remove();
  }
});

it("keeps unread messages when returning to the bottom of a historical window", async () => {
  const previousUi = useUIStore.getState();
  const previousGroups = useGroupStore.getState();
  const setUnread = vi.fn();
  const setScrollButton = vi.fn();
  let state: ReturnType<typeof useAppTabState>;
  function VisibilityHarness() {
    state = useAppTabState({
      activeTab: "chat",
      runtimeActors: [],
      selectedGroupId: "g1",
      isSmallScreen: false,
      setActiveTab: vi.fn(),
      setShowScrollButton: setScrollButton,
      setChatUnreadCount: setUnread,
    });
    return (
      <div
        ref={(element) => {
          state.eventContainerRef.current = element;
          if (!element) return;
          Object.defineProperties(element, {
            scrollHeight: { configurable: true, value: 1000 },
            clientHeight: { configurable: true, value: 400 },
            scrollTop: { configurable: true, value: 600 },
          });
        }}
      />
    );
  }
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  try {
    useGroupStore.setState({
      selectedGroupId: "g1",
      chatByGroup: {
        g1: {
          ...EMPTY_CHAT_BUCKET,
          chatWindow: {
            groupId: "g1",
            centerEventId: "earlier",
            centerIndex: 0,
            events: [],
            hasMoreBefore: true,
            hasMoreAfter: true,
          },
        },
      },
    });
    useUIStore.setState({
      activeTab: "chat",
      isSmallScreen: false,
      workspaceFileViewerGroupId: "g1",
      chatSessions: {},
    });
    await act(async () => root.render(<VisibilityHarness />));
    setUnread.mockClear();
    setScrollButton.mockClear();

    await act(async () => useUIStore.getState().setWorkspaceFileViewerGroupId(""));
    expect(setUnread).not.toHaveBeenCalled();
    expect(setScrollButton).toHaveBeenCalledWith("g1", true);
    expect(state!.chatAtBottomRef.current).toBe(false);

    // The same physical position is the true bottom once the live list is shown.
    await act(async () => {
      useUIStore.getState().setWorkspaceFileViewerGroupId("g1");
      useGroupStore.setState((previous) => ({
        chatByGroup: {
          ...previous.chatByGroup,
          g1: { ...previous.chatByGroup.g1, chatWindow: null },
        },
      }));
    });
    setUnread.mockClear();
    setScrollButton.mockClear();
    await act(async () => useUIStore.getState().setWorkspaceFileViewerGroupId(""));
    expect(setUnread).toHaveBeenCalledWith("g1", 0);
    expect(setScrollButton).toHaveBeenCalledWith("g1", false);
    expect(state!.chatAtBottomRef.current).toBe(true);
  } finally {
    act(() => root.unmount());
    useUIStore.setState({
      activeTab: previousUi.activeTab,
      isSmallScreen: previousUi.isSmallScreen,
      workspaceFileViewerGroupId: previousUi.workspaceFileViewerGroupId,
      chatSessions: previousUi.chatSessions,
    });
    useGroupStore.setState({
      selectedGroupId: previousGroups.selectedGroupId,
      chatByGroup: previousGroups.chatByGroup,
    });
    host.remove();
  }
});
