import type { CSSProperties } from "react";
import type { ResourceTab } from "../../stores/useResourcePanelStore";

export type SplitSide = "left" | "right";

/**
 * Two open tabs side by side, as in an editor's split view or a diff tool. The focused side is
 * the selected tab: picking a tab elsewhere opens it there.
 */
export type ResourceSplit = { left: string; right: string; focus: SplitSide; ratio: number };

/** A side's header: as tall as the panel's keys, which grow for touch. */
export const SPLIT_HEADER = { pointer: 32, touch: 44 };
export const MIN_RATIO = 0.2;
export const MAX_RATIO = 0.8;

export const clampRatio = (ratio: number) => Math.min(MAX_RATIO, Math.max(MIN_RATIO, ratio));
export const percent = (value: number) => `${(value * 100).toFixed(3)}%`;

/**
 * The other tab a split starts with: the one read before this one, or else its neighbour. A
 * lone tab is its own partner, for reading two places in one document.
 */
export function splitPartner(tabs: ResourceTab[], activeKey: string, previousKey: string) {
  if (previousKey !== activeKey && tabs.some((tab) => tab.key === previousKey)) return previousKey;
  const index = tabs.findIndex((tab) => tab.key === activeKey);
  return (tabs[index + 1] ?? tabs[index - 1])?.key ?? activeKey;
}

/** Starts a split with both tabs in their tab-strip order, the selected one focused. */
export function startSplit(tabs: ResourceTab[], activeKey: string, partner: string): ResourceSplit {
  if (partner === activeKey)
    return { left: activeKey, right: activeKey, focus: "left", ratio: 0.5 };
  const activeFirst =
    tabs.findIndex((tab) => tab.key === activeKey) < tabs.findIndex((tab) => tab.key === partner);
  return activeFirst
    ? { left: activeKey, right: partner, focus: "left", ratio: 0.5 }
    : { left: partner, right: activeKey, focus: "right", ratio: 0.5 };
}

/**
 * The pair shown last time, back with its divider, when the selected tab is one of the two and
 * both are still open; otherwise null.
 */
export function resumeSplit(
  last: ResourceSplit | null,
  tabs: ResourceTab[],
  activeKey: string,
): ResourceSplit | null {
  if (!last || (activeKey !== last.left && activeKey !== last.right)) return null;
  if (!tabs.some((tab) => tab.key === last.left) || !tabs.some((tab) => tab.key === last.right))
    return null;
  return { ...last, focus: activeKey === last.left ? "left" : "right" };
}

/**
 * Keeps a split in step with the tabs: the selected tab is always on one side, a tab selected
 * from the strip, **All** or a message opens on the focused side, and a side whose tab closed
 * takes another one. Both sides may show the same tab. Closing every tab ends the split.
 */
export function followTabs(
  split: ResourceSplit,
  tabs: ResourceTab[],
  activeKey: string,
): ResourceSplit | null {
  if (!tabs.length) return null;
  const open = new Set(tabs.map((tab) => tab.key));
  let { left, right, focus } = split;
  if (activeKey !== split[focus]) {
    if (activeKey === left) focus = "left";
    else if (activeKey === right) focus = "right";
    else if (open.has(activeKey)) {
      if (focus === "left") left = activeKey;
      else right = activeKey;
    }
  }
  // Another tab than the other side's, or that one when it is the only one left.
  const spare = (other: string) => tabs.find((tab) => tab.key !== other)?.key ?? other;
  if (!open.has(left)) left = spare(right);
  if (!open.has(right)) right = spare(left);
  if (left === split.left && right === split.right && focus === split.focus) return split;
  return { ...split, left, right, focus };
}

/**
 * The side a followed link moves: the side already showing its file, as the selection moves
 * there, or else the side the link was followed from, where the file opens.
 */
export function navigationSide(
  split: ResourceSplit,
  targetKey: string | null,
  origin: SplitSide,
): SplitSide {
  if (!targetKey || split[origin] === targetKey) return origin;
  if (split.left === targetKey) return "left";
  if (split.right === targetKey) return "right";
  return origin;
}

/** Where a side's header and content sit in the panel body. A phone stacks the two. */
export function splitBoxes(
  side: SplitSide,
  ratio: number,
  stacked: boolean,
  header = SPLIT_HEADER.pointer,
): { header: CSSProperties; content: CSSProperties } {
  const start = side === "left" ? 0 : ratio;
  const end = side === "left" ? ratio : 1;
  if (stacked)
    return {
      header: { top: percent(start), left: 0, right: 0, height: header },
      content: {
        top: `calc(${percent(start)} + ${header}px)`,
        left: 0,
        right: 0,
        bottom: percent(1 - end),
      },
    };
  return {
    header: { top: 0, left: percent(start), right: percent(1 - end), height: header },
    content: { top: header, left: percent(start), right: percent(1 - end), bottom: 0 },
  };
}
