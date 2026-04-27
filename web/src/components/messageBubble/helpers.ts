import type { ChatMessageData, LedgerEvent, StreamingActivity } from "../../types";

function isMarkdownTableSeparatorCell(cell: string): boolean {
  return /^:?-{3,}:?$/.test(String(cell || "").trim());
}

function containsMarkdownTable(text: string): boolean {
  const lines = String(text || "").split(/\r?\n/);
  for (let index = 0; index < lines.length - 1; index += 1) {
    const header = String(lines[index] || "").trim();
    const separator = String(lines[index + 1] || "").trim();
    if (!header || !separator || !header.includes("|") || !separator.includes("-")) continue;

    const headerCells = header.split("|").map((cell) => cell.trim()).filter(Boolean);
    const separatorCells = separator.split("|").map((cell) => cell.trim()).filter(Boolean);
    if (headerCells.length < 2) continue;
    if (separatorCells.length !== headerCells.length) continue;
    if (separatorCells.every(isMarkdownTableSeparatorCell)) return true;
  }
  return false;
}

export function mayContainMarkdown(text: string): boolean {
  const value = String(text || "");
  if (!value.trim()) return false;
  // Internal delivery manifests should stay compact plain text instead of
  // picking up prose list spacing from Markdown rendering.
  if (/^\[cccc\]\s+(Attachments|References):/m.test(value)) return false;
  if (containsMarkdownTable(value)) return true;
  return /(```|`[^`\n]+`|\[[^\]]+\]\([^)]+\)|^#{1,6}\s|^\s*[-*+]\s|^\s*\d+\.\s|^\s*>\s)/m.test(value);
}

export function formatStreamingActivityKind(kind: string): string {
  const normalized = String(kind || "").trim();
  switch (normalized) {
    case "queued":
      return "queue";
    case "thinking":
      return "think";
    case "plan":
      return "plan";
    case "search":
      return "search";
    case "command":
      return "run";
    case "patch":
      return "patch";
    case "tool":
      return "tool";
    case "reply":
      return "reply";
    case "error":
      return "error";
    default:
      return normalized || "step";
  }
}

export function getStructuredStreamingActivityLabel(activity: StreamingActivity): string {
  const command = String(activity.command || "").trim();
  if (command) return command;
  const filePaths = Array.isArray(activity.file_paths)
    ? activity.file_paths.map((item) => String(item || "").trim()).filter((item) => item)
    : [];
  if (filePaths.length > 0) return filePaths.join(", ");
  const toolName = String(activity.tool_name || "").trim();
  const serverName = String(activity.server_name || "").trim();
  if (toolName && serverName) return `${serverName}:${toolName}`;
  if (toolName) return toolName;
  const query = String(activity.query || "").trim();
  if (query) return query;
  return String(activity.summary || "").trim();
}

export function formatEventLine(ev: LedgerEvent): string {
  if (ev.kind === "chat.message" && ev.data && typeof ev.data === "object") {
    const msg = ev.data as ChatMessageData;
    return String(msg.text || "");
  }
  return "";
}

export function getMessageBubbleMotionClass({
  isStreaming,
  isOptimistic,
  isNewlyArrived,
  isUserMessage,
  streamPhase,
}: {
  isStreaming: boolean;
  isOptimistic: boolean;
  isNewlyArrived?: boolean;
  isUserMessage?: boolean;
  streamPhase?: string;
}): string {
  const phase = String(streamPhase || "").trim().toLowerCase();
  if (!isStreaming && !isOptimistic) {
    if (!isNewlyArrived) return "";
    return isUserMessage
      ? "cccc-message-bubble-enter cccc-message-bubble-enter-outgoing"
      : "cccc-message-bubble-enter cccc-message-bubble-enter-incoming";
  }
  if (phase === "commentary") return "cccc-transient-bubble cccc-transient-bubble-commentary";
  return "cccc-transient-bubble";
}
