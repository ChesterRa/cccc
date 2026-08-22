import type { ChatMessageData, MessageMode } from "../types";

function recipientList(value: unknown): string[] {
  if (!Array.isArray(value)) return [];
  return value.map((token) => String(token || "").trim()).filter(Boolean);
}

export function projectCrossGroupRecipients(
  data: Pick<ChatMessageData, "dst_to" | "to"> | null | undefined,
): string[] {
  const destinationRecipients = recipientList(data?.dst_to);
  if (destinationRecipients.length > 0) return destinationRecipients;

  const canonicalRecipients = recipientList(data?.to);
  return canonicalRecipients.length > 0 ? canonicalRecipients : ["@foreman"];
}

function isMessageMode(value: unknown): value is MessageMode {
  return value === "send" || value === "request_reply" || value === "mail";
}

/**
 * Return the mode the user chose for the visible destination. Cross-group
 * source rows are local Send audit records, so their destination mode lives in
 * dst_message_mode rather than message_mode.
 */
export function projectMessageMode(
  data:
    | Pick<ChatMessageData, "dst_group_id" | "dst_message_mode" | "message_mode">
    | null
    | undefined,
): MessageMode | undefined {
  const hasDestination = String(data?.dst_group_id || "").trim().length > 0;
  if (hasDestination && isMessageMode(data?.dst_message_mode)) return data.dst_message_mode;
  return isMessageMode(data?.message_mode) ? data.message_mode : undefined;
}
