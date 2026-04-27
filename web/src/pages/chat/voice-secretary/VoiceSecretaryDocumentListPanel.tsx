import type { TFunction } from "i18next";
import type { AssistantVoiceDocument } from "../../../types";
import { classNames } from "../../../utils/classNames";

type VoiceSecretaryDocumentListPanelProps = {
  actionBusy: string;
  activeDocumentPath: string;
  captureTargetDocumentPath: string;
  creatingDocument: boolean;
  documents: AssistantVoiceDocument[];
  documentsCountLabel: string;
  isDark: boolean;
  newDocumentTitleDraft: string;
  t: TFunction;
  documentKey: (document: AssistantVoiceDocument) => string;
  documentPath: (document: AssistantVoiceDocument) => string;
  onArchiveDocument: (document: AssistantVoiceDocument) => void;
  onCancelCreateDocument: () => void;
  onCreateDocument: () => void;
  onNewDocumentTitleChange: (value: string) => void;
  onSelectDocument: (document: AssistantVoiceDocument) => void;
  onSetCaptureTargetDocument: (document: AssistantVoiceDocument) => void;
  onStartCreateDocument: () => void;
};

export function VoiceSecretaryDocumentListPanel({
  actionBusy,
  activeDocumentPath,
  captureTargetDocumentPath,
  creatingDocument,
  documents,
  documentsCountLabel,
  isDark,
  newDocumentTitleDraft,
  t,
  documentKey,
  documentPath,
  onArchiveDocument,
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
        "flex min-h-0 flex-col rounded-[26px] border",
        isDark ? "border-white/10 bg-white/[0.035]" : "border-black/10 bg-[rgb(250,250,250)]",
      )}
    >
      <div className="flex shrink-0 items-start justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-3.5 py-3">
        <div className="min-w-0">
          <div className={classNames("text-sm font-semibold", isDark ? "text-slate-100" : "text-gray-900")}>
            {t("voiceSecretaryDocumentsTitle", { defaultValue: "Working documents" })}
          </div>
          <div className="mt-0.5 text-[10px] leading-4 text-[var(--color-text-muted)]">
            {documentsCountLabel}
            {documents.length ? (
              <span>
                {" · "}
                {t("voiceSecretaryDefaultDocumentLegend", {
                  defaultValue: "default gets new transcript",
                })}
              </span>
            ) : null}
          </div>
        </div>
        <button
          type="button"
          onClick={onStartCreateDocument}
          disabled={!!actionBusy}
          className={classNames(
            "rounded-full border px-3 py-1.5 text-[11px] font-semibold whitespace-nowrap transition-colors disabled:opacity-60",
            isDark ? "border-white/10 text-slate-300 hover:bg-white/10" : "border-black/10 bg-white text-gray-700 hover:bg-black/5",
          )}
        >
          {actionBusy === "new_doc"
            ? t("voiceSecretaryCreatingDocument", { defaultValue: "Creating..." })
            : t("voiceSecretaryNewDocumentShort", { defaultValue: "New" })}
        </button>
      </div>
      <div className="min-h-0 flex-1 space-y-1.5 overflow-auto scrollbar-hide p-2.5">
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
                  "rounded-full px-2 py-1 text-[11px] font-medium transition-colors disabled:opacity-60",
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
                  "rounded-full px-2 py-1 text-[11px] font-semibold transition-colors disabled:opacity-60",
                  isDark ? "bg-white text-[rgb(20,20,22)] hover:bg-white/90" : "bg-[rgb(35,36,37)] text-white hover:bg-black",
                )}
              >
                {actionBusy === "new_doc"
                  ? t("voiceSecretaryCreatingDocument", { defaultValue: "Creating..." })
                  : t("voiceSecretaryCreateDocument", { defaultValue: "Create" })}
              </button>
            </div>
          </div>
        ) : null}
        {documents.length ? documents.map((document) => {
          const docId = documentKey(document);
          const docPath = documentPath(document);
          const viewing = docPath && docPath === activeDocumentPath;
          const captureTarget = docPath && docPath === captureTargetDocumentPath;
          return (
            <div
              key={docId || document.title}
              role="button"
              tabIndex={0}
              onClick={() => onSelectDocument(document)}
              onKeyDown={(event) => {
                if (event.key !== "Enter" && event.key !== " ") return;
                event.preventDefault();
                onSelectDocument(document);
              }}
              className={classNames(
                "group flex w-full min-w-0 flex-col gap-1.5 rounded-2xl border px-3 py-2.5 text-left transition-colors",
                viewing
                  ? isDark
                    ? "border-white/14 bg-white/[0.08] text-white shadow-[0_10px_30px_-24px_rgba(255,255,255,0.32)]"
                    : "border-black/12 bg-white text-[rgb(35,36,37)] shadow-[0_10px_30px_-24px_rgba(15,23,42,0.14)]"
                  : isDark
                    ? "border-transparent text-slate-300 hover:border-white/10 hover:bg-white/8"
                    : "border-transparent text-gray-700 hover:border-black/10 hover:bg-white",
              )}
            >
              <span className="flex min-w-0 items-center justify-between gap-2">
                <span className="truncate text-sm font-semibold">{document.title || docId}</span>
                <span className="flex w-[5.5rem] shrink-0 items-center justify-end gap-1">
                  <button
                    type="button"
                    aria-label={t("voiceSecretaryArchiveDocumentItemAriaLabel", {
                      title: document.title || docId,
                      defaultValue: "Archive {{title}}",
                    })}
                    title={t("voiceSecretaryArchiveDocument", { defaultValue: "Archive viewed" })}
                    disabled={!docPath || actionBusy === "archive_doc"}
                    onClick={(event) => {
                      event.stopPropagation();
                      onArchiveDocument(document);
                    }}
                    onKeyDown={(event) => {
                      event.stopPropagation();
                    }}
                    className={classNames(
                      "inline-flex h-6 items-center justify-center rounded-full border px-2 opacity-0 transition-all disabled:cursor-default group-hover:opacity-100 group-focus-within:opacity-100",
                      isDark
                        ? "border-white/10 bg-white/[0.04] text-slate-400 hover:bg-white/10 hover:text-slate-100 disabled:opacity-35"
                        : "border-black/10 bg-white text-gray-500 hover:bg-black/5 hover:text-gray-900 disabled:opacity-35",
                    )}
                  >
                    <span className="text-[10px] font-semibold leading-none">
                      {t("voiceSecretaryArchiveShort", { defaultValue: "Archive" })}
                    </span>
                  </button>
                  <button
                    type="button"
                    aria-pressed={!!captureTarget}
                    aria-label={captureTarget
                      ? t("voiceSecretaryDefaultDocumentActiveAriaLabel", {
                          title: document.title || docId,
                          defaultValue: "{{title}} is the default document for new transcript",
                        })
                      : t("voiceSecretarySetDefaultDocumentAriaLabel", {
                          title: document.title || docId,
                          defaultValue: "Set {{title}} as the default document for new transcript",
                        })}
                    title={captureTarget
                      ? t("voiceSecretaryDefaultDocumentHint", {
                          defaultValue: "New transcript is written here by default",
                        })
                      : t("voiceSecretarySetDefaultDocumentHint", {
                          defaultValue: "Set as the default document for new transcript",
                        })}
                    disabled={!docPath || !!captureTarget}
                    onClick={(event) => {
                      event.stopPropagation();
                      onSetCaptureTargetDocument(document);
                    }}
                    onKeyDown={(event) => {
                      event.stopPropagation();
                    }}
                    className={classNames(
                      "relative flex h-7 w-7 items-center justify-center rounded-full border transition-colors disabled:cursor-default",
                      captureTarget
                        ? isDark
                          ? "border-white/70 bg-white/10 shadow-[0_0_0_3px_rgba(255,255,255,0.08)]"
                          : "border-[rgb(35,36,37)] bg-white shadow-[0_0_0_3px_rgba(35,36,37,0.08)]"
                        : isDark
                          ? "border-white/25 bg-white/[0.03] hover:border-white/50 hover:bg-white/[0.08] disabled:opacity-45"
                          : "border-gray-300 bg-white hover:border-[rgb(35,36,37)]/35 hover:bg-[rgb(245,245,245)] disabled:opacity-45",
                    )}
                  >
                    {captureTarget ? (
                      <span
                        aria-hidden="true"
                        className={classNames(
                          "h-3 w-3 rounded-full",
                          isDark ? "bg-white" : "bg-[rgb(35,36,37)]",
                        )}
                      />
                    ) : null}
                  </button>
                </span>
              </span>
              {document.workspace_path ? (
                <span className="truncate text-[11px] text-[var(--color-text-muted)]">{document.workspace_path}</span>
              ) : null}
            </div>
          );
        }) : (
          <div className="flex h-full items-center justify-center px-3 text-center text-xs text-[var(--color-text-muted)]">
            {t("voiceSecretaryNoDocumentsHint", { defaultValue: "Start recording or create a document." })}
          </div>
        )}
      </div>
    </aside>
  );
}
