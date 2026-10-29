// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { GroupSidebarItem } from "../../components/layout/GroupSidebarItem";
import type { GroupMeta } from "../../types";
import { PendingReplyNavigationContext, PendingSummaryContext } from "./context";

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
