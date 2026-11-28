import { create } from "zustand";
import type { AttachmentResource } from "./resourcePanelModel";
import { useResourcePanelStore } from "./useResourcePanelStore";

/** One attachment a message can show in the page, under the name the message gives it. */
export type AttachmentPreviewItem = { resource: AttachmentResource; title: string };

/** The attachment shown over the whole page, by its place in its message. */
export type AttachmentViewing = {
  panelGroupId: string;
  messageKey: string;
  index: number;
  /** Reached with the previous and next keys rather than clicked: a video waits to be played. */
  stepped: boolean;
};

type AttachmentPreviewState = {
  /**
   * A message's previewable attachments in the message's own order, for the messages that the
   * full-screen preview or an open panel tab shows. Others are not kept.
   */
  sets: Record<string, AttachmentPreviewItem[]>;
  viewing: AttachmentViewing | null;
  /** Keeps a message's attachments current, such as after its upload is confirmed. */
  remember: (panelGroupId: string, messageKey: string, items: AttachmentPreviewItem[]) => void;
  view: (
    panelGroupId: string,
    messageKey: string,
    items: AttachmentPreviewItem[],
    index: number,
  ) => void;
  /** Moves to the previous or next attachment of the same message; it stops at either end. */
  step: (delta: -1 | 1) => void;
  close: () => void;
};

/**
 * The message key of pictures still waiting in the composer. They exist only as local files
 * until sent, so they can be viewed at full size but have nothing to download or to keep open
 * in the resource panel.
 */
export const COMPOSER_DRAFT_MESSAGE_KEY = "composer-draft";

export function attachmentSetKey(panelGroupId: string, messageKey: string) {
  return `${panelGroupId}\n${messageKey}`;
}

/** The message's attachments around one of them, or null when it is the only one. */
export function attachmentSequence(items: AttachmentPreviewItem[] | undefined, index: number) {
  const position = items ? items.findIndex((item) => item.resource.index === index) : -1;
  if (!items || position < 0 || items.length < 2) return null;
  return {
    position,
    count: items.length,
    previous: items[position - 1] ?? null,
    next: items[position + 1] ?? null,
  };
}

const sameItems = (a: AttachmentPreviewItem[] | undefined, b: AttachmentPreviewItem[]) =>
  !!a && JSON.stringify(a) === JSON.stringify(b);

export const useAttachmentPreviewStore = create<AttachmentPreviewState>((set, get) => ({
  sets: {},
  viewing: null,
  remember: (panelGroupId, messageKey, items) => {
    const key = attachmentSetKey(panelGroupId, messageKey);
    if (sameItems(get().sets[key], items)) return;
    set((state) => ({ sets: { ...state.sets, [key]: items } }));
  },
  view: (panelGroupId, messageKey, items, index) =>
    set((state) => ({
      sets: { ...state.sets, [attachmentSetKey(panelGroupId, messageKey)]: items },
      viewing: { panelGroupId, messageKey, index, stepped: false },
    })),
  step: (delta) => {
    const { viewing, sets } = get();
    if (!viewing) return;
    const items = sets[attachmentSetKey(viewing.panelGroupId, viewing.messageKey)];
    const sequence = attachmentSequence(items, viewing.index);
    const target = sequence ? (delta < 0 ? sequence.previous : sequence.next) : null;
    if (!target) return;
    set({ viewing: { ...viewing, index: target.resource.index, stepped: true } });
  },
  close: () => {
    if (!get().viewing) return;
    set({ viewing: null });
    forgetUnshown();
  },
}));

/** Drops the attachments of messages that neither the preview nor any panel tab shows. */
function forgetUnshown() {
  const { sets, viewing } = useAttachmentPreviewStore.getState();
  const shown = new Set<string>();
  if (viewing) shown.add(attachmentSetKey(viewing.panelGroupId, viewing.messageKey));
  for (const [groupId, panel] of Object.entries(useResourcePanelStore.getState().groups)) {
    for (const tab of panel.tabs) {
      if (tab.kind === "attachment") shown.add(attachmentSetKey(groupId, tab.messageKey));
    }
  }
  const unshown = Object.keys(sets).filter((key) => !shown.has(key));
  if (!unshown.length) return;
  const kept = { ...sets };
  for (const key of unshown) delete kept[key];
  useAttachmentPreviewStore.setState({ sets: kept });
}

useResourcePanelStore.subscribe((state, previous) => {
  if (state.groups !== previous.groups) forgetUnshown();
});

/** Whether the preview or a panel tab shows an attachment of this message. */
export function useAttachmentSetShown(panelGroupId: string, messageKey: string): boolean {
  const inPanel = useResourcePanelStore(
    (state) =>
      state.groups[panelGroupId]?.tabs.some(
        (tab) => tab.kind === "attachment" && tab.messageKey === messageKey,
      ) ?? false,
  );
  const inPreview = useAttachmentPreviewStore(
    (state) =>
      state.viewing?.panelGroupId === panelGroupId && state.viewing.messageKey === messageKey,
  );
  return inPanel || inPreview;
}
