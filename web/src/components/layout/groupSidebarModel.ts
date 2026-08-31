export interface SidebarReorderState {
  isSmallScreen: boolean;
  isCollapsed: boolean;
  readOnly?: boolean;
}

export function canReorderSidebarGroups({
  isSmallScreen,
  isCollapsed,
  readOnly,
}: SidebarReorderState): boolean {
  return !isSmallScreen && !isCollapsed && !readOnly;
}

export function groupSidebarScrollClass(isCollapsed: boolean): string {
  const padding = isCollapsed
    ? "px-2 pt-2 pb-[calc(0.5rem+env(safe-area-inset-bottom,0px))]"
    : "px-3 pt-3 pb-[calc(0.75rem+env(safe-area-inset-bottom,0px))]";
  return `min-h-0 flex-1 overflow-auto overscroll-contain touch-pan-y scrollbar-hide ${padding}`;
}
