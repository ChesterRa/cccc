// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { GroupSidebarItem } from "../../components/layout/GroupSidebarItem";
import type { GroupMeta } from "../../types";
import { useUIStore } from "../../stores/useUIStore";
import { PendingReplyNavigationContext, PendingSummaryContext } from "./context";
import { pendingFilterIndicator } from "./indicator";
import { PendingIndicatorText } from "./PendingIndicator";
import type { Summary } from "./usePendingSummary";

it("opens the Group's waiting view without selecting or dragging its row", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const open = vi.fn();
  const select = vi.fn();
  try {
    await act(async () =>
      root.render(
        <PendingSummaryContext.Provider
          value={{ summaries: { g: { state: "fresh", pending: 2 } }, refresh: () => {} }}
        >
          <PendingReplyNavigationContext.Provider value={open}>
            <GroupSidebarItem
              group={{ group_id: "g", title: "Alpha" } as GroupMeta}
              isActive={false}
              isCollapsed={false}
              onSelect={select}
            />
          </PendingReplyNavigationContext.Provider>
        </PendingSummaryContext.Provider>,
      ),
    );
    const button = host.querySelector<HTMLButtonElement>(
      "button[data-pending-state], button:has([data-pending-state])",
    )!;
    expect(button).toBeTruthy();
    expect(host.querySelector('[role="button"]')?.contains(button)).toBe(false);
    await act(async () => button.click());
    expect(open).toHaveBeenCalledWith("g");
    expect(select).not.toHaveBeenCalled();
    await act(async () =>
      root.render(
        <GroupSidebarItem
          group={{ group_id: "g" } as GroupMeta}
          isActive={false}
          isCollapsed={false}
          onSelect={select}
        />,
      ),
    );
    expect(host.querySelector("[data-pending-state]")).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("tells the user when the running daemon cannot count waiting replies", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const summary = { state: "unknown" as const, pending: null, daemonUnsupported: true };
  try {
    await act(async () =>
      root.render(
        <PendingSummaryContext.Provider value={{ summaries: { g: summary }, refresh: () => {} }}>
          <PendingReplyNavigationContext.Provider value={vi.fn()}>
            <GroupSidebarItem
              group={{ group_id: "g", title: "Alpha" } as GroupMeta}
              isActive={false}
              isCollapsed={false}
              onSelect={vi.fn()}
            />
          </PendingReplyNavigationContext.Provider>
        </PendingSummaryContext.Provider>,
      ),
    );
    const badge = host.querySelector("[data-pending-state]")!;
    expect(badge.textContent).toBe("?");
    const button = host.querySelector<HTMLButtonElement>("button:has([data-pending-state])")!;
    expect(button.getAttribute("aria-label")).toMatch(
      /^Alpha · (openPendingRepliesDaemonUnsupported|.*not supported by the running CCCC daemon)/,
    );
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});

it("hides sidebar indicators in both layouts without changing the waiting view", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const select = vi.fn();
  const open = vi.fn();
  let summary: Summary = { state: "fresh", pending: 2 };
  const render = async (collapsed: boolean) =>
    act(async () =>
      root.render(
        <PendingSummaryContext.Provider value={{ summaries: { g: summary }, refresh: () => {} }}>
          <PendingReplyNavigationContext.Provider value={open}>
            <GroupSidebarItem
              group={{ group_id: "g", title: "Alpha" } as GroupMeta}
              isActive={false}
              isCollapsed={collapsed}
              onSelect={select}
            />
            <div data-waiting-view>
              <PendingIndicatorText indicator={pendingFilterIndicator(summary, false)!} />
            </div>
          </PendingReplyNavigationContext.Provider>
        </PendingSummaryContext.Provider>,
      ),
    );
  try {
    useUIStore.setState({ showPendingPrompts: true });
    await render(false);
    expect(host.querySelectorAll("[data-pending-state]")).toHaveLength(2);
    await act(async () => useUIStore.getState().setShowPendingPrompts(false));
    expect(host.querySelectorAll("[data-pending-state]")).toHaveLength(1);
    expect(host.querySelector("[data-waiting-view]")?.textContent).toBe("2");
    for (const state of ["stale", "unknown"] as const) {
      summary = { state, pending: state === "stale" ? 0 : null };
      await render(false);
      // Hiding a navigation badge does not turn an uncertain count into zero.
      expect(host.querySelectorAll("[data-pending-state]")).toHaveLength(1);
      expect(host.querySelector("[data-waiting-view]")?.textContent).toBe("?");
    }
    summary = { state: "fresh", pending: 2 };
    await render(true);
    const group = host.querySelector<HTMLButtonElement>('button[aria-label="Alpha"]')!;
    expect(group).toBeTruthy();
    expect(group.querySelector("[data-pending-state]")).toBeNull();
    await act(async () => group.click());
    expect(select).toHaveBeenCalledOnce();
    expect(open).not.toHaveBeenCalled();
    await act(async () => useUIStore.getState().setShowPendingPrompts(true));
    expect(group.querySelector("[data-pending-state]")?.textContent).toBe("2");
  } finally {
    await act(async () => root.unmount());
    useUIStore.setState({ showPendingPrompts: true });
    localStorage.removeItem("cccc-pending-prompts");
    host.remove();
  }
});
