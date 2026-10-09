import type { TFunction } from "i18next";
import { VoiceLiveTranscript } from "./VoiceLiveTranscript";
import type { VoiceTranscriptPreview } from "./voiceStreamModel";
import { useMemo, type ReactNode } from "react";
import { MarkdownDocumentSurface } from "../../../components/document/MarkdownDocumentSurface";
import { Archive } from "lucide-react";
import {
  DownloadIcon,
  FileTextIcon,
  MessageSquareQuoteIcon,
  MoreIcon,
} from "../../../components/Icons";
import { Popover, PopoverContent, PopoverTrigger } from "../../../components/ui/popover";
import { classNames } from "../../../utils/classNames";
import {
  isDisplayableFinalVoiceTranscriptItem,
  type VoiceTranscriptItem,
} from "./voiceStreamModel";
import { VoiceWorkspaceFrame } from "./VoiceWorkspaceFrame";
import { VoiceTranscriptRecordingIndicator } from "./VoiceTranscriptRecordingIndicator";
import { VoiceFinalTranscriptRows } from "./VoiceFinalTranscriptRows";

export type VoiceWorkspaceView = "document" | "transcript";

type VoiceSecretaryWorkspacePanelProps = {
  navigation?: ReactNode;
  statusLine?: ReactNode;
  footer?: ReactNode;
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
  statusLine,
  footer,
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
  const actionClassName =
    "inline-flex min-h-8 items-center gap-1 whitespace-nowrap rounded-lg px-2 text-xs font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--color-bg-tertiary)] hover:text-[var(--color-text-primary)] disabled:pointer-events-none disabled:opacity-40 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]";
  const menuItemClassName =
    "flex min-h-9 w-full items-center gap-2 rounded-lg px-2.5 text-left text-sm text-[var(--color-text-primary)] transition-colors hover:bg-[var(--color-bg-tertiary)] disabled:pointer-events-none disabled:opacity-40";
  const isDefaultDocument =
    !!activeDocumentWritePath && activeDocumentWritePath === captureTargetDocumentPath;
  const documentEmpty = !documentEditing && !documentLoading && !documentDraft.trim();
  const metaItems: ReactNode[] =
    view === "document"
      ? [
          <span
            key="location"
            data-voice-document-location
            className="min-w-0 truncate"
            title={activeDocumentPath || undefined}
          >
            {activeDocumentPath ? (
              <span data-voice-document-path>{activeDocumentPath}</span>
            ) : (
              t("voiceSecretaryWorkingDocumentPendingShort", {
                defaultValue: "Auto-create on transcript",
              })
            )}
          </span>,
          isDefaultDocument ? (
            <span key="default">
              {t("voiceSecretaryDefaultDocumentBadge", { defaultValue: "Default document" })}
            </span>
          ) : null,
          documentHasUnsavedEdits ? (
            <span key="unsaved" className="font-medium text-amber-700 dark:text-amber-300">
              {t("voiceSecretaryUnsavedEditsBadge", { defaultValue: "Unsaved edits" })}
            </span>
          ) : null,
          documentRemoteChanged ? (
            <span key="remote" className="font-medium text-[var(--color-accent-primary)]">
              {t("voiceSecretaryRemoteChangedBadge", { defaultValue: "Remote update available" })}
            </span>
          ) : null,
        ]
      : [
          <span key="count">
            {t("voiceSecretaryTranscriptCount", {
              count: transcriptCount,
              defaultValue: "{{count}} entries",
            })}
          </span>,
        ];
  const visibleMeta = metaItems.filter(Boolean);
  return (
    <VoiceWorkspaceFrame
      recording={recording}
      processing={processingRows.length > 0}
      level={recordingAudioLevel}
      isDark={isDark}
    >
      {navigation}
      <div
        data-voice-document-header
        className="flex shrink-0 flex-wrap items-start justify-between gap-x-4 gap-y-2 border-b border-[var(--glass-border-subtle)] pb-3"
      >
        <div data-voice-document-heading className="min-w-0 flex-1">
          <h3
            data-voice-document-title
            className="break-words text-lg font-semibold tracking-[-0.01em] text-[var(--color-text-primary)]"
          >
            {documentDisplayTitle}
          </h3>
          <p
            data-voice-document-meta
            className="mt-0.5 flex min-w-0 flex-wrap items-center gap-x-1.5 text-xs leading-5 text-[var(--color-text-muted)]"
          >
            {visibleMeta.map((item, index) => (
              <span key={index} className="inline-flex min-w-0 items-center gap-1.5">
                {index ? <span aria-hidden="true">·</span> : null}
                {item}
              </span>
            ))}
          </p>
        </div>
        <div
          data-voice-document-actions
          className="flex shrink-0 flex-wrap items-center justify-end gap-1"
        >
          <div
            data-voice-document-views
            role="group"
            aria-label={t("voiceSecretaryWorkspaceViewSelector", {
              defaultValue: "Voice Secretary workspace view",
            })}
            className="mr-1 inline-flex items-center gap-0.5"
          >
            {(["document", "transcript"] as VoiceWorkspaceView[]).map((nextView) => {
              const active = view === nextView;
              return (
                <button
                  key={nextView}
                  type="button"
                  className={classNames(
                    "min-h-8 rounded-lg px-2.5 text-xs font-medium transition-colors focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]",
                    active
                      ? "bg-[var(--color-bg-tertiary)] text-[var(--color-text-primary)]"
                      : "text-[var(--color-text-muted)] hover:text-[var(--color-text-primary)]",
                  )}
                  onClick={() => onChangeView(nextView)}
                  aria-pressed={active}
                >
                  {nextView === "document"
                    ? t("voiceSecretaryWorkspaceViewDocument", { defaultValue: "Document" })
                    : t("voiceSecretaryWorkspaceViewTranscript", { defaultValue: "Transcript" })}
                </button>
              );
            })}
          </div>
          {view === "document" && documentRemoteChanged ? (
            <button
              type="button"
              className={actionClassName}
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
                actionClassName,
                "font-semibold text-[var(--color-text-primary)]",
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
            <button
              type="button"
              onClick={onToggleDocumentEditing}
              disabled={documentLoading}
              className={actionClassName}
            >
              {documentEditing
                ? t("voiceSecretaryPreviewDocument", { defaultValue: "Preview" })
                : t("voiceSecretaryEditDocument", { defaultValue: "Edit" })}
            </button>
          ) : null}
          {view === "document" && activeDocumentPath ? (
            <Popover>
              <PopoverTrigger asChild>
                <button
                  type="button"
                  data-voice-document-more
                  className={classNames(actionClassName, "px-1.5")}
                  aria-label={t("voiceSecretaryDocumentMoreActions")}
                  title={t("voiceSecretaryDocumentMoreActions")}
                >
                  <MoreIcon size={15} aria-hidden="true" />
                </button>
              </PopoverTrigger>
              <PopoverContent align="end" sideOffset={6} className="w-52 rounded-xl p-1.5">
                <button
                  type="button"
                  onClick={onQuoteDocument}
                  disabled={documentLoading || documentHasUnsavedEdits}
                  className={menuItemClassName}
                  title={
                    documentHasUnsavedEdits
                      ? t("voiceSecretaryQuoteDocumentSaveFirst", {
                          defaultValue: "Save document edits before quoting it in chat",
                        })
                      : undefined
                  }
                >
                  <MessageSquareQuoteIcon size={15} aria-hidden="true" />
                  {t("voiceSecretaryQuoteDocumentInChat", { defaultValue: "Quote in chat" })}
                </button>
                <button
                  type="button"
                  onClick={onDownloadDocument}
                  disabled={documentLoading}
                  className={menuItemClassName}
                >
                  <DownloadIcon size={15} aria-hidden="true" />
                  {t("voiceSecretaryDownloadDocument", { defaultValue: "Download .md" })}
                </button>
                <button
                  type="button"
                  onClick={onArchiveDocument}
                  disabled={!!actionBusy || documentLoading}
                  className={menuItemClassName}
                >
                  <Archive size={15} aria-hidden="true" />
                  {actionBusy === "archive_doc"
                    ? t("voiceSecretaryArchivingDocument", { defaultValue: "Archiving..." })
                    : t("voiceSecretaryArchiveShort", { defaultValue: "Archive" })}
                </button>
              </PopoverContent>
            </Popover>
          ) : null}
          {view === "transcript" ? (
            <button
              type="button"
              onClick={onClearTranscript}
              disabled={!transcriptCount || recording}
              className={actionClassName}
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
      {statusLine}

      {view === "document" && documentEmpty ? (
        <div
          data-voice-document-empty
          className="voice-document-content flex min-h-0 flex-1 flex-col items-center justify-center px-6 py-10 text-center"
        >
          <FileTextIcon
            size={28}
            aria-hidden="true"
            className="text-[var(--color-text-tertiary)]"
          />
          <p className="mt-3 text-sm font-semibold text-[var(--color-text-primary)]">
            {t("voiceSecretaryDocumentEmptyTitle")}
          </p>
          <p className="mt-1 max-w-md text-sm leading-6 text-[var(--color-text-muted)]">
            {t("voiceSecretaryDocumentEmptyHint")}
          </p>
        </div>
      ) : view === "document" ? (
        <MarkdownDocumentSurface
          bare
          className="voice-document-content mt-3 min-h-0 flex-1 overflow-auto scrollbar-subtle"
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
          isDark={isDark}
          loading={documentLoading}
          loadingLabel={t("voiceSecretaryDocumentLoading", {
            defaultValue: "Loading document content...",
          })}
          minHeightClassName="min-h-[200px] lg:min-h-0"
          onEditValueChange={onEditDocumentChange}
        />
      ) : (
        <div className="voice-document-content mt-3 min-h-0 flex-1 space-y-2 overflow-y-auto scrollbar-subtle pr-1 [scrollbar-gutter:stable]">
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
            <div className="flex h-full min-h-[200px] items-center justify-center px-6 text-center text-sm leading-6 text-[var(--color-text-muted)]">
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
      {footer}
    </VoiceWorkspaceFrame>
  );
}
