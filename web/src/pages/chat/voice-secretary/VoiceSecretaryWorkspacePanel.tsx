import type { TFunction } from "i18next";
import { useMemo } from "react";
import { MarkdownDocumentSurface } from "../../../components/document/MarkdownDocumentSurface";
import { classNames } from "../../../utils/classNames";
import type { VoiceSecretaryCaptureMode } from "../VoiceSecretaryComposerControl";
import { StreamTextAnimate } from "./VoiceSecretaryStreamText";
import {
  buildSpeakerConversationItems,
  filterVoiceStreamItemsForDocument,
  type VoiceDiarizationStreamItem,
  type VoiceStreamItem,
} from "./voiceStreamModel";

export type VoiceWorkspaceView = "document" | "stream";

type VoiceSecretaryWorkspacePanelProps = {
  activeDocumentPath: string;
  activeDocumentWritePath: string;
  actionBusy: string;
  canClearActivity: boolean;
  captureTargetDocumentPath: string;
  documentDisplayTitle: string;
  documentDraft: string;
  documentEditing: boolean;
  documentHasUnsavedEdits: boolean;
  documentRemoteChanged: boolean;
  isDark: boolean;
  latestDiarization: VoiceDiarizationStreamItem | null;
  liveTranscriptId: string;
  recording: boolean;
  t: TFunction;
  voiceStreamItems: VoiceStreamItem[];
  view: VoiceWorkspaceView;
  onChangeView: (view: VoiceWorkspaceView) => void;
  onClearActivity: () => void;
  onDownloadDocument: () => void;
  onEditDocumentChange: (value: string) => void;
  onLoadLatestDocument: () => void;
  onSaveDocument: () => void;
  onToggleDocumentEditing: () => void;
  voiceModeLabel: (mode: VoiceSecretaryCaptureMode) => string;
  formatTime: (value: number) => string;
  formatFullTime: (value: number) => string;
  formatOffset: (value: number) => string;
  normalizeTranscriptText: (value: string) => string;
};

const EMPTY_SPEAKER_SEGMENTS: NonNullable<VoiceDiarizationStreamItem["segments"]> = [];
const EMPTY_SPEAKER_TRANSCRIPT_SEGMENTS: NonNullable<VoiceDiarizationStreamItem["speakerTranscriptSegments"]> = [];

function speakerBubbleClass(speakerLabel: string, isDark: boolean): string {
  const tones = isDark
    ? [
      "border-cyan-300/20 bg-cyan-400/10",
      "border-emerald-300/20 bg-emerald-400/10",
      "border-amber-300/20 bg-amber-400/10",
      "border-fuchsia-300/20 bg-fuchsia-400/10",
    ]
    : [
      "border-cyan-200 bg-cyan-50/70",
      "border-emerald-200 bg-emerald-50/70",
      "border-amber-200 bg-amber-50/70",
      "border-fuchsia-200 bg-fuchsia-50/70",
    ];
  if (!speakerLabel) return isDark ? "border-white/10 bg-white/[0.04]" : "border-black/[0.08] bg-white";
  const hash = Array.from(speakerLabel).reduce((total, char) => total + char.charCodeAt(0), 0);
  return tones[hash % tones.length];
}

function speakerBadgeClass(speakerLabel: string, isLive: boolean, isDark: boolean): string {
  if (speakerLabel) return isDark ? "bg-white/10 text-slate-100" : "bg-white/80 text-gray-900";
  if (isLive) return isDark ? "bg-cyan-300/15 text-cyan-100" : "bg-white text-cyan-800";
  return isDark ? "bg-white/10 text-slate-200" : "bg-[rgb(245,245,245)] text-gray-700";
}

