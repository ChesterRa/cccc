import type { TFunction } from "i18next";
import type { AssistantVoiceDocument } from "../../../types";
import { safeAttachmentFilename } from "../../../utils/messageAttachmentLinks";

function voiceDocumentDownloadFileName(
  document: AssistantVoiceDocument,
  displayTitle: string,
): string {
  const workspaceName = String(document.document_path || document.workspace_path || "")
    .split(/[\\/]/)
    .pop();
  const title = displayTitle.trim() || document.title.trim() || workspaceName || "";
  const name = safeAttachmentFilename(
    title.replace(/[\\/:*?"<>|]/g, "_"),
    "voice-secretary-document",
  );
  return /\.md$/i.test(name) ? name : `${name}.md`;
}

function downloadMarkdownDocument(fileName: string, content: string): void {
  if (typeof document === "undefined") return;
  const blob = new Blob([content], { type: "text/markdown;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = fileName;
  anchor.rel = "noopener";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  window.setTimeout(() => URL.revokeObjectURL(url), 0);
}

export function downloadVoiceDocument(
  activeDocument: AssistantVoiceDocument | null,
  documentDisplayTitle: string,
  documentDraft: string,
  showNotice: (notice: { message: string }) => void,
  t: TFunction,
) {
  if (!activeDocument) return;
  const fileName = voiceDocumentDownloadFileName(activeDocument, documentDisplayTitle);
  downloadMarkdownDocument(fileName, documentDraft);
  showNotice({
    message: t("voiceSecretaryDocumentDownloaded", {
      fileName,
      defaultValue: "Downloaded {{fileName}}.",
    }),
  });
}
