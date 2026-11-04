import { useCallback, useRef } from "react";

import * as api from "../services/api";
import { beginPendingPageWork } from "../stores/usePendingPageWork";
import type { ReplyTarget } from "../types";
import {
  captureSendComposerState,
  clearSendComposerState,
  describeSendFailure,
  restoreFailedSendComposerState,
} from "./chat/chatComposerState";
import {
  buildSlashCommandToolArgumentsForItem,
  parseSlashCommandInput,
  resolveCapsuleSkillSlashCommand,
  resolveSlashCommandGuard,
} from "../utils/slashCommands";
import { useSlashCommandState } from "./useSlashCommandState";

type TranslateFn = (key: string, options?: Record<string, unknown>) => string;

export type OptimisticSlashDispatchResult = { ok: boolean; dispatchText: string };

export type SlashDispatchMessageOptions = {
  replyTarget?: ReplyTarget;
  command?: string;
  capabilityId?: string;
  clientId?: string;
};

type SlashAttempt = { clientId: string; restore: () => void };

function summarizeCapabilityUseResult(result: unknown): string {
  const record = result && typeof result === "object" ? (result as Record<string, unknown>) : {};
  const nested =
    record.result && typeof record.result === "object"
      ? (record.result as Record<string, unknown>)
      : {};
  const content = Array.isArray(nested.content) ? nested.content : [];
  for (const item of content) {
    if (!item || typeof item !== "object") continue;
    const text = String((item as { text?: unknown }).text || "").trim();
    if (text) return text.slice(0, 240);
  }
  const candidates = [record.message, nested.message, record.real_tool_name, record.tool_name];
  for (const candidate of candidates) {
    const text = String(candidate || "").trim();
    if (text) return text.slice(0, 240);
  }
  return "";
}

export async function dispatchSlashMessageOptimistically(args: {
  dispatchText: string;
  command?: string;
  capabilityId?: string;
  replyTarget?: ReplyTarget;
  dispatchMessage: (text: string, options?: SlashDispatchMessageOptions) => Promise<boolean>;
  beginDispatch: (text: string, options: SlashDispatchMessageOptions) => SlashAttempt;
}): Promise<OptimisticSlashDispatchResult> {
  const dispatchText = String(args.dispatchText || "").trim();
  if (!dispatchText) return { ok: false, dispatchText: "" };
  const options = {
    replyTarget: args.replyTarget || null,
    command: String(args.command || "").trim() || undefined,
    capabilityId: String(args.capabilityId || "").trim() || undefined,
  };
  const attempt = args.beginDispatch(dispatchText, options);
  try {
    const sent = await args.dispatchMessage(dispatchText, {
      ...options,
      clientId: attempt.clientId,
    });
    if (!sent) attempt.restore();
    return { ok: sent, dispatchText };
  } catch (error) {
    attempt.restore();
    throw error;
  }
}

