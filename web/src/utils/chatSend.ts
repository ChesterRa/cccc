import type { Actor, LedgerEvent } from "../types";

export type ChatTFunction = (key: string, options?: Record<string, unknown>) => string;

export function shouldBlockLocalCrossGroupAttachments(input: {
  attachmentCount: number;
  targets: Array<{ isCrossGroup?: boolean; isRemote?: boolean }>;
}): boolean {
  if (input.attachmentCount <= 0) return false;
  return input.targets.some((target) => Boolean(target.isCrossGroup) && !target.isRemote);
}

export function supportsChatStreamingPlaceholder(
  actor: Pick<Actor, "runtime" | "runner" | "runner_effective">,
): boolean {
  const runtime = String(actor.runtime || "").trim();
  if (!runtime) return false;
  return runtime !== "custom";
}

export function isFormalChatMessageEvent(event: LedgerEvent): boolean {
  return String(event.kind || "").trim() === "chat.message" && !event._streaming;
}

export function formatSendMessageError(args: {
  code?: unknown;
  message?: unknown;
  t: ChatTFunction;
}): string {
  const code = String(args.code || "").trim();
  const message = String(args.message || "").trim();
  if (!code) return message || args.t("sendFailed", { defaultValue: "Failed to send message." });
  if (!message) return code;
  return `${code}: ${message}`;
}
