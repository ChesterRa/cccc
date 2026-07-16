export function nextActorConfigTabId(
  tabIds: readonly string[],
  activeId: string,
  key: string,
): string {
  if (tabIds.length === 0) return activeId;
  const activeIndex = Math.max(0, tabIds.indexOf(activeId));

  if (key === "Home") return tabIds[0];
  if (key === "End") return tabIds[tabIds.length - 1];
  if (key === "ArrowRight") return tabIds[(activeIndex + 1) % tabIds.length];
  if (key === "ArrowLeft") return tabIds[(activeIndex - 1 + tabIds.length) % tabIds.length];
  return activeId;
}
