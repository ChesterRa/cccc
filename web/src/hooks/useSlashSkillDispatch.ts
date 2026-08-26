import { useCallback } from "react";

import * as api from "../services/api";
import type { ChatFilter } from "../stores/useUIStore";
import type { LedgerEvent, ReplyTarget } from "../types";
import { formatSendMessageError, type ChatTFunction } from "../utils/chatSend";
import type { SlashDispatchMessageOptions } from "./useSlashCommands";

export async function sendSlashSkillMessageRequest(args: {
  selectedGroupId: string;
  message: string;
  command?: string;
  capabilityId?: string;
  toTokens: string[];
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
        args.localId,
        [],
        args.replyTarget.text,
      );
    }
    return api.sendMessage(
      args.selectedGroupId,
      args.message,
      args.toTokens,
      undefined,
      "send",
      args.localId,
      [],
    );
  }
  return api.dispatchSlashSkill(args.selectedGroupId, {
    taskText: args.message,
    command,
    capabilityId,
    to: args.toTokens,
    clientId: args.localId,
    replyTo: args.replyTarget?.eventId || "",
    quoteText: args.replyTarget?.text || "",
  });
}

export function useSlashSkillDispatch(args: {
  selectedGroupId: string;
  toTokens: string[];
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

  return useCallback(
    async (text: string, options?: SlashDispatchMessageOptions): Promise<boolean> => {
      const message = String(text || "").trim();
      if (!selectedGroupId || !message) return false;
      const command = String(options?.command || "").trim();
      const capabilityId = String(options?.capabilityId || "").trim();
      const replyTarget: ReplyTarget = options?.replyTarget || null;
      const localId = `local_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
      const resp = await sendSlashSkillMessageRequest({
        selectedGroupId,
        message,
        command,
        capabilityId,
        toTokens,
        localId,
        replyTarget,
      });
      if (!resp.ok) {
        showError(
          formatSendMessageError({ code: resp.error.code, message: resp.error.message, t }),
        );
        return false;
      }

      clearDraft(selectedGroupId);
      setChatUnreadCount(selectedGroupId, 0);
      setChatFilter(selectedGroupId, "all");
      setChatMobileSurface(selectedGroupId, "messages");
      onMessageSent?.();
      return true;
    },
    [
      clearDraft,
      onMessageSent,
      selectedGroupId,
      setChatFilter,
      setChatMobileSurface,
      setChatUnreadCount,
      showError,
      t,
      toTokens,
    ],
  );
}
