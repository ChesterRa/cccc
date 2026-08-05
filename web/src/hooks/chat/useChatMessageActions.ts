import { useCallback } from "react";
import type { TFunction } from "i18next";
import { useGroupStore, useUIStore } from "../../stores";
import {
  CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
  type ChatScrollSnapshot,
} from "../../stores/useUIStore";
import type { Actor, GroupMeta, LedgerEvent } from "../../types";
import { buildReplyComposerState } from "../../utils/chatReply";
import { copyTextToClipboard } from "../../utils/copy";
import { appendSenderPerspective, getMessageInsight } from "../../utils/messagePerspective";
import { canOpenSourceMessageLocally } from "./chatTabBasics";

export function useChatMessageActions(input: {
  selectedGroupId: string;
  actors: Actor[];
  groups: GroupMeta[];
  groupSettings: Parameters<typeof buildReplyComposerState>[3];
  composerRef?: React.RefObject<HTMLTextAreaElement | null>;
  setChatAtBottom: (value: boolean) => void;
  hasForeman: boolean;
  inChatWindow: boolean;
  t: TFunction;
  showError: (message: string) => void;
  showNotice: (notice: { message: string }) => void;
  setDestGroupId: (groupId: string) => void;
  setReplyToText: (value: string) => void;
  setReplyTarget: (
    target: ReturnType<typeof buildReplyComposerState> extends infer R
      ? R extends { replyTarget: infer T }
        ? T | null
        : never
      : never,
  ) => void;
  setRecipientsModal: (eventId: string | null) => void;
  setRelayModal: (eventId: string | null, groupId?: string, event?: LedgerEvent) => void;
  openChatWindow: (groupId: string, eventId: string) => Promise<unknown>;
  closeChatWindow: (groupId?: string) => void;
  setShowScrollButton: (groupId: string, value: boolean) => void;
  setChatUnreadCount: (groupId: string, value: number) => void;
  setChatScrollSnapshot: (groupId: string, snapshot: ChatScrollSnapshot) => void;
  setNewActorRole: (role: "foreman" | "peer") => void;
  openModal: (name: "addActor") => void;
  loadMoreHistory: (groupId: string) => Promise<unknown>;
}) {
  const { inChatWindow, selectedGroupId, setChatScrollSnapshot } = input;
  const copyMessageLink = useCallback(
    async (eventId: string) => {
      const eid = String(eventId || "").trim();
      if (!eid || !input.selectedGroupId) return;
      const url = new URL(window.location.origin + window.location.pathname);
      url.searchParams.set("group", input.selectedGroupId);
      url.searchParams.set("event", eid);
      url.searchParams.set("tab", "chat");
      if (await copyTextToClipboard(url.toString())) input.showNotice({ message: "Link copied" });
      else input.showError("Failed to copy link");
    },
    [input],
  );

  const copyMessageText = useCallback(
    async (event: LedgerEvent) => {
      if (event.kind !== "chat.message") return;
      const text = String(event.data && "text" in event.data ? event.data.text || "" : "");
      const copyText = appendSenderPerspective(
        text,
        getMessageInsight(event.data),
        input.t("chat:senderPerspective", { defaultValue: "Sender perspective" }),
      );
      if (!copyText) return;
      if (await copyTextToClipboard(copyText)) {
        input.showNotice({
          message: input.t("chat:contentCopied", { defaultValue: "Content copied" }),
        });
      } else input.showError(input.t("common:copyFailed", { defaultValue: "Copy failed" }));
    },
    [input],
  );

  const startReply = useCallback(
    (event: LedgerEvent) => {
      const state = buildReplyComposerState(
        event,
        input.selectedGroupId,
        input.actors,
        input.groupSettings,
      );
      if (!state) {
        input.showError(
          input.t("replyTargetUnavailable", {
            defaultValue: "This message is not ready for replies yet.",
          }),
        );
        return;
      }
      if (state.destGroupId) input.setDestGroupId(state.destGroupId);
      input.setReplyToText(state.toText);
      input.setReplyTarget(state.replyTarget);
      requestAnimationFrame(() => input.composerRef?.current?.focus());
    },
    [input],
  );

  const openSourceMessage = useCallback(
    (srcGroupId: string, srcEventId: string) => {
      const groupId = String(srcGroupId || "").trim();
      const eventId = String(srcEventId || "").trim();
      if (!groupId || !eventId) return;
      if (!canOpenSourceMessageLocally(input.groups, groupId)) {
        input.showError(
          input.t("sourceMessageNotLocal", {
            groupId,
            defaultValue:
              "Original message is in a remote group that is not open locally: {{groupId}}",
          }),
        );
        return;
      }
      const url = new URL(window.location.href);
      url.searchParams.set("group", groupId);
      url.searchParams.set("event", eventId);
      url.searchParams.set("tab", "chat");
      window.history.replaceState({}, "", `${url.pathname}?${url.searchParams.toString()}`);
      if (input.selectedGroupId === groupId) {
        useUIStore.getState().setActiveTab("chat");
        void input.openChatWindow(groupId, eventId);
      } else useGroupStore.getState().setSelectedGroupId(groupId);
    },
    [input],
  );

  const exitChatWindow = useCallback(() => {
    input.closeChatWindow(input.selectedGroupId);
    const url = new URL(window.location.href);
    url.searchParams.delete("event");
    url.searchParams.delete("tab");
    window.history.replaceState({}, "", url.pathname + (url.search ? url.search : ""));
  }, [input]);

  const handleScrollButtonClick = useCallback(() => {
    input.setChatAtBottom(true);
    if (!input.selectedGroupId) return;
    input.setShowScrollButton(input.selectedGroupId, false);
    input.setChatUnreadCount(input.selectedGroupId, 0);
    input.setChatScrollSnapshot(input.selectedGroupId, {
      coordinateVersion: CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
      mode: "follow",
      anchorId: "",
      offsetPx: 0,
      updatedAt: Date.now(),
    });
  }, [input]);

  const handleScrollChange = useCallback(
    (isAtBottom: boolean) => {
      input.setChatAtBottom(isAtBottom);
      if (!input.selectedGroupId) return;
      input.setShowScrollButton(input.selectedGroupId, !isAtBottom);
      if (isAtBottom) input.setChatUnreadCount(input.selectedGroupId, 0);
    },
    [input],
  );

  const handleScrollSnapshot = useCallback(
    (snapshot: ChatScrollSnapshot, overrideGroupId?: string) => {
      if (inChatWindow && !overrideGroupId) return;
      const groupId = String(overrideGroupId || selectedGroupId || "").trim();
      if (groupId) setChatScrollSnapshot(groupId, snapshot);
    },
    [inChatWindow, selectedGroupId, setChatScrollSnapshot],
  );

  return {
    copyMessageLink,
    copyMessageText,
    startReply,
    cancelReply: () => input.setReplyTarget(null),
    showRecipients: (eventId: string) => input.setRecipientsModal(eventId),
    relayMessage: (event: LedgerEvent) =>
      input.setRelayModal(event.id ?? null, input.selectedGroupId, event),
    openSourceMessage,
    exitChatWindow,
    handleScrollButtonClick,
    handleScrollChange,
    handleScrollSnapshot,
    addAgent: () => {
      input.setNewActorRole(input.hasForeman ? "peer" : "foreman");
      input.openModal("addActor");
    },
    loadCurrentGroupHistory: () =>
      input.selectedGroupId ? input.loadMoreHistory(input.selectedGroupId) : Promise.resolve(),
  };
}
