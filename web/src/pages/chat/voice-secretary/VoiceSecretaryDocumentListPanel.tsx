import type { TFunction } from "i18next";
import type { ReactNode } from "react";
import { VoiceDocumentRow, type VoiceDocumentRowContext } from "./VoiceDocumentRow";
import type { AssistantVoiceDocument } from "../../../types";
import { classNames } from "../../../utils/classNames";

type VoiceSecretaryDocumentListPanelProps = VoiceDocumentRowContext & {
  navigation?: ReactNode;
  footer?: ReactNode;
  headerActions?: ReactNode;
  /** Replaces the flat document list, e.g. with a folder tree. */
  children?: ReactNode;
  creatingDocument: boolean;
  documents: AssistantVoiceDocument[];
  newDocumentTitleDraft: string;
  onCancelCreateDocument: () => void;
  onCreateDocument: () => void;
  onNewDocumentTitleChange: (value: string) => void;
  onStartCreateDocument: () => void;
};

export function VoiceDocumentListEmpty({ t }: { t: TFunction }) {
  return (
    <div className="flex h-full items-center justify-center px-3 py-6 text-center text-xs text-[var(--color-text-muted)]">
      {t("voiceSecretaryNoDocumentsHint", {
        defaultValue: "Start recording or create a document.",
      })}
    </div>
  );
}

export function VoiceSecretaryDocumentListPanel(props: VoiceSecretaryDocumentListPanelProps) {
  const {
    actionBusy,
    navigation,
    footer,
    headerActions,
    children,
    creatingDocument,
    documents,
    isDark,
    newDocumentTitleDraft,
    t,
    documentKey,
    onCancelCreateDocument,
    onCreateDocument,
    onNewDocumentTitleChange,
    onStartCreateDocument,
  } = props;
  const rowContext: VoiceDocumentRowContext = props;
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
        <div className="flex shrink-0 items-center gap-1">
          {headerActions}
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
      </div>
      <div className="min-h-0 flex-1 space-y-0.5 overflow-auto scrollbar-hide p-2.5">
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
        {children ??
          (documents.length ? (
            documents.map((document) => (
              <VoiceDocumentRow
                key={documentKey(document) || document.title}
                ctx={rowContext}
                document={document}
              />
            ))
          ) : (
            <VoiceDocumentListEmpty t={t} />
          ))}
      </div>
      {footer}
    </aside>
  );
}
