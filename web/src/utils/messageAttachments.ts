import type { MessageAttachment } from "../types";

const IMAGE_ATTACHMENT_EXTENSIONS = new Set([
  ".png",
  ".jpg",
  ".jpeg",
  ".gif",
  ".webp",
  ".svg",
  ".bmp",
  ".avif",
]);

// Containers browsers commonly play inline.
const VIDEO_ATTACHMENT_EXTENSIONS = new Set([".mp4", ".m4v", ".mov", ".webm"]);

const AUDIO_ATTACHMENT_EXTENSIONS = new Set([
  ".mp3",
  ".wav",
  ".ogg",
  ".oga",
  ".opus",
  ".m4a",
  ".aac",
  ".flac",
]);

const MARKDOWN_ATTACHMENT_EXTENSIONS = new Set([".md", ".markdown"]);

// Files people read as they are. HTML and SVG stay out: shown as text they say little, and
// neither may run in the page.
const TEXT_ATTACHMENT_EXTENSIONS = new Set([
  ".txt",
  ".log",
  ".json",
  ".jsonl",
  ".yaml",
  ".yml",
  ".toml",
  ".csv",
  ".tsv",
  ".xml",
  ".ini",
  ".sql",
  ".diff",
  ".patch",
  ".sh",
  ".py",
  ".js",
  ".ts",
  ".tsx",
  ".jsx",
  ".mjs",
  ".cjs",
  ".rs",
  ".go",
  ".java",
  ".kt",
  ".c",
  ".h",
  ".cpp",
  ".hpp",
  ".cs",
  ".rb",
  ".php",
  ".swift",
  ".lua",
  ".ps1",
  ".bat",
  ".css",
  ".scss",
  ".vue",
  ".svelte",
  ".conf",
  ".cfg",
]);

export type DocumentPreviewKind = "markdown" | "pdf" | "text";

function attachmentPathOrName(attachment: MessageAttachment): string {
  return String(attachment.path || attachment.title || "")
    .trim()
    .toLowerCase();
}

function attachmentExtension(attachment: MessageAttachment): string {
  const raw = attachmentPathOrName(attachment);
  const queryIndex = raw.indexOf("?");
  const clean = queryIndex >= 0 ? raw.slice(0, queryIndex) : raw;
  const dot = clean.lastIndexOf(".");
  if (dot < 0) return "";
  return clean.slice(dot);
}

function hasAttachmentExtension(attachment: MessageAttachment, extensions: Set<string>): boolean {
  if (extensions.has(attachmentExtension(attachment))) return true;
  // Blob paths are content hashes; the original name lives in the title.
  const title = String(attachment.title || "")
    .trim()
    .toLowerCase();
  const dot = title.lastIndexOf(".");
  return dot >= 0 && extensions.has(title.slice(dot));
}

export function isImageAttachment(attachment: MessageAttachment): boolean {
  const kind = String(attachment.kind || "")
    .trim()
    .toLowerCase();
  if (kind === "image") return true;
  const mime = String(attachment.mime_type || "")
    .trim()
    .toLowerCase();
  if (mime.startsWith("image/")) return true;
  return IMAGE_ATTACHMENT_EXTENSIONS.has(attachmentExtension(attachment));
}

export function isVideoAttachment(attachment: MessageAttachment): boolean {
  if (isImageAttachment(attachment)) return false;
  const mime = String(attachment.mime_type || "")
    .trim()
    .toLowerCase();
  if (mime.startsWith("video/")) return true;
  if (mime.startsWith("audio/")) return false;
  return hasAttachmentExtension(attachment, VIDEO_ATTACHMENT_EXTENSIONS);
}

export function isAudioAttachment(attachment: MessageAttachment): boolean {
  if (isImageAttachment(attachment) || isVideoAttachment(attachment)) return false;
  const mime = String(attachment.mime_type || "")
    .trim()
    .toLowerCase();
  if (mime.startsWith("audio/")) return true;
  return hasAttachmentExtension(attachment, AUDIO_ATTACHMENT_EXTENSIONS);
}

/** What a file name alone says about how it opens in the page. */
export function fileKindForName(
  name: string,
): "image" | "video" | "audio" | DocumentPreviewKind | null {
  const attachment = { kind: "file", path: "", title: name, mime_type: "" };
  if (isImageAttachment(attachment)) return "image";
  if (isVideoAttachment(attachment)) return "video";
  if (isAudioAttachment(attachment)) return "audio";
  return attachmentDocumentKind(attachment);
}

/** How a file opens in the page, or null when it stays a download. */
export function attachmentDocumentKind(attachment: MessageAttachment): DocumentPreviewKind | null {
  if (
    isImageAttachment(attachment) ||
    isVideoAttachment(attachment) ||
    isAudioAttachment(attachment)
  )
    return null;
  const mime = String(attachment.mime_type || "")
    .trim()
    .toLowerCase()
    .split(";")[0];
  if (mime === "application/pdf" || hasAttachmentExtension(attachment, new Set([".pdf"])))
    return "pdf";
  if (
    mime === "text/markdown" ||
    hasAttachmentExtension(attachment, MARKDOWN_ATTACHMENT_EXTENSIONS)
  )
    return "markdown";
  if (mime === "text/plain" || hasAttachmentExtension(attachment, TEXT_ATTACHMENT_EXTENSIONS))
    return "text";
  return null;
}

export function isSvgAttachment(attachment: MessageAttachment): boolean {
  const mime = String(attachment.mime_type || "")
    .trim()
    .toLowerCase();
  if (mime === "image/svg+xml") return true;
  return attachmentExtension(attachment) === ".svg";
}

export function isRedundantWecomImagePlaceholder(
  text: string,
  attachments: MessageAttachment[],
  sourcePlatform?: string,
): boolean {
  if (
    String(sourcePlatform || "")
      .trim()
      .toLowerCase() !== "wecom"
  )
    return false;
  if (!attachments.length || !attachments.every((attachment) => isImageAttachment(attachment)))
    return false;
  const normalized = String(text || "")
    .trim()
    .toLowerCase();
  return normalized === "[image]" || /^\[file(?:: [^\]]+)?\](?:\s+\S+)?$/.test(normalized);
}
