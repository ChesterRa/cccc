import type { TFunction } from "i18next";
import { useMemo } from "react";
import { MarkdownDocumentSurface } from "../../../components/document/MarkdownDocumentSurface";
import { classNames } from "../../../utils/classNames";
import type { VoiceTranscriptItem } from "./voiceStreamModel";
import { VoiceTranscriptRecordingIndicator } from "./VoiceTranscriptRecordingIndicator";

export type VoiceWorkspaceView = "document" | "transcript";

type VoiceSecretaryWorkspacePanelProps = {
  activeDocumentPath: string;
  activeDocumentWritePath: string;
  actionBusy: string;
  captureTargetDocumentPath: string;
  documentDisplayTitle: string;
  documentDraft: string;
  documentEditing: boolean;
  documentHasUnsavedEdits: boolean;
  documentRemoteChanged: boolean;
  isDark: boolean;
  recording: boolean;
  recordingAudioLevels: number[];
  t: TFunction;
  transcriptItems: VoiceTranscriptItem[];
  view: VoiceWorkspaceView;
  onChangeView: (view: VoiceWorkspaceView) => void;
  onClearTranscript: () => void;
  onDownloadDocument: () => void;
  onEditDocumentChange: (value: string) => void;
  onLoadLatestDocument: () => void;
  onSaveDocument: () => void;
  onToggleDocumentEditing: () => void;
  formatTime: (value: number) => string;
  formatFullTime: (value: number) => string;
  normalizeTranscriptText: (value: string) => string;
};

