// @vitest-environment happy-dom
import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vite-plus/test";
import type { GroupMeta } from "../../types";
import { GroupSidebar, type GroupSidebarProps } from "./GroupSidebar";

vi.mock("react-i18next", () => ({
  useTranslation: () => ({ t: (key: string) => key, i18n: { language: "en" } }),
}));

const noop = () => {};
const sidebar: GroupSidebarProps = {
  orderedGroups: [
    { group_id: "working", title: "Working" },
    { group_id: "other", title: "Other" },
  ] as GroupMeta[],
  archivedGroupIds: [],
  selectedGroupId: "working",
  isOpen: true,
  isCollapsed: false,
  sidebarWidth: 248,
  isDark: false,
  readOnly: false,
  onSelectGroup: noop,
  onClose: noop,
  onToggleCollapse: noop,
  onResizeWidth: noop,
  onReorderSection: noop,
  onArchiveGroup: noop,
  onRestoreGroup: noop,
};

async function openMenuItem(host: HTMLElement, groupTitle: string, label: string) {
  const trigger = Array.from(host.querySelectorAll<HTMLButtonElement>("button")).find((button) =>
    String(button.getAttribute("aria-label") || "").includes(groupTitle),
  );
  expect(trigger).toBeDefined();
  await act(async () => trigger!.click());
  return Array.from(document.querySelectorAll<HTMLElement>("[role=menuitem]")).find((item) =>
    item.textContent?.includes(label),
  );
}

it("opens a group's Automation settings from that group's menu", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const openAutomation = vi.fn();
  try {
    await act(async () =>
      root.render(<GroupSidebar {...sidebar} onOpenGroupAutomation={openAutomation} />),
    );
    const item = await openMenuItem(host, "Other", "groupAutomation");
    expect(item).toBeDefined();
    await act(async () => item!.click());
    expect(openAutomation).toHaveBeenCalledWith("other");

    // Read-only viewers get no Automation entry.
    await act(async () =>
      root.render(<GroupSidebar {...sidebar} readOnly onOpenGroupAutomation={openAutomation} />),
    );
    expect(document.body.textContent).not.toContain("groupAutomation");
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
