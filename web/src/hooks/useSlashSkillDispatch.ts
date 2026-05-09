import { useCallback } from "react";

import * as api from "../services/api";
import type { ChatFilter } from "../stores/useUIStore";
import type { LedgerEvent } from "../types";
import {
  formatSendMessageError,
  getGroupSendBlockedMessage,
  type ChatTFunction,
  type GroupSendBlockedReason,
} from "../utils/chatSend";

export function useSlashSkillDispatch(args: {
  selectedGroupId: string;
  toTokens: string[];
  priority: "normal" | "attention" | string;
  replyRequired: boolean;
  groupSendBlockedReason: GroupSendBlockedReason | null;
  clearDraft: (groupId: string) => void;
  setChatUnreadCount: (groupId: string, count: number) => void;
  setChatFilter: (groupId: string, filter: ChatFilter) => void;
  setChatMobileSurface: (groupId: string, surface: "messages" | "presentation") => void;
  enqueueOutbox: (groupId: string, localId: string, event: LedgerEvent) => void;
  removeOutbox: (groupId: string, localId: string) => void;
  showError: (message: string) => void;
  onMessageSent?: () => void;
  t: ChatTFunction;
}) {
  const {
    selectedGroupId,
    toTokens,
    priority,
    replyRequired,
    groupSendBlockedReason,
    clearDraft,
    setChatUnreadCount,
    setChatFilter,
    setChatMobileSurface,
    enqueueOutbox,
    removeOutbox,
    showError,
    onMessageSent,
    t,
  } = args;

  return useCallback(async (text: string): Promise<boolean> => {
    const message = String(text || "").trim();
    if (!selectedGroupId || !message) return false;
    if (groupSendBlockedReason) {
      showError(getGroupSendBlockedMessage(groupSendBlockedReason, t));
      return false;
    }

    const localId = `local_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
    const prio = replyRequired ? "attention" : (priority || "normal");
    const optimisticEvent: LedgerEvent = {
      id: localId,
      kind: "chat.message",
      ts: new Date().toISOString(),
      by: "user",
      group_id: selectedGroupId,
      data: {
        text: message,
        to: toTokens,
        priority: prio,
        reply_required: replyRequired,
        client_id: localId,
        refs: [],
        format: "plain",
        attachments: [],
        _optimistic: true,
      } as LedgerEvent["data"],
    };
    enqueueOutbox(selectedGroupId, localId, optimisticEvent);

    const resp = await api.sendMessage(
      selectedGroupId,
      message,
      toTokens,
      undefined,
      prio as "normal" | "attention",
      replyRequired,
      localId,
      [],
    );
    if (!resp.ok) {
      removeOutbox(selectedGroupId, localId);
      showError(formatSendMessageError({
        code: resp.error.code,
        message: resp.error.message,
        groupSendBlockedReason,
        t,
      }));
      return false;
    }

    clearDraft(selectedGroupId);
    setChatUnreadCount(selectedGroupId, 0);
    setChatFilter(selectedGroupId, "all");
    setChatMobileSurface(selectedGroupId, "messages");
    onMessageSent?.();
    return true;
  }, [
    clearDraft,
    enqueueOutbox,
    groupSendBlockedReason,
    onMessageSent,
    priority,
    removeOutbox,
    replyRequired,
    selectedGroupId,
    setChatFilter,
    setChatMobileSurface,
    setChatUnreadCount,
    showError,
    t,
    toTokens,
  ]);
}
