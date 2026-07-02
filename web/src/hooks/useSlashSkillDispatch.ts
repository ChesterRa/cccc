import { useCallback } from "react";

import * as api from "../services/api";
import type { ChatFilter } from "../stores/useUIStore";
import type { LedgerEvent, ReplyTarget } from "../types";
import {
  formatSendMessageError,
  getGroupSendBlockedMessage,
  type ChatTFunction,
  type GroupSendBlockedReason,
} from "../utils/chatSend";
import type { SlashDispatchMessageOptions } from "./useSlashCommands";

export async function sendSlashSkillMessageRequest(args: {
  selectedGroupId: string;
  message: string;
  command?: string;
  capabilityId?: string;
  toTokens: string[];
  priority: "normal" | "attention";
  replyRequired: boolean;
  localId: string;
  replyTarget: ReplyTarget;
}) {
  const command = String(args.command || "").trim();
  const capabilityId = String(args.capabilityId || "").trim();
  if (!command || !capabilityId) {
    if (args.replyTarget) {
      return api.replyMessage(
        args.selectedGroupId,
        args.message,
        args.toTokens,
        args.replyTarget.eventId,
        undefined,
        args.priority,
        args.replyRequired,
        args.localId,
        [],
      );
    }
    return api.sendMessage(
      args.selectedGroupId,
      args.message,
      args.toTokens,
      undefined,
      args.priority,
      args.replyRequired,
      args.localId,
      [],
    );
  }
  return api.dispatchSlashSkill(
    args.selectedGroupId,
    {
      taskText: args.message,
      command,
      capabilityId,
      to: args.toTokens,
      priority: args.priority,
      replyRequired: args.replyRequired,
      clientId: args.localId,
      replyTo: args.replyTarget?.eventId || "",
      quoteText: args.replyTarget?.text || "",
    },
  );
}

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
  void enqueueOutbox;
  void removeOutbox;

  return useCallback(async (text: string, options?: SlashDispatchMessageOptions): Promise<boolean> => {
    const message = String(text || "").trim();
    if (!selectedGroupId || !message) return false;
    const command = String(options?.command || "").trim();
    const capabilityId = String(options?.capabilityId || "").trim();
    const replyTarget: ReplyTarget = options?.replyTarget || null;
    if (groupSendBlockedReason) {
      showError(getGroupSendBlockedMessage(groupSendBlockedReason, t));
      return false;
    }

    const localId = `local_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
    const prio = replyRequired ? "attention" : (priority || "normal");
    const resp = await sendSlashSkillMessageRequest({
      selectedGroupId,
      message,
      command,
      capabilityId,
      toTokens,
      priority: prio as "normal" | "attention",
      replyRequired,
      localId,
      replyTarget,
    });
    if (!resp.ok) {
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
    groupSendBlockedReason,
    onMessageSent,
    priority,
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
