export interface SidebarReorderState {
  isCollapsed: boolean;
  readOnly?: boolean;
}

// The whole row is the drag activator on every viewport; there is no separate
// drag handle. Reordering is off only where the row cannot host the gesture.
export type SidebarReorderActivation = "disabled" | "row";

export function getSidebarReorderActivation({
  isCollapsed,
  readOnly,
}: SidebarReorderState): SidebarReorderActivation {
  return isCollapsed || readOnly ? "disabled" : "row";
}

export interface SidebarSensorActivationConstraints {
  mouse: { distance: number };
  touch: { delay: number; tolerance: number };
}

// Mouse and touch are separate sensors on purpose. A single pointer sensor
// receives touch input too, so a distance constraint on it would start a drag
// from the first few pixels of a scroll gesture and the list would stop
// scrolling. Touch therefore requires a long press; a mouse drag can start
// from a short distance because it never competes with scrolling.
export function getSidebarSensorActivationConstraints(): SidebarSensorActivationConstraints {
  return { mouse: { distance: 4 }, touch: { delay: 250, tolerance: 8 } };
}

export function groupSidebarScrollClass(isCollapsed: boolean): string {
  const padding = isCollapsed
    ? "px-2 pt-2 pb-[calc(0.5rem+env(safe-area-inset-bottom,0px))]"
    : "px-2 pt-2 pb-[calc(0.75rem+env(safe-area-inset-bottom,0px))]";
  return `min-h-0 flex-1 overflow-auto overscroll-contain touch-pan-y scrollbar-hide ${padding}`;
}

// Shared density for sortable and read-only rows; touch targets stay at least 44px.
export const groupSidebarRowClass =
  "w-full px-2 py-1.5 md:py-0.5 !rounded-lg transition-colors min-h-[44px] md:min-h-9 pointer-coarse:min-h-[44px] pointer-coarse:py-1.5 flex items-center gap-1.5 relative";
