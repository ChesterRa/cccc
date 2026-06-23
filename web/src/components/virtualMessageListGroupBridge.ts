import type { ChatMessageData, LedgerEvent } from "../types";

export function buildGroupBridgeDisplayNameMap(messages: LedgerEvent[]): Map<string, string> {
  const map = new Map<string, string>();
  for (const message of messages || []) {
    if (String(message.kind || "").trim() !== "chat.message") continue;
    const senderId = String(message.by || "").trim();
    if (!senderId.startsWith("group_bridge:")) continue;
    const data = message.data as ChatMessageData | undefined;
    const sourceName = String(data?.source_user_name || "").trim();
    if (!sourceName) continue;
    map.set(senderId, sourceName);
  }
  return map;
}
