import type { ChatMessageData } from "../types";

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
