// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { GroupSidebarItem } from "../../components/layout/GroupSidebarItem";
import type { GroupMeta } from "../../types";
import { useUIStore } from "../../stores/useUIStore";
import { PendingReplyNavigationContext, PendingSummaryContext } from "./context";
import type { Summary } from "./usePendingSummary";

it("keeps waiting replies off every Group row, expanded or collapsed", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const select = vi.fn();
  const summaries: Record<string, Summary> = {
    g: { state: "fresh", pending: 2 },
    unsupported: { state: "unknown", pending: null, daemonUnsupported: true },
  };
  try {
    useUIStore.setState({ showPendingPrompts: true });
    for (const collapsed of [false, true]) {
      for (const groupId of ["g", "unsupported"]) {
        await act(async () =>
          root.render(
            <PendingSummaryContext.Provider value={{ summaries, refresh: () => {} }}>
              <PendingReplyNavigationContext.Provider value={vi.fn()}>
                <GroupSidebarItem
                  group={{ group_id: groupId, title: "Alpha" } as GroupMeta}
                  isActive={false}
                  isCollapsed={collapsed}
                  onSelect={select}
                />
              </PendingReplyNavigationContext.Provider>
            </PendingSummaryContext.Provider>,
          ),
        );
        // They are gathered beside Chet instead of scattered across the Groups.
        expect(host.querySelector("[data-pending-state]")).toBeNull();
      }
    }
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
