import type { Actor, LedgerEvent } from "../types";

export type ChatTFunction = (key: string, options?: Record<string, unknown>) => string;
export type GroupSendBlockedReason = "paused" | "stopped";

export function supportsChatStreamingPlaceholder(actor: Pick<Actor, "runtime" | "runner" | "runner_effective">): boolean {
  const runtime = String(actor.runtime || "").trim();
  if (!runtime) return false;
  return runtime !== "custom";
}

export function isFormalChatMessageEvent(event: LedgerEvent): boolean {
  return String(event.kind || "").trim() === "chat.message" && !event._streaming;
}

export function getGroupSendBlockedReason({
  lifecycleState,
  runtimeRunning: _runtimeRunning,
  actorCount: _actorCount,
}: {
  lifecycleState?: unknown;
  runtimeRunning: boolean;
  actorCount: number;
}): GroupSendBlockedReason | null {
  const state = String(lifecycleState || "").trim().toLowerCase();
  if (state === "paused") return "paused";
  if (state === "stopped") return "stopped";
  return null;
}

export function getGroupSendBlockedMessage(reason: GroupSendBlockedReason, t: ChatTFunction): string {
  if (reason === "paused") {
    return t("sendBlockedGroupPaused", {
      defaultValue: "This group is paused. Resume the group before sending a message to agents.",
    });
  }
  return t("sendBlockedGroupStopped", {
    defaultValue: "This group is not running. Start the group before sending a message to agents.",
  });
}

export function formatSendMessageError(args: {
  code?: unknown;
  message?: unknown;
  groupSendBlockedReason?: GroupSendBlockedReason | null;
  t: ChatTFunction;
}): string {
  const code = String(args.code || "").trim();
  if (code === "no_enabled_recipients" && args.groupSendBlockedReason) {
    return getGroupSendBlockedMessage(args.groupSendBlockedReason, args.t);
  }
  const message = String(args.message || "").trim();
  if (!code) return message || args.t("sendFailed", { defaultValue: "Failed to send message." });
  if (!message) return code;
  return `${code}: ${message}`;
}
