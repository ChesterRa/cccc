// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import { PendingSummaryContext } from "../../features/pendingReplies/context";
import { useUIStore } from "../../stores/useUIStore";
import type { GroupMeta } from "../../types";
import { AppHeader, type AppHeaderProps } from "./AppHeader";
import { GroupSidebar, type GroupSidebarProps } from "./GroupSidebar";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

const noop = () => {};
const sidebar: GroupSidebarProps = {
  orderedGroups: [
    { group_id: "working", title: "Working" },
    { group_id: "archived", title: "Archived group" },
  ] as GroupMeta[],
  archivedGroupIds: ["archived"],
  selectedGroupId: "working",
  isOpen: true,
  isCollapsed: false,
  sidebarWidth: 248,
  isDark: false,
  readOnly: true,
  onSelectGroup: noop,
  onClose: noop,
  onToggleCollapse: noop,
  onResizeWidth: noop,
  onReorderSection: noop,
  onArchiveGroup: noop,
  onRestoreGroup: noop,
};
const header: AppHeaderProps = {
  theme: "light",
  textScale: 100,
  onThemeChange: noop,
  onTextScaleChange: noop,
  selectedGroupId: "working",
  groupDoc: null,
  selectedGroupRunning: false,
  selectedGroupRuntimeStatus: null,
  sseStatus: "connected",
  onOpenSidebar: noop,
  onOpenSearch: noop,
  onOpenContext: noop,
  onOpenSettings: noop,
  canAccessAccount: false,
  onOpenAccount: noop,
  onOpenMobileMenu: noop,
};

it("hides pending summaries on the sidebar, collapsed rail and phone entry without hiding the archived Group count", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const context = {
    summaries: {
      working: { state: "fresh" as const, pending: 2 },
      archived: { state: "unknown" as const, pending: null },
    },
    refresh: noop,
  };
  const render = async (collapsed: boolean) =>
    act(async () =>
      root.render(
        <PendingSummaryContext.Provider value={context}>
          <GroupSidebar {...sidebar} isCollapsed={collapsed} />
          <AppHeader {...header} />
        </PendingSummaryContext.Provider>,
      ),
    );
  try {
    useUIStore.setState({ showPendingPrompts: true });
    await render(false);
    const aside = host.querySelector("aside")!;
    const phone = host.querySelector('[data-sidebar-toggle="true"]')!;
    expect(aside.querySelectorAll("[data-pending-state]")).toHaveLength(2);
    expect(phone.querySelector("[data-pending-state]")).not.toBeNull();
    expect(phone.getAttribute("aria-label")).toContain("pendingRepliesIndicator");
    const archived = [...aside.querySelectorAll("button")].find((button) =>
      button.textContent?.includes("archivedGroups"),
    )!;
    expect(archived.textContent).toContain("1");
    await act(async () => useUIStore.getState().setShowPendingPrompts(false));
    expect(aside.querySelector("[data-pending-state]")).toBeNull();
    expect(phone.querySelector("[data-pending-state]")).toBeNull();
    expect(phone.getAttribute("aria-label")).toBe("openSidebar");
    expect(archived.textContent).toContain("1");
    await render(true);
    expect(host.querySelector("[data-pending-state]")).toBeNull();
    await act(async () => useUIStore.getState().setShowPendingPrompts(true));
    expect(host.querySelectorAll("[data-pending-state]").length).toBeGreaterThanOrEqual(2);
  } finally {
    await act(async () => root.unmount());
    useUIStore.setState({ showPendingPrompts: true });
    localStorage.removeItem("cccc-pending-prompts");
    host.remove();
  }
});
