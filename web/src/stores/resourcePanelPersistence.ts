import { clampRatio, resumeSplit } from "../components/resourcePanel/splitLayout";
import {
  resourceKey,
  type GroupPanel,
  type ResourceDescriptor,
  type ResourceTab,
} from "./resourcePanelModel";

export const RESOURCE_PANELS_KEY = "cccc-resource-panel-tabs";
type Panels = Record<string, GroupPanel>;
type RecordValue = Record<string, unknown>;
const record = (value: unknown): RecordValue | null =>
  value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as RecordValue)
    : null;
const nonempty = (value: unknown): value is string => typeof value === "string" && !!value.trim();

/** The index used when opening a quote counts only Presentation references. */
export function presentationReferenceIndex(eventId: string, refKey: string): number | null {
  const prefix = `${eventId}:`;
  if (!eventId || !refKey.startsWith(prefix)) return null;
  const suffix = refKey.slice(prefix.length);
  if (!/^(0|[1-9]\d*)$/.test(suffix)) return null;
  const index = Number(suffix);
  return Number.isSafeInteger(index) ? index : null;
}

/** Explicitly projects both saved and live tabs; new runtime fields never leak into storage. */
function projectTab(value: unknown): ResourceTab | null {
  const tab = record(value);
  if (!tab || typeof tab.title !== "string") return null;
  let resource: ResourceDescriptor;
  if (tab.kind === "workspace") {
    if (![tab.scopeKey, tab.scopeUrl, tab.path].every(nonempty)) return null;
    resource = {
      kind: "workspace",
      scopeKey: tab.scopeKey as string,
      scopeUrl: tab.scopeUrl as string,
      path: tab.path as string,
    };
  } else if (tab.kind === "attachment") {
    if (
      ![tab.messageKey, tab.eventId, tab.blobGroupId, tab.blobPath].every(nonempty) ||
      !Number.isSafeInteger(tab.index) ||
      Number(tab.index) < 0 ||
      !["image", "video", "markdown", "text", "pdf"].includes(String(tab.media)) ||
      typeof tab.isSvg !== "boolean"
    )
      return null;
    resource = {
      kind: "attachment",
      messageKey: tab.messageKey as string,
      eventId: tab.eventId as string,
      index: tab.index as number,
      blobGroupId: tab.blobGroupId as string,
      blobPath: tab.blobPath as string,
      media: tab.media as "image" | "video" | "markdown" | "text" | "pdf",
      isSvg: tab.isSvg,
      ...(typeof tab.sender === "string" ? { sender: tab.sender } : {}),
      ...(typeof tab.sentAt === "string" ? { sentAt: tab.sentAt } : {}),
    };
  } else if (tab.kind === "presentation") {
    if (
      !nonempty(tab.slotId) ||
      !Number.isInteger(tab.slotIndex) ||
      Number(tab.slotIndex) < 1 ||
      Number(tab.slotIndex) > 4 ||
      !nonempty(tab.refKey)
    )
      return null;
    if (
      tab.refKey !== "live" &&
      (!nonempty(tab.focusEventId) ||
        presentationReferenceIndex(tab.focusEventId, tab.refKey) === null)
    )
      return null;
    resource = {
      kind: "presentation",
      slotId: tab.slotId,
      slotIndex: tab.slotIndex as number,
      refKey: tab.refKey,
      ...(tab.refKey !== "live" ? { focusEventId: tab.focusEventId as string } : {}),
    };
  } else return null;
  return { ...resource, key: resourceKey(resource), title: tab.title };
}

function projectPanels(value: unknown): Panels {
  const groups = record(value);
  if (!groups) return {};
  const panels: Panels = Object.create(null);
  for (const [groupId, value] of Object.entries(groups)) {
    const panel = record(value);
    if (
      !groupId ||
      !panel ||
      !Array.isArray(panel.tabs) ||
      typeof panel.activeKey !== "string" ||
      typeof panel.open !== "boolean" ||
      typeof panel.maximized !== "boolean"
    )
      continue;
    const tabs: ResourceTab[] = [];
    const keys = new Set<string>();
    for (const raw of panel.tabs) {
      const tab = projectTab(raw);
      if (!tab || keys.has(tab.key)) continue;
      keys.add(tab.key);
      tabs.push(tab);
    }
    const activeKey = keys.has(panel.activeKey) ? panel.activeKey : (tabs[0]?.key ?? "");
    const rawSplit = record(panel.split);
    const split =
      rawSplit &&
      typeof rawSplit.left === "string" &&
      typeof rawSplit.right === "string" &&
      (rawSplit.focus === "left" || rawSplit.focus === "right") &&
      typeof rawSplit.ratio === "number" &&
      Number.isFinite(rawSplit.ratio)
        ? resumeSplit(
            {
              left: rawSplit.left,
              right: rawSplit.right,
              focus: rawSplit.focus,
              ratio: clampRatio(rawSplit.ratio),
            },
            tabs,
            activeKey,
          )
        : null;
    panels[groupId] = {
      tabs,
      activeKey,
      open: panel.open && !!tabs.length,
      maximized: panel.open && panel.maximized && !!tabs.length,
      closed: [],
      split,
    };
  }
  return panels;
}

export function serializeResourcePanels(groups: Panels): string {
  const panels = projectPanels(groups);
  // The closed list is an in-memory undo history, not part of the saved view.
  return JSON.stringify({
    v: 1,
    groups: Object.fromEntries(
      Object.entries(panels).map(([id, panel]) => {
        const { closed: _closed, ...saved } = panel;
        return [id, saved];
      }),
    ),
  });
}

export function readResourcePanels(storage?: Pick<Storage, "getItem">): Panels {
  try {
    const raw = (storage ?? sessionStorage).getItem(RESOURCE_PANELS_KEY);
    const value = raw ? record(JSON.parse(raw)) : null;
    return value?.v === 1 ? projectPanels(value.groups) : {};
  } catch {
    return {};
  }
}

export function saveResourcePanels(groups: Panels, storage?: Pick<Storage, "setItem">): void {
  try {
    (storage ?? sessionStorage).setItem(RESOURCE_PANELS_KEY, serializeResourcePanels(groups));
  } catch {
    // An unavailable or full store must not prevent use of the resource panel.
  }
}
