import { useCallback, type Dispatch, type SetStateAction } from "react";
import type { TFunction } from "i18next";
import { archiveVoiceAssistantDocument } from "../../../services/api";
import type { AssistantVoiceDocument } from "../../../types";
import { findVoiceDocument, voiceDocumentPath } from "./voiceComposerUtils";
import {
  archiveVoiceDocumentWithReferenceCleanup,
  type ClearVoiceDocumentReferences,
} from "./voiceDocumentReferenceLifecycle";

type RefreshAssistant = (opts?: { quiet?: boolean }) => Promise<void>;
export type VoiceSecretaryAction =
  | ""
  | "enable"
  | "transcribe"
  | "save_doc"
  | "new_doc"
  | "instruct_doc"
  | "instruct_ask"
  | "archive_doc"
  | "clear_ask";

export function useVoiceDocumentArchive({
  selectedGroupId,
  activeDocumentWritePath,
  viewedDocumentPath,
  documents,
  archivedDocumentPathsRef,
  captureTargetDocumentPathRef,
  isCurrentGroup,
  loadDocumentDraft,
  clearReferences,
  refreshAssistant,
  setActionBusy,
  setDocuments,
  setViewedDocumentPath,
  setDocumentEditing,
  setCaptureTargetDocumentPath,
  showError,
  showNotice,
  t,
}: {
  selectedGroupId: string;
  activeDocumentWritePath: string;
  viewedDocumentPath: string;
  documents: AssistantVoiceDocument[];
  archivedDocumentPathsRef: { current: Set<string> };
  captureTargetDocumentPathRef: { current: string };
  isCurrentGroup: (groupId: string) => boolean;
  loadDocumentDraft: (document: AssistantVoiceDocument | null) => void;
  clearReferences: ClearVoiceDocumentReferences;
  refreshAssistant: RefreshAssistant;
  setActionBusy: Dispatch<SetStateAction<VoiceSecretaryAction>>;
  setDocuments: Dispatch<SetStateAction<AssistantVoiceDocument[]>>;
  setViewedDocumentPath: Dispatch<SetStateAction<string>>;
  setDocumentEditing: Dispatch<SetStateAction<boolean>>;
  setCaptureTargetDocumentPath: Dispatch<SetStateAction<string>>;
  showError: (message: string) => void;
  showNotice: (notice: { message: string }) => void;
  t: TFunction;
}) {
  return useCallback(
    async (targetDocument?: AssistantVoiceDocument | null) => {
      const groupId = String(selectedGroupId || "").trim();
      const documentPath = targetDocument
        ? voiceDocumentPath(targetDocument)
        : activeDocumentWritePath || viewedDocumentPath;
      if (!groupId || !documentPath) return;
      const archivedDocument = targetDocument || findVoiceDocument(documents, documentPath);
      const title = String(archivedDocument?.title || documentPath).trim();
      if (
        !window.confirm(
          t("voiceSecretaryArchiveDocumentConfirm", {
            title,
            defaultValue: 'Archive document "{{title}}"?',
          }),
        )
      )
        return;

      const fallbackDocument = archivedDocument || {
        document_id: "",
        document_path: documentPath,
        title,
        status: "archived",
      };
      const isActiveTarget = documentPath === (activeDocumentWritePath || viewedDocumentPath);
      setActionBusy("archive_doc");
      try {
        const response = await archiveVoiceDocumentWithReferenceCleanup({
          groupId,
          documentPath,
          fallbackDocument,
          clearReferences,
          archiveDocument: archiveVoiceAssistantDocument,
        });
        if (!isCurrentGroup(groupId)) return;
        if (!response.ok) {
          showError(response.error.message);
          return;
        }
        archivedDocumentPathsRef.current.add(documentPath);
        setDocuments((current) =>
          current.filter((document) => voiceDocumentPath(document) !== documentPath),
        );
        if (isActiveTarget) {
          setViewedDocumentPath("");
          loadDocumentDraft(null);
          setDocumentEditing(false);
        }
        if (captureTargetDocumentPathRef.current === documentPath) {
          captureTargetDocumentPathRef.current = "";
          setCaptureTargetDocumentPath("");
        }
        showNotice({
          message: t("voiceSecretaryDocumentArchived", {
            defaultValue: "Voice Secretary working document archived.",
          }),
        });
        await refreshAssistant({ quiet: true });
      } catch {
        if (!isCurrentGroup(groupId)) return;
        showError(
          t("voiceSecretaryDocumentArchiveFailed", {
            defaultValue: "Failed to archive the Voice Secretary document.",
          }),
        );
      } finally {
        if (isCurrentGroup(groupId)) setActionBusy("");
      }
    },
    [
      activeDocumentWritePath,
      archivedDocumentPathsRef,
      captureTargetDocumentPathRef,
      clearReferences,
      documents,
      isCurrentGroup,
      loadDocumentDraft,
      refreshAssistant,
      selectedGroupId,
      setActionBusy,
      setCaptureTargetDocumentPath,
      setDocumentEditing,
      setDocuments,
      setViewedDocumentPath,
      showError,
      showNotice,
      t,
      viewedDocumentPath,
    ],
  );
}