export function VoiceSecretaryWorkspacePanel({
  activeDocumentPath,
  activeDocumentWritePath,
  actionBusy,
  captureTargetDocumentPath,
  documentDisplayTitle,
  documentDraft,
  documentEditing,
  documentHasUnsavedEdits,
  documentRemoteChanged,
  isDark,
  recording,
  recordingAudioLevels,
  t,
  transcriptItems,
  view,
  onChangeView,
  onClearTranscript,
  onDownloadDocument,
  onEditDocumentChange,
  onLoadLatestDocument,
  onSaveDocument,
  onToggleDocumentEditing,
  formatTime,
  formatFullTime,
  normalizeTranscriptText,
}: VoiceSecretaryWorkspacePanelProps) {
  const processingRows = useMemo(() => transcriptItems.filter((item) => item.processingPhase === "separating_speakers"), [transcriptItems]);
  const failedRows = useMemo(() => transcriptItems.filter((item) => item.processingPhase === "failed"), [transcriptItems]);
  const transcriptRows = useMemo(() => transcriptItems.filter((item) => item.phase === "final" && !item.processingPhase), [transcriptItems]);
  const transcriptCount = transcriptRows.length;
  return (
    <section
      className={classNames(
        "flex min-h-0 flex-col rounded-[24px] border p-3",
        isDark ? "border-white/10 bg-black/10" : "border-black/[0.06] bg-white/70",
      )}
    >
      <div className="flex shrink-0 flex-wrap items-start justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-1 pb-3">
        <div className="min-w-0 flex-1">
          <div className={classNames("break-words text-xl font-semibold tracking-[-0.02em]", isDark ? "text-slate-100" : "text-gray-900")}>
            {documentDisplayTitle}
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-1.5">
            <div
              className={classNames(
                "inline-flex rounded-full border p-0.5",
                isDark ? "border-white/10 bg-white/[0.04]" : "border-black/10 bg-white",
              )}
              role="group"
              aria-label={t("voiceSecretaryWorkspaceViewSelector", { defaultValue: "Voice Secretary workspace view" })}
            >
              {(["document", "transcript"] as VoiceWorkspaceView[]).map((nextView) => {
                const active = view === nextView;
                return (
                  <button
                    key={nextView}
                    type="button"
                    className={classNames(
                      "rounded-full px-2.5 py-1 text-[10px] font-semibold transition-colors",
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
                      : t("voiceSecretaryWorkspaceViewTranscript", { defaultValue: "Transcript" })}
                  </button>
                );
              })}
            </div>
            <span className={classNames("rounded-full px-2 py-0.5 text-[10px] font-medium", isDark ? "bg-white/10 text-slate-100" : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]")}>
              {view === "transcript"
                ? t("voiceSecretaryTranscriptCount", { count: transcriptCount, defaultValue: "{{count}} entries" })
                : t("voiceSecretaryMarkdownBadge", { defaultValue: "Markdown" })}
            </span>
            {view === "document" ? (
              <span className={classNames("rounded-full px-2 py-0.5 text-[10px] font-medium", activeDocumentPath ? (isDark ? "bg-white/10 text-slate-200" : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]") : (isDark ? "bg-slate-800 text-slate-300" : "bg-gray-100 text-gray-600"))}>
                {activeDocumentPath
                  ? t("voiceSecretaryRepoBackedBadge", { defaultValue: "Repo-backed" })
                  : t("voiceSecretaryWaitingTranscriptBadge", { defaultValue: "Waiting for transcript" })}
              </span>
            ) : null}
            {view === "document" && activeDocumentWritePath && activeDocumentWritePath === captureTargetDocumentPath ? (
              <span className={classNames("rounded-full px-2 py-0.5 text-[10px] font-medium", isDark ? "bg-white/10 text-slate-200" : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]")}>
                {t("voiceSecretaryDefaultDocumentBadge", { defaultValue: "Default document" })}
              </span>
            ) : null}
            {view === "document" && documentHasUnsavedEdits ? (
              <span className={classNames("rounded-full px-2 py-0.5 text-[10px] font-medium", isDark ? "bg-amber-500/10 text-amber-200" : "bg-amber-50 text-amber-700")}>
                {t("voiceSecretaryUnsavedEditsBadge", { defaultValue: "Unsaved edits" })}
              </span>
            ) : null}
            {view === "document" && documentRemoteChanged ? (
              <span className={classNames("rounded-full px-2 py-0.5 text-[10px] font-medium", isDark ? "bg-white/10 text-slate-200" : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]")}>
                {t("voiceSecretaryRemoteChangedBadge", { defaultValue: "Remote update available" })}
              </span>
            ) : null}
            {view === "document" ? (
              <span
                className={classNames(
                  "inline-flex min-w-0 max-w-full items-center gap-1.5 rounded-full px-2 py-0.5 text-[10px] font-medium",
                  isDark ? "bg-black/20 text-slate-300" : "bg-[rgb(245,245,245)] text-gray-600",
                )}
                title={activeDocumentPath || undefined}
              >
                <span className="shrink-0">
                  {activeDocumentPath
                    ? t("voiceSecretaryRepoMarkdownLabel", { defaultValue: "Repo markdown" })
                    : t("voiceSecretaryWorkingDocumentPendingShort", { defaultValue: "Auto-create on transcript" })}
                </span>
                {activeDocumentPath ? (
                  <span className="min-w-0 truncate font-normal text-[var(--color-text-muted)]">
                    {activeDocumentPath}
                  </span>
                ) : null}
              </span>
            ) : null}
          </div>
        </div>
        <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
          {view === "document" && documentRemoteChanged ? (
            <button
              type="button"
              className={classNames(
                "rounded-full border px-2.5 py-1.5 text-[11px] font-semibold transition-colors disabled:opacity-60",
                isDark ? "border-white/10 text-slate-300 hover:bg-white/10" : "border-black/10 text-gray-700 hover:bg-black/5",
              )}
              onClick={onLoadLatestDocument}
              disabled={!activeDocumentPath}
              title={t("voiceSecretaryLoadLatestDocumentHint", {
                defaultValue: "Load the latest document from the daemon. Unsaved local edits in this panel will be replaced.",
              })}
            >
              {t("voiceSecretaryLoadLatestDocument", { defaultValue: "Load latest" })}
            </button>
          ) : null}
          {view === "document" && (documentEditing || documentHasUnsavedEdits) ? (
            <button
              type="button"
              className={classNames(
                "rounded-full border px-2.5 py-1.5 text-[11px] font-semibold transition-colors disabled:opacity-60",
                isDark ? "border-white/10 text-slate-300 hover:bg-white/10" : "border-black/10 text-gray-700 hover:bg-black/5",
              )}
              onClick={onSaveDocument}
              disabled={!!actionBusy}
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
                disabled={!activeDocumentPath}
                className={classNames(
                  "rounded-full border px-2.5 py-1.5 text-[11px] font-semibold transition-colors disabled:opacity-50",
                  isDark ? "border-white/10 text-slate-300 hover:bg-white/10" : "border-black/10 text-gray-700 hover:bg-black/5",
                )}
              >
                {t("voiceSecretaryDownloadDocument", { defaultValue: "Download .md" })}
              </button>
              <button
                type="button"
                onClick={onToggleDocumentEditing}
                className={classNames(
                  "rounded-full border px-2.5 py-1.5 text-[11px] font-semibold transition-colors",
                  isDark ? "border-white/10 text-slate-300 hover:bg-white/10" : "border-black/10 text-gray-700 hover:bg-black/5",
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
                "rounded-full border px-2.5 py-1.5 text-[11px] font-semibold transition-colors disabled:opacity-50",
                isDark ? "border-white/10 text-slate-300 hover:bg-white/10" : "border-black/10 text-gray-700 hover:bg-black/5",
              )}
              title={recording
                ? t("voiceSecretaryClearTranscriptDisabledRecording", { defaultValue: "Stop recording before clearing transcript entries." })
                : t("voiceSecretaryClearTranscriptTitle", { defaultValue: "Clear visible transcript entries for this document." })}
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
          editAriaLabel={t("voiceSecretaryDocumentEditAriaLabel", { defaultValue: "Edit Voice Secretary working document markdown" })}
          editPlaceholder={t("voiceSecretaryDocumentPlaceholder", {
            defaultValue: "Voice Secretary will maintain a markdown working document here as transcript arrives. You can edit it directly.",
          })}
          emptyLabel={t("voiceSecretaryDocumentPreviewEmpty", {
            defaultValue: "Transcript and Voice Secretary edits will appear here.",
          })}
          isDark={isDark}
          minHeightClassName="min-h-[280px] lg:min-h-0"
          onEditValueChange={onEditDocumentChange}
        />
      ) : (
        <div className="mt-3 min-h-0 flex-1 space-y-2 overflow-y-auto scrollbar-subtle pr-1 [scrollbar-gutter:stable]">
          {recording ? (
            <VoiceTranscriptRecordingIndicator
              isDark={isDark}
              label={t("voiceSecretaryTranscriptRecordingIndicator", { defaultValue: "Recording audio. Final transcript appears after Save." })}
              levels={recordingAudioLevels}
            />
          ) : processingRows.length ? (
            <VoiceTranscriptRecordingIndicator
              isDark={isDark}
              label={t("voiceSecretaryTranscriptAnalyzingAudio", { defaultValue: "Analyzing final audio..." })}
              levels={recordingAudioLevels}
            />
          ) : null}
          {!recording && !processingRows.length && failedRows.length ? (
            <div className={classNames(
              "rounded-2xl border px-3 py-2.5 text-sm",
              isDark ? "border-red-300/20 bg-red-300/10 text-red-100" : "border-red-200 bg-red-50 text-red-800",
            )}>
              {normalizeTranscriptText(failedRows[0]?.text || t("voiceSecretaryTranscriptFinalFailed", {
                defaultValue: "Final audio analysis failed.",
              }))}
            </div>
          ) : null}
          {transcriptRows.length ? transcriptRows.map((item) => {
            const itemText = normalizeTranscriptText(item.text);
            const timeLabel = formatTime(item.updatedAt);
            const fullTimeLabel = formatFullTime(item.updatedAt);
            const sourceLabel = String(item.sourceLabel || "").trim();
            const sourceDetail = String(item.sourceDetail || "").trim();
            const speakerLabel = String(item.speakerLabel || "").trim();
            return (
              <div
                key={item.id}
                className={classNames(
                  "rounded-2xl border px-3 py-2.5",
                  isDark ? "border-white/10 bg-white/[0.04]" : "border-black/[0.08] bg-white",
                )}
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                    <span className={classNames(
                      "rounded-full px-2 py-0.5 text-[10px] font-semibold",
                      isDark ? "bg-white/10 text-slate-200" : "bg-[rgb(245,245,245)] text-gray-700",
                    )}>
                      {t("voiceSecretaryTranscriptHeard", { defaultValue: "Transcript" })}
                    </span>
                    {sourceLabel ? (
                      <span
                        className={classNames(
                          "rounded-full px-2 py-0.5 text-[10px] font-semibold",
                          isDark ? "bg-emerald-300/12 text-emerald-100" : "bg-emerald-50 text-emerald-800",
                        )}
                        title={sourceDetail || sourceLabel}
                      >
                        {sourceLabel}
                      </span>
                    ) : null}
                    {speakerLabel ? (
                      <span
                        className={classNames(
                          "rounded-full px-2 py-0.5 text-[10px] font-semibold",
                          isDark ? "bg-sky-300/12 text-sky-100" : "bg-sky-50 text-sky-800",
                        )}
                      >
                        {speakerLabel}
                      </span>
                    ) : null}
                  </div>
                  {timeLabel ? (
                    <time
                      className="shrink-0 text-[10px] tabular-nums text-[var(--color-text-muted)]"
                      dateTime={new Date(item.updatedAt).toISOString()}
                      title={fullTimeLabel}
                    >
                      {timeLabel}
                    </time>
                  ) : null}
                </div>
                {itemText ? (
                  <div className={classNames(
                    "mt-2 whitespace-pre-wrap break-words text-sm leading-6",
                    isDark ? "text-slate-100" : "text-gray-900",
                  )}>
                    {itemText}
                  </div>
                ) : null}
                {sourceDetail ? (
                  <div className="mt-1 truncate text-[10px] text-[var(--color-text-muted)]">
                    {sourceDetail}
                  </div>
                ) : null}
              </div>
            );
          }) : !recording && !processingRows.length && !failedRows.length ? (
            <div className="flex h-full min-h-[280px] items-center justify-center rounded-2xl border border-dashed border-[var(--glass-border-subtle)] px-4 text-center text-sm text-[var(--color-text-muted)]">
              {activeDocumentPath
                ? t("voiceSecretaryTranscriptEmpty", { defaultValue: "Document-mode transcript for this document will appear here." })
                : t("voiceSecretaryTranscriptNeedsDocument", { defaultValue: "Choose or create a document to see its transcript." })}
            </div>
          ) : null}
        </div>
      )}
    </section>
  );
}
