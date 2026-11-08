import type { ResourceTab } from "../../stores/useResourcePanelStore";

/**
 * How many attachment viewers stay mounted besides the selected one, newest first. A mounted PDF
 * keeps its page and a video its position, but each can use substantial memory in the browser, so
 * older ones load again from the start when selected. Images and the other kinds keep separate
 * counts, so looking through a series of screenshots does not unload the document being read.
 */
export const KEEP_IMAGES = 8;
export const KEEP_DOCUMENTS = 4;

export type RecentAttachments = { images: string[]; documents: string[] };

export const NO_RECENT_ATTACHMENTS: RecentAttachments = { images: [], documents: [] };

const same = (a: string[], b: string[]) =>
  a.length === b.length && a.every((key, index) => key === b[index]);

/**
 * Drops closed tabs from the lists, so they do not hold a place, and puts the selected attachment
 * first in its list. Returns the same lists when nothing changed.
 */
export function noteRecentAttachment(
  recent: RecentAttachments,
  tabs: ResourceTab[],
  activeKey: string,
): RecentAttachments {
  const open = new Set(tabs.map((tab) => tab.key));
  let images = recent.images.filter((key) => open.has(key));
  let documents = recent.documents.filter((key) => open.has(key));
  const tab = tabs.find((item) => item.key === activeKey);
  if (tab?.kind === "attachment") {
    const first = (list: string[], limit: number) =>
      [tab.key, ...list.filter((key) => key !== tab.key)].slice(0, limit);
    if (tab.media === "image") images = first(images, KEEP_IMAGES);
    else documents = first(documents, KEEP_DOCUMENTS);
  }
  return same(images, recent.images) && same(documents, recent.documents)
    ? recent
    : { images, documents };
}

/** The attachment tabs whose viewers stay mounted: the selected one and the most recent ones. */
export function mountedAttachments(
  tabs: ResourceTab[],
  activeKey: string,
  recent: RecentAttachments,
): Set<string> {
  const kept = new Set([...recent.images, ...recent.documents]);
  return new Set(
    tabs
      .filter((tab) => tab.kind === "attachment" && (tab.key === activeKey || kept.has(tab.key)))
      .map((tab) => tab.key),
  );
}
