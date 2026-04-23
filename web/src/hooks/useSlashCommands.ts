import { useCallback, useEffect, type RefObject, useState } from "react";

import * as api from "../services/api";
import {
  buildSlashCommandsFromCapabilityState,
  buildSlashCommandToolArgumentsForItem,
  parseSlashCommandInput,
  type SlashCommandItem,
} from "../utils/slashCommands";

type TranslateFn = (key: string, options?: Record<string, unknown>) => string;

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

export function useSlashCommands(args: {
  selectedGroupId: string;
  fileInputRef?: RefObject<HTMLInputElement | null>;
  clearComposer: () => void;
  showError: (message: string) => void;
  showNotice: (payload: { message: string }) => void;
  onExecuted?: () => void;
  t: TranslateFn;
}) {
  const {
    selectedGroupId,
    fileInputRef,
    clearComposer,
    showError,
    showNotice,
    onExecuted,
    t,
  } = args;
  const [slashCommands, setSlashCommands] = useState<SlashCommandItem[]>([]);

  useEffect(() => {
    let cancelled = false;
    const gid = String(selectedGroupId || "").trim();
    if (!gid) {
      setSlashCommands([]);
      return;
    }
    void api.fetchGroupCapabilityState(gid, "user")
      .then((resp) => {
        if (cancelled) return;
        if (!resp.ok) {
          setSlashCommands([]);
          return;
        }
        setSlashCommands(buildSlashCommandsFromCapabilityState(resp.result));
      })
      .catch(() => {
        if (!cancelled) setSlashCommands([]);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedGroupId]);

  const tryExecuteSlashCommand = useCallback(async (opts: {
    text: string;
    composerFilesCount: number;
    hasReplyTarget: boolean;
    hasQuotedPresentationRef: boolean;
    sendGroupId: string;
  }): Promise<boolean> => {
    const gid = String(selectedGroupId || "").trim();
    const slashCommand = parseSlashCommandInput(opts.text, slashCommands);
    if (!slashCommand || !gid) return false;

    if (opts.composerFilesCount > 0) {
      showError("Slash command does not support attachments.");
      return true;
    }
    if (opts.hasReplyTarget) {
      showError("Slash command does not support replies.");
      return true;
    }
    if (opts.hasQuotedPresentationRef) {
      showError("Slash command does not support quoted presentation views.");
      return true;
    }
    if (String(opts.sendGroupId || "").trim() && String(opts.sendGroupId || "").trim() !== gid) {
      showError("Slash command does not support cross-group send.");
      return true;
    }

    try {
      const resp = await api.useGroupCapability(gid, {
        actorId: "user",
        capabilityId: slashCommand.item.capabilityId,
        toolName: slashCommand.item.realToolName || slashCommand.item.toolName,
        toolArguments: buildSlashCommandToolArgumentsForItem(slashCommand.item, slashCommand.argsText),
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
      clearComposer();
      if (fileInputRef?.current) fileInputRef.current.value = "";
      const summary = summarizeCapabilityUseResult(resp.result);
      showNotice({ message: summary || `Executed ${slashCommand.item.command}` });
      onExecuted?.();
      return true;
    } catch (error) {
      showError(error instanceof Error ? error.message : "slash command failed");
      return true;
    }
  }, [clearComposer, fileInputRef, onExecuted, selectedGroupId, showError, showNotice, slashCommands, t]);

  return {
    slashCommands,
    tryExecuteSlashCommand,
  };
}
