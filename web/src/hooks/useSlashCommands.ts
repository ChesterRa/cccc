import { useCallback, useRef } from "react";

import * as api from "../services/api";
import type { ReplyTarget } from "../types";
import {
  buildSlashCommandToolArgumentsForItem,
  parseSlashCommandInput,
  resolveCapsuleSkillSlashCommand,
  resolveSlashCommandGuard,
} from "../utils/slashCommands";
import { useSlashCommandState } from "./useSlashCommandState";

type TranslateFn = (key: string, options?: Record<string, unknown>) => string;

export type OptimisticSlashDispatchResult = {
  ok: boolean;
  dispatchText: string;
};

export type SlashDispatchMessageOptions = {
  replyTarget?: ReplyTarget;
};

function summarizeCapabilityUseResult(result: unknown): string {
  const record = result && typeof result === "object" ? result as Record<string, unknown> : {};
  const nested = record.result && typeof record.result === "object" ? record.result as Record<string, unknown> : {};
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
  originalText: string;
  replyTarget?: ReplyTarget;
  dispatchMessage: (text: string, options?: SlashDispatchMessageOptions) => Promise<boolean>;
  clearComposer: () => void;
  restoreComposerText: (text: string) => void;
}): Promise<OptimisticSlashDispatchResult> {
  const dispatchText = String(args.dispatchText || "").trim();
  if (!dispatchText) return { ok: false, dispatchText: "" };
  args.clearComposer();
  const sent = await args.dispatchMessage(dispatchText, { replyTarget: args.replyTarget || null });
  if (!sent) {
    args.restoreComposerText(args.originalText);
  }
  return { ok: sent, dispatchText };
}

export function useSlashCommands(args: {
  selectedGroupId: string;
  clearComposer: () => void;
  restoreComposerText: (text: string) => void;
  showError: (message: string) => void;
  showNotice: (payload: { message: string }) => void;
  dispatchMessage?: (text: string, options?: SlashDispatchMessageOptions) => Promise<boolean>;
  onExecuted?: () => void;
  t: TranslateFn;
}) {
  const { selectedGroupId, clearComposer, restoreComposerText, showError, showNotice, dispatchMessage, onExecuted, t } = args;
  const slashInFlightRef = useRef(false);
  const { slashCommands, refreshSlashCommands } = useSlashCommandState(selectedGroupId);

  const tryExecuteSlashCommand = useCallback(async (opts: {
    text: string;
    composerFilesCount: number;
    hasReplyTarget: boolean;
    replyTarget?: ReplyTarget;
    replyRequired: boolean;
    hasQuotedPresentationRef: boolean;
    sendGroupId: string;
  }): Promise<boolean> => {
    const gid = String(selectedGroupId || "").trim();
    const slashCommand = parseSlashCommandInput(opts.text, slashCommands);
    if (!slashCommand || !gid) return false;
    if (slashInFlightRef.current) return true;

    const item = slashCommand.item;
    const guard = resolveSlashCommandGuard({ ...opts, sourceType: item.sourceType, selectedGroupId: gid }, {
      attachmentsUnsupported: t("slashCommandAttachmentUnsupported", { defaultValue: "Slash command does not support attachments." }),
      repliesUnsupported: t("slashCommandReplyUnsupported", { defaultValue: "Slash command does not support replying to a specific message yet." }),
      quotedPresentationUnsupported: t("slashCommandQuotedPresentationUnsupported", { defaultValue: "Slash command does not support quoted presentation views." }),
      crossGroupUnsupported: t("slashCommandCrossGroupUnsupported", { defaultValue: "Slash command does not support cross-group send." }),
    });
    if (!guard.ok) {
      showError(guard.message);
      return true;
    }

    slashInFlightRef.current = true;
    try {
      if (item.sourceType === "builtin_command") {
        const dispatchText = [item.command, slashCommand.argsText].filter(Boolean).join(" ").trim();
        if (!dispatchText || !dispatchMessage) return true;
        const sent = await dispatchSlashMessageOptimistically({
          dispatchText,
          originalText: opts.text,
          replyTarget: opts.replyTarget || null,
          dispatchMessage,
          clearComposer,
          restoreComposerText,
        });
        if (!sent.ok) return true;
        void refreshSlashCommands();
        onExecuted?.();
        return true;
      }

      if (item.sourceType === "capsule_skill") {
        const resolution = resolveCapsuleSkillSlashCommand(item, slashCommand.argsText, {
          missingArgs: (command) => t("slashCommandMissingArgs", {
            command,
            defaultValue: "Enter a task after {{command}}.",
          }),
        });
        if (resolution.kind === "dispatch" && dispatchMessage) {
          const sent = await dispatchSlashMessageOptimistically({
            dispatchText: resolution.dispatchText,
            originalText: opts.text,
            replyTarget: opts.replyTarget || null,
            dispatchMessage,
            clearComposer,
            restoreComposerText,
          });
          if (!sent.ok) return true;
          void refreshSlashCommands();
          onExecuted?.();
          return true;
        }
        showError(resolution.kind === "missing_args"
          ? resolution.message
          : t("sendFailed", { defaultValue: "Failed to send message." }));
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
        const code = String(resp.error.code || "").trim();
        const message = String(resp.error.message || "").trim();
        showError(message ? (code ? `${code}: ${message}` : message) : t("sendFailed", { defaultValue: "Failed to send message." }));
        return true;
      }
      const summary = summarizeCapabilityUseResult(resp.result);
      clearComposer();
      void refreshSlashCommands();
      showNotice({
        message: summary || `Executed ${item.command}`,
      });
      onExecuted?.();
      return true;
    } catch (error) {
      showError(error instanceof Error ? error.message : "slash command failed");
      return true;
    } finally {
      slashInFlightRef.current = false;
    }
  }, [clearComposer, dispatchMessage, onExecuted, refreshSlashCommands, restoreComposerText, selectedGroupId, showError, showNotice, slashCommands, t]);

  return { slashCommands, refreshSlashCommands, tryExecuteSlashCommand };
}
