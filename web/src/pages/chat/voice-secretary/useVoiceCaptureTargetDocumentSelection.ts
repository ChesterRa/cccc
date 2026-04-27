import { useCallback, useRef } from "react";
import type { MutableRefObject } from "react";
import type { TFunction } from "i18next";
import type { AssistantVoiceDocument } from "../../../types";
import { selectVoiceAssistantDocument } from "../../../services/api";

type UseVoiceCaptureTargetDocumentSelectionArgs = {
  selectedGroupId: string;
  recording: boolean;
  captureTargetDocumentPathRef: MutableRefObject<string>;
  getDocumentPath: (document: AssistantVoiceDocument | null | undefined) => string;
  setCaptureTargetDocumentPath: (path: string) => void;
  clearTranscriptFlushTimer: () => void;
  clearTranscriptMaxFlushTimer: () => void;
  flushBrowserTranscriptWindow: (
    triggerKind: string,
    opts?: { documentPath?: string },
  ) => Promise<void>;
  refreshAssistant: (opts?: { quiet?: boolean }) => Promise<void>;
  showError: (message: string) => void;
  showNotice: (notice: { message: string }) => void;
  t: TFunction;
};

export function useVoiceCaptureTargetDocumentSelection({
  selectedGroupId,
  recording,
  captureTargetDocumentPathRef,
  getDocumentPath,
  setCaptureTargetDocumentPath,
  clearTranscriptFlushTimer,
  clearTranscriptMaxFlushTimer,
  flushBrowserTranscriptWindow,
  refreshAssistant,
  showError,
  showNotice,
  t,
}: UseVoiceCaptureTargetDocumentSelectionArgs) {
  const selectionSeqRef = useRef(0);

  return useCallback(async (document: AssistantVoiceDocument) => {
    const nextPath = getDocumentPath(document);
    const currentPath = String(captureTargetDocumentPathRef.current || "").trim();
    if (!nextPath || nextPath === currentPath) return;
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;

    const seq = selectionSeqRef.current + 1;
    selectionSeqRef.current = seq;
    captureTargetDocumentPathRef.current = nextPath;
    setCaptureTargetDocumentPath(nextPath);

    try {
      clearTranscriptFlushTimer();
      clearTranscriptMaxFlushTimer();
      if (recording && currentPath) {
        await flushBrowserTranscriptWindow("document_switch", { documentPath: currentPath });
      }
      const resp = await selectVoiceAssistantDocument(gid, nextPath, { by: "user" });
      if (seq !== selectionSeqRef.current) return;
      if (!resp.ok) {
        captureTargetDocumentPathRef.current = currentPath;
        setCaptureTargetDocumentPath(currentPath);
        showError(resp.error.message);
        await refreshAssistant({ quiet: true });
        return;
      }
      setCaptureTargetDocumentPath(nextPath);
      captureTargetDocumentPathRef.current = nextPath;
      showNotice({
        message: t("voiceSecretaryCaptureTargetChanged", {
          title: String(resp.result.document?.title || document.title || ""),
          defaultValue: "Default document changed to {{title}}.",
        }),
      });
    } catch {
      if (seq !== selectionSeqRef.current) return;
      captureTargetDocumentPathRef.current = currentPath;
      setCaptureTargetDocumentPath(currentPath);
      showError(t("voiceSecretaryCaptureTargetChangeFailed", { defaultValue: "Failed to change the default document." }));
    }
  }, [
    captureTargetDocumentPathRef,
    clearTranscriptFlushTimer,
    clearTranscriptMaxFlushTimer,
    flushBrowserTranscriptWindow,
    getDocumentPath,
    recording,
    refreshAssistant,
    selectedGroupId,
    setCaptureTargetDocumentPath,
    showError,
    showNotice,
    t,
  ]);
}
