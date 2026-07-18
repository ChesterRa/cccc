export function getMessageInsight(data: unknown): string {
  if (!data || typeof data !== "object") return "";
  const value = (data as { insight?: unknown }).insight;
  return typeof value === "string" ? value.trim() : "";
}

export function appendSenderPerspective(
  text: string,
  insight: string,
  label = "Sender perspective",
): string {
  const body = String(text || "").trim();
  const perspective = String(insight || "").trim();
  if (!perspective) return body;
  const projection = `${String(label || "Sender perspective").trim()}:\n${perspective}`;
  return body ? `${body}\n\n${projection}` : projection;
}

export function appendQuotedOriginalPerspective(
  text: string,
  insight: string,
  sourceActor = "sender",
): string {
  const body = String(text || "").trim();
  const perspective = String(insight || "").trim();
  if (!perspective) return body;
  const actor = String(sourceActor || "sender").trim() || "sender";
  const projection = `Original sender perspective (quoted from ${actor}):\n${perspective}`;
  return body ? `${body}\n\n${projection}` : projection;
}
