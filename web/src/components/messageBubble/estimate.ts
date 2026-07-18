import type { LedgerEvent, MessageAttachment } from "../../types";
import { isImageAttachment } from "../../utils/messageAttachments";

const DEFAULT_MESSAGE_HEIGHT = 72;
const DEFAULT_STREAMING_HEIGHT = 108;
const DEFAULT_QUEUED_PLACEHOLDER_HEIGHT = 84;
const DEFAULT_REPLY_CONTEXT_HEIGHT = 60;
const DEFAULT_SINGLE_IMAGE_ATTACHMENT_HEIGHT = 224;
const DEFAULT_GRID_IMAGE_ATTACHMENT_HEIGHT = 188;
const DEFAULT_FILE_ATTACHMENT_HEIGHT = 40;
const DEFAULT_CODE_BLOCK_HEIGHT = 80;
const AVG_CHARS_PER_LINE = 40;
const MAX_TEXT_HEIGHT = 400;

function getEstimatedTextHeight(text: string): number {
  if (!text) return 0;
  const lineCount = text.split("\n").length;
  const wrapLines = Math.ceil(text.length / AVG_CHARS_PER_LINE);
  const estimatedLines = Math.max(lineCount, wrapLines);
  return Math.min(estimatedLines * 20, MAX_TEXT_HEIGHT);
}

function getEstimatedAttachmentHeight(attachments: MessageAttachment[]): number {
  const imageCount = attachments.filter((attachment) => isImageAttachment(attachment)).length;
  const fileCount = attachments.length - imageCount;

  let total = 0;
  if (imageCount === 1) {
    total += DEFAULT_SINGLE_IMAGE_ATTACHMENT_HEIGHT;
  } else if (imageCount > 1) {
    total += Math.ceil(imageCount / 2) * DEFAULT_GRID_IMAGE_ATTACHMENT_HEIGHT;
  }
  if (imageCount > 0 && fileCount > 0) {
    total += 12;
  }
  total += fileCount * DEFAULT_FILE_ATTACHMENT_HEIGHT;
  return total;
}

export function estimateMessageRowHeight(
  message: LedgerEvent | undefined,
  options?: { collapseHeader?: boolean },
): number {
  if (!message) return 100;

  const data = message.data as
    | {
        text?: string;
        insight?: string;
        attachments?: MessageAttachment[];
        quote_text?: string;
        activities?: Array<{ kind?: string; summary?: string }>;
      }
    | undefined;

  const text = String(data?.text || "");
  const insight = String(data?.insight || "");
  const attachments = Array.isArray(data?.attachments) ? data.attachments : [];
  const quoteText = String(data?.quote_text || "");
  const activities = Array.isArray(data?.activities) ? data.activities : [];

  const headerOffset = options?.collapseHeader ? -28 : 0;

  if (message._streaming) {
    const isQueuedOnlyPlaceholder =
      !text.trim() &&
      attachments.length === 0 &&
      activities.length === 1 &&
      String(activities[0]?.kind || "") === "queued" &&
      String(activities[0]?.summary || "") === "queued";

    if (isQueuedOnlyPlaceholder) {
      return Math.max(56, DEFAULT_QUEUED_PLACEHOLDER_HEIGHT + headerOffset);
    }
    return Math.max(
      72,
      DEFAULT_STREAMING_HEIGHT +
        getEstimatedTextHeight(text) +
        (insight.trim() ? 32 + getEstimatedTextHeight(insight) : 0) +
        headerOffset,
    );
  }

  let height = DEFAULT_MESSAGE_HEIGHT;
  height += getEstimatedTextHeight(text);
  if (insight.trim()) {
    height += 32 + getEstimatedTextHeight(insight);
  }

  const codeBlockCount = (text.match(/```/g) || []).length / 2;
  if (codeBlockCount > 0) {
    height += codeBlockCount * DEFAULT_CODE_BLOCK_HEIGHT;
  }

  height += getEstimatedAttachmentHeight(attachments);

  if (quoteText.trim()) {
    height += DEFAULT_REPLY_CONTEXT_HEIGHT;
  }

  return Math.max(64, height + headerOffset);
}
