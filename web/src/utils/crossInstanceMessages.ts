import type { ChatMessageData, ConnectMessageRouting, LedgerEvent } from "../types";

type CrossInstanceMessageData =
  | {
      source_platform?: unknown;
      src_group_id?: unknown;
      src_instance_id?: unknown;
      dst_instance_id?: unknown;
    }
  | null
  | undefined;

export function isCrossInstanceInboundMessage(
  by: unknown,
  data: CrossInstanceMessageData,
): boolean {
  const sender = String(by || "").trim();
  if (sender.startsWith("group_bridge:") || sender.startsWith("connect:")) return true;
  const sourcePlatform =
    typeof data?.source_platform === "string" ? String(data.source_platform || "").trim() : "";
  const sourceGroupId =
    typeof data?.src_group_id === "string" ? String(data.src_group_id || "").trim() : "";
  return sourcePlatform === "group_bridge_session" && Boolean(sourceGroupId);
}

export function isConnectMessage(data: CrossInstanceMessageData): boolean {
  return Boolean(data?.src_instance_id || data?.dst_instance_id);
}

export function connectGroupLabel(groupId: string, title?: string, instanceName?: string): string {
  const group = title?.trim() || groupId;
  return instanceName?.trim() ? `${instanceName.trim()} / ${group}` : group;
}

export function replyObligationActor(source: LedgerEvent, reply: LedgerEvent): string | null {
  const originalData = source.data as ChatMessageData;
  const replyData = reply.data as ChatMessageData;
  if (replyData?.reply_to !== source.id) return null;
  if (!originalData?.dst_instance_id) {
    return isCrossInstanceInboundMessage(reply.by, replyData) ? null : reply.by || null;
  }
  const original = originalData.connect_message;
  const response = replyData.connect_message;
  const sender = response?.sender;
  if (
    !original ||
    !response ||
    !sender?.id ||
    !sender.generation ||
    originalData.dst_instance_id !== original.target?.instance_id ||
    originalData.dst_group_id !== original.target?.group_id ||
    reply.by !== `connect:${response.source?.instance_id}` ||
    !sameConnectGroup(original.target, response.source) ||
    !sameConnectGroup(original.source, response.target) ||
    response.reply_to?.event_id !== source.id ||
    !original.delivery_id ||
    response.reply_to?.delivery_id !== original.delivery_id ||
    !original.recipients?.some(
      (actor) => actor.id === sender.id && actor.generation === sender.generation,
    )
  )
    return null;
  return sender.id;
}

function sameConnectGroup(
  left: ConnectMessageRouting["source"] | undefined,
  right: ConnectMessageRouting["source"] | undefined,
): boolean {
  return Boolean(
    left?.instance_id &&
    left.device_id &&
    left.group_id &&
    left.instance_id === right?.instance_id &&
    left.device_id === right.device_id &&
    left.group_id === right.group_id,
  );
}
