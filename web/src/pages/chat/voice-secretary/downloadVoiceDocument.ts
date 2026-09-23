import type { TFunction } from "i18next";
import type { AssistantVoiceDocument } from "../../../types";
import { downloadMarkdownDocument, voiceDocumentDownloadFileName } from "./voiceComposerUtils";

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
