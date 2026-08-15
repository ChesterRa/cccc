import type { PresentationMessageRef, TaskMessageRef, VoiceDocumentMessageRef } from "../../types";
import { getPresentationRefChipLabel } from "../../utils/presentationRefs";
import { getTaskRefChipLabel } from "../../utils/taskRefs";
import { getVoiceDocumentRefLabel } from "../../utils/voiceDocumentRefs";

export function buildMessageCopyText(input: {
  quoteText?: string;
  messageText: string;
  insight: string;
  insightLabel: string;
  presentationRefs: PresentationMessageRef[];
  voiceDocumentRefs: VoiceDocumentMessageRef[];
  taskRefs: TaskMessageRef[];
  attachments: { title: string; path: string }[];
}): string {
  const sections: string[] = [];
  const quote = String(input.quoteText || "").trim();
  const message = String(input.messageText || "").trim();
  if (quote) sections.push(`> ${quote}`);
  if (message) sections.push(message);
  const insight = String(input.insight || "").trim();
  if (insight) {
    sections.push(`${String(input.insightLabel || "Sender perspective").trim()}:\n${insight}`);
  }
  if (input.presentationRefs.length > 0) {
    sections.push(
      [
        "Presentation refs:",
        ...input.presentationRefs.map((ref) => `- ${getPresentationRefChipLabel(ref)}`),
      ].join("\n"),
    );
  }
  if (input.taskRefs.length > 0) {
    sections.push(
      ["Tasks:", ...input.taskRefs.map((ref) => `- ${getTaskRefChipLabel(ref)}`)].join("\n"),
    );
  }
  if (input.voiceDocumentRefs.length > 0) {
    sections.push(
      [
        "Documents:",
        ...input.voiceDocumentRefs.map(
          (ref) => `- ${getVoiceDocumentRefLabel(ref)} (${ref.document_path})`,
        ),
      ].join("\n"),
    );
  }
  if (input.attachments.length > 0) {
    sections.push(
      [
        "Attachments:",
        ...input.attachments.map((attachment) => {
          const title = String(attachment.title || "").trim();
          if (title) return `- ${title}`;
          const parts = String(attachment.path || "").split("/");
          return `- ${parts[parts.length - 1] || "file"}`;
        }),
      ].join("\n"),
    );
  }
  return sections.join("\n\n").trim();
}
