type GroupBridgeMessageData = {
  source_platform?: unknown;
  src_group_id?: unknown;
} | null | undefined;

export function isGroupBridgeInboundMessage(by: unknown, data: GroupBridgeMessageData): boolean {
  const sender = String(by || "").trim();
  if (sender.startsWith("group_bridge:")) return true;
  const sourcePlatform = typeof data?.source_platform === "string" ? String(data.source_platform || "").trim() : "";
  const sourceGroupId = typeof data?.src_group_id === "string" ? String(data.src_group_id || "").trim() : "";
  return sourcePlatform === "group_bridge_session" && Boolean(sourceGroupId);
}