export function useSlashCommands(args: {
  selectedGroupId: string;
  beginDispatch: (text: string, options: SlashDispatchMessageOptions) => SlashAttempt;
  showError: (message: string) => void;
  showNotice: (payload: { message: string }) => void;
  dispatchMessage?: (text: string, options?: SlashDispatchMessageOptions) => Promise<boolean>;
  onExecuted?: () => void;
  t: TranslateFn;
}) {
  const { selectedGroupId, beginDispatch, showError, showNotice, dispatchMessage, onExecuted, t } =
    args;
  const slashInFlightRef = useRef(false);
  const { slashCommands, refreshSlashCommands } = useSlashCommandState(selectedGroupId);

  const tryExecuteSlashCommand = useCallback(
    async (opts: {
      text: string;
      composerFilesCount: number;
      hasReplyTarget: boolean;
      replyTarget?: ReplyTarget;
      hasQuotedPresentationRef: boolean;
      hasQuotedVoiceDocumentRef: boolean;
      sendGroupId: string;
    }): Promise<boolean> => {
      const gid = String(selectedGroupId || "").trim();
      const slashCommand = parseSlashCommandInput(opts.text, slashCommands);
      if (!slashCommand || !gid) return false;
      if (slashInFlightRef.current) return true;

      const item = slashCommand.item;
      const guard = resolveSlashCommandGuard(
        { ...opts, sourceType: item.sourceType, selectedGroupId: gid },
        {
          attachmentsUnsupported: t("slashCommandAttachmentUnsupported", {
            defaultValue: "Slash command does not support attachments.",
          }),
          repliesUnsupported: t("slashCommandReplyUnsupported", {
            defaultValue: "Slash command does not support replying to a specific message yet.",
          }),
          quotedPresentationUnsupported: t("slashCommandQuotedPresentationUnsupported", {
            defaultValue: "Slash command does not support quoted presentation views.",
          }),
          quotedVoiceDocumentUnsupported: t("slashCommandQuotedVoiceDocumentUnsupported", {
            defaultValue: "Slash command does not support quoted voice documents.",
          }),
          crossGroupUnsupported: t("slashCommandCrossGroupUnsupported", {
            defaultValue: "Slash command does not support cross-group send.",
          }),
        },
      );
      if (!guard.ok) {
        showError(guard.message);
        return true;
      }

      slashInFlightRef.current = true;
      // A dynamic tool has no server-side client ID. Preserve its input on a
      // failure, but do not promise an idempotent retry.
      const dynamicSnapshot =
        item.sourceType === "dynamic_tool"
          ? captureSendComposerState(
              gid,
              `local_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`,
              null,
            )
          : null;
      const finishPageWork = beginPendingPageWork();
      let dynamicAfterClear: ReturnType<typeof clearSendComposerState> | undefined;
      const preserveDynamicFailure = () => {
        if (dynamicSnapshot) restoreFailedSendComposerState(dynamicSnapshot, dynamicAfterClear);
      };
      try {
        dynamicAfterClear = dynamicSnapshot ? clearSendComposerState(gid) : undefined;
        if (item.sourceType === "builtin_command") {
          const dispatchText = [item.command, slashCommand.argsText]
            .filter(Boolean)
            .join(" ")
            .trim();
          if (!dispatchText || !dispatchMessage) return true;
          const sent = await dispatchSlashMessageOptimistically({
            dispatchText,
            replyTarget: opts.replyTarget || null,
            dispatchMessage,
            beginDispatch,
          });
          if (!sent.ok) return true;
          void refreshSlashCommands();
          onExecuted?.();
          return true;
        }

        if (item.sourceType === "capsule_skill") {
          const resolution = resolveCapsuleSkillSlashCommand(item, slashCommand.argsText);
          if (resolution.kind === "dispatch" && dispatchMessage) {
            const sent = await dispatchSlashMessageOptimistically({
              dispatchText: resolution.dispatchText,
              command: item.command,
              capabilityId: item.capabilityId,
              replyTarget: opts.replyTarget || null,
              dispatchMessage,
              beginDispatch,
            });
            if (!sent.ok) return true;
            void refreshSlashCommands();
            onExecuted?.();
            return true;
          }
          showError(t("sendFailed", { defaultValue: "Failed to send message." }));
          return true;
        }

        const resp = await api.useGroupCapability(gid, {
          actorId: "user",
          capabilityId: item.capabilityId,
          toolName: item.realToolName || item.toolName,
          toolArguments: buildSlashCommandToolArgumentsForItem(item, slashCommand.argsText),
          scope: "session",
          ttlSeconds: 3600,
          reason: "chat_slash_command",
        });
        if (!resp.ok) {
          preserveDynamicFailure();
          const code = String(resp.error.code || "").trim();
          const message = String(resp.error.message || "").trim();
          const errorMessage = message
            ? code
              ? `${code}: ${message}`
              : message
            : t("sendFailed", { defaultValue: "Failed to send message." });
          showError(describeSendFailure(gid, errorMessage));
          return true;
        }
        const summary = summarizeCapabilityUseResult(resp.result);
        void refreshSlashCommands();
        showNotice({ message: summary || `Executed ${item.command}` });
        onExecuted?.();
        return true;
      } catch (error) {
        preserveDynamicFailure();
        showError(
          describeSendFailure(gid, error instanceof Error ? error.message : "slash command failed"),
        );
        return true;
      } finally {
        slashInFlightRef.current = false;
        finishPageWork();
      }
    },
    [
      dispatchMessage,
      onExecuted,
      refreshSlashCommands,
      beginDispatch,
      selectedGroupId,
      showError,
      showNotice,
      slashCommands,
      t,
    ],
  );

  return { slashCommands, refreshSlashCommands, tryExecuteSlashCommand };
}
