// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { useUIStore } from "../stores";
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
