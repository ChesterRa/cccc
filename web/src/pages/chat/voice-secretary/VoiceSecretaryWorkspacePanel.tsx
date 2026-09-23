import type { TFunction } from "i18next";
import { VoiceLiveTranscript } from "./VoiceLiveTranscript";
import type { VoiceTranscriptPreview } from "./voiceStreamModel";
import { useMemo, type ReactNode } from "react";
import { MarkdownDocumentSurface } from "../../../components/document/MarkdownDocumentSurface";
import { MessageSquareQuoteIcon } from "../../../components/Icons";
import { classNames } from "../../../utils/classNames";
import {
  isDisplayableFinalVoiceTranscriptItem,
  type VoiceTranscriptItem,
} from "./voiceStreamModel";
import { VoiceBeam } from "voice-glow";
import { VoiceTranscriptRecordingIndicator } from "./VoiceTranscriptRecordingIndicator";
import { VoiceFinalTranscriptRows } from "./VoiceFinalTranscriptRows";

export type VoiceWorkspaceView = "document" | "transcript";

type VoiceSecretaryWorkspacePanelProps = {
  navigation?: ReactNode;
  livePreview?: VoiceTranscriptPreview | null;
  activeDocumentPath: string;
  activeDocumentWritePath: string;
  actionBusy: string;
  captureTargetDocumentPath: string;
  documentDisplayTitle: string;
  documentDraft: string;
  documentEditing: boolean;
  documentHasUnsavedEdits: boolean;
  documentLoading: boolean;
  documentRemoteChanged: boolean;
  isDark: boolean;
  recording: boolean;
  /** Per-frame microphone level getter, 0–1. */
  recordingAudioLevel: () => number;
  t: TFunction;
  transcriptItems: VoiceTranscriptItem[];
  view: VoiceWorkspaceView;
  onChangeView: (view: VoiceWorkspaceView) => void;
  onArchiveDocument: () => void;
  onClearTranscript: () => void;
  onDownloadDocument: () => void;
  onEditDocumentChange: (value: string) => void;
  onLoadLatestDocument: () => void;
  onQuoteDocument: () => void;
  onSaveDocument: () => void;
  onToggleDocumentEditing: () => void;
  formatTime: (value: number) => string;
  formatFullTime: (value: number) => string;
  normalizeTranscriptText: (value: string) => string;
};

