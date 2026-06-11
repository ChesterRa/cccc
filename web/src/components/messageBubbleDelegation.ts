// Classify a message by its delegation contract marker so the destination chip
// can say "Relayed to ..." for an agent relay request instead of "Sent to ...",
// which reads like a user direct cross-send (T412). Display-only.

export function isDelegationRequestText(text: string): boolean {
  return String(text || "").includes("[cccc-delegation:v1]");
}

export function isDelegationResultText(text: string): boolean {
  return String(text || "").includes("[cccc-delegation-result:v1]");
}

// i18n key for the destination ("→") chip given the message text.
export function destinationChipKey(text: string): "relayedTo" | "sentTo" {
  return isDelegationRequestText(text) ? "relayedTo" : "sentTo";
}

export function isDelegationSourceOutbound({
  rawText,
  srcGroupId,
  dstGroupId,
}: {
  rawText: string;
  srcGroupId?: string;
  dstGroupId?: string;
}): boolean {
  return isDelegationRequestText(rawText) && !String(srcGroupId || "").trim() && !!String(dstGroupId || "").trim();
}

export function isDelegationSourceOutboundEvent(data: unknown): boolean {
  if (!data || typeof data !== "object") return false;
  const row = data as { text?: unknown; src_group_id?: unknown; dst_group_id?: unknown };
  return isDelegationSourceOutbound({
    rawText: typeof row.text === "string" ? row.text : "",
    srcGroupId: typeof row.src_group_id === "string" ? row.src_group_id : "",
    dstGroupId: typeof row.dst_group_id === "string" ? row.dst_group_id : "",
  });
}

const PROTOCOL_COMMENT_OPEN = "<!-- cccc-delegation-protocol";
const PROTOCOL_COMMENT_CLOSE = "-->";

// Default display text: drop the machine-protocol comment so the bubble shows
// the natural chat body, not the protocol log. Non-delegation messages pass
// through unchanged.
export function getDelegationDisplayText(rawText: string): string {
  const raw = String(rawText || "");
  const start = raw.indexOf(PROTOCOL_COMMENT_OPEN);
  if (start < 0) return raw;
  const end = raw.indexOf(PROTOCOL_COMMENT_CLOSE, start + PROTOCOL_COMMENT_OPEN.length);
  const before = raw.slice(0, start);
  const after = end >= 0 ? raw.slice(end + PROTOCOL_COMMENT_CLOSE.length) : "";
  return `${before}${after}`.trim();
}

// Extract the full protocol block inside the comment (without the markers).
export function getDelegationProtocolText(rawText: string): string {
  const raw = String(rawText || "");
  const start = raw.indexOf(PROTOCOL_COMMENT_OPEN);
  if (start < 0) return "";
  const end = raw.indexOf(PROTOCOL_COMMENT_CLOSE, start + PROTOCOL_COMMENT_OPEN.length);
  if (end < 0) return "";
  return raw.slice(start + PROTOCOL_COMMENT_OPEN.length, end).trim();
}

export function getDelegationSourceOutboundStatus(rawText: string): string {
  if (!isDelegationRequestText(rawText)) return "";
  return "已联系目标组，等待对方回复。";
}
