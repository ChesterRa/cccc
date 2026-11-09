import { requestWorkspaceNavigation } from "../../stores/workspaceNavigation";
import { useCallback, useLayoutEffect, useRef } from "react";
import type { TFunction } from "i18next";
import { useGroupStore, useUIStore } from "../../stores";
import {
  CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION,
  groupMessagesVisible,
  type ChatScrollSnapshot,
} from "../../stores/useUIStore";
import type { Actor, LedgerEvent } from "../../types";
import { buildReplyComposerState } from "../../utils/chatReply";
import { copyTextToClipboard } from "../../utils/copy";
import { canOpenSourceMessageLocally } from "./chatTabBasics";
import { isLiveChatBottom } from "./chatScrollFocus";

/** Take the opened message out of the address, as leaving its view does. */
function clearOpenedMessageFromUrl() {
  const url = new URL(window.location.href);
  url.searchParams.delete("event");
  url.searchParams.delete("tab");
  window.history.replaceState({}, "", url.pathname + (url.search ? url.search : ""));
}

export function useChatMessageActions(input: {
  selectedGroupId: string;
  actors: Actor[];
  groupSettings: Parameters<typeof buildReplyComposerState>[3];
  /** The selected Group's generation, for messages Chet wrote in it. */
  groupGeneration: string;
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
  loadMoreHistory: (groupId: string) => Promise<void | false>;
}) {
  // Message rows are memoized and receive these click actions. Each keeps one identity and
  // takes the input of the last committed render when it is called, once, for its whole run:
  // a row neither re-renders because its caller built a new input object nor keeps an earlier
  // render's actors or settings. Scroll and view handlers run from effects and keep theirs.
  const actionInput = useRef(input);
  useLayoutEffect(() => {
    actionInput.current = input;
  });
  const { inChatWindow, selectedGroupId, setChatScrollSnapshot } = input;
  const copyMessageLink = useCallback(async (eventId: string) => {
    const input = actionInput.current;
    const eid = String(eventId || "").trim();
    if (!eid || !input.selectedGroupId) return;
    const url = new URL(window.location.origin + window.location.pathname);
    url.searchParams.set("group", input.selectedGroupId);
    url.searchParams.set("event", eid);
    url.searchParams.set("tab", "chat");
    if (await copyTextToClipboard(url.toString()))
      input.showNotice({ message: input.t("chat:linkCopied") });
    else input.showError(input.t("common:copyFailed", { defaultValue: "Copy failed" }));
  }, []);

  const startReply = useCallback((event: LedgerEvent) => {
    const input = actionInput.current;
    const state = buildReplyComposerState(
      event,
      input.selectedGroupId,
      input.actors,
      input.groupSettings,
      input.groupGeneration,
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
  }, []);

  const openSourceMessage = useCallback((srcGroupId: string, srcEventId: string) => {
    const input = actionInput.current;
    const groupId = String(srcGroupId || "").trim();
    const eventId = String(srcEventId || "").trim();
    if (!groupId || !eventId) return;
    if (!canOpenSourceMessageLocally(useGroupStore.getState().groups, groupId)) {
      input.showError(
        input.t("sourceMessageNotLocal", {
          groupId,
          defaultValue:
            "Original message is in a remote group that is not open locally: {{groupId}}",
        }),
      );
      return;
    }
    const navigate = () => {
      const url = new URL(window.location.href);
      url.searchParams.set("group", groupId);
      url.searchParams.set("event", eventId);
      url.searchParams.set("tab", "chat");
      window.history.replaceState({}, "", `${url.pathname}?${url.searchParams.toString()}`);
      const store = useGroupStore.getState();
      if (store.selectedGroupId !== groupId) store.setSelectedGroupId(groupId);
      useUIStore.getState().setActiveTab("chat");
      void input.openChatWindow(groupId, eventId).then((opened) => {
        // Only for the message still awaited, not after the reader moved to another Group
        // or message: the view stayed where it was, so say so and clear it from the address.
        if (
          opened !== false ||
          useGroupStore.getState().selectedGroupId !== groupId ||
          new URL(window.location.href).searchParams.get("event") !== eventId
        )
          return;
        clearOpenedMessageFromUrl();
        input.showError(input.t("chat:messageWindowFailed"));
      });
    };
    if (useGroupStore.getState().selectedGroupId === groupId) navigate();
    else requestWorkspaceNavigation(navigate);
  }, []);

  const exitChatWindow = useCallback(() => {
    input.closeChatWindow(input.selectedGroupId);
    clearOpenedMessageFromUrl();
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
      if (!groupMessagesVisible(input.selectedGroupId, useUIStore.getState())) return;
      // A context window can end before the live timeline. Its bottom is not
      // evidence that the reader has seen the latest live messages.
      const atLiveBottom = isLiveChatBottom(isAtBottom, input.inChatWindow);
      input.setChatAtBottom(atLiveBottom);
      if (!input.selectedGroupId) return;
      input.setShowScrollButton(input.selectedGroupId, !atLiveBottom);
      if (atLiveBottom) input.setChatUnreadCount(input.selectedGroupId, 0);
    },
    [input],
  );

  const showRecipients = useCallback(
    (eventId: string) => actionInput.current.setRecipientsModal(eventId),
    [],
  );
  const relayMessage = useCallback((event: LedgerEvent) => {
    const input = actionInput.current;
    input.setRelayModal(event.id ?? null, input.selectedGroupId, event);
  }, []);

  const handleScrollSnapshot = useCallback(
    (snapshot: ChatScrollSnapshot, overrideGroupId?: string) => {
      if (inChatWindow && !overrideGroupId) return;
      const groupId = String(overrideGroupId || selectedGroupId || "").trim();
      if (groupId && groupMessagesVisible(groupId, useUIStore.getState()))
        setChatScrollSnapshot(groupId, snapshot);
    },
    [inChatWindow, selectedGroupId, setChatScrollSnapshot],
  );

  return {
    copyMessageLink,
    startReply,
    cancelReply: () => input.setReplyTarget(null),
    showRecipients,
    relayMessage,
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
      input.selectedGroupId
        ? input.loadMoreHistory(input.selectedGroupId)
        : Promise.resolve<false>(false),
  };
}