export function VoiceSecretaryWorkspacePanel({
  navigation,
  livePreview,
  activeDocumentPath,
  activeDocumentWritePath,
  actionBusy,
  captureTargetDocumentPath,
  documentDisplayTitle,
  documentDraft,
  documentEditing,
  documentHasUnsavedEdits,
  documentLoading,
  documentRemoteChanged,
  isDark,
  recording,
  recordingAudioLevel,
  t,
  transcriptItems,
  view,
  onChangeView,
  onArchiveDocument,
  onClearTranscript,
  onDownloadDocument,
  onEditDocumentChange,
  onLoadLatestDocument,
  onQuoteDocument,
  onSaveDocument,
  onToggleDocumentEditing,
  formatTime,
  formatFullTime,
  normalizeTranscriptText,
}: VoiceSecretaryWorkspacePanelProps) {
  const processingRows = useMemo(
    () => transcriptItems.filter((item) => item.processingPhase === "separating_speakers"),
    [transcriptItems],
  );
  const failedRows = useMemo(
    () => transcriptItems.filter((item) => item.processingPhase === "failed"),
    [transcriptItems],
  );
  const transcriptRows = useMemo(
    () => transcriptItems.filter(isDisplayableFinalVoiceTranscriptItem),
    [transcriptItems],
  );
  const transcriptCount = transcriptRows.length;
  const documentActionClassName = classNames(
    "inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs font-semibold transition-colors disabled:opacity-50",
    isDark
      ? "border-white/10 bg-white/[0.04] text-slate-300 hover:bg-white/10"
      : "border-black/10 bg-white text-gray-600 hover:bg-black/5",
  );
  const analyzingAudio = !recording && processingRows.length > 0;
  return (
    <VoiceBeam
      type="mobile"
      active={recording || analyzingAudio}
      level={recordingAudioLevel}
      processing={analyzingAudio}
      theme={isDark ? "dark" : "light"}
      data-voice-workspace-glow
      className="flex min-h-0 flex-col"
    >
      <section
        data-voice-document-panel
        className="flex min-h-0 flex-1 flex-col rounded-xl border border-[var(--glass-panel-border)] bg-[var(--color-bg-primary)] p-3"
      >
        {navigation}
        <div
          data-voice-document-header
          className="flex shrink-0 flex-wrap items-start justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-1 pb-3"
        >
          <div data-voice-document-heading className="min-w-0 flex-1">
            <div
              data-voice-document-title
              className={classNames(
                "break-words text-xl font-semibold tracking-[-0.02em]",
                isDark ? "text-slate-100" : "text-gray-900",
              )}
            >
              {documentDisplayTitle}
            </div>
            <div data-voice-document-meta className="mt-2 flex flex-wrap items-center gap-1.5">
              <div
                className={classNames(
                  "inline-flex rounded-full border p-0.5",
                  isDark ? "border-white/10 bg-white/[0.04]" : "border-black/10 bg-white",
                )}
                data-voice-document-views
                role="group"
                aria-label={t("voiceSecretaryWorkspaceViewSelector", {
                  defaultValue: "Voice Secretary workspace view",
                })}
              >
                {(["document", "transcript"] as VoiceWorkspaceView[]).map((nextView) => {
                  const active = view === nextView;
                  return (
                    <button
                      key={nextView}
                      type="button"
                      className={classNames(
                        "rounded-full px-2.5 py-1 text-xs font-semibold transition-colors",
                        active
                          ? isDark
                            ? "bg-white text-slate-950"
                            : "bg-[rgb(35,36,37)] text-white"
                          : isDark
                            ? "text-slate-300 hover:bg-white/10"
                            : "text-gray-600 hover:bg-black/5",
                      )}
                      onClick={() => onChangeView(nextView)}
                      aria-pressed={active}
                    >
                      {nextView === "document"
                        ? t("voiceSecretaryWorkspaceViewDocument", { defaultValue: "Document" })
                        : t("voiceSecretaryWorkspaceViewTranscript", {
                            defaultValue: "Transcript",
                          })}
                    </button>
                  );
                })}
              </div>
              {view === "transcript" ? (
                <span
                  className={classNames(
                    "rounded-full px-2 py-0.5 text-xs font-medium",
                    isDark
                      ? "bg-white/10 text-slate-100"
                      : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]",
                  )}
                >
                  {t("voiceSecretaryTranscriptCount", {
                    count: transcriptCount,
                    defaultValue: "{{count}} entries",
                  })}
                </span>
              ) : null}
              {view === "document" && !activeDocumentPath ? (
                <span
                  className={classNames(
                    "rounded-full px-2 py-0.5 text-xs font-medium",
                    isDark ? "bg-slate-800 text-slate-300" : "bg-gray-100 text-gray-600",
                  )}
                >
                  {t("voiceSecretaryWaitingTranscriptBadge", {
                    defaultValue: "Waiting for transcript",
                  })}
                </span>
              ) : null}
              {view === "document" &&
              activeDocumentWritePath &&
              activeDocumentWritePath === captureTargetDocumentPath ? (
                <span
                  className={classNames(
                    "rounded-full px-2 py-0.5 text-xs font-medium",
                    isDark
                      ? "bg-white/10 text-slate-200"
                      : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]",
                  )}
                >
                  {t("voiceSecretaryDefaultDocumentBadge", { defaultValue: "Default document" })}
                </span>
              ) : null}
              {view === "document" && activeDocumentPath ? (
                <>
                  <button
                    type="button"
                    onClick={onQuoteDocument}
                    disabled={documentLoading || documentHasUnsavedEdits}
                    className={documentActionClassName}
                    title={
                      documentHasUnsavedEdits
                        ? t("voiceSecretaryQuoteDocumentSaveFirst", {
                            defaultValue: "Save document edits before quoting it in chat",
                          })
                        : t("voiceSecretaryQuoteDocumentInChat", { defaultValue: "Quote in chat" })
                    }
                  >
                    <MessageSquareQuoteIcon size={12} aria-hidden="true" />
                    {t("voiceSecretaryQuoteDocumentInChat", { defaultValue: "Quote in chat" })}
                  </button>
                  <button
                    type="button"
                    onClick={onArchiveDocument}
                    disabled={!!actionBusy || documentLoading}
                    className={documentActionClassName}
                    title={t("voiceSecretaryArchiveDocument", { defaultValue: "Archive viewed" })}
                  >
                    {actionBusy === "archive_doc"
                      ? t("voiceSecretaryArchivingDocument", { defaultValue: "Archiving..." })
                      : t("voiceSecretaryArchiveShort", { defaultValue: "Archive" })}
                  </button>
                </>
              ) : null}
              {view === "document" && documentHasUnsavedEdits ? (
                <span
                  className={classNames(
                    "rounded-full px-2 py-0.5 text-xs font-medium",
                    isDark ? "bg-amber-500/10 text-amber-200" : "bg-amber-50 text-amber-700",
                  )}
                >
                  {t("voiceSecretaryUnsavedEditsBadge", { defaultValue: "Unsaved edits" })}
                </span>
              ) : null}
              {view === "document" && documentRemoteChanged ? (
                <span
                  className={classNames(
                    "rounded-full px-2 py-0.5 text-xs font-medium",
                    isDark
                      ? "bg-white/10 text-slate-200"
                      : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]",
                  )}
                >
                  {t("voiceSecretaryRemoteChangedBadge", {
                    defaultValue: "Remote update available",
                  })}
                </span>
              ) : null}
              {view === "document" ? (
                <span
                  className={classNames(
                    "inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-full px-2 py-0.5 text-xs font-medium",
                    isDark ? "bg-black/20 text-slate-300" : "bg-[rgb(245,245,245)] text-gray-600",
                  )}
                  data-voice-document-location
                  title={activeDocumentPath || undefined}
                >
                  <span className="shrink-0">
                    {activeDocumentPath
                      ? t("voiceSecretaryRepoMarkdownLabel", { defaultValue: "Repo markdown" })
                      : t("voiceSecretaryWorkingDocumentPendingShort", {
                          defaultValue: "Auto-create on transcript",
                        })}
                  </span>
                  {activeDocumentPath ? (
                    <span
                      data-voice-document-path
                      className="min-w-0 truncate font-normal text-[var(--color-text-muted)]"
                    >
                      {activeDocumentPath}
                    </span>
                  ) : null}
                </span>
              ) : null}
            </div>
          </div>
          <div
            data-voice-document-actions
            className="flex shrink-0 flex-wrap items-center justify-end gap-2"
          >
            {view === "document" && documentRemoteChanged ? (
              <button
                type="button"
                className={classNames(
                  "rounded-full border px-2.5 py-1.5 text-xs font-semibold transition-colors disabled:opacity-60",
                  isDark
                    ? "border-white/10 text-slate-300 hover:bg-white/10"
                    : "border-black/10 text-gray-700 hover:bg-black/5",
                )}
                onClick={onLoadLatestDocument}
                disabled={!activeDocumentPath || documentLoading}
                title={t("voiceSecretaryLoadLatestDocumentHint", {
                  defaultValue:
                    "Load the latest document from the daemon. Unsaved local edits in this panel will be replaced.",
                })}
              >
                {t("voiceSecretaryLoadLatestDocument", { defaultValue: "Load latest" })}
              </button>
            ) : null}
            {view === "document" && (documentEditing || documentHasUnsavedEdits) ? (
              <button
                type="button"
                className={classNames(
                  "rounded-full border px-2.5 py-1.5 text-xs font-semibold transition-colors disabled:opacity-60",
                  isDark
                    ? "border-white/10 text-slate-300 hover:bg-white/10"
                    : "border-black/10 text-gray-700 hover:bg-black/5",
                )}
                onClick={onSaveDocument}
                disabled={!!actionBusy || documentLoading}
              >
                {actionBusy === "save_doc"
                  ? t("voiceSecretarySavingDocument", { defaultValue: "Saving..." })
                  : t("voiceSecretarySaveDocument", { defaultValue: "Save edits" })}
              </button>
            ) : null}
            {view === "document" ? (
              <>
                <button
                  type="button"
                  onClick={onDownloadDocument}
                  disabled={!activeDocumentPath || documentLoading}
                  className={classNames(
                    "rounded-full border px-2.5 py-1.5 text-xs font-semibold transition-colors disabled:opacity-50",
                    isDark
                      ? "border-white/10 text-slate-300 hover:bg-white/10"
                      : "border-black/10 text-gray-700 hover:bg-black/5",
                  )}
                >
                  {t("voiceSecretaryDownloadDocument", { defaultValue: "Download .md" })}
                </button>
                <button
                  type="button"
                  onClick={onToggleDocumentEditing}
                  disabled={documentLoading}
                  className={classNames(
                    "rounded-full border px-2.5 py-1.5 text-xs font-semibold transition-colors disabled:opacity-50",
                    isDark
                      ? "border-white/10 text-slate-300 hover:bg-white/10"
                      : "border-black/10 text-gray-700 hover:bg-black/5",
                  )}
                >
                  {documentEditing
                    ? t("voiceSecretaryPreviewDocument", { defaultValue: "Preview" })
                    : t("voiceSecretaryEditDocument", { defaultValue: "Edit" })}
                </button>
              </>
            ) : null}
            {view === "transcript" ? (
              <button
                type="button"
                onClick={onClearTranscript}
                disabled={!transcriptCount || recording}
                className={classNames(
                  "rounded-full border px-2.5 py-1.5 text-xs font-semibold transition-colors disabled:opacity-50",
                  isDark
                    ? "border-white/10 text-slate-300 hover:bg-white/10"
                    : "border-black/10 text-gray-700 hover:bg-black/5",
                )}
                title={
                  recording
                    ? t("voiceSecretaryClearTranscriptDisabledRecording", {
                        defaultValue: "Stop recording before clearing transcript entries.",
                      })
                    : t("voiceSecretaryClearTranscriptTitle", {
                        defaultValue: "Clear visible transcript entries for this document.",
                      })
                }
              >
                {t("voiceSecretaryClearTranscript", { defaultValue: "Clear" })}
              </button>
            ) : null}
          </div>
        </div>

        {view === "document" ? (
          <MarkdownDocumentSurface
            className="mt-3 min-h-0 flex-1 overflow-auto scrollbar-subtle"
            content={documentDraft}
            editValue={documentDraft}
            editing={documentEditing}
            editAriaLabel={t("voiceSecretaryDocumentEditAriaLabel", {
              defaultValue: "Edit Voice Secretary working document markdown",
            })}
            editPlaceholder={t("voiceSecretaryDocumentPlaceholder", {
              defaultValue:
                "Voice Secretary will maintain a markdown working document here as transcript arrives. You can edit it directly.",
            })}
            emptyLabel={t("voiceSecretaryDocumentPreviewEmpty", {
              defaultValue: "Transcript and Voice Secretary edits will appear here.",
            })}
            isDark={isDark}
            loading={documentLoading}
            loadingLabel={t("voiceSecretaryDocumentLoading", {
              defaultValue: "Loading document content...",
            })}
            minHeightClassName="min-h-[280px] lg:min-h-0"
            onEditValueChange={onEditDocumentChange}
          />
        ) : (
          <div className="mt-3 min-h-0 flex-1 space-y-2 overflow-y-auto scrollbar-subtle pr-1 [scrollbar-gutter:stable]">
            {recording ? (
              <VoiceTranscriptRecordingIndicator
                compact
                isDark={isDark}
                label={t("voiceSecretaryTranscriptRecordingIndicator", {
                  defaultValue: "Recording audio. Final transcript appears after Save.",
                })}
              />
            ) : processingRows.length ? (
              <VoiceTranscriptRecordingIndicator
                isDark={isDark}
                label={t("voiceSecretaryTranscriptAnalyzingAudio", {
                  defaultValue: "Analyzing final audio...",
                })}
              />
            ) : null}
            {!recording && !processingRows.length && failedRows.length ? (
              <div
                className={classNames(
                  "rounded-2xl border px-3 py-2.5 text-sm",
                  isDark
                    ? "border-red-300/20 bg-red-300/10 text-red-100"
                    : "border-red-200 bg-red-50 text-red-800",
                )}
              >
                {normalizeTranscriptText(
                  failedRows[0]?.text ||
                    t("voiceSecretaryTranscriptFinalFailed", {
                      defaultValue: "Final audio analysis failed.",
                    }),
                )}
              </div>
            ) : null}
            <VoiceLiveTranscript
              preview={livePreview}
              documentPath={
                activeDocumentWritePath || activeDocumentPath || captureTargetDocumentPath
              }
              recording={recording}
              label={t("voiceSecretaryLiveOriginal", { defaultValue: "Live original transcript" })}
            />
            {transcriptRows.length ? (
              <VoiceFinalTranscriptRows
                transcriptRows={transcriptRows}
                isDark={isDark}
                normalizeTranscriptText={normalizeTranscriptText}
                formatTime={formatTime}
                formatFullTime={formatFullTime}
              />
            ) : !recording && !processingRows.length && !failedRows.length ? (
              <div className="flex h-full min-h-[280px] items-center justify-center rounded-2xl border border-dashed border-[var(--glass-border-subtle)] px-4 text-center text-sm text-[var(--color-text-muted)]">
                {activeDocumentPath
                  ? t("voiceSecretaryTranscriptEmpty", {
                      defaultValue: "Document-mode transcript for this document will appear here.",
                    })
                  : t("voiceSecretaryTranscriptNeedsDocument", {
                      defaultValue: "Choose or create a document to see its transcript.",
                    })}
              </div>
            ) : null}
          </div>
        )}
      </section>
    </VoiceBeam>
  );
}
