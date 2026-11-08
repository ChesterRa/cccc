import type { ResourceTab } from "../../stores/useResourcePanelStore";

/** Where a tab came from, shown under its name so two of the same name can be told apart. */
export function resourceSource(
  tab: ResourceTab,
  t: (key: string, options?: Record<string, unknown>) => string,
  language: string,
) {
  if (tab.kind === "workspace") return tab.path;
  if (tab.kind === "presentation") return t("resourcePanel.fromSlot", { slot: tab.slotIndex });
  const time = sentTime(tab.sentAt, language);
  return tab.sender && time
    ? t("resourcePanel.fromMessageBy", { sender: tab.sender, time })
    : t("resourcePanel.fromMessage");
}

/** Today's messages show the time; older ones the date as well. */
function sentTime(value: string | undefined, language: string) {
  const date = value ? new Date(value) : null;
  if (!date || Number.isNaN(date.getTime())) return "";
  const today = new Date().toDateString() === date.toDateString();
  return date.toLocaleString(language, {
    ...(today ? {} : { month: "numeric", day: "numeric" }),
    hour: "2-digit",
    minute: "2-digit",
  });
}