function SpeakerProcessingGlyph({ isDark }: { isDark: boolean }) {
  return (
    <span className="relative inline-flex size-3.5 shrink-0 items-center justify-center" aria-hidden="true">
      <span className={classNames(
        "absolute inset-0 rounded-full border",
        isDark ? "border-white/25" : "border-black/20",
      )} />
      <span className={classNames(
        "absolute inset-0 rounded-full border border-transparent border-t-current animate-spin",
        isDark ? "text-slate-100" : "text-[rgb(35,36,37)]",
      )} />
      <span className={classNames(
        "size-1.5 rounded-full animate-pulse",
        isDark ? "bg-slate-100" : "bg-[rgb(35,36,37)]",
      )} />
    </span>
  );
}

function SpeakerProcessingDots({ isDark }: { isDark: boolean }) {
  return (
    <span className="inline-flex items-end gap-0.5" aria-hidden="true">
      {[0, 1, 2].map((index) => (
        <span
          key={index}
          className={classNames(
            "block h-1 w-1 rounded-full animate-bounce",
            isDark ? "bg-slate-200/80" : "bg-[rgb(35,36,37)]/70",
          )}
          style={{ animationDelay: `${index * 120}ms` }}
        />
      ))}
    </span>
  );
}

function SpeakerProcessingBadge({ isDark, label }: { isDark: boolean; label: string }) {
  return (
    <span className={classNames(
      "inline-flex items-center gap-1.5 rounded-full border px-2 py-0.5 text-[10px] font-semibold",
      isDark
        ? "border-white/15 bg-white/[0.06] text-slate-100 shadow-[0_0_18px_rgba(255,255,255,0.05)]"
        : "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] shadow-[0_10px_28px_rgba(0,0,0,0.04)]",
    )}>
      <SpeakerProcessingGlyph isDark={isDark} />
      <span>{label}</span>
      <SpeakerProcessingDots isDark={isDark} />
    </span>
  );
}

function SpeakerProcessingCallout({ isDark, label }: { isDark: boolean; label: string }) {
  return (
    <div className={classNames(
      "relative overflow-hidden rounded-2xl border px-3 py-2.5",
      isDark
        ? "border-white/12 bg-white/[0.045] text-slate-100"
        : "border-black/[0.08] bg-[rgb(248,248,248)] text-[rgb(35,36,37)]",
    )}>
      <div className={classNames(
        "pointer-events-none absolute inset-y-0 left-0 w-20 animate-pulse bg-gradient-to-r to-transparent",
        isDark ? "from-white/10" : "from-black/[0.045]",
      )} />
      <div className="relative flex items-center gap-2">
        <SpeakerProcessingGlyph isDark={isDark} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5 text-xs font-semibold">
            <span>{label}</span>
            <SpeakerProcessingDots isDark={isDark} />
          </div>
          <div className={classNames("mt-2 flex h-3 items-end gap-1", isDark ? "text-slate-200" : "text-[rgb(35,36,37)]")}>
            {[0, 1, 2, 3, 4].map((index) => (
              <span
                key={index}
                className={classNames(
                  "block w-1 rounded-full animate-pulse",
                  isDark ? "bg-slate-200/70" : "bg-[rgb(35,36,37)]/55",
                )}
                style={{
                  height: `${5 + (index % 3) * 3}px`,
                  animationDelay: `${index * 110}ms`,
                }}
              />
            ))}
            <div className={classNames(
              "ml-1 h-px min-w-0 flex-1 animate-pulse",
              isDark ? "bg-white/15" : "bg-black/10",
            )} />
          </div>
        </div>
      </div>
    </div>
  );
}

