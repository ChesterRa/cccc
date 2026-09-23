import type { TFunction } from "i18next";
import type { ReactNode } from "react";
import { VoiceDocumentRowMenu } from "./VoiceDocumentRowMenu";
import type { AssistantVoiceDocument } from "../../../types";
import { classNames } from "../../../utils/classNames";

type VoiceSecretaryDocumentListPanelProps = {
  actionBusy: string;
  navigation?: ReactNode;
  footer?: ReactNode;
  emptyLabel?: string;
  onMoveDocument?: (document: AssistantVoiceDocument) => void;
  onRenameDocument?: (document: AssistantVoiceDocument) => void;
  recording?: boolean;
  onArchiveDocument?: (document: AssistantVoiceDocument) => void;
  onDeleteDocument?: (document: AssistantVoiceDocument) => void;
  activeDocumentPath: string;
  captureTargetDocumentPath: string;
  creatingDocument: boolean;
  documents: AssistantVoiceDocument[];
  isDark: boolean;
  newDocumentTitleDraft: string;
  t: TFunction;
  documentKey: (document: AssistantVoiceDocument) => string;
  documentPath: (document: AssistantVoiceDocument) => string;
  onCancelCreateDocument: () => void;
  onCreateDocument: () => void;
  onNewDocumentTitleChange: (value: string) => void;
  onSelectDocument: (document: AssistantVoiceDocument) => void;
  onSetCaptureTargetDocument: (document: AssistantVoiceDocument) => void;
  onStartCreateDocument: () => void;
};