export function VoiceSecretaryWorkspacePanel({
  activeDocumentPath,
  activeDocumentWritePath,
  actionBusy,
  canClearActivity,
  captureTargetDocumentPath,
  documentDisplayTitle,
  documentDraft,
  documentEditing,
  documentHasUnsavedEdits,
  documentRemoteChanged,
  isDark,
  latestDiarization,
  liveTranscriptId,
  recording,
  t,
  voiceStreamItems,
  view,
  onChangeView,
  onClearActivity,
  onDownloadDocument,
  onEditDocumentChange,
  onLoadLatestDocument,
  onSaveDocument,
  onToggleDocumentEditing,
  voiceModeLabel,
  formatTime,
  formatFullTime,
  formatOffset,
  normalizeTranscriptText,
}: VoiceSecretaryWorkspacePanelProps) {
  const streamDocumentPath = String(activeDocumentWritePath || activeDocumentPath || captureTargetDocumentPath || "").trim();
  const visibleVoiceStreamItems = useMemo(
    () => filterVoiceStreamItemsForDocument(voiceStreamItems, streamDocumentPath),
    [streamDocumentPath, voiceStreamItems],
  );
  const speakerSegments = latestDiarization?.segments || EMPTY_SPEAKER_SEGMENTS;
  const speakerTranscriptSegments = latestDiarization?.speakerTranscriptSegments || EMPTY_SPEAKER_TRANSCRIPT_SEGMENTS;
  const conversationItems = useMemo(() => {
    if (view !== "stream") return [];
    return buildSpeakerConversationItems(
      visibleVoiceStreamItems,
      speakerSegments,
      speakerTranscriptSegments,
    );
  }, [speakerSegments, speakerTranscriptSegments, view, visibleVoiceStreamItems]);
  const speakerProcessing = view === "stream" && latestDiarization?.status === "working";
  const speakerProcessingLabel = t("voiceSecretaryDiarizationWorkingShort", { defaultValue: "Processing speakers" });
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
            {view === "stream"
              ? t("voiceSecretaryVoiceStreamTitle", { defaultValue: "Voice stream" })
              : documentDisplayTitle}
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
              {(["document", "stream"] as VoiceWorkspaceView[]).map((nextView) => {
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
                      : t("voiceSecretaryWorkspaceViewStream", { defaultValue: "Voice stream" })}
                  </button>
                );
              })}
            </div>
            <span className={classNames("rounded-full px-2 py-0.5 text-[10px] font-medium", isDark ? "bg-white/10 text-slate-100" : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]")}>
              {view === "stream"
                ? t("voiceSecretaryVoiceStreamCount", { count: conversationItems.length, defaultValue: "{{count}} entries" })
                : t("voiceSecretaryMarkdownBadge", { defaultValue: "Markdown" })}
            </span>
            {view === "stream" ? (
              speakerProcessing ? (
                <SpeakerProcessingBadge isDark={isDark} label={speakerProcessingLabel} />
              ) : (
                <span className={classNames("rounded-full px-2 py-0.5 text-[10px] font-medium", isDark ? "bg-white/10 text-slate-200" : "bg-[rgb(245,245,245)] text-gray-700")}>
                  {latestDiarization?.status === "ready"
                    ? latestDiarization.speakerCount > 0
                      ? t("voiceSecretaryDiarizationReadyShort", { count: latestDiarization.speakerCount, defaultValue: "{{count}} speakers ready" })
                      : t("voiceSecretaryDiarizationNoSpeakersShort", { defaultValue: "No speakers detected" })
                    : latestDiarization?.status === "failed"
                      ? t("voiceSecretaryDiarizationFailedShort", { defaultValue: "Speaker error" })
                      : t("voiceSecretaryDiarizationFinalOnlyShort", { defaultValue: "Speakers after stop" })}
                </span>
              )
            ) : null}
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
          {view === "stream" && canClearActivity ? (
            <button
              type="button"
              className={classNames(
                "rounded-full border px-2.5 py-1.5 text-[11px] font-semibold transition-colors disabled:opacity-60",
                isDark ? "border-white/10 text-slate-300 hover:bg-white/10" : "border-black/10 text-gray-700 hover:bg-black/5",
              )}
              onClick={onClearActivity}
              disabled={actionBusy === "clear_ask"}
            >
              {actionBusy === "clear_ask"
                ? t("voiceSecretaryClearingRequests", { defaultValue: "Clearing..." })
                : t("voiceSecretaryClearRequests", { defaultValue: "Clear" })}
            </button>
          ) : null}
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
          ) : null}
          {view === "document" ? (
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
        <div className="mt-3 min-h-0 flex-1 space-y-2 overflow-y-auto scrollbar-hide pr-1 [scrollbar-gutter:stable]">
          {!latestDiarization && recording ? (
            <div className={classNames(
              "rounded-2xl border border-dashed px-3 py-2.5 text-xs leading-5",
              isDark ? "border-white/10 bg-white/[0.03] text-slate-300" : "border-black/10 bg-white text-gray-600",
            )}>
              {t("voiceSecretaryDiarizationWaitingForAudio", { defaultValue: "Speaker turns will appear after recording stops." })}
            </div>
          ) : null}
          {speakerProcessing ? (
            <SpeakerProcessingCallout isDark={isDark} label={speakerProcessingLabel} />
          ) : null}
          {conversationItems.length ? conversationItems.map((item) => {
            const timeLabel = formatTime(item.updatedAt);
            const fullTimeLabel = formatFullTime(item.updatedAt);
            const itemText = normalizeTranscriptText(item.text);
            const isLive = recording && liveTranscriptId === item.sourceItemId;
            const speakerLabel = item.speakerLabel;
            const offsetLabel = Number.isFinite(Number(item.startMs)) && Number.isFinite(Number(item.endMs)) && Number(item.endMs) > Number(item.startMs)
              ? `${formatOffset(Number(item.startMs))}-${formatOffset(Number(item.endMs))}`
              : "";
            return (
              <div
                key={item.id}
                className={classNames(
                  "rounded-2xl border px-3 py-2.5",
                  isLive ? (isDark ? "border-cyan-300/20 bg-cyan-400/10" : "border-cyan-200 bg-cyan-50/70") : speakerBubbleClass(speakerLabel, isDark),
                )}
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                    <span className={classNames(
                      "rounded-full px-2 py-0.5 text-[10px] font-semibold",
                      speakerBadgeClass(speakerLabel, isLive, isDark),
                    )}>
                      {speakerLabel || (isLive
                        ? t("voiceSecretaryTranscriptLive", { defaultValue: "Live" })
                        : t("voiceSecretaryTranscriptHeard", { defaultValue: "Heard" }))}
                    </span>
                    {offsetLabel ? (
                      <span className="text-[10px] text-[var(--color-text-muted)]">{offsetLabel}</span>
                    ) : null}
                  </div>
                  <span className="flex min-w-0 items-center gap-1.5 text-[10px] text-[var(--color-text-muted)]">
                    <span className="min-w-0 truncate">{voiceModeLabel(item.mode)}</span>
                    {timeLabel ? (
                      <time
                        className="shrink-0 tabular-nums"
                        dateTime={new Date(item.updatedAt).toISOString()}
                        title={fullTimeLabel}
                      >
                        {timeLabel}
                      </time>
                    ) : null}
                  </span>
                </div>
                {itemText ? (
                  <div className={classNames(
                    "mt-2 whitespace-pre-wrap break-words text-sm leading-6",
                    isDark ? "text-slate-100" : "text-gray-900",
                  )}>
                    <StreamTextAnimate text={itemText} />
                  </div>
                ) : null}
                {item.documentTitle || item.documentPath ? (
                  <div className="mt-2 truncate text-[11px] text-[var(--color-text-muted)]">
                    {item.documentTitle || item.documentPath}
                  </div>
                ) : null}
              </div>
            );
          }) : (
            <div className="flex h-full min-h-[280px] items-center justify-center rounded-2xl border border-dashed border-[var(--glass-border-subtle)] px-4 text-center text-sm text-[var(--color-text-muted)]">
              {recording
                ? t("voiceSecretaryVoiceStreamListening", { defaultValue: "Listening... speech will appear here as it is recognized." })
                : t("voiceSecretaryVoiceStreamEmpty", { defaultValue: "Start recording to see the full voice stream here." })}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