export function VoiceSecretaryDocumentListPanel({
  actionBusy,
  navigation,
  footer,
  emptyLabel,
  onMoveDocument,
  onRenameDocument,
  recording = false,
  onArchiveDocument,
  onDeleteDocument,
  activeDocumentPath,
  captureTargetDocumentPath,
  creatingDocument,
  documents,
  isDark,
  newDocumentTitleDraft,
  t,
  documentKey,
  documentPath,
  onCancelCreateDocument,
  onCreateDocument,
  onNewDocumentTitleChange,
  onSelectDocument,
  onSetCaptureTargetDocument,
  onStartCreateDocument,
}: VoiceSecretaryDocumentListPanelProps) {
  return (
    <aside
      className={classNames(
        "flex min-h-0 flex-col rounded-xl border border-[var(--glass-panel-border)] bg-[var(--color-bg-secondary)]",
      )}
    >
      <div className="flex shrink-0 items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-3.5 py-3">
        <div
          className={classNames(
            "min-w-0 text-sm font-semibold",
            isDark ? "text-slate-100" : "text-gray-900",
          )}
        >
          {t("voiceSecretaryDocumentsTitle", { defaultValue: "Working documents" })}
        </div>
        <button
          type="button"
          onClick={onStartCreateDocument}
          disabled={!!actionBusy}
          className={classNames(
            "rounded-full border px-3 py-1.5 text-xs font-semibold whitespace-nowrap transition-colors disabled:opacity-60",
            isDark
              ? "border-white/10 text-slate-300 hover:bg-white/10"
              : "border-black/10 bg-white text-gray-700 hover:bg-black/5",
          )}
        >
          {actionBusy === "new_doc"
            ? t("voiceSecretaryCreatingDocument", { defaultValue: "Creating..." })
            : t("voiceSecretaryNewDocumentShort", { defaultValue: "New" })}
        </button>
      </div>
      <div className="min-h-0 flex-1 space-y-1.5 overflow-auto scrollbar-hide p-2.5">
        {navigation}
        {creatingDocument ? (
          <div
            className={classNames(
              "mb-2 space-y-2 rounded-2xl border p-2.5",
              isDark ? "border-white/10 bg-white/[0.04]" : "border-black/10 bg-white",
            )}
          >
            <input
              value={newDocumentTitleDraft}
              autoFocus
              onChange={(event) => onNewDocumentTitleChange(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  onCreateDocument();
                }
                if (event.key === "Escape") {
                  event.preventDefault();
                  onCancelCreateDocument();
                }
              }}
              placeholder={t("voiceSecretaryNewDocumentNamePlaceholder", {
                defaultValue: "Document name",
              })}
              className={classNames(
                "w-full rounded-lg border px-2.5 py-1.5 text-xs outline-none transition-colors",
                isDark
                  ? "border-white/10 bg-black/20 text-slate-100 placeholder:text-slate-500 focus:border-white/30"
                  : "border-black/10 bg-white text-gray-900 placeholder:text-gray-400 focus:border-black/25",
              )}
            />
            <div className="flex items-center justify-end gap-1.5">
              <button
                type="button"
                onClick={onCancelCreateDocument}
                disabled={actionBusy === "new_doc"}
                className={classNames(
                  "rounded-full px-2 py-1 text-xs font-medium transition-colors disabled:opacity-60",
                  isDark ? "text-slate-400 hover:bg-white/8" : "text-gray-500 hover:bg-black/5",
                )}
              >
                {t("cancel", { defaultValue: "Cancel" })}
              </button>
              <button
                type="button"
                onClick={onCreateDocument}
                disabled={actionBusy === "new_doc"}
                className={classNames(
                  "rounded-full px-2 py-1 text-xs font-semibold transition-colors disabled:opacity-60",
                  isDark
                    ? "bg-white text-[rgb(20,20,22)] hover:bg-white/90"
                    : "bg-[rgb(35,36,37)] text-white hover:bg-black",
                )}
              >
                {actionBusy === "new_doc"
                  ? t("voiceSecretaryCreatingDocument", { defaultValue: "Creating..." })
                  : t("voiceSecretaryCreateDocument", { defaultValue: "Create" })}
              </button>
            </div>
          </div>
        ) : null}
        {documents.length ? (
          documents.map((document) => {
            const docId = documentKey(document);
            const docPath = documentPath(document);
            const viewing = docPath && docPath === activeDocumentPath;
            const captureTarget = docPath && docPath === captureTargetDocumentPath;
            return (
              <VoiceDocumentRowMenu
                key={docId || document.title}
                title={document.title || docId}
                disabled={!!actionBusy || recording}
                captureTarget={!!captureTarget}
                viewing={!!viewing}
                t={t}
                onSelect={() => onSelectDocument(document)}
                onSetCaptureTarget={
                  docPath ? () => onSetCaptureTargetDocument(document) : undefined
                }
                onArchive={onArchiveDocument ? () => onArchiveDocument(document) : undefined}
                onDelete={onDeleteDocument ? () => onDeleteDocument(document) : undefined}
                onMove={onMoveDocument ? () => onMoveDocument(document) : undefined}
                onRename={onRenameDocument ? () => onRenameDocument(document) : undefined}
              >
                {(trigger) => (
                  <div
                    role="button"
                    tabIndex={0}
                    onClick={() => onSelectDocument(document)}
                    onKeyDown={(event) => {
                      if (event.target !== event.currentTarget) return;
                      if (event.key !== "Enter" && event.key !== " ") return;
                      event.preventDefault();
                      onSelectDocument(document);
                    }}
                    className={classNames(
                      "group/item flex w-full min-w-0 flex-col gap-1.5 rounded-2xl border py-2.5 pl-3 pr-1.5 text-left transition-colors focus-visible:outline-none focus-visible:ring-2",
                      viewing
                        ? isDark
                          ? "border-white/14 bg-white/[0.08] text-white shadow-[0_10px_30px_-24px_rgba(255,255,255,0.32)] focus-visible:ring-white/35"
                          : "border-black/12 bg-white text-[rgb(35,36,37)] shadow-[0_10px_30px_-24px_rgba(15,23,42,0.14)] focus-visible:ring-black/25"
                        : isDark
                          ? "border-transparent text-slate-300 hover:border-white/10 hover:bg-white/8 focus-visible:ring-white/35"
                          : "border-transparent text-gray-700 hover:border-black/10 hover:bg-white focus-visible:ring-black/25",
                    )}
                  >
                    <span className="flex min-w-0 items-center gap-1.5">
                      <span
                        title={document.title || docId}
                        className="min-w-0 flex-1 line-clamp-2 break-words text-sm font-semibold"
                      >
                        {document.title || docId}
                      </span>
                      {captureTarget ? (
                        <span
                          data-voice-document-default
                          className={classNames(
                            "shrink-0 rounded-full px-1.5 py-0.5 text-[10px] font-semibold leading-none",
                            isDark ? "bg-white/[0.14] text-white" : "bg-[rgb(35,36,37)] text-white",
                          )}
                        >
                          {t("voiceSecretaryCaptureTargetBadge", { defaultValue: "Default" })}
                        </span>
                      ) : null}
                      {trigger}
                    </span>
                  </div>
                )}
              </VoiceDocumentRowMenu>
            );
          })
        ) : (
          <div className="flex h-full items-center justify-center px-3 text-center text-xs text-[var(--color-text-muted)]">
            {emptyLabel ||
              t("voiceSecretaryNoDocumentsHint", {
                defaultValue: "Start recording or create a document.",
              })}
          </div>
        )}
      </div>
      {footer}
    </aside>
  );
}
