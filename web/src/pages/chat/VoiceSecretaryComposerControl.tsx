import "./voice-secretary/voiceWorkspaceMobile.css";
import {
  useVoiceAudioStore,
  voiceAudioSnapshot,
  type VoiceAudioPreferences,
} from "../../stores/useVoiceAudioStore";
import { useVoiceAudioDevices } from "../../features/voice/useVoiceAudioDevices";
import { secretaryReadinessIssue } from "../../features/voice/secretaryReadiness";
import type { SecretaryReadiness } from "../../services/api/voiceSecretary";
import { VISUAL_VIEWPORT_BOX } from "../../hooks/useViewportHeight";
import { VoiceMobileMenu } from "./voice-secretary/VoiceMobileMenu";
import { VoicePromptAutoRefineOption } from "./voice-secretary/VoicePromptAutoRefineOption";
import type { VoiceSecretaryCaptureMode } from "./voice-secretary/voiceSecretaryTypes";
import { VoiceComposerStatus } from "./voice-secretary/VoiceComposerStatus";
import { VoicePromptDraftReview } from "./voice-secretary/VoicePromptDraftReview";
import { useVoiceRequestStatus } from "./voice-secretary/useVoiceRequestStatus";
import {
  currentVoiceAskTask,
  readVoiceAskNotice,
  saveVoiceAskNotice,
  voiceAskReplyIsCurrent,
  type VoiceAskNotice,
} from "./voice-secretary/voiceAskStatus";
import { queueVoiceSocketError } from "./voice-secretary/voiceSocketError";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type {
  AssistantVoiceAskFeedback,
  AssistantVoiceDocument,
  AssistantVoicePromptDraft,
  AssistantStateResult,
  AssistantVoiceTranscriptSegmentResult,
  BuiltinAssistant,
  SecretaryTaskSummary,
  VoiceDocumentMessageRef,
} from "../../types";
import { classNames } from "../../utils/classNames";
import {
  ChevronDownIcon,
  ChevronLeftIcon,
  CloseIcon,
  CopyIcon,
  MaximizeIcon,
  MicrophoneIcon,
  SettingsIcon,
  SparklesIcon,
  StopIcon,
  TerminalIcon,
} from "../../components/Icons";
import { LazyMarkdownRenderer } from "../../components/LazyMarkdownRenderer";
import { Popover, PopoverContent, PopoverTrigger } from "../../components/ui/popover";
import {
  ackVoiceAssistantPromptDraft,
  appendVoiceAssistantInput,
  appendVoiceAssistantTranscriptSegment,
  cancelSecretaryTask,
  clearVoiceAssistantAskRequests,
  fetchLatestVoiceAssistantMeetingSession,
  fetchVoiceAssistantMeetingSession,
  fetchVoiceAssistantDocumentContent,
  fetchVoiceAssistantStatus,
  fetchVoiceAssistantWorkspace,
  retryVoiceAssistantTranscriptPersistence,
  saveVoiceAssistantDocument,
  sendVoiceAssistantDocumentInstruction,
  updateVoiceAssistantRecordingLease,
  withAuthToken,
} from "../../services/api";
import { useUIStore, useModalStore, useGroupStore } from "../../stores";
import { getChatSession } from "../../stores/useUIStore";
import { useModalA11y } from "../../hooks/useModalA11y";
import { AnimatedShinyText } from "../../registry/magicui/animated-shiny-text";
import { copyTextToClipboard } from "../../utils/copy";
import { downloadVoiceDocument } from "./voice-secretary/downloadVoiceDocument";
import { VoiceDocumentLibrary as VoiceSecretaryDocumentListPanel } from "./voice-secretary/VoiceDocumentLibrary";
import { VoiceSecretaryWorkspacePanel } from "./voice-secretary/VoiceSecretaryWorkspacePanel";
import { SecretaryTasks } from "./voice-secretary/SecretaryTasks";
import { useSecretaryTasks } from "./voice-secretary/useSecretaryTasks";
import { VoiceAskThread } from "./voice-secretary/VoiceAskThread";
import { VoicePanelInputBar } from "./voice-secretary/VoicePanelInputBar";
import { secretaryTaskRunning } from "./voice-secretary/secretaryTaskLineModel";
import { resolveVoiceComposerActivity } from "./voice-secretary/voiceComposerActivity";
import { useVoiceCaptureTargetDocumentSelection } from "./voice-secretary/useVoiceCaptureTargetDocumentSelection";
import {
  useVoiceDocumentArchive,
  type VoiceSecretaryAction,
} from "./voice-secretary/useVoiceDocumentArchive";
import {
  clearRemovedVoiceDocumentReferences,
  useVoiceDocumentLedgerProjection,
  useVoiceDocumentReferenceCleanup,
} from "./voice-secretary/voiceDocumentReferenceLifecycle";
import { useVoiceAudioLevelMeter } from "./voice-secretary/useVoiceAudioLevelMeter";
import {
  BrowserSpeechRecoveryBudget,
  isQuietBrowserSpeechError,
  shouldScheduleBrowserSpeechErrorRestart,
} from "./voice-secretary/browserSpeechRecoveryModel";
import {
  shouldCloseVoiceCaptureSocket,
  voiceCaptureStopAction,
} from "./voice-secretary/voiceCaptureStopModel";
import {
  claimVoiceCaptureLock,
  createVoiceCaptureOwnerId,
  createVoiceRecordingSessionId,
  openVoiceCaptureChannel,
  refreshVoiceCaptureLock,
  releaseVoiceCaptureLock,
  type VoiceCaptureChannelMessage,
} from "./voice-secretary/voiceCaptureLock";
import {
  resolveVoiceCaptureTarget,
  voicePanelViewForComposerMode,
  type VoiceCaptureTarget,
  type VoicePanelView,
} from "./voice-secretary/voiceCaptureTarget";
import type {
  BrowserAudioSupportIssue,
  BrowserSpeechRecognition,
  BrowserSpeechSupportIssue,
  VoiceRecordingStopReason,
} from "./voice-secretary/voiceBrowserSpeechTypes";
import {
  abortBrowserSpeechRecognition,
  getBrowserAudioSupportIssue,
  getBrowserMicrophoneSupportIssue,
  getBrowserSpeechRecognitionConstructor,
  getBrowserSpeechSupportIssue,
  mediaRecorderSupported,
  mediaStreamHasLiveAudio,
  resolveBrowserSpeechLanguage,
  stopMediaStream,
} from "./voice-secretary/voiceBrowserSpeechSupport";
import { documentFinalAsrDisposition } from "./voice-secretary/voiceFinalAsrPolicy";
import { shouldSettleLiveVoiceActivityStream } from "./voice-secretary/voiceActivityStreamModel";
import {
  assistantVoiceTimestampMs,
  askFeedbackDisplayText,
  askFeedbackStatusKey,
  compactVoiceTranscriptSummaryText,
  displayAskFeedbackStatus,
  findVoiceDocument,
  formatVoiceActivityFullTimeMs,
  formatVoiceActivityTimeMs,
  hasFinalAskReply,
  hashComposerSnapshot,
  isLowValueBrowserSpeechFragment,
  mergeTranscriptChunks,
  nextUncommittedServiceTranscriptText,
  normalizeBrowserTranscriptChunk,
  normalizeVoiceRecognitionLanguageForBackend,
  numberFromUnknown,
  promptDraftApplyMode,
  recordFromUnknown,
  resolveVoiceDocumentPath,
  visibleVoiceDocuments,
  voiceDocumentKey,
  voiceDocumentMatches,
  voiceDocumentPath,
  voiceLanguageOptionValues,
  voiceReplyDismissKey,
  voicePromptRequestOwnership,
  voiceTranscriptItemsFromMeetingSession,
} from "./voice-secretary/voiceComposerUtils";
import {
  resolveAutoOpenVoiceReplyBubbleRequestId,
  trackActiveVoiceReplyRequests,
} from "./voice-secretary/voiceReplyBubbleModel";
import {
  isMeaningfulVoiceDispatchText,
  voiceServiceStopDispatchKind,
} from "./voice-secretary/voiceServiceStopDispatch";
import {
  beginVoiceAssistantRefresh,
  resetVoiceAssistantVisibleLoading,
  shouldApplyVoiceAssistantRefresh,
  shouldFinishVoiceAssistantVisibleLoading,
  type VoiceAssistantLoadingOwnership,
} from "./voice-secretary/voiceAssistantLoadingOwnership";
import {
  appendFinalVoiceTranscriptItem,
  annotateVoiceTranscriptItemsWithSpeakers,
  createVoiceTranscriptItem,
  createVoiceTranscriptPreview,
  filterVoiceTranscriptItemsForDocument,
  mergeVoiceTranscriptItems,
  replaceVoiceTranscriptProcessingItem,
  replaceVoiceTranscriptSessionItems,
  type VoiceTranscriptItem,
  type VoiceTranscriptPreview,
  type VoiceTranscriptPreviewPhase,
  upsertLiveVoiceTranscriptItem,
  voiceTranscriptPreviewBelongsToGroup,
} from "./voice-secretary/voiceStreamModel";
import { resolveVoiceServiceReadiness } from "./voice-secretary/voiceServiceReadiness";
import {
  voiceRecordingLeaseConflictFromDetails,
  voiceRecordingLeaseIsDefinitelyLost,
  type VoiceRecordingLeaseConflict,
} from "./voice-secretary/voiceRecordingLease";
import {
  createVoiceRecordingSessionScope,
  voiceRecordingCaptureMode,
  voiceRecordingDispatchTarget,
  voiceRecordingTargetDocumentPath,
  voiceRecordingTargetGroupId,
  type VoiceRecordingSessionScope,
} from "./voice-secretary/voiceRecordingSessionScope";
import { routeVoiceTextToComposerGroup } from "./voice-secretary/voiceComposerDraftRouting";
import { buildVoiceDocumentMessageRef } from "../../utils/voiceDocumentRefs";
import { createVoiceRequestDispatchGate } from "./voice-secretary/voiceRequestDispatchGate";
import * as voicePcmTransport from "./voice-secretary/voicePcmBackpressure";
import { Pcm16Resampler } from "./voice-secretary/voicePcmResampler";
import {
  getUserMediaWithTimeout,
  VoiceAudioCaptureTimeoutError,
} from "./voice-secretary/voiceAudioCapture";
import {
  voiceCaptureDispatchTarget,
  voiceCaptureTransportMode,
} from "./voice-secretary/voiceDictationRoute";
import {
  createDocumentContentLoadTracker,
  documentContentLoadingMatches,
  documentNeedsContentLoad,
} from "./voice-secretary/documentContentLoad";
export type VoiceSecretaryComposerControlProps = {
  isDark: boolean;
  selectedGroupId: string;
  busy: string;
  buttonClassName?: string;
  buttonSizePx?: number;
  disabled?: boolean;
  variant?: "button" | "assistantRow";
  captureMode?: VoiceSecretaryCaptureMode;
  onCaptureModeChange?: (mode: VoiceSecretaryCaptureMode) => void;
  onQuoteDocument?: (ref: VoiceDocumentMessageRef) => void;
  composerText?: string;
  composerContext?: Record<string, unknown>;
  onPromptDraft?: (text: string, opts?: { mode?: "replace" | "append" }) => void;
  onFocusComposer?: () => void;
  initiallyOpen?: boolean;
  statusPortalTarget?: HTMLElement | null;
};
export type { VoiceSecretaryCaptureMode } from "./voice-secretary/voiceSecretaryTypes";
type WorkspaceRefreshOptions = {
  quiet?: boolean;
  passive?: boolean;
  forceActiveDocumentContent?: boolean;
  refreshChangedActiveDocumentContent?: boolean;
};
type WorkspaceRefreshRead = {
  groupId: string;
  dataSeq: number;
  queued: WorkspaceRefreshOptions | null;
};
const ASK_THREAD_EXCLUDED_TARGETS = new Set(["document", "composer"]);
const VOICE_RECORDING_LEASE_TTL_SECONDS = 30;
const BROWSER_DEFAULT_MIC_LABEL = "browser_default";
const SERVICE_DEFAULT_MIC_LABEL = "service_default";
const BROWSER_SPEECH_MAX_WINDOW_FALLBACK_MS = 300_000;
const BROWSER_SPEECH_MIN_MAX_WINDOW_MS = 10_000;
const BROWSER_SPEECH_RESTART_BASE_MS = 500;
const BROWSER_SPEECH_RESTART_MAX_MS = 8000;
const BROWSER_SPEECH_ERROR_RESTART_FALLBACK_MS = 900;
const BROWSER_SPEECH_RECOVERABLE_ERRORS = new Set([
  "no-speech",
  "aborted",
  "network",
  "audio-capture",
]);
const BROWSER_SPEECH_FATAL_ERRORS = new Set(["not-allowed", "service-not-allowed"]);
const VOICE_DOCUMENT_METADATA_POLL_MS = 30_000;
const VOICE_LIVE_TRANSCRIPT_VISIBLE_MS = 60_000;
const TWO_LINE_STATUS_STYLE = {
  display: "-webkit-box",
  WebkitLineClamp: 2,
  WebkitBoxOrient: "vertical",
  overflow: "hidden",
} as const;

function voiceDocumentRevisionKey(document: AssistantVoiceDocument | null | undefined): string {
  if (!document) return "";
  return [
    String(document.content_sha256 || "").trim(),
    String(document.revision_count ?? "").trim(),
    String(document.updated_at || "").trim(),
    String(document.content_chars ?? "").trim(),
  ].join("|");
}

function browserSpeechRestartDelayMs(transientErrorCount: number): number {
  const count = Math.max(1, transientErrorCount);
  return Math.min(BROWSER_SPEECH_RESTART_MAX_MS, BROWSER_SPEECH_RESTART_BASE_MS * count);
}

export function VoiceSecretaryComposerControl({
  isDark,
  selectedGroupId,
  busy,
  buttonClassName = "",
  buttonSizePx = 44,
  disabled,
  variant = "button",
  statusPortalTarget,
  captureMode: composerCaptureMode = "prompt",
  onCaptureModeChange,
  onQuoteDocument,
  composerText = "",
  composerContext = {},
  onPromptDraft,
  onFocusComposer,
  initiallyOpen = false,
}: VoiceSecretaryComposerControlProps) {
  const { t } = useTranslation("chat");
  const showError = useUIStore((state) => state.showError);
  const showNotice = useUIStore((state) => state.showNotice);
  const isSmallScreen = useUIStore((state) => state.isSmallScreen);
  const workspaceScrollRef = useRef<HTMLDivElement | null>(null);
  const [mobileDocumentListOpen, setMobileDocumentListOpen] = useState(false);
  const panelInputRef = useRef<HTMLDivElement | null>(null);
  const refreshSeq = useRef(0);
  const workspaceRefreshRef = useRef<WorkspaceRefreshRead | null>(null);
  const invalidateWorkspaceRefresh = useCallback(() => {
    refreshSeq.current++;
    workspaceRefreshRef.current = null;
  }, []);
  const visibleLoadSeqRef = useRef(0);
  const recognitionRef = useRef<BrowserSpeechRecognition | null>(null);
  const mediaRecorderRef = useRef<MediaRecorder | null>(null);
  const mediaStreamRef = useRef<MediaStream | null>(null);
  const mediaChunksRef = useRef<Blob[]>([]);
  const serviceAudioContextRef = useRef<AudioContext | null>(null);
  const serviceAudioProcessorRef = useRef<ScriptProcessorNode | null>(null);
  const serviceAudioSourceRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const serviceAudioWsRef = useRef<WebSocket | null>(null);
  const serviceAudioExpectedCloseRunIdRef = useRef(0);
  const serviceAudioPendingPcmRef = useRef<Uint8Array[]>([]);
  const serviceAudioResamplerRef = useRef<Pcm16Resampler | null>(null);
  const serviceAudioSeqRef = useRef(0);
  const serviceFinalTranscriptRef = useRef("");
  const serviceLatestPartialTranscriptRef = useRef("");
  const serviceCommittedTranscriptRef = useRef("");
  const serviceDocumentCommittedTranscriptRef = useRef("");
  const serviceAsrBackendRef = useRef("assistant_service_local_asr");
  const serviceFinalAsrTextRef = useRef("");
  const serviceAudioDurationMsRef = useRef(0);
  const serviceCommittedEndMsRef = useRef(0);
  const serviceDocumentCommittedEndMsRef = useRef(0);
  const serviceFinalSpeakerSegmentsRef = useRef<Record<string, unknown>[]>([]);
  const serviceProvisionalSpeakerSegmentsRef = useRef<Record<string, unknown>[]>([]);
  const servicePartialCommitTimerRef = useRef<number | null>(null);
  const serviceDocumentCheckpointTimerRef = useRef<number | null>(null);
  const voiceCaptureOwnerIdRef = useRef(createVoiceCaptureOwnerId());
  const voiceRecordingSessionIdRef = useRef("");
  const voiceRecordingSessionScopeRef = useRef<VoiceRecordingSessionScope | null>(null);
  const voiceRecordingLeaseGroupIdRef = useRef("");
  const voiceRecordingLeaseIdRef = useRef("");
  const voiceRecordingLeaseAcquiredRef = useRef(false);
  const recordingRunIdRef = useRef(0);
  const recordingStartingRef = useRef(false);
  const recordingStoppingRef = useRef(false);
  const browserSpeechFinalizingRunIdRef = useRef(0);
  const pendingDiarizationSessionsRef = useRef<Set<string>>(new Set());
  const recordingRef = useRef(false);
  const lastRecordingStopReasonRef = useRef<VoiceRecordingStopReason | null>(null);
  const transcriptFlushTimerRef = useRef<number | null>(null);
  const transcriptMaxFlushTimerRef = useRef<number | null>(null);
  const transcriptSegmentSeqRef = useRef(0);
  const browserFinalTranscriptBufferRef = useRef("");
  const browserSpeechReceivedFinalRef = useRef(false);
  const browserSpeechHadErrorRef = useRef(false);
  const browserSpeechStopRequestedRef = useRef(false);
  const browserSpeechRestartTimerRef = useRef<number | null>(null);
  const browserSpeechStopFinalizeTimerRef = useRef<number | null>(null);
  const browserSpeechMediaCleanupRef = useRef<(() => void) | null>(null);
  const browserSpeechRecoveryRef = useRef(new BrowserSpeechRecoveryBudget());
  const pendingPromptRequestIdRef = useRef("");
  const requestDispatchGateRef = useRef(createVoiceRequestDispatchGate());
  const pendingPromptGroupIdRef = useRef("");
  const pendingAskRequestIdRef = useRef("");
  const askObservationRevisionRef = useRef(0);
  const askNoticeRef = useRef<VoiceAskNotice>(readVoiceAskNotice(selectedGroupId));
  const pendingAskTaskRef = useRef<SecretaryTaskSummary | null>(null);
  const pendingPromptComposerHashRef = useRef("");
  const lastVoiceLedgerSignalRef = useRef("");
  const dismissedVoiceReplyKeysRef = useRef<Set<string>>(new Set());
  const localVoiceReplyRequestIdsRef = useRef<Set<string>>(new Set());
  const askFeedbackReplyKeyByRequestIdRef = useRef<Map<string, string>>(new Map());
  const viewedDocumentPathRef = useRef("");
  const captureTargetDocumentPathRef = useRef("");
  const transcriptDocumentPathRef = useRef("");
  const documentBaseTitleRef = useRef("");
  const documentBaseContentRef = useRef("");
  const documentTitleDraftRef = useRef("");
  const documentDraftRef = useRef("");
  const voiceStreamItemIdRef = useRef("");
  const liveTranscriptPreviewRef = useRef<VoiceTranscriptPreview | null>(null);
  const archivedDocumentPathsRef = useRef<Set<string>>(new Set());
  const clearVoiceDocumentReferences = useVoiceDocumentReferenceCleanup();
  const selectedGroupIdRef = useRef("");
  const unmountCleanupRef = useRef<() => void>(() => undefined);
  selectedGroupIdRef.current = String(selectedGroupId || "").trim();
  const isCurrentGroup = useCallback(
    (gid: string) => String(gid || "").trim() === selectedGroupIdRef.current,
    [],
  );
  const recordingTargetGroupId = useCallback(
    () =>
      voiceRecordingTargetGroupId(
        voiceRecordingSessionScopeRef.current,
        selectedGroupIdRef.current,
      ),
    [],
  );
  const recordingTargetDocumentPath = useCallback(
    (fallbackPath = "") =>
      voiceRecordingTargetDocumentPath(voiceRecordingSessionScopeRef.current, fallbackPath),
    [],
  );
  const releaseDaemonVoiceRecordingLease = useCallback((groupId?: string) => {
    if (!voiceRecordingLeaseAcquiredRef.current) return;
    const gid = String(groupId || voiceRecordingLeaseGroupIdRef.current || "").trim();
    if (!gid) return;
    const leaseId = voiceRecordingLeaseIdRef.current;
    voiceRecordingLeaseAcquiredRef.current = false;
    voiceRecordingLeaseGroupIdRef.current = "";
    voiceRecordingLeaseIdRef.current = "";
    setRecordingGroupId("");
    setRecordingGroupTitle("");
    void updateVoiceAssistantRecordingLease(gid, {
      action: "release",
      ownerId: voiceCaptureOwnerIdRef.current,
      leaseId,
    });
  }, []);
  const releaseVoiceRecordingGuards = useCallback(
    (groupId?: string) => {
      releaseDaemonVoiceRecordingLease(groupId);
      releaseVoiceCaptureLock(voiceCaptureOwnerIdRef.current);
    },
    [releaseDaemonVoiceRecordingLease],
  );
  const acquireDaemonVoiceRecordingLease = useCallback(
    async (
      groupId: string,
      opts: { captureMode: string; recognitionBackend: string; dispatchTarget: string },
    ): Promise<VoiceRecordingLeaseConflict | null> => {
      const resp = await updateVoiceAssistantRecordingLease(groupId, {
        action: "acquire",
        ownerId: voiceCaptureOwnerIdRef.current,
        ttlSeconds: VOICE_RECORDING_LEASE_TTL_SECONDS,
        captureMode: opts.captureMode,
        recognitionBackend: opts.recognitionBackend,
        dispatchTarget: opts.dispatchTarget,
      });
      if (resp.ok) {
        voiceRecordingLeaseAcquiredRef.current = true;
        voiceRecordingLeaseGroupIdRef.current = groupId;
        voiceRecordingLeaseIdRef.current = resp.result.leaseId || "";
        setRecordingGroupId(groupId);
        setRecordingGroupTitle(
          String(resp.result.lease?.group_title || resp.result.lease?.group_id || groupId).trim(),
        );
        return null;
      }
      const conflict = voiceRecordingLeaseConflictFromDetails(resp.error.details);
      if (conflict) return conflict;
      throw new Error(resp.error.message || "Voice Secretary recording lease failed");
    },
    [],
  );
  const [open, setOpen] = useState(false);
  const [showAssistantModeMenu, setShowAssistantModeMenu] = useState(false);
  const [showAssistantLanguageMenu, setShowAssistantLanguageMenu] = useState(false);
  const assistantModeTriggerRef = useRef<HTMLButtonElement>(null);
  const assistantLanguageTriggerRef = useRef<HTMLButtonElement>(null);
  const [loading, setLoading] = useState(false);
  const [actionBusy, setActionBusy] = useState<VoiceSecretaryAction>("");
  const recognitionLanguageOverride = useUIStore(
    (state) => getChatSession(selectedGroupId, state.chatSessions).voiceRecognitionLanguage,
  );
  const promptAutoRefine = useUIStore(
    (state) => getChatSession(selectedGroupId, state.chatSessions).voicePromptAutoRefine,
  );
  const [assistant, setAssistant] = useState<BuiltinAssistant | null>(null);
  const [documents, setDocuments] = useState<AssistantVoiceDocument[]>([]);
  const documentsRef = useRef<AssistantVoiceDocument[]>([]);
  const [viewedDocumentPath, setViewedDocumentPath] = useState("");
  const [captureTargetDocumentPath, setCaptureTargetDocumentPath] = useState("");
  const [documentTitleDraft, setDocumentTitleDraft] = useState("");
  const [documentDraft, setDocumentDraft] = useState("");
  const [documentBaseTitle, setDocumentBaseTitle] = useState("");
  const [documentBaseContent, setDocumentBaseContent] = useState("");
  const [documentEditing, setDocumentEditing] = useState(false);
  const [documentRemoteChanged, setDocumentRemoteChanged] = useState(false);
  const [documentContentLoadingPath, setDocumentContentLoadingPath] = useState("");
  const documentContentLoadTracker = useRef(createDocumentContentLoadTracker());
  const [creatingDocument, setCreatingDocument] = useState(false);
  const [newDocumentTitleDraft, setNewDocumentTitleDraft] = useState("");
  const [documentInstruction, setDocumentInstruction] = useState("");
  const [panelView, setPanelView] = useState<VoicePanelView>(() =>
    voicePanelViewForComposerMode(composerCaptureMode, "document"),
  );
  const [recordingTarget, setRecordingTarget] = useState<VoiceCaptureTarget | null>(null);
  const [recording, setRecording] = useState(false);
  const [recordingStarting, setRecordingStarting] = useState(false);
  const [recordingGroupId, setRecordingGroupId] = useState("");
  const [recordingGroupTitle, setRecordingGroupTitle] = useState("");
  const [speechError, setSpeechError] = useState("");
  const [browserSpeechRecoveryCode, setBrowserSpeechRecoveryCode] = useState("");
  const [lastRecordingStopReason, setLastRecordingStopReason] =
    useState<VoiceRecordingStopReason | null>(null);
  const [speechSupported, setSpeechSupported] = useState(() => !getBrowserSpeechSupportIssue());
  const [serviceAudioSupported, setServiceAudioSupported] = useState(() =>
    mediaRecorderSupported(),
  );
  const audioPreferences = useVoiceAudioStore((state) => state.preferences);
  const { label: audioDeviceLabel, refresh: refreshAudioDevices } = useVoiceAudioDevices(open);
  const capturedAudioPreferencesRef = useRef<VoiceAudioPreferences | null>(null);
  const [pendingPromptRequestId, setPendingPromptRequestId] = useState("");
  const [pendingPromptProblem, setPendingPromptProblem] = useState("");
  const [pendingPromptTask, setPendingPromptTask] = useState<SecretaryTaskSummary | null>(null);
  const [promptCancelBusy, setPromptCancelBusy] = useState(false);
  const [executionOpen, setExecutionOpen] = useState(false);
  const [executionTaskId, setExecutionTaskId] = useState("");
  const executionGroupLabel =
    useGroupStore(
      (state) => state.groups.find((group) => group.group_id === selectedGroupId)?.title,
    ) || selectedGroupId;
  const [pendingAskRequestId, setPendingAskRequestId] = useState("");
  const [askNotice, setAskNotice] = useState<VoiceAskNotice>(() =>
    readVoiceAskNotice(selectedGroupId),
  );
  const [pendingAskTask, setPendingAskTask] = useState<SecretaryTaskSummary | null>(null);
  const [askStatusReadError, setAskStatusReadError] = useState("");
  const [askObservationVersion, setAskObservationVersion] = useState(0);
  const [pendingPromptDraft, setPendingPromptDraft] = useState<AssistantVoicePromptDraft | null>(
    null,
  );
  const [promptNeedsReview, setPromptNeedsReview] = useState(false);
  const clearPendingPromptRequest = useCallback((clearDraft: boolean) => {
    pendingPromptRequestIdRef.current = "";
    pendingPromptGroupIdRef.current = "";
    pendingPromptComposerHashRef.current = "";
    setPendingPromptRequestId("");
    setPendingPromptProblem("");
    setPendingPromptTask(null);
    setPromptNeedsReview(false);
    if (clearDraft) setPendingPromptDraft(null);
  }, []);
  const [askFeedbackItems, setAskFeedbackItems] = useState<AssistantVoiceAskFeedback[]>([]);
  const [liveTranscriptPreview, setLiveTranscriptPreview] = useState<VoiceTranscriptPreview | null>(
    null,
  );
  const [voiceTranscriptItems, setVoiceTranscriptItems] = useState<VoiceTranscriptItem[]>([]);
  const [voiceWorkspaceView, setVoiceWorkspaceView] = useState<"document" | "transcript">(
    "document",
  );
  const [activityClockMs, setActivityClockMs] = useState(() => Date.now());
  const [voiceReplyBubbleRequestId, setVoiceReplyBubbleRequestId] = useState("");
  const [copiedVoiceReplyRequestId, setCopiedVoiceReplyRequestId] = useState("");
  const {
    getLevel: getVoiceAudioLevel,
    reset: resetVoiceAudioLevel,
    updateFromSamples: updateVoiceAudioLevelsFromSamples,
  } = useVoiceAudioLevelMeter();
  const setRecordingStartingFlag = useCallback((next: boolean) => {
    recordingStartingRef.current = next;
    setRecordingStarting(next);
  }, []);
  const clearRecordingStopReason = useCallback(() => {
    lastRecordingStopReasonRef.current = null;
    setLastRecordingStopReason(null);
  }, []);
  const reportRecordingStopReason = useCallback(
    (
      code: string,
      opts: { detail?: string; backend?: string; groupId?: string; runId?: number } = {},
    ) => {
      const reason: VoiceRecordingStopReason = {
        code,
        detail: String(opts.detail || "").trim(),
        backend: String(opts.backend || "").trim(),
        groupId: String(
          opts.groupId || voiceRecordingLeaseGroupIdRef.current || selectedGroupIdRef.current || "",
        ).trim(),
        runId: opts.runId,
        at: Date.now(),
      };
      const previous = lastRecordingStopReasonRef.current;
      const sameRecentReason =
        previous &&
        previous.code === reason.code &&
        previous.detail === reason.detail &&
        previous.backend === reason.backend &&
        previous.groupId === reason.groupId &&
        previous.runId === reason.runId &&
        reason.at - previous.at < 1500;
      if (sameRecentReason) return;
      lastRecordingStopReasonRef.current = reason;
      setLastRecordingStopReason(reason);
    },
    [],
  );
  const isActiveRecordingRun = useCallback(
    (runId: number) => runId > 0 && recordingRunIdRef.current === runId,
    [],
  );
  const capturedConfigRef = useRef<BuiltinAssistant["config"] | null>(null);
  const getActiveCaptureConfig = useCallback(
    () =>
      recordingRef.current || recordingStartingRef.current || recordingStoppingRef.current
        ? capturedConfigRef.current
        : null,
    [],
  );
  const beginRecordingRun = useCallback(() => {
    if (recordingStartingRef.current || recordingStoppingRef.current || recordingRef.current)
      return 0;
    capturedAudioPreferencesRef.current = voiceAudioSnapshot();
    capturedConfigRef.current = {
      ...(assistant?.config || {}),
      recognition_language:
        recognitionLanguageOverride || assistant?.config?.recognition_language || "auto",
    };
    recordingRunIdRef.current += 1;
    voiceRecordingSessionIdRef.current = createVoiceRecordingSessionId();
    transcriptSegmentSeqRef.current = 0;
    setRecordingStartingFlag(true);
    setSpeechError("");
    setBrowserSpeechRecoveryCode("");
    clearRecordingStopReason();
    return recordingRunIdRef.current;
  }, [
    assistant?.config,
    recognitionLanguageOverride,
    clearRecordingStopReason,
    setRecordingStartingFlag,
  ]);
  const finishRecordingStart = useCallback(
    (runId: number) => {
      if (isActiveRecordingRun(runId)) setRecordingStartingFlag(false);
    },
    [isActiveRecordingRun, setRecordingStartingFlag],
  );
  const endRecordingRun = useCallback(
    (runId?: number) => {
      if (runId && !isActiveRecordingRun(runId)) return;
      recordingStoppingRef.current = false;
      if (!runId || browserSpeechFinalizingRunIdRef.current === runId) {
        browserSpeechFinalizingRunIdRef.current = 0;
      }
      if (!runId || voiceRecordingSessionScopeRef.current?.runId === runId) {
        voiceRecordingSessionScopeRef.current = null;
      }
      capturedConfigRef.current = null;
      recordingRunIdRef.current += 1;
      setRecordingTarget(null);
      setRecordingStartingFlag(false);
    },
    [isActiveRecordingRun, setRecordingStartingFlag],
  );
  const latestVoiceLedgerEvent = useVoiceDocumentLedgerProjection(
    selectedGroupId,
    clearVoiceDocumentReferences,
  );

  useEffect(() => {
    recordingRef.current = recording;
    if (recording) {
      setRecordingStartingFlag(false);
      voiceStreamItemIdRef.current = "";
    }
  }, [recording, setRecordingStartingFlag]);

  useEffect(() => {
    const channel = openVoiceCaptureChannel();
    if (!channel) return undefined;
    const handleMessage = (event: MessageEvent<VoiceCaptureChannelMessage>) => {
      const message = event.data || {};
      if (message.type !== "probe") return;
      if (String(message.ownerId || "") !== voiceCaptureOwnerIdRef.current) return;
      if (!recordingRef.current) return;
      channel.postMessage({
        type: "alive",
        ownerId: voiceCaptureOwnerIdRef.current,
        groupId: voiceRecordingLeaseGroupIdRef.current || selectedGroupId,
        sentAt: Date.now(),
      } satisfies VoiceCaptureChannelMessage);
    };
    channel.addEventListener("message", handleMessage);
    return () => {
      channel.removeEventListener("message", handleMessage);
      channel.close();
    };
  }, [selectedGroupId]);

  useEffect(() => {
    const releaseCurrentLock = () => releaseVoiceRecordingGuards();
    window.addEventListener("pagehide", releaseCurrentLock);
    window.addEventListener("beforeunload", releaseCurrentLock);
    return () => {
      window.removeEventListener("pagehide", releaseCurrentLock);
      window.removeEventListener("beforeunload", releaseCurrentLock);
      releaseCurrentLock();
    };
  }, [releaseVoiceRecordingGuards]);

  const activeDocument = useMemo(() => {
    const path = String(viewedDocumentPath || "").trim();
    if (path) {
      const match = findVoiceDocument(documents, path);
      if (match) return match;
    }
    return documents.find((document) => String(document.status || "active") === "active") || null;
  }, [viewedDocumentPath, documents]);
  const activeDocumentWritePath = useMemo(
    () => voiceDocumentPath(activeDocument),
    [activeDocument],
  );
  const activeDocumentTitle = String(activeDocument?.title || "").trim();
  const documentDisplayTitle =
    activeDocumentTitle ||
    documentTitleDraft.trim() ||
    t("voiceSecretaryWorkdocTitle", { defaultValue: "Voice Secretary workdoc" });
  const documentHasUnsavedEdits = documentDraft !== documentBaseContent;
  const documentContentLoading = documentContentLoadingMatches(
    documentContentLoadingPath,
    activeDocumentWritePath,
  );
  const defaultCaptureDocument = useMemo(() => {
    const targetPath = String(captureTargetDocumentPath || "").trim();
    if (targetPath) {
      const match = findVoiceDocument(documents, targetPath);
      if (match) return match;
    }
    return activeDocument;
  }, [activeDocument, captureTargetDocumentPath, documents]);
  const defaultCaptureDocumentPath =
    voiceDocumentPath(defaultCaptureDocument) || String(captureTargetDocumentPath || "").trim();
  const captureTarget = resolveVoiceCaptureTarget({
    recordingTarget,
    panelOpen: open,
    panelView,
    composerMode: composerCaptureMode,
    promptAutoRefine,
    viewedDocumentPath: activeDocumentWritePath,
    defaultDocumentPath: defaultCaptureDocumentPath,
  });
  const captureMode = captureTarget.mode;
  const effectiveCaptureTargetDocumentPath = captureTarget.documentPath;
  const captureTargetDocumentTitle =
    String(findVoiceDocument(documents, effectiveCaptureTargetDocumentPath)?.title || "").trim() ||
    documentDisplayTitle;
  const captureDispatchTarget = captureTarget.dispatchTarget;
  const captureNeedsSecretary = captureDispatchTarget !== "composer";
  const composerNeedsSecretary =
    voiceCaptureDispatchTarget({ captureMode: composerCaptureMode, promptAutoRefine }) !==
    "composer";
  const captureTransportMode = voiceCaptureTransportMode(captureDispatchTarget);
  const globalSecretary = assistant?.health?.secretary as
    | (Partial<SecretaryReadiness> & { busy?: boolean; pending?: boolean; status?: string })
    | undefined;
  const secretaryIssue = secretaryReadinessIssue(globalSecretary);
  const secretaryNotReadyReason = secretaryIssue ? t(`settings:${secretaryIssue.titleKey}`) : "";
  const secretaryReadinessDiagnostic = secretaryIssue?.detail || "";
  const composerSecretaryUnavailableReason = composerNeedsSecretary ? secretaryNotReadyReason : "";
  const captureConfig =
    recording || recordingStarting || recordingStoppingRef.current
      ? capturedConfigRef.current || assistant?.config
      : assistant?.config;
  const recognitionBackend = String(captureConfig?.recognition_backend || "browser_asr").trim();
  const rawConfiguredRecognitionLanguage =
    (!(recording || recordingStarting || recordingStoppingRef.current) &&
      recognitionLanguageOverride) ||
    String(captureConfig?.recognition_language || "auto").trim() ||
    "auto";
  const configuredRecognitionLanguage = normalizeVoiceRecognitionLanguageForBackend(
    rawConfiguredRecognitionLanguage,
    recognitionBackend,
  );
  const effectiveRecognitionLanguage =
    recognitionBackend === "browser_asr"
      ? resolveBrowserSpeechLanguage(
          configuredRecognitionLanguage,
          typeof navigator !== "undefined" ? navigator.language : "",
        )
      : configuredRecognitionLanguage === "auto"
        ? typeof navigator !== "undefined" && navigator.language
          ? navigator.language
          : "en-US"
        : configuredRecognitionLanguage;
  const getRecordingRecognitionLanguage = useCallback(() => {
    const config = getActiveCaptureConfig();
    if (!config) return effectiveRecognitionLanguage;
    const backend = String(config.recognition_backend || "browser_asr");
    const language = normalizeVoiceRecognitionLanguageForBackend(
      String(config.recognition_language || "auto"),
      backend,
    );
    return backend === "browser_asr"
      ? resolveBrowserSpeechLanguage(language, navigator.language)
      : language === "auto"
        ? navigator.language || "en-US"
        : language;
  }, [effectiveRecognitionLanguage, getActiveCaptureConfig]);
  const voiceLanguageOptions = useMemo(
    () => voiceLanguageOptionValues(rawConfiguredRecognitionLanguage, recognitionBackend),
    [rawConfiguredRecognitionLanguage, recognitionBackend],
  );
  const voiceLanguageLabel = useCallback(
    (value: string) => {
      switch (value) {
        case "mixed":
          return t("voiceSecretaryLanguageMixed", { defaultValue: "Mixed" });
        case "auto":
          return t("voiceSecretaryLanguageAuto", { defaultValue: "System default" });
        case "zh-CN":
          return t("voiceSecretaryLanguageChinese", { defaultValue: "Chinese" });
        case "en-US":
          return t("voiceSecretaryLanguageEnglish", { defaultValue: "English" });
        case "ja-JP":
          return t("voiceSecretaryLanguageJapanese", { defaultValue: "Japanese" });
        case "ko-KR":
          return t("voiceSecretaryLanguageKorean", { defaultValue: "Korean" });
        case "fr-FR":
          return t("voiceSecretaryLanguageFrench", { defaultValue: "French" });
        case "de-DE":
          return t("voiceSecretaryLanguageGerman", { defaultValue: "German" });
        case "es-ES":
          return t("voiceSecretaryLanguageSpanish", { defaultValue: "Spanish" });
        default:
          return value;
      }
    },
    [t],
  );
  const voiceLanguageShortLabel = useCallback(
    (value: string) => {
      switch (value) {
        case "mixed":
          return t("voiceSecretaryLanguageShortMixed", { defaultValue: "MIX" });
        case "auto":
          return t("voiceSecretaryLanguageShortAuto", { defaultValue: "SYS" });
        case "zh-CN":
          return t("voiceSecretaryLanguageShortChinese", { defaultValue: "ZH" });
        case "en-US":
          return t("voiceSecretaryLanguageShortEnglish", { defaultValue: "EN" });
        case "ja-JP":
          return t("voiceSecretaryLanguageShortJapanese", { defaultValue: "JA" });
        case "ko-KR":
          return t("voiceSecretaryLanguageShortKorean", { defaultValue: "KO" });
        case "fr-FR":
          return t("voiceSecretaryLanguageShortFrench", { defaultValue: "FR" });
        case "de-DE":
          return t("voiceSecretaryLanguageShortGerman", { defaultValue: "DE" });
        case "es-ES":
          return t("voiceSecretaryLanguageShortSpanish", { defaultValue: "ES" });
        default:
          return (
            String(value || "")
              .slice(0, 2)
              .toUpperCase() || "ASR"
          );
      }
    },
    [t],
  );
  const configuredRecognitionLanguageLabel = voiceLanguageLabel(configuredRecognitionLanguage);
  const configuredRecognitionLanguageShortLabel = voiceLanguageShortLabel(
    configuredRecognitionLanguage,
  );
  const autoDocumentMaxWindowMs = useMemo(() => {
    const raw = captureConfig?.auto_document_max_window_seconds;
    if (raw === null || captureConfig?.auto_document_enabled === false) return null;
    return (
      numberFromUnknown(
        raw,
        BROWSER_SPEECH_MAX_WINDOW_FALLBACK_MS / 1000,
        BROWSER_SPEECH_MIN_MAX_WINDOW_MS / 1000,
        300,
      ) * 1000
    );
  }, [captureConfig?.auto_document_max_window_seconds, captureConfig?.auto_document_enabled]);
  const browserSpeechReady = recognitionBackend === "browser_asr";
  const serviceReadiness = resolveVoiceServiceReadiness({
    assistant: assistant ? { ...assistant, config: captureConfig } : null,
  });
  const serviceAsrReady = serviceReadiness.serviceAsrReady;
  const browserSpeechSupportIssue = browserSpeechReady ? getBrowserSpeechSupportIssue() : "";
  const serviceAudioSupportIssue = serviceAsrReady ? getBrowserAudioSupportIssue() : "";
  const getBrowserSpeechIssueMessage = useCallback(
    (issue: BrowserSpeechSupportIssue) => {
      if (issue === "unsupported") {
        return t("voiceSecretaryBrowserUnsupported", {
          defaultValue:
            "Browser speech recognition is not available in this browser page. Try another current browser.",
        });
      }
      return "";
    },
    [t],
  );
  const getAudioSupportIssueMessage = useCallback(
    (issue: BrowserAudioSupportIssue) => {
      if (issue === "embedded_workspace") {
        return t("voiceSecretaryOpenInstanceForMicrophone");
      }
      if (issue === "secure_context") {
        return t("voiceSecretarySecureContextRequired", {
          defaultValue:
            "Microphone capture requires a secure browser context. Open this page through localhost or HTTPS, not a raw WSL IP over HTTP.",
        });
      }
      if (issue === "get_user_media") {
        return t("voiceSecretaryGetUserMediaUnavailable", {
          defaultValue:
            "This browser page cannot access the microphone API. Open it through localhost or HTTPS and allow microphone access.",
        });
      }
      return "";
    },
    [t],
  );
  const getAudioCaptureErrorMessage = useCallback(
    (error: unknown) => {
      const errorName =
        typeof error === "object" && error && "name" in error
          ? String((error as { name?: unknown }).name || "")
          : "";
      if (errorName === "NotAllowedError" || errorName === "SecurityError") {
        return {
          message: t("voiceSecretaryMicPermissionBlocked", {
            defaultValue:
              "Microphone permission is blocked. Allow microphone access for this site in the browser, then try again.",
          }),
        };
      }
      if (errorName === "NotFoundError" || errorName === "DevicesNotFoundError") {
        return {
          message: t("voiceSecretaryMicNotFound", {
            defaultValue: "No microphone was found or the selected microphone is unavailable.",
          }),
        };
      }
      if (errorName === "NotReadableError" || errorName === "TrackStartError") {
        return {
          message: t("voiceSecretaryMicBusyOrUnavailable", {
            defaultValue:
              "The microphone could not be started. Check whether another app is using it or the OS blocked access.",
          }),
        };
      }
      if (errorName === "AbortError") {
        return {
          message: t("voiceSecretaryMicBusyOrUnavailable", {
            defaultValue:
              "The microphone could not be started. Check whether another app is using it or the OS blocked access.",
          }),
        };
      }
      if (error instanceof VoiceAudioCaptureTimeoutError) {
        return {
          message: t("voiceSecretaryMicPermissionPending", {
            defaultValue:
              "Microphone access is still waiting for browser permission. Allow it from the address bar site settings, or close another page that is using the microphone, then retry.",
          }),
        };
      }
      if (errorName === "OverconstrainedError" || errorName === "ConstraintNotSatisfiedError") {
        return {
          message: t("voiceSecretarySelectedMicUnavailable", {
            defaultValue:
              "The selected microphone is unavailable. Reset to the system default microphone and try again.",
          }),
        };
      }
      return {
        message: t("voiceSecretaryAudioCaptureFailed", { defaultValue: "Audio capture failed." }),
      };
    },
    [t],
  );
  const controlDisabled = disabled || !selectedGroupId || busy === "send";
  const recognitionLanguageDisabled = controlDisabled || recording || recordingStarting;
  const isAssistantRow = variant === "assistantRow";
  const inputDeviceId =
    (recording || recordingStarting ? capturedAudioPreferencesRef.current : null)?.inputDeviceId ??
    audioPreferences.inputDeviceId;
  const selectedAudioDeviceLabel = inputDeviceId
    ? audioDeviceLabel("input", inputDeviceId)
    : SERVICE_DEFAULT_MIC_LABEL;
  useEffect(() => {
    viewedDocumentPathRef.current = viewedDocumentPath;
  }, [viewedDocumentPath]);

  useEffect(() => {
    captureTargetDocumentPathRef.current = captureTargetDocumentPath;
  }, [captureTargetDocumentPath]);

  useEffect(() => {
    documentsRef.current = documents;
    documentTitleDraftRef.current = documentTitleDraft;
    documentDraftRef.current = documentDraft;
    documentBaseTitleRef.current = documentBaseTitle;
    documentBaseContentRef.current = documentBaseContent;
  }, [documentBaseContent, documentBaseTitle, documentDraft, documentTitleDraft, documents]);

  const loadDocumentDraft = useCallback((document: AssistantVoiceDocument | null) => {
    const title = String(document?.title || "");
    const content = String(document?.content || "");
    documentTitleDraftRef.current = title;
    documentDraftRef.current = content;
    documentBaseTitleRef.current = title;
    documentBaseContentRef.current = content;
    setDocumentTitleDraft(title);
    setDocumentDraft(content);
    setDocumentBaseTitle(title);
    setDocumentBaseContent(content);
    setDocumentRemoteChanged(false);
  }, []);

  const updateDocumentDraft = useCallback((value: string) => {
    documentDraftRef.current = value;
    setDocumentDraft(value);
  }, []);

  const clearLiveTranscriptPreview = useCallback(() => {
    liveTranscriptPreviewRef.current = null;
    voiceStreamItemIdRef.current = "";
    setLiveTranscriptPreview(null);
  }, []);

  const updateLiveTranscriptPreview = useCallback(
    (
      text: string,
      phase: VoiceTranscriptPreviewPhase,
      opts?: { startMs?: number; endMs?: number },
    ) => {
      const clean = normalizeBrowserTranscriptChunk(text);
      if (!clean) return;
      if (shouldSettleLiveVoiceActivityStream(liveTranscriptPreviewRef.current, clean, phase)) {
        voiceStreamItemIdRef.current = "";
        clearLiveTranscriptPreview();
      }
      const now = Date.now();
      const streamId =
        voiceStreamItemIdRef.current ||
        `voice-stream-${now.toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
      voiceStreamItemIdRef.current = streamId;
      const pendingFinalText = normalizeBrowserTranscriptChunk(
        browserFinalTranscriptBufferRef.current,
      );
      const preview = createVoiceTranscriptPreview({
        id: streamId,
        cleanText: clean,
        phase,
        pendingFinalText,
        metadata: {
          mode: voiceRecordingCaptureMode(voiceRecordingSessionScopeRef.current, captureMode),
          groupId: recordingTargetGroupId(),
          sessionId: voiceRecordingSessionIdRef.current,
          documentTitle: captureTargetDocumentTitle,
          documentPath: effectiveCaptureTargetDocumentPath,
          language: getRecordingRecognitionLanguage(),
        },
        timing: opts,
        now,
      });
      liveTranscriptPreviewRef.current = preview;
      setLiveTranscriptPreview(preview);
      setActivityClockMs(now);
    },
    [
      captureMode,
      clearLiveTranscriptPreview,
      effectiveCaptureTargetDocumentPath,
      captureTargetDocumentTitle,
      getRecordingRecognitionLanguage,
      recordingTargetGroupId,
    ],
  );

  const pushVoiceTranscriptItem = useCallback(
    (text: string, opts?: { startMs?: number; endMs?: number; documentPath?: string }) => {
      const clean = normalizeBrowserTranscriptChunk(text);
      if (!clean) return;
      const documentPath = String(
        opts?.documentPath || effectiveCaptureTargetDocumentPath || "",
      ).trim();
      if (!documentPath) return;
      const now = Date.now();
      const liveId = voiceStreamItemIdRef.current;
      const documentTitle =
        String(findVoiceDocument(documents, documentPath)?.title || "").trim() ||
        captureTargetDocumentTitle;
      const item = createVoiceTranscriptItem({
        id: `voice-transcript-${now.toString(36)}-${Math.random().toString(36).slice(2, 8)}`,
        cleanText: clean,
        metadata: {
          mode: "document",
          sessionId: voiceRecordingSessionIdRef.current,
          documentTitle,
          documentPath,
          language: getRecordingRecognitionLanguage(),
        },
        timing: opts,
        now,
      });
      setVoiceTranscriptItems((prev) => appendFinalVoiceTranscriptItem(prev, item, liveId));
    },
    [
      captureTargetDocumentTitle,
      documents,
      effectiveCaptureTargetDocumentPath,
      getRecordingRecognitionLanguage,
    ],
  );

  const finalizeLiveTranscriptPreview = useCallback(() => {
    clearLiveTranscriptPreview();
    setActivityClockMs(Date.now());
  }, [clearLiveTranscriptPreview]);

  useEffect(() => {
    if (!liveTranscriptPreview || recording) return;
    if (activityClockMs - liveTranscriptPreview.updatedAt <= VOICE_LIVE_TRANSCRIPT_VISIBLE_MS)
      return;
    finalizeLiveTranscriptPreview();
  }, [activityClockMs, finalizeLiveTranscriptPreview, liveTranscriptPreview, recording]);

  const restoreLatestVoiceMeetingSession = useCallback(
    async (opts?: { replaceSession?: boolean; sessionId?: string }) => {
      const gid = String(selectedGroupId || "").trim();
      const documentPath = String(transcriptDocumentPathRef.current || "").trim();
      const sessionId = String(opts?.sessionId || "").trim();
      if (!gid || (!documentPath && !sessionId)) return false;
      const resp = sessionId
        ? await fetchVoiceAssistantMeetingSession(gid, sessionId)
        : await fetchLatestVoiceAssistantMeetingSession(gid, { documentPath });
      if (!isCurrentGroup(gid)) return false;
      if (!resp.ok || !resp.result.session) return false;
      const restoredItems = voiceTranscriptItemsFromMeetingSession(resp.result.session, {
        documentPathFallback: documentPath,
      });
      if (!restoredItems.length) return false;
      setVoiceTranscriptItems((prev) =>
        opts?.replaceSession
          ? replaceVoiceTranscriptSessionItems(prev, restoredItems)
          : mergeVoiceTranscriptItems(prev, restoredItems),
      );
      return resp.result.session.diarization_ready === true;
    },
    [isCurrentGroup, selectedGroupId],
  );

  const loadAudioDevices = useCallback(async () => {
    setServiceAudioSupported(mediaRecorderSupported());
    await refreshAudioDevices();
  }, [refreshAudioDevices]);

  const rememberAskRequest = useCallback(
    (gid: string, requestId: string, task: SecretaryTaskSummary | null = null) => {
      const notice = { requestId, hidden: false };
      saveVoiceAskNotice(gid, notice);
      if (!isCurrentGroup(gid)) return;
      localVoiceReplyRequestIdsRef.current.add(requestId);
      askNoticeRef.current = notice;
      setAskNotice(notice);
      pendingAskRequestIdRef.current = requestId;
      setPendingAskRequestId(requestId);
      pendingAskTaskRef.current = task;
      setPendingAskTask(task);
      setAskStatusReadError("");
      setVoiceReplyBubbleRequestId("");
      askObservationRevisionRef.current++;
      setAskObservationVersion((current) => current + 1);
    },
    [isCurrentGroup],
  );
  const applyAskSnapshot = useCallback(
    (gid: string, result: AssistantStateResult) => {
      if (!isCurrentGroup(gid)) return;
      const items = result.ask_requests || [];
      setAskFeedbackItems(items);
      const requestId = askNoticeRef.current.requestId;
      if (!requestId) return;
      const task = currentVoiceAskTask(
        result.secretary_tasks,
        gid,
        requestId,
        pendingAskTaskRef.current,
      );
      pendingAskTaskRef.current = task;
      setPendingAskTask(task);
      const feedback = items.find((item) => item.request_id === requestId) || null;
      const awaitingResult = !voiceAskReplyIsCurrent(task, feedback);
      pendingAskRequestIdRef.current = awaitingResult ? requestId : "";
      setPendingAskRequestId(awaitingResult ? requestId : "");
    },
    [isCurrentGroup],
  );
  const receiveAskStatus = useCallback(
    (gid: string, requestId: string, result: AssistantStateResult) => {
      if (!isCurrentGroup(gid) || pendingAskRequestIdRef.current !== requestId) return;
      askObservationRevisionRef.current++;
      setAskStatusReadError("");
      if (result.assistant) setAssistant(result.assistant);
      applyAskSnapshot(gid, result);
    },
    [applyAskSnapshot, isCurrentGroup],
  );
  const receiveAskError = useCallback(
    (gid: string, requestId: string, message: string) => {
      if (isCurrentGroup(gid) && pendingAskRequestIdRef.current === requestId)
        setAskStatusReadError(message);
    },
    [isCurrentGroup],
  );
  const acceptAskRetry = useCallback(
    (task: SecretaryTaskSummary) => {
      if (!isCurrentGroup(task.target.group_id) || task.target.kind !== "ask") return;
      rememberAskRequest(task.target.group_id, task.target.request_id, task);
    },
    [isCurrentGroup, rememberAskRequest],
  );
  useVoiceRequestStatus({
    groupId: selectedGroupId,
    requestId: pendingAskRequestId,
    enabled: !!pendingAskRequestId,
    kind: "ask",
    version: askObservationVersion,
    onStatus: receiveAskStatus,
    onError: receiveAskError,
  });

  const refreshAssistant = useCallback(
    async function refresh(opts?: WorkspaceRefreshOptions): Promise<void> {
      const gid = String(selectedGroupId || "").trim();
      if (!gid) return;
      const pending = workspaceRefreshRef.current;
      if (opts?.passive && pending?.groupId === gid && pending.dataSeq === refreshSeq.current) {
        // Repeated focus/ledger/poll signals must not invalidate every slow read.
        // Keep one follow-up to observe changes that arrived after the read began.
        pending.queued = {
          ...opts,
          refreshChangedActiveDocumentContent:
            opts.refreshChangedActiveDocumentContent ||
            pending.queued?.refreshChangedActiveDocumentContent,
        };
        return;
      }
      const quiet = Boolean(opts?.quiet);
      const ownership = beginVoiceAssistantRefresh(
        { dataSeq: refreshSeq.current, visibleLoadingSeq: visibleLoadSeqRef.current },
        { quiet },
      );
      refreshSeq.current = ownership.next.dataSeq;
      visibleLoadSeqRef.current = ownership.next.visibleLoadingSeq;
      const read: WorkspaceRefreshRead = {
        groupId: gid,
        dataSeq: ownership.request.dataSeq,
        queued: null,
      };
      workspaceRefreshRef.current = read;
      const currentOwnership = (): VoiceAssistantLoadingOwnership => ({
        dataSeq: refreshSeq.current,
        visibleLoadingSeq: visibleLoadSeqRef.current,
      });
      if (ownership.shouldShowLoading) setLoading(true);
      const askRevision = askObservationRevisionRef.current;
      try {
        const promptRequestId =
          String(pendingPromptGroupIdRef.current || "").trim() === gid
            ? String(pendingPromptRequestIdRef.current || "").trim()
            : "";
        const resp = await fetchVoiceAssistantWorkspace(gid, { promptRequestId });
        if (
          !shouldApplyVoiceAssistantRefresh(
            currentOwnership(),
            ownership.request,
            isCurrentGroup(gid),
          )
        )
          return;
        if (!resp.ok) {
          if (!quiet) showError(resp.error.message);
          return;
        }
        setAssistant(resp.result.assistant || null);
        const promptDraft = resp.result.prompt_draft || null;
        if (
          promptDraft &&
          pendingPromptGroupIdRef.current === gid &&
          pendingPromptRequestIdRef.current &&
          promptDraft.request_id === pendingPromptRequestIdRef.current
        ) {
          setPendingPromptDraft(promptDraft);
          setPendingPromptProblem("");
        }
        if (askRevision === askObservationRevisionRef.current) applyAskSnapshot(gid, resp.result);
        let nextDocuments = visibleVoiceDocuments(resp.result.documents || []);
        // This response passed the refresh-ownership check. Server-visible documents
        // supersede local archive guards, including restores from another client.
        for (const document of nextDocuments) {
          archivedDocumentPathsRef.current.delete(voiceDocumentPath(document));
        }
        clearRemovedVoiceDocumentReferences(
          clearVoiceDocumentReferences,
          gid,
          documentsRef.current,
          nextDocuments,
        );
        setDocuments(nextDocuments);
        const serverCaptureTargetPath =
          resolveVoiceDocumentPath(
            nextDocuments,
            String(
              resp.result.capture_target_document_path || resp.result.active_document_path || "",
            ).trim(),
          ) ||
          resolveVoiceDocumentPath(
            nextDocuments,
            String(
              resp.result.capture_target_document_id || resp.result.active_document_id || "",
            ).trim(),
          );
        const currentCaptureTargetPath = String(captureTargetDocumentPathRef.current || "").trim();
        const currentCaptureTargetExists = currentCaptureTargetPath
          ? Boolean(findVoiceDocument(nextDocuments, currentCaptureTargetPath))
          : false;
        const resolvedCaptureTargetPath =
          serverCaptureTargetPath || (currentCaptureTargetExists ? currentCaptureTargetPath : "");
        captureTargetDocumentPathRef.current = resolvedCaptureTargetPath;
        setCaptureTargetDocumentPath(resolvedCaptureTargetPath);
        const serverViewedPath =
          resolveVoiceDocumentPath(
            nextDocuments,
            String(
              resp.result.active_document_path || resp.result.capture_target_document_path || "",
            ).trim(),
          ) ||
          resolveVoiceDocumentPath(
            nextDocuments,
            String(
              resp.result.active_document_id || resp.result.capture_target_document_id || "",
            ).trim(),
          );
        const currentViewedPath = String(viewedDocumentPathRef.current || "").trim();
        const currentViewedDocument = findVoiceDocument(nextDocuments, currentViewedPath);
        const serverViewedDocument = findVoiceDocument(nextDocuments, serverViewedPath);
        let nextActiveDocument =
          currentViewedDocument || serverViewedDocument || nextDocuments[0] || null;
        const nextViewedPath = voiceDocumentPath(nextActiveDocument);
        const previousActiveDocument = nextViewedPath
          ? findVoiceDocument(documentsRef.current, nextViewedPath)
          : null;
        const activeDocumentRevisionChanged = Boolean(
          opts?.refreshChangedActiveDocumentContent &&
          previousActiveDocument &&
          nextActiveDocument &&
          voiceDocumentRevisionKey(previousActiveDocument) !==
            voiceDocumentRevisionKey(nextActiveDocument),
        );
        setViewedDocumentPath(nextViewedPath);
        if (nextActiveDocument) {
          if (
            nextViewedPath &&
            (opts?.forceActiveDocumentContent ||
              activeDocumentRevisionChanged ||
              documentNeedsContentLoad(nextActiveDocument))
          ) {
            const contentLoad = documentContentLoadTracker.current.begin();
            setDocumentContentLoadingPath(nextViewedPath);
            try {
              const docResp = await fetchVoiceAssistantDocumentContent(gid, nextViewedPath);
              if (
                !shouldApplyVoiceAssistantRefresh(
                  currentOwnership(),
                  ownership.request,
                  isCurrentGroup(gid),
                )
              )
                return;
              if (docResp.ok && docResp.result.document) {
                nextActiveDocument = docResp.result.document;
                nextDocuments = nextDocuments.map((document) =>
                  voiceDocumentMatches(document, nextViewedPath)
                    ? docResp.result.document!
                    : document,
                );
                setDocuments(nextDocuments);
              }
            } finally {
              // A superseded refresh still owns the indicator it raised unless a
              // newer load took over; clearing is decided per load, not per refresh.
              if (documentContentLoadTracker.current.end(contentLoad) && isCurrentGroup(gid)) {
                setDocumentContentLoadingPath("");
              }
            }
          }
          const serverTitle = String(nextActiveDocument.title || "");
          const serverContent = String(nextActiveDocument.content || "");
          const localDirty = documentDraftRef.current !== documentBaseContentRef.current;
          const sameDocument = currentViewedPath
            ? voiceDocumentMatches(nextActiveDocument, currentViewedPath)
            : false;
          const serverChangedFromBase =
            serverTitle !== documentBaseTitleRef.current ||
            serverContent !== documentBaseContentRef.current;
          if (!localDirty || !sameDocument) {
            loadDocumentDraft(nextActiveDocument);
          } else if (serverChangedFromBase) {
            setDocumentRemoteChanged(true);
          }
        } else {
          loadDocumentDraft(null);
        }
      } finally {
        if (
          shouldFinishVoiceAssistantVisibleLoading(
            currentOwnership(),
            ownership.request,
            isCurrentGroup(gid),
          )
        ) {
          setLoading(false);
        }
        if (workspaceRefreshRef.current === read) {
          workspaceRefreshRef.current = null;
          if (read.queued && read.dataSeq === refreshSeq.current && isCurrentGroup(gid))
            void refresh(read.queued);
        }
      }
    },
    [
      applyAskSnapshot,
      clearVoiceDocumentReferences,
      isCurrentGroup,
      loadDocumentDraft,
      selectedGroupId,
      showError,
    ],
  );

  useEffect(() => {
    if (!open) return;
    void refreshAssistant();
  }, [open, refreshAssistant]);

  useEffect(() => {
    if (!open || panelView !== "document") return undefined;
    let cancelled = false;
    const poll = () => {
      if (cancelled) return;
      if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
      void refreshAssistant({
        quiet: true,
        passive: true,
        refreshChangedActiveDocumentContent: true,
      });
      for (const sessionId of pendingDiarizationSessionsRef.current) {
        void restoreLatestVoiceMeetingSession({ replaceSession: true, sessionId }).then((ready) => {
          if (ready) pendingDiarizationSessionsRef.current.delete(sessionId);
        });
      }
    };
    const timer = window.setInterval(poll, VOICE_DOCUMENT_METADATA_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [panelView, open, refreshAssistant, restoreLatestVoiceMeetingSession]);

  useEffect(() => {
    if (!latestVoiceLedgerEvent) return;
    const kind = String(latestVoiceLedgerEvent.kind || "").trim();
    if (
      !open &&
      !pendingAskRequestId &&
      !pendingPromptRequestId &&
      !localVoiceReplyRequestIdsRef.current.size
    )
      return;
    const eventId = String(latestVoiceLedgerEvent.id || "").trim();
    const eventKey = eventId || [kind, String(latestVoiceLedgerEvent.ts || "").trim()].join(":");
    if (!eventKey || eventKey === lastVoiceLedgerSignalRef.current) return;
    lastVoiceLedgerSignalRef.current = eventKey;
    void refreshAssistant({ quiet: true, passive: true });
    if (!open) return;
    const dataRecord =
      latestVoiceLedgerEvent.data && typeof latestVoiceLedgerEvent.data === "object"
        ? (latestVoiceLedgerEvent.data as Record<string, unknown>)
        : null;
    if (kind === "assistant.voice.session" && dataRecord) {
      const action = String(dataRecord.action || "").trim();
      const sessionId = String(dataRecord.session_id || "").trim();
      if (action === "diarization_ready") {
        pendingDiarizationSessionsRef.current.delete(sessionId);
        if (panelView !== "document") return;
        window.setTimeout(() => {
          void restoreLatestVoiceMeetingSession({ replaceSession: true, sessionId });
        }, 250);
      } else if (action === "diarization_failed") {
        pendingDiarizationSessionsRef.current.delete(sessionId);
        if (panelView !== "document") return;
        const documentPath = String(
          transcriptDocumentPathRef.current || captureTargetDocumentPathRef.current || "",
        ).trim();
        if (sessionId && documentPath) {
          const now =
            assistantVoiceTimestampMs(String(latestVoiceLedgerEvent.ts || "")) || Date.now();
          const message =
            String(dataRecord.error_message || "").trim() ||
            t("voiceSecretaryTranscriptFinalFailed", {
              defaultValue: "Final audio analysis failed.",
            });
          setVoiceTranscriptItems((prev) =>
            replaceVoiceTranscriptProcessingItem(prev, {
              id: `${sessionId}-analysis-failed`,
              sessionId,
              phase: "interim",
              text: message,
              mode: "document",
              documentPath,
              language: getRecordingRecognitionLanguage(),
              processingPhase: "failed",
              updatedAt: now,
              createdAt: now,
            }),
          );
        }
      }
    }
  }, [
    latestVoiceLedgerEvent,
    open,
    pendingAskRequestId,
    pendingPromptRequestId,
    refreshAssistant,
    restoreLatestVoiceMeetingSession,
    panelView,
    getRecordingRecognitionLanguage,
    t,
  ]);

  const receivePromptStatus = useCallback(
    (gid: string, requestId: string, result: AssistantStateResult) => {
      if (
        pendingPromptRequestIdRef.current !== requestId ||
        pendingPromptGroupIdRef.current !== gid
      )
        return;
      const task = result.secretary_tasks?.find(
        (item) => item.target.request_id === requestId && !item.superseded_by,
      );
      const health = result.assistant?.health?.secretary as Partial<SecretaryReadiness> | undefined;
      const issue = secretaryReadinessIssue(health);
      setPendingPromptTask(task || null);
      if (task) {
        setPendingPromptProblem(
          task.projection_error ||
            (["needs_user", "conflict", "failed", "cancelled", "unconfirmed"].includes(task.phase)
              ? task.diagnostic ||
                task.receipt?.output.reply_text ||
                t(`settings:voiceSettings.phases.${task.phase}`)
              : ""),
        );
      } else if (issue) {
        setPendingPromptProblem(
          [t(`settings:${issue.titleKey}`), issue.detail].filter(Boolean).join(" · "),
        );
      }
      const draft = result.prompt_draft;
      if (draft?.request_id === requestId) {
        setPendingPromptDraft(draft);
        setPendingPromptProblem("");
      }
    },
    [t],
  );
  const receivePromptError = useCallback((groupId: string, requestId: string, message: string) => {
    if (
      pendingPromptGroupIdRef.current === groupId &&
      pendingPromptRequestIdRef.current === requestId
    )
      setPendingPromptProblem(message);
  }, []);
  useVoiceRequestStatus({
    groupId: pendingPromptGroupIdRef.current,
    requestId: pendingPromptRequestId,
    enabled: !pendingPromptDraft,
    onStatus: receivePromptStatus,
    onError: receivePromptError,
  });

  useEffect(() => {
    const hasVisibleTranscript = liveTranscriptPreview
      ? recording || Date.now() - liveTranscriptPreview.updatedAt < VOICE_LIVE_TRANSCRIPT_VISIBLE_MS
      : false;
    if (!hasVisibleTranscript) return undefined;
    if (typeof window === "undefined") return undefined;
    const timer = window.setInterval(() => {
      setActivityClockMs(Date.now());
    }, 1000);
    return () => {
      window.clearInterval(timer);
    };
  }, [liveTranscriptPreview, recording]);

  const acknowledgePromptDraft = useCallback(
    async (
      draft: AssistantVoicePromptDraft,
      status: "applied" | "dismissed" | "stale",
      targetGroupId?: string,
    ) => {
      const gid = String(targetGroupId || selectedGroupId || "").trim();
      if (!gid || !draft.request_id) return;
      const resp = await ackVoiceAssistantPromptDraft(gid, {
        requestId: draft.request_id,
        status,
        by: "user",
      });
      if (!resp.ok) throw new Error(resp.error.message);
      if (isCurrentGroup(gid) && resp.ok && resp.result.assistant) {
        setAssistant(resp.result.assistant);
      }
    },
    [isCurrentGroup, selectedGroupId],
  );

  const applyPromptDraft = useCallback(
    async (draft: AssistantVoicePromptDraft, reviewed = false) => {
      const groupId = pendingPromptGroupIdRef.current;
      if (!groupId || draft.request_id !== pendingPromptRequestIdRef.current) return;
      if (draft.status === "no_change") {
        clearPendingPromptRequest(true);
        showNotice({ message: t("voiceSecretaryPromptDraftNoChange") });
        return;
      }
      const text = String(draft.draft_text || "").trim();
      if (!text || draft.status !== "pending") return;
      const applyMode = promptDraftApplyMode(draft);
      const expectedSnapshotHash = pendingPromptComposerHashRef.current;
      if (
        !reviewed &&
        draft.composer_snapshot_hash &&
        draft.composer_snapshot_hash !== expectedSnapshotHash
      ) {
        setPromptNeedsReview(true);
        return;
      }
      const applied = routeVoiceTextToComposerGroup({
        groupId,
        text,
        mode: applyMode,
        expectedSnapshotHash: reviewed ? undefined : expectedSnapshotHash,
      });
      if (applied === "changed") {
        setPromptNeedsReview(true);
        return;
      }
      if (applied === "ignored") return;
      clearPendingPromptRequest(true);
      try {
        await acknowledgePromptDraft(draft, "applied", groupId);
      } catch {
        showError(t("voiceSecretaryPromptDraftAckFailed"));
        return;
      }
      showNotice({
        message:
          applyMode === "replace"
            ? t("voiceSecretaryPromptDraftReplaced", {
                defaultValue: "Refined prompt replaced the composer.",
              })
            : t("voiceSecretaryPromptDraftFilled", {
                defaultValue: "Refined prompt appended to the composer.",
              }),
      });
    },
    [acknowledgePromptDraft, clearPendingPromptRequest, showError, showNotice, t],
  );

  useEffect(() => {
    if (!pendingPromptDraft || promptNeedsReview) return;
    const requested = String(
      pendingPromptRequestIdRef.current || pendingPromptRequestId || "",
    ).trim();
    if (!requested || pendingPromptDraft.request_id !== requested) return;
    void applyPromptDraft(pendingPromptDraft);
  }, [applyPromptDraft, pendingPromptDraft, pendingPromptRequestId, promptNeedsReview]);

  const dismissPromptDraft = async () => {
    const draft = pendingPromptDraft;
    const groupId = pendingPromptGroupIdRef.current;
    if (!draft || !groupId) return;
    try {
      await acknowledgePromptDraft(draft, "dismissed", groupId);
      if (pendingPromptRequestIdRef.current === draft.request_id) clearPendingPromptRequest(true);
    } catch (error) {
      showError(error instanceof Error ? error.message : String(error));
    }
  };

  useEffect(() => {
    if (!open || !serviceAsrReady) return;
    void loadAudioDevices();
  }, [loadAudioDevices, open, serviceAsrReady]);

  useEffect(() => {
    const ownership = resetVoiceAssistantVisibleLoading({
      dataSeq: refreshSeq.current,
      visibleLoadingSeq: visibleLoadSeqRef.current,
    });
    refreshSeq.current = ownership.dataSeq;
    visibleLoadSeqRef.current = ownership.visibleLoadingSeq;
    const keepActiveRecording = recordingRef.current;
    const keepPendingPromptRequest = Boolean(pendingPromptRequestIdRef.current.trim());
    if (!keepActiveRecording) {
      const recognition = recognitionRef.current;
      recognitionRef.current = null;
      abortBrowserSpeechRecognition(recognition);
      browserSpeechStopRequestedRef.current = true;
      browserSpeechRecoveryRef.current.reset();
      const recorder = mediaRecorderRef.current;
      mediaRecorderRef.current = null;
      if (recorder && recorder.state !== "inactive") {
        recorder.onstop = null;
        try {
          recorder.stop();
        } catch {
          // ignore browser cleanup failure
        }
      }
      const cleanupBrowserSpeechMedia = browserSpeechMediaCleanupRef.current;
      browserSpeechMediaCleanupRef.current = null;
      if (cleanupBrowserSpeechMedia) cleanupBrowserSpeechMedia();
      resetVoiceAudioLevel();
      stopMediaStream(mediaStreamRef.current);
      mediaStreamRef.current = null;
      mediaChunksRef.current = [];
    }
    setOpen(false);
    setLoading(false);
    setActionBusy("");
    setShowAssistantModeMenu(false);
    setShowAssistantLanguageMenu(false);
    setAssistant(null);
    setDocuments([]);
    archivedDocumentPathsRef.current.clear();
    documentContentLoadTracker.current.reset();
    setDocumentContentLoadingPath("");
    setViewedDocumentPath("");
    setCaptureTargetDocumentPath("");
    loadDocumentDraft(null);
    setDocumentEditing(false);
    setDocumentInstruction("");
    setExecutionTaskId("");
    if (!keepActiveRecording) {
      setRecording(false);
      setRecordingTarget(null);
    }
    setSpeechError("");
    if (!keepActiveRecording) {
      if (!keepPendingPromptRequest) clearPendingPromptRequest(true);
      pendingAskRequestIdRef.current = "";
      dismissedVoiceReplyKeysRef.current.clear();
      localVoiceReplyRequestIdsRef.current.clear();
      askFeedbackReplyKeyByRequestIdRef.current.clear();
      setPendingAskRequestId("");
      setAskFeedbackItems([]);
      liveTranscriptPreviewRef.current = null;
      setLiveTranscriptPreview(null);
      setVoiceTranscriptItems([]);
    }
    setVoiceWorkspaceView("document");
    voiceStreamItemIdRef.current = "";
    setActivityClockMs(Date.now());
    setVoiceReplyBubbleRequestId("");
    setCopiedVoiceReplyRequestId("");
    const nextAskNotice = readVoiceAskNotice(selectedGroupId);
    askNoticeRef.current = nextAskNotice;
    setAskNotice(nextAskNotice);
    pendingAskTaskRef.current = null;
    setPendingAskTask(null);
    setAskFeedbackItems([]);
    setAskStatusReadError("");
    pendingAskRequestIdRef.current = nextAskNotice.requestId;
    setPendingAskRequestId(nextAskNotice.requestId);
    askObservationRevisionRef.current++;
    if (!keepActiveRecording) {
      releaseVoiceRecordingGuards();
      if (transcriptFlushTimerRef.current !== null) {
        window.clearTimeout(transcriptFlushTimerRef.current);
        transcriptFlushTimerRef.current = null;
      }
      if (transcriptMaxFlushTimerRef.current !== null) {
        window.clearTimeout(transcriptMaxFlushTimerRef.current);
        transcriptMaxFlushTimerRef.current = null;
      }
      if (servicePartialCommitTimerRef.current !== null) {
        window.clearTimeout(servicePartialCommitTimerRef.current);
        servicePartialCommitTimerRef.current = null;
      }
      if (serviceDocumentCheckpointTimerRef.current !== null) {
        window.clearTimeout(serviceDocumentCheckpointTimerRef.current);
        serviceDocumentCheckpointTimerRef.current = null;
      }
      browserFinalTranscriptBufferRef.current = "";
      serviceLatestPartialTranscriptRef.current = "";
      serviceCommittedTranscriptRef.current = "";
      serviceDocumentCommittedTranscriptRef.current = "";
      serviceFinalAsrTextRef.current = "";
      serviceAudioResamplerRef.current = null;
      serviceAudioDurationMsRef.current = 0;
      serviceCommittedEndMsRef.current = 0;
      serviceDocumentCommittedEndMsRef.current = 0;
      serviceFinalSpeakerSegmentsRef.current = [];
      serviceProvisionalSpeakerSegmentsRef.current = [];
      if (browserSpeechRestartTimerRef.current !== null) {
        window.clearTimeout(browserSpeechRestartTimerRef.current);
        browserSpeechRestartTimerRef.current = null;
      }
      if (browserSpeechStopFinalizeTimerRef.current !== null) {
        window.clearTimeout(browserSpeechStopFinalizeTimerRef.current);
        browserSpeechStopFinalizeTimerRef.current = null;
      }
    }
  }, [
    clearPendingPromptRequest,
    loadDocumentDraft,
    releaseVoiceRecordingGuards,
    selectedGroupId,
    resetVoiceAudioLevel,
  ]);

  useEffect(() => {
    if (initiallyOpen) setOpen(true);
  }, [initiallyOpen]);
  useEffect(() => {
    if (!selectedGroupId) return;
    void refreshAssistant({ quiet: true });
  }, [refreshAssistant, selectedGroupId]);

  const clearTranscriptFlushTimer = useCallback(() => {
    if (transcriptFlushTimerRef.current === null) return;
    window.clearTimeout(transcriptFlushTimerRef.current);
    transcriptFlushTimerRef.current = null;
  }, []);

  const clearTranscriptMaxFlushTimer = useCallback(() => {
    if (transcriptMaxFlushTimerRef.current === null) return;
    window.clearTimeout(transcriptMaxFlushTimerRef.current);
    transcriptMaxFlushTimerRef.current = null;
  }, []);

  const clearBrowserSpeechRestartTimer = useCallback(() => {
    if (browserSpeechRestartTimerRef.current === null) return;
    window.clearTimeout(browserSpeechRestartTimerRef.current);
    browserSpeechRestartTimerRef.current = null;
  }, []);

  const clearBrowserSpeechStopFinalizeTimer = useCallback(() => {
    if (browserSpeechStopFinalizeTimerRef.current === null) return;
    window.clearTimeout(browserSpeechStopFinalizeTimerRef.current);
    browserSpeechStopFinalizeTimerRef.current = null;
  }, []);

  const clearServicePartialCommitTimer = useCallback(() => {
    if (servicePartialCommitTimerRef.current === null) return;
    window.clearTimeout(servicePartialCommitTimerRef.current);
    servicePartialCommitTimerRef.current = null;
  }, []);

  const clearServiceDocumentCheckpointTimer = useCallback(() => {
    if (serviceDocumentCheckpointTimerRef.current === null) return;
    window.clearTimeout(serviceDocumentCheckpointTimerRef.current);
    serviceDocumentCheckpointTimerRef.current = null;
  }, []);

  const clearBrowserSpeechMediaHandlers = useCallback(() => {
    const cleanup = browserSpeechMediaCleanupRef.current;
    browserSpeechMediaCleanupRef.current = null;
    if (cleanup) cleanup();
  }, []);

  const attachMediaStreamDiagnostics = useCallback(
    (stream: MediaStream, opts: { backend: string; groupId: string; runId: number }) => {
      const cleanupCallbacks: Array<() => void> = [];
      stream.getAudioTracks().forEach((track, index) => {
        const previousEnded = track.onended;
        const previousMute = track.onmute;
        const previousUnmute = track.onunmute;
        const trackLabel = String(track.label || `audio-track-${index + 1}`).trim();
        const describeTrack = () => `${trackLabel} (${track.readyState || "unknown"})`;
        const onEnded = (event: Event) => {
          if (typeof previousEnded === "function") previousEnded.call(track, event);
          if (!isActiveRecordingRun(opts.runId)) return;
          reportRecordingStopReason("media_track_ended", {
            backend: opts.backend,
            groupId: opts.groupId,
            runId: opts.runId,
            detail: describeTrack(),
          });
          setSpeechError(
            t("voiceSecretaryRecordingStoppedTrackEnded", {
              detail: trackLabel,
              defaultValue: "Microphone input ended unexpectedly: {{detail}}.",
            }),
          );
        };
        const onMute = (event: Event) => {
          if (typeof previousMute === "function") previousMute.call(track, event);
          if (!isActiveRecordingRun(opts.runId)) return;
          reportRecordingStopReason("media_track_muted", {
            backend: opts.backend,
            groupId: opts.groupId,
            runId: opts.runId,
            detail: describeTrack(),
          });
        };
        const onUnmute = (event: Event) => {
          if (typeof previousUnmute === "function") previousUnmute.call(track, event);
          if (!isActiveRecordingRun(opts.runId)) return;
          reportRecordingStopReason("media_track_unmuted", {
            backend: opts.backend,
            groupId: opts.groupId,
            runId: opts.runId,
            detail: describeTrack(),
          });
        };
        track.onended = onEnded;
        track.onmute = onMute;
        track.onunmute = onUnmute;
        cleanupCallbacks.push(() => {
          if (track.onended === onEnded) track.onended = previousEnded;
          if (track.onmute === onMute) track.onmute = previousMute;
          if (track.onunmute === onUnmute) track.onunmute = previousUnmute;
        });
      });
      return () => {
        cleanupCallbacks.forEach((cleanup) => cleanup());
      };
    },
    [isActiveRecordingRun, reportRecordingStopReason, t],
  );

  const applyTranscriptAppendResult = useCallback(
    (result: AssistantVoiceTranscriptSegmentResult) => {
      if (result.assistant) setAssistant(result.assistant);
      if ((result.document_updated || result.input_event_created) && result.document) {
        const document = result.document;
        const docPath = voiceDocumentPath(document);
        if (
          docPath &&
          !archivedDocumentPathsRef.current.has(docPath) &&
          String(document.status || "active").trim() !== "archived"
        ) {
          invalidateWorkspaceRefresh();
          if (viewedDocumentPathRef.current === docPath) {
            documentContentLoadTracker.current.reset();
            setDocumentContentLoadingPath("");
          }
          setDocuments((prev) => {
            const index = prev.findIndex((item) => voiceDocumentPath(item) === docPath);
            if (index < 0) return [document, ...prev];
            const next = [...prev];
            next[index] = document;
            return next;
          });
          if (!captureTargetDocumentPathRef.current) {
            captureTargetDocumentPathRef.current = docPath;
            setCaptureTargetDocumentPath(docPath);
          }
          const viewingDocumentPath = String(viewedDocumentPathRef.current || "").trim();
          const localDirty = documentDraftRef.current !== documentBaseContentRef.current;
          if (!viewingDocumentPath || viewingDocumentPath === docPath) {
            setViewedDocumentPath(docPath);
            if (!localDirty || !viewingDocumentPath) loadDocumentDraft(document);
            else setDocumentRemoteChanged(true);
          }
        }
      }
    },
    [invalidateWorkspaceRefresh, loadDocumentDraft],
  );

  const applyDocumentMutationResult = useCallback(
    (document: AssistantVoiceDocument | undefined, assistantNext?: BuiltinAssistant) => {
      if (assistantNext) setAssistant(assistantNext);
      if (!document) return;
      const docPath = voiceDocumentPath(document);
      if (!docPath) return;
      // An accepted write is newer than every workspace read already in flight.
      invalidateWorkspaceRefresh();
      if (viewedDocumentPathRef.current === docPath) {
        documentContentLoadTracker.current.reset();
        setDocumentContentLoadingPath("");
      }
      setDocuments((prev) => {
        const index = prev.findIndex((item) => voiceDocumentPath(item) === docPath);
        if (index < 0) return [document, ...prev];
        const next = [...prev];
        next[index] = document;
        return next;
      });
      if (!viewedDocumentPathRef.current || viewedDocumentPathRef.current === docPath) {
        setViewedDocumentPath(docPath);
        loadDocumentDraft(document);
      }
    },
    [invalidateWorkspaceRefresh, loadDocumentDraft],
  );

  const appendTranscriptSegment = useCallback(
    async (
      text: string,
      opts?: {
        flush?: boolean;
        triggerKind?: string;
        source?: string;
        inputDeviceLabel?: string;
        documentPath?: string;
        startMs?: number;
        endMs?: number;
        speakerSegments?: Record<string, unknown>[];
      },
    ): Promise<boolean> => {
      const sessionScope = voiceRecordingSessionScopeRef.current;
      const gid = recordingTargetGroupId();
      if (!gid || (!sessionScope && !captureNeedsSecretary)) return false;
      const cleanText = String(text || "").trim();
      const flush = Boolean(opts?.flush);
      if (!cleanText && !flush) return false;
      const targetDocumentPath = recordingTargetDocumentPath(
        opts?.documentPath || captureTargetDocumentPathRef.current,
      );
      const segmentSeq = transcriptSegmentSeqRef.current + 1;
      transcriptSegmentSeqRef.current = segmentSeq;
      try {
        const resp = await appendVoiceAssistantTranscriptSegment(gid, {
          sessionId: voiceRecordingSessionIdRef.current,
          segmentId: cleanText ? `seg-${segmentSeq}` : "",
          documentPath: targetDocumentPath,
          text: cleanText,
          language: getRecordingRecognitionLanguage(),
          isFinal: true,
          flush,
          startMs: opts?.startMs,
          endMs: opts?.endMs,
          trigger: {
            mode: "meeting",
            trigger_kind: opts?.triggerKind || (flush ? "push_to_talk_stop" : "meeting_window"),
            capture_mode: serviceAsrReady ? "service" : "browser",
            recognition_backend: opts?.source || recognitionBackend,
            client_session_id: voiceRecordingSessionIdRef.current,
            input_device_label:
              opts?.inputDeviceLabel ||
              (serviceAsrReady ? selectedAudioDeviceLabel : BROWSER_DEFAULT_MIC_LABEL),
            language: getRecordingRecognitionLanguage(),
            document_path: targetDocumentPath,
            speaker_segments: opts?.speakerSegments || [],
          },
          by: "user",
        });
        if (!resp.ok) {
          if (isCurrentGroup(gid)) showError(resp.error.message);
          return false;
        }
        if (isCurrentGroup(gid)) {
          applyTranscriptAppendResult(resp.result);
          const resultDocumentPath = voiceDocumentPath(resp.result.document) || targetDocumentPath;
          if (cleanText && resultDocumentPath) {
            pushVoiceTranscriptItem(cleanText, {
              documentPath: resultDocumentPath,
              startMs: opts?.startMs,
              endMs: opts?.endMs,
            });
          }
        }
        return true;
      } catch {
        if (!isCurrentGroup(gid)) return false;
        showError(
          t("voiceSecretaryTranscriptAppendFailed", {
            defaultValue: "Failed to save Voice Secretary transcript segment.",
          }),
        );
        return false;
      }
    },
    [
      applyTranscriptAppendResult,
      captureNeedsSecretary,
      getRecordingRecognitionLanguage,
      isCurrentGroup,
      pushVoiceTranscriptItem,
      recognitionBackend,
      recordingTargetDocumentPath,
      recordingTargetGroupId,
      selectedAudioDeviceLabel,
      serviceAsrReady,
      showError,
      t,
    ],
  );

  const flushServiceDocumentCheckpoint = useCallback(
    async (triggerKind = "max_window"): Promise<boolean> => {
      clearServiceDocumentCheckpointTimer();
      if (serviceAsrBackendRef.current === "external_provider_asr") return true;
      if (
        voiceRecordingCaptureMode(voiceRecordingSessionScopeRef.current, captureMode) !== "document"
      )
        return false;
      const committedDocumentText = normalizeBrowserTranscriptChunk(
        serviceDocumentCommittedTranscriptRef.current,
      );
      const finalAsrText = normalizeBrowserTranscriptChunk(serviceFinalAsrTextRef.current);
      const streamingText = normalizeBrowserTranscriptChunk(
        serviceCommittedTranscriptRef.current || serviceFinalTranscriptRef.current,
      );
      const stableText =
        committedDocumentText && finalAsrText.startsWith(committedDocumentText)
          ? finalAsrText
          : streamingText;
      const newText = nextUncommittedServiceTranscriptText(stableText, committedDocumentText);
      if (!newText) return false;
      const startMs = serviceDocumentCommittedEndMsRef.current;
      const endMs = Math.max(
        startMs,
        serviceCommittedEndMsRef.current || serviceAudioDurationMsRef.current,
      );
      const ok = await appendTranscriptSegment(newText, {
        flush: true,
        triggerKind,
        source: "assistant_service_local_asr_streaming",
        inputDeviceLabel: selectedAudioDeviceLabel,
        documentPath: recordingTargetDocumentPath(effectiveCaptureTargetDocumentPath),
        startMs,
        endMs,
        speakerSegments: serviceFinalSpeakerSegmentsRef.current.length
          ? serviceFinalSpeakerSegmentsRef.current
          : serviceProvisionalSpeakerSegmentsRef.current,
      });
      if (!ok) return false;
      serviceDocumentCommittedTranscriptRef.current = mergeTranscriptChunks(
        committedDocumentText,
        newText,
      );
      serviceDocumentCommittedEndMsRef.current = endMs;
      return true;
    },
    [
      appendTranscriptSegment,
      captureMode,
      clearServiceDocumentCheckpointTimer,
      effectiveCaptureTargetDocumentPath,
      recordingTargetDocumentPath,
      selectedAudioDeviceLabel,
    ],
  );

  const scheduleServiceDocumentCheckpoint = useCallback(() => {
    if (serviceAsrBackendRef.current === "external_provider_asr") return;
    if (
      voiceRecordingCaptureMode(voiceRecordingSessionScopeRef.current, captureMode) !== "document"
    )
      return;
    if (autoDocumentMaxWindowMs === null) return;
    if (serviceDocumentCheckpointTimerRef.current !== null) return;
    serviceDocumentCheckpointTimerRef.current = window.setTimeout(() => {
      serviceDocumentCheckpointTimerRef.current = null;
      void flushServiceDocumentCheckpoint("max_window");
    }, autoDocumentMaxWindowMs);
  }, [autoDocumentMaxWindowMs, captureMode, flushServiceDocumentCheckpoint]);

  const sendInstructionTranscript = useCallback(
    async (
      text: string,
      opts?: {
        triggerKind?: string;
        targetGroupId?: string;
        documentPath?: string;
        referenceDocument?: boolean;
      },
    ): Promise<boolean> => {
      const explicitGroupId = String(opts?.targetGroupId || "").trim();
      const sessionScope = explicitGroupId ? null : voiceRecordingSessionScopeRef.current;
      const gid = explicitGroupId || recordingTargetGroupId();
      const instruction = normalizeBrowserTranscriptChunk(text);
      if (!gid || (!sessionScope && !captureNeedsSecretary) || !instruction) return false;
      const requestId = `voice-ask-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
      const fallbackDocumentPath =
        opts?.documentPath ||
        captureTargetDocumentPathRef.current ||
        activeDocumentWritePath ||
        viewedDocumentPath;
      const currentDocumentPath = explicitGroupId
        ? String(fallbackDocumentPath || "").trim()
        : recordingTargetDocumentPath(fallbackDocumentPath);
      try {
        const resp = await appendVoiceAssistantInput(gid, {
          kind: "voice_instruction",
          taskKind: "ask",
          documentPath: opts?.referenceDocument ? currentDocumentPath || undefined : undefined,
          instruction,
          requestId,
          inputAppendId: requestId,
          trigger: {
            trigger_kind: opts?.triggerKind || "voice_instruction",
            mode: "voice_instruction",
            target_kind: "secretary",
            current_document_path: currentDocumentPath,
            recognition_backend: String(
              getActiveCaptureConfig()?.recognition_backend || recognitionBackend,
            ),
            language: getRecordingRecognitionLanguage(),
          },
          by: "user",
        });
        if (!resp.ok) {
          if (isCurrentGroup(gid)) showError(resp.error.message);
          return false;
        }
        const nextRequestId = String(resp.result.request_id || requestId).trim();
        rememberAskRequest(gid, nextRequestId);
        if (isCurrentGroup(gid)) {
          setAskFeedbackItems((prev) =>
            [
              {
                request_id: nextRequestId,
                status: "pending",
                request_text: instruction,
                request_preview: instruction.slice(0, 240),
                target_kind: "secretary",
                created_at: new Date().toISOString(),
                updated_at: new Date().toISOString(),
              },
              ...prev.filter((item) => item.request_id !== nextRequestId),
            ].slice(0, 10),
          );
          finalizeLiveTranscriptPreview();
          applyDocumentMutationResult(resp.result.document, resp.result.assistant);
          showNotice({
            message: t(
              resp.result.secretary_processing_deferred
                ? "voiceSecretaryInputSavedForLater"
                : "voiceSecretaryDocumentInstructionQueued",
              { defaultValue: "Request sent to Voice Secretary." },
            ),
          });
          void refreshAssistant({ quiet: true });
        }
        return true;
      } catch {
        if (!isCurrentGroup(gid)) return false;
        showError(
          t("voiceSecretaryDocumentInstructionFailed", {
            defaultValue: "Failed to send the request to Voice Secretary.",
          }),
        );
        return false;
      }
    },
    [
      viewedDocumentPath,
      activeDocumentWritePath,
      applyDocumentMutationResult,
      captureNeedsSecretary,
      getActiveCaptureConfig,
      getRecordingRecognitionLanguage,
      finalizeLiveTranscriptPreview,
      isCurrentGroup,
      recognitionBackend,
      recordingTargetDocumentPath,
      recordingTargetGroupId,
      refreshAssistant,
      rememberAskRequest,
      showError,
      showNotice,
      t,
    ],
  );

  const requestPromptRefine = useCallback(
    async (
      text: string,
      triggerKind = "prompt_refine",
      opts?: {
        operation?: "append_to_composer_end" | "replace_with_refined_prompt";
        targetGroupId?: string;
        composerText?: string;
        composerContext?: Record<string, unknown>;
      },
    ) => {
      const explicitGroupId = String(opts?.targetGroupId || "").trim();
      const sessionScope = explicitGroupId ? null : voiceRecordingSessionScopeRef.current;
      const gid = explicitGroupId || recordingTargetGroupId();
      const voiceTranscript = normalizeBrowserTranscriptChunk(text);
      const snapshot = String(
        explicitGroupId
          ? (opts?.composerText ?? composerText ?? "")
          : (sessionScope?.composerText ?? composerText ?? ""),
      );
      const requestComposerContext = explicitGroupId
        ? (opts?.composerContext ?? composerContext)
        : (sessionScope?.composerContext ?? composerContext);
      if (!gid || (!voiceTranscript && !snapshot.trim())) return;
      const operation = opts?.operation || "append_to_composer_end";
      const snapshotHash = hashComposerSnapshot(snapshot);
      const nowMs = Date.now();
      const existingRequestId = String(
        pendingPromptRequestIdRef.current || pendingPromptRequestId || "",
      ).trim();
      const requestOwnership = voicePromptRequestOwnership({
        requestId: existingRequestId,
        pendingGroupId: pendingPromptGroupIdRef.current,
        targetGroupId: gid,
        task: pendingPromptTask,
      });
      if (requestOwnership === "other_group") {
        if (isCurrentGroup(gid)) {
          showError(
            t("voiceSecretaryPromptPendingOtherGroup", {
              defaultValue:
                "Another Group already has a prompt refinement in progress. Wait for it to finish before starting another.",
            }),
          );
        }
        return;
      }
      if (requestOwnership === "none" && existingRequestId) {
        clearPendingPromptRequest(true);
      }
      const reuseExistingRequest = Boolean(
        requestOwnership === "same_group" &&
        operation === "append_to_composer_end" &&
        !pendingPromptDraft &&
        !pendingPromptProblem,
      );
      const requestId = reuseExistingRequest
        ? existingRequestId
        : `voice-prompt-${nowMs.toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
      const inputAppendId = `voice-prompt-input-${nowMs.toString(36)}-${Math.random()
        .toString(36)
        .slice(2, 8)}`;
      pendingPromptRequestIdRef.current = requestId;
      pendingPromptComposerHashRef.current = snapshotHash;
      pendingPromptGroupIdRef.current = gid;
      setPendingPromptRequestId(requestId);
      setPendingPromptProblem("");
      setPendingPromptTask(null);
      setPendingPromptDraft(null);
      setPromptNeedsReview(false);
      try {
        const resp = await appendVoiceAssistantInput(gid, {
          kind: "prompt_refine",
          voiceTranscript,
          composerText: snapshot,
          requestId,
          inputAppendId,
          operation,
          composerSnapshotHash: snapshotHash,
          composerContext: requestComposerContext,
          language: getRecordingRecognitionLanguage(),
          trigger: {
            trigger_kind: triggerKind,
            mode: "prompt",
            recognition_backend: String(
              getActiveCaptureConfig()?.recognition_backend || recognitionBackend,
            ),
            language: getRecordingRecognitionLanguage(),
          },
          by: "user",
        });
        if (
          pendingPromptRequestIdRef.current !== requestId ||
          pendingPromptGroupIdRef.current !== gid
        )
          return;
        if (!resp.ok) {
          clearPendingPromptRequest(false);
          if (isCurrentGroup(gid)) showError(resp.error.message);
          return;
        }
        if (!isCurrentGroup(gid)) return;
        if (resp.result.assistant) setAssistant(resp.result.assistant);
        finalizeLiveTranscriptPreview();
        const deferred = resp.result.secretary_processing_deferred;
        if (deferred || resp.result.secretary_processing_error) {
          setPendingPromptProblem(
            resp.result.secretary_processing_error || t("voiceSecretaryInputSavedForLater"),
          );
        }
        showNotice({
          message:
            deferred || resp.result.secretary_processing_error
              ? t("voiceSecretaryInputSavedForLater")
              : operation === "replace_with_refined_prompt"
                ? t("voiceSecretaryPromptOptimizeQueued", {
                    defaultValue: "Prompt optimization queued.",
                  })
                : t("voiceSecretaryPromptRefineQueued", {
                    defaultValue: "Prompt refinement queued.",
                  }),
        });
        void refreshAssistant({ quiet: true });
      } catch {
        if (
          pendingPromptRequestIdRef.current !== requestId ||
          pendingPromptGroupIdRef.current !== gid
        )
          return;
        clearPendingPromptRequest(false);
        if (!isCurrentGroup(gid)) return;
        showError(
          t("voiceSecretaryPromptRefineFailed", {
            defaultValue: "Failed to send prompt refinement to Voice Secretary.",
          }),
        );
      }
    },
    [
      clearPendingPromptRequest,
      composerContext,
      composerText,
      getActiveCaptureConfig,
      getRecordingRecognitionLanguage,
      finalizeLiveTranscriptPreview,
      isCurrentGroup,
      pendingPromptRequestId,
      pendingPromptDraft,
      pendingPromptProblem,
      pendingPromptTask,
      recognitionBackend,
      recordingTargetGroupId,
      refreshAssistant,
      showError,
      showNotice,
      t,
    ],
  );

  const appendDirectDictationToComposer = useCallback(
    (text: string): boolean => {
      const transcript = String(text || "").trim();
      if (!transcript || !onPromptDraft) return false;
      const groupId = recordingTargetGroupId();
      if (isCurrentGroup(groupId)) {
        onPromptDraft(transcript, { mode: "append" });
      } else {
        routeVoiceTextToComposerGroup({ groupId, text: transcript, mode: "append" });
      }
      finalizeLiveTranscriptPreview();
      showNotice({
        message: t("voiceDirectDictationFilled", {
          defaultValue: "Speech transcript appended to the composer.",
        }),
      });
      return true;
    },
    [
      finalizeLiveTranscriptPreview,
      isCurrentGroup,
      onPromptDraft,
      recordingTargetGroupId,
      showNotice,
      t,
    ],
  );

  const takeBrowserFinalTranscriptBuffer = useCallback((): string => {
    const text = String(browserFinalTranscriptBufferRef.current || "").trim();
    browserFinalTranscriptBufferRef.current = "";
    return text;
  }, []);

  const flushBrowserTranscriptWindow = useCallback(
    async (triggerKind = "meeting_window", opts?: { documentPath?: string }): Promise<void> => {
      clearTranscriptFlushTimer();
      clearTranscriptMaxFlushTimer();
      const dispatchTarget = voiceRecordingDispatchTarget(
        voiceRecordingSessionScopeRef.current,
        captureDispatchTarget,
      );
      const documentPath = recordingTargetDocumentPath(
        opts?.documentPath || captureTargetDocumentPathRef.current,
      );
      if (dispatchTarget !== "document" && triggerKind !== "push_to_talk_stop") {
        return;
      }
      const text = takeBrowserFinalTranscriptBuffer();
      if (dispatchTarget === "composer") {
        appendDirectDictationToComposer(text);
        return;
      }
      if (dispatchTarget === "prompt") {
        if (!isMeaningfulVoiceDispatchText(text)) return;
        await requestPromptRefine(text, triggerKind || "prompt_refine");
        return;
      }
      if (dispatchTarget === "instruction") {
        if (!isMeaningfulVoiceDispatchText(text)) return;
        await sendInstructionTranscript(text, { triggerKind });
        return;
      }
      await appendTranscriptSegment(text, {
        flush: true,
        triggerKind,
        source: "browser_asr",
        inputDeviceLabel: BROWSER_DEFAULT_MIC_LABEL,
        documentPath,
      });
      finalizeLiveTranscriptPreview();
    },
    [
      appendTranscriptSegment,
      appendDirectDictationToComposer,
      captureDispatchTarget,
      clearTranscriptFlushTimer,
      clearTranscriptMaxFlushTimer,
      finalizeLiveTranscriptPreview,
      requestPromptRefine,
      recordingTargetDocumentPath,
      sendInstructionTranscript,
      takeBrowserFinalTranscriptBuffer,
    ],
  );

  const scheduleTranscriptMaxFlush = useCallback(
    (triggerKind: string) => {
      if (
        voiceRecordingDispatchTarget(
          voiceRecordingSessionScopeRef.current,
          captureDispatchTarget,
        ) !== "document"
      )
        return;
      if (autoDocumentMaxWindowMs === null) return;
      if (transcriptMaxFlushTimerRef.current !== null) return;
      const documentPath = captureTargetDocumentPathRef.current;
      transcriptMaxFlushTimerRef.current = window.setTimeout(() => {
        transcriptMaxFlushTimerRef.current = null;
        void flushBrowserTranscriptWindow(triggerKind, { documentPath });
      }, autoDocumentMaxWindowMs);
    },
    [autoDocumentMaxWindowMs, captureDispatchTarget, flushBrowserTranscriptWindow],
  );

  const queueBrowserFinalTranscript = useCallback(
    (text: string) => {
      const clean = normalizeBrowserTranscriptChunk(text);
      if (!clean) return;
      if (!browserFinalTranscriptBufferRef.current && isLowValueBrowserSpeechFragment(clean))
        return;
      browserSpeechRecoveryRef.current.reset();
      browserSpeechReceivedFinalRef.current = true;
      browserSpeechHadErrorRef.current = false;
      setSpeechError("");
      const merged = mergeTranscriptChunks(browserFinalTranscriptBufferRef.current, clean);
      browserFinalTranscriptBufferRef.current = merged;
      scheduleTranscriptMaxFlush("max_window");
    },
    [scheduleTranscriptMaxFlush],
  );

  const cleanupServiceAudio = useCallback(
    (
      runId?: number,
      stopReason?: { code: string; detail?: string; backend?: string; groupId?: string },
    ) => {
      if (runId && !isActiveRecordingRun(runId)) return;
      if (stopReason?.code) {
        reportRecordingStopReason(stopReason.code, {
          detail: stopReason.detail,
          backend: stopReason.backend || "assistant_service_local_asr",
          groupId: stopReason.groupId,
          runId,
        });
      }
      const ws = serviceAudioWsRef.current;
      serviceAudioWsRef.current = null;
      serviceAudioExpectedCloseRunIdRef.current = 0;
      if (ws && shouldCloseVoiceCaptureSocket(ws.readyState)) {
        try {
          if (runId) serviceAudioExpectedCloseRunIdRef.current = runId;
          ws.close(1000, "cleanup");
        } catch {
          // ignore websocket cleanup failure
        }
      }
      const processor = serviceAudioProcessorRef.current;
      serviceAudioProcessorRef.current = null;
      if (processor) {
        processor.onaudioprocess = null;
        try {
          processor.disconnect();
        } catch {
          // ignore audio processor cleanup failure
        }
      }
      const source = serviceAudioSourceRef.current;
      serviceAudioSourceRef.current = null;
      if (source) {
        try {
          source.disconnect();
        } catch {
          // ignore audio source cleanup failure
        }
      }
      const audioContext = serviceAudioContextRef.current;
      serviceAudioContextRef.current = null;
      if (audioContext && audioContext.state !== "closed") {
        audioContext.onstatechange = null;
        void audioContext.close().catch(() => undefined);
      }
      const recorder = mediaRecorderRef.current;
      mediaRecorderRef.current = null;
      if (recorder) {
        recorder.ondataavailable = null;
        recorder.onerror = null;
        recorder.onstop = null;
      }
      clearBrowserSpeechMediaHandlers();
      resetVoiceAudioLevel();
      stopMediaStream(mediaStreamRef.current);
      mediaStreamRef.current = null;
      mediaChunksRef.current = [];
      serviceFinalTranscriptRef.current = "";
      serviceLatestPartialTranscriptRef.current = "";
      serviceCommittedTranscriptRef.current = "";
      serviceDocumentCommittedTranscriptRef.current = "";
      serviceFinalAsrTextRef.current = "";
      serviceAudioPendingPcmRef.current = [];
      serviceAudioResamplerRef.current = null;
      serviceAudioDurationMsRef.current = 0;
      serviceCommittedEndMsRef.current = 0;
      serviceDocumentCommittedEndMsRef.current = 0;
      serviceFinalSpeakerSegmentsRef.current = [];
      serviceProvisionalSpeakerSegmentsRef.current = [];
      clearServicePartialCommitTimer();
      clearServiceDocumentCheckpointTimer();
      releaseVoiceRecordingGuards();
      recordingRef.current = false;
      setRecording(false);
      endRecordingRun(runId);
    },
    [
      clearBrowserSpeechMediaHandlers,
      clearServiceDocumentCheckpointTimer,
      clearServicePartialCommitTimer,
      endRecordingRun,
      isActiveRecordingRun,
      releaseVoiceRecordingGuards,
      reportRecordingStopReason,
      resetVoiceAudioLevel,
    ],
  );

  const releaseLocalMicrophoneCapture = useCallback(() => {
    clearBrowserSpeechMediaHandlers();
    resetVoiceAudioLevel();
    stopMediaStream(mediaStreamRef.current);
    mediaStreamRef.current = null;
  }, [clearBrowserSpeechMediaHandlers, resetVoiceAudioLevel]);

  const finalizeBrowserRecordingRun = useCallback(
    async (runId: number, triggerKind: string) => {
      if (!isActiveRecordingRun(runId) || browserSpeechFinalizingRunIdRef.current === runId) return;
      browserSpeechFinalizingRunIdRef.current = runId;
      try {
        await flushBrowserTranscriptWindow(triggerKind);
      } finally {
        if (isActiveRecordingRun(runId)) {
          releaseVoiceRecordingGuards();
          recordingRef.current = false;
          setRecording(false);
          endRecordingRun(runId);
        }
      }
    },
    [
      endRecordingRun,
      flushBrowserTranscriptWindow,
      isActiveRecordingRun,
      releaseVoiceRecordingGuards,
    ],
  );

  const stopBrowserSpeech = useCallback(() => {
    const stopRunId = recordingRunIdRef.current;
    recordingStoppingRef.current = true;
    setRecordingStartingFlag(false);
    browserSpeechStopRequestedRef.current = true;
    browserSpeechRecoveryRef.current.reset();
    clearBrowserSpeechRestartTimer();
    clearBrowserSpeechStopFinalizeTimer();
    if (voiceCaptureStopAction().releaseLocalMicrophoneNow) {
      releaseLocalMicrophoneCapture();
    }
    const finalizeStoppedSpeech = (recognition: BrowserSpeechRecognition | null) => {
      if (stopRunId && !isActiveRecordingRun(stopRunId)) return;
      if (recognition && recognitionRef.current === recognition) recognitionRef.current = null;
      abortBrowserSpeechRecognition(recognition);
      releaseLocalMicrophoneCapture();
      setRecording(false);
      if (stopRunId) void finalizeBrowserRecordingRun(stopRunId, "push_to_talk_stop");
    };
    const recognition = recognitionRef.current;
    setRecording(false);
    if (!recognition) {
      finalizeStoppedSpeech(null);
      return;
    }
    browserSpeechStopFinalizeTimerRef.current = window.setTimeout(() => {
      browserSpeechStopFinalizeTimerRef.current = null;
      finalizeStoppedSpeech(recognition);
    }, 2000);
    try {
      // stop() lets the browser emit any final result before onend performs the stop flush.
      recognition.stop();
    } catch {
      clearBrowserSpeechStopFinalizeTimer();
      finalizeStoppedSpeech(recognition);
    }
  }, [
    clearBrowserSpeechRestartTimer,
    clearBrowserSpeechStopFinalizeTimer,
    finalizeBrowserRecordingRun,
    isActiveRecordingRun,
    releaseLocalMicrophoneCapture,
    setRecordingStartingFlag,
  ]);
  const stopServiceAudio = useCallback(() => {
    const stopRunId = recordingRunIdRef.current;
    recordingStoppingRef.current = true;
    setRecordingStartingFlag(false);
    const ws = serviceAudioWsRef.current;
    const processor = serviceAudioProcessorRef.current;
    if (processor) processor.onaudioprocess = null;
    clearServiceDocumentCheckpointTimer();
    if (voiceCaptureStopAction().releaseLocalMicrophoneNow) {
      releaseLocalMicrophoneCapture();
    }
    setRecording(false);
    if (ws && ws.readyState === WebSocket.OPEN) {
      try {
        voicePcmTransport.drainVoicePcmQueue(ws, serviceAudioPendingPcmRef.current);
        serviceAudioSeqRef.current += 1;
        ws.send(
          JSON.stringify({
            type: "stop",
            seq: serviceAudioSeqRef.current,
            skip_final_document_apply:
              voiceRecordingCaptureMode(voiceRecordingSessionScopeRef.current, captureMode) ===
                "document" &&
              Boolean(
                normalizeBrowserTranscriptChunk(serviceDocumentCommittedTranscriptRef.current),
              ),
          }),
        );
        return;
      } catch {
        // fall through to local cleanup
      }
    }
    const recorder = mediaRecorderRef.current;
    if (!recorder) {
      cleanupServiceAudio(stopRunId || undefined);
      return;
    }
    try {
      if (recorder.state !== "inactive") {
        recorder.stop();
        return;
      }
    } catch {
      // fall through to cleanup
    }
    cleanupServiceAudio(stopRunId || undefined);
  }, [
    captureMode,
    cleanupServiceAudio,
    clearServiceDocumentCheckpointTimer,
    releaseLocalMicrophoneCapture,
    setRecordingStartingFlag,
  ]);

  const stopCurrentRecording = useCallback(() => {
    if (mediaRecorderRef.current || serviceAudioWsRef.current) {
      stopServiceAudio();
      return;
    }
    stopBrowserSpeech();
  }, [stopBrowserSpeech, stopServiceAudio]);

  const closePanel = useCallback(() => {
    setOpen(false);
  }, []);
  const settingsOpen = useModalStore((state) => state.modals.settings);
  const openSettingsTarget = useModalStore((state) => state.openSettingsTarget);
  const canAccessGlobalSettings = useUIStore((state) => state.canAccessGlobalSettings);
  const settingsTriggerRef = useRef<HTMLElement | null>(null);
  const openVoiceSettings = useCallback(() => {
    // Hiding the workspace blurs its button before the settings effect runs.
    // Capture the actual trigger while it still owns focus (gear or Configure).
    settingsTriggerRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    openSettingsTarget({ scope: "global", tab: "voice", voiceSection: "secretary" });
  }, [openSettingsTarget]);
  const askTasks = useSecretaryTasks(
    selectedGroupId,
    open && !settingsOpen && !executionOpen && panelView === "ask",
    acceptAskRetry,
  );
  // Keep the parent in the modal stack while its settings are open. Its trigger
  // stays mounted, so the existing stack can restore focus on the way back.
  const { modalRef } = useModalA11y(open, closePanel, { preserveTerminalKeys: true });
  const previousSettingsOpenRef = useRef(settingsOpen);
  useEffect(() => {
    const closed = previousSettingsOpenRef.current && !settingsOpen;
    previousSettingsOpenRef.current = settingsOpen;
    if (!closed) return;
    void refreshAssistant({ quiet: true });
    const trigger = settingsTriggerRef.current;
    settingsTriggerRef.current = null;
    const frame = requestAnimationFrame(() => {
      if (trigger && modalRef.current?.contains(trigger)) trigger.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [settingsOpen, refreshAssistant, modalRef]);
  useEffect(() => {
    const refresh = () => void refreshAssistant({ quiet: true, passive: true });
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refreshAssistant]);

  useEffect(() => {
    if (controlDisabled) {
      setShowAssistantModeMenu(false);
      setShowAssistantLanguageMenu(false);
      return;
    }
    const triggers = [
      {
        element: assistantModeTriggerRef.current,
        open: showAssistantModeMenu,
        close: () => setShowAssistantModeMenu(false),
      },
      {
        element: assistantLanguageTriggerRef.current,
        open: showAssistantLanguageMenu,
        close: () => setShowAssistantLanguageMenu(false),
      },
    ].filter((item) => item.open && item.element);
    if (!triggers.length) return;
    // Portals must close when responsive CSS hides their trigger.
    const observer = new ResizeObserver(() => {
      for (const item of triggers) {
        if (!item.element?.getClientRects().length) item.close();
      }
    });
    for (const item of triggers) observer.observe(item.element!);
    return () => observer.disconnect();
  }, [controlDisabled, showAssistantModeMenu, showAssistantLanguageMenu]);

  useEffect(() => {
    unmountCleanupRef.current = () => {
      invalidateWorkspaceRefresh();
      const recognition = recognitionRef.current;
      recognitionRef.current = null;
      abortBrowserSpeechRecognition(recognition);
      browserSpeechStopRequestedRef.current = true;
      browserSpeechRecoveryRef.current.reset();
      clearBrowserSpeechRestartTimer();
      clearBrowserSpeechStopFinalizeTimer();
      clearTranscriptFlushTimer();
      clearTranscriptMaxFlushTimer();
      clearServicePartialCommitTimer();
      clearServiceDocumentCheckpointTimer();
      cleanupServiceAudio();
      releaseVoiceRecordingGuards();
      voiceRecordingSessionScopeRef.current = null;
    };
  }, [
    cleanupServiceAudio,
    clearBrowserSpeechRestartTimer,
    clearBrowserSpeechStopFinalizeTimer,
    clearServiceDocumentCheckpointTimer,
    clearServicePartialCommitTimer,
    clearTranscriptFlushTimer,
    clearTranscriptMaxFlushTimer,
    invalidateWorkspaceRefresh,
    releaseVoiceRecordingGuards,
  ]);
  useEffect(() => () => unmountCleanupRef.current(), []);

  const startBrowserSpeech = useCallback(
    async (runId: number, latestAssistant: BuiltinAssistant | null) => {
      const gid = String(selectedGroupId || "").trim();
      const browserBackendSelected = latestAssistant?.config?.recognition_backend === "browser_asr";
      const browserRecognitionLanguage = getRecordingRecognitionLanguage();
      const failStart = () => endRecordingRun(runId);
      if (!browserBackendSelected) {
        failStart();
        showError(
          t("voiceSecretaryBrowserBackendRequired", {
            defaultValue:
              "Switch recognition to Browser ASR in global Voice Secretary settings first.",
          }),
        );
        return;
      }
      const microphoneIssue = getBrowserMicrophoneSupportIssue();
      if (microphoneIssue) {
        failStart();
        const message = getAudioSupportIssueMessage(microphoneIssue);
        setSpeechError(message);
        showError(message);
        return;
      }
      const supportIssue = getBrowserSpeechSupportIssue();
      setSpeechSupported(!supportIssue);
      if (supportIssue) {
        failStart();
        const message = getBrowserSpeechIssueMessage(supportIssue);
        setSpeechError(message);
        showError(message);
        return;
      }
      const SpeechRecognition = getBrowserSpeechRecognitionConstructor();
      if (!SpeechRecognition) {
        failStart();
        const message = t("voiceSecretaryBrowserUnsupported", {
          defaultValue:
            "Browser speech recognition is not available in this browser page. Try another current browser.",
        });
        setSpeechError(message);
        showError(message);
        return;
      }
      const activeLock = await claimVoiceCaptureLock(voiceCaptureOwnerIdRef.current, gid);
      if (!isActiveRecordingRun(runId)) return;
      if (activeLock) {
        failStart();
        showError(
          t("voiceSecretaryAnotherRecording", {
            groupId: activeLock.groupId,
            defaultValue:
              "Voice Secretary is already recording in group {{groupId}} in another active tab. Stop that recording before starting another one.",
          }),
        );
        return;
      }
      try {
        const activeLease = await acquireDaemonVoiceRecordingLease(gid, {
          captureMode: captureTransportMode,
          recognitionBackend: "browser_asr",
          dispatchTarget: captureDispatchTarget,
        });
        if (!isActiveRecordingRun(runId)) return;
        if (activeLease) {
          failStart();
          releaseVoiceRecordingGuards(gid);
          showError(
            t("voiceSecretaryAnotherRecording", {
              groupId: activeLease.groupTitle || activeLease.groupId,
              defaultValue:
                "Voice Secretary is already recording in group {{groupId}}. Stop that recording before starting another one.",
            }),
          );
          return;
        }
      } catch (error) {
        failStart();
        releaseVoiceRecordingGuards(gid);
        const message = error instanceof Error ? error.message : String(error || "");
        showError(
          message ||
            t("voiceSecretaryRecordingLeaseFailed", {
              defaultValue: "Could not start Voice Secretary recording.",
            }),
        );
        return;
      }

      const existingRecognition = recognitionRef.current;
      recognitionRef.current = null;
      clearBrowserSpeechRestartTimer();
      clearBrowserSpeechStopFinalizeTimer();
      clearBrowserSpeechMediaHandlers();
      resetVoiceAudioLevel();
      abortBrowserSpeechRecognition(existingRecognition);
      stopMediaStream(mediaStreamRef.current);
      mediaStreamRef.current = null;
      browserSpeechReceivedFinalRef.current = false;
      browserSpeechHadErrorRef.current = false;
      browserSpeechStopRequestedRef.current = false;
      browserSpeechRecoveryRef.current.reset();

      let stream: MediaStream;
      try {
        stream = await getUserMediaWithTimeout({ audio: true });
      } catch (error) {
        failStart();
        releaseVoiceRecordingGuards(gid);
        const { message } = getAudioCaptureErrorMessage(error);
        setSpeechError(message);
        showError(message);
        return;
      }
      if (!isActiveRecordingRun(runId)) {
        stopMediaStream(stream);
        return;
      }
      if (!mediaStreamHasLiveAudio(stream)) {
        failStart();
        stopMediaStream(stream);
        releaseVoiceRecordingGuards(gid);
        const message = t("voiceSecretaryMicNotFound", {
          defaultValue: "No microphone was found or the selected microphone is unavailable.",
        });
        setSpeechError(message);
        showError(message);
        return;
      }

      // Browser SpeechRecognition owns its own capture lifecycle. Keep
      // getUserMedia as a permission/device probe only; holding a parallel page
      // stream can destabilize Edge/Chromium Web Speech capture.
      stopMediaStream(stream);
      mediaStreamRef.current = null;
      voiceRecordingSessionScopeRef.current = createVoiceRecordingSessionScope({
        runId,
        sessionId: voiceRecordingSessionIdRef.current,
        groupId: gid,
        documentPath: effectiveCaptureTargetDocumentPath,
        captureMode,
        dispatchTarget: captureDispatchTarget,
        composerText,
        composerContext,
      });
      recordingRef.current = true;
      setRecording(true);
      finishRecordingStart(runId);
      setSpeechError("");
      refreshVoiceCaptureLock(voiceCaptureOwnerIdRef.current, gid);
      void loadAudioDevices();

      const SpeechRecognitionCtor = SpeechRecognition;
      const stopAfterFatalSpeechFailure = (
        recognition: BrowserSpeechRecognition | null,
        message: string,
        showToast = true,
      ) => {
        if (!isActiveRecordingRun(runId)) return;
        recordingStoppingRef.current = true;
        browserSpeechHadErrorRef.current = true;
        browserSpeechStopRequestedRef.current = true;
        setBrowserSpeechRecoveryCode("");
        reportRecordingStopReason("browser_speech_fatal_error", {
          backend: "browser_asr",
          groupId: gid,
          runId,
          detail: message,
        });
        clearBrowserSpeechRestartTimer();
        clearBrowserSpeechStopFinalizeTimer();
        clearBrowserSpeechMediaHandlers();
        resetVoiceAudioLevel();
        if (recognition && recognitionRef.current === recognition) recognitionRef.current = null;
        abortBrowserSpeechRecognition(recognition);
        stopMediaStream(mediaStreamRef.current);
        mediaStreamRef.current = null;
        setRecording(false);
        void finalizeBrowserRecordingRun(runId, "meeting_window");
        setSpeechError(message);
        if (showToast) showError(message);
      };

      const scheduleRecoverableSpeechRestart = (
        recognition: BrowserSpeechRecognition,
        delayMs: number,
      ) => {
        clearBrowserSpeechRestartTimer();
        const fallbackMs = Math.max(BROWSER_SPEECH_ERROR_RESTART_FALLBACK_MS, delayMs);
        browserSpeechRestartTimerRef.current = window.setTimeout(() => {
          browserSpeechRestartTimerRef.current = null;
          if (!isActiveRecordingRun(runId)) return;
          if (browserSpeechStopRequestedRef.current) return;
          if (recognitionRef.current !== recognition) return;
          recognitionRef.current = null;
          abortBrowserSpeechRecognition(recognition);
          setRecording(true);
          startRecognitionCycle();
        }, fallbackMs);
      };

      function startRecognitionCycle(delayMs = 0): void {
        const runCycle = () => {
          if (!isActiveRecordingRun(runId)) return;
          browserSpeechRestartTimerRef.current = null;
          if (browserSpeechStopRequestedRef.current || !browserBackendSelected) {
            recognitionRef.current = null;
            clearBrowserSpeechMediaHandlers();
            stopMediaStream(mediaStreamRef.current);
            mediaStreamRef.current = null;
            releaseVoiceRecordingGuards();
            endRecordingRun(runId);
            setRecording(false);
            if (!browserSpeechStopRequestedRef.current)
              void flushBrowserTranscriptWindow("meeting_window");
            return;
          }

          browserSpeechRecoveryRef.current.beginCycle();
          const cycleStartedAt = performance.now();
          const recognition = new SpeechRecognitionCtor();
          recognition.continuous = true;
          recognition.interimResults = true;
          recognition.lang = browserRecognitionLanguage;
          recognition.maxAlternatives = 1;
          recognition.onspeechstart = () => {
            if (!isActiveRecordingRun(runId)) return;
            clearTranscriptFlushTimer();
          };
          recognition.onresult = (event) => {
            if (!isActiveRecordingRun(runId)) return;
            let finalText = "";
            let interimText = "";
            for (
              let resultIndex = event.resultIndex;
              resultIndex < event.results.length;
              resultIndex += 1
            ) {
              const result = event.results[resultIndex];
              let text = "";
              for (let altIndex = 0; altIndex < result.length; altIndex += 1) {
                text += result[altIndex]?.transcript || "";
              }
              if (result.isFinal) finalText += text;
              else interimText += text;
            }
            const hasFinalText = Boolean(finalText.trim());
            const cleanInterimText = interimText.replace(/\s+/g, " ").trim();
            if (hasFinalText || cleanInterimText) {
              browserSpeechHadErrorRef.current = false;
              browserSpeechRecoveryRef.current.recordResult();
              clearTranscriptFlushTimer();
            }
            if (cleanInterimText) {
              updateLiveTranscriptPreview(cleanInterimText, "interim");
            }
            if (hasFinalText) {
              queueBrowserFinalTranscript(finalText);
              updateLiveTranscriptPreview(finalText, "final");
            }
          };
          recognition.onerror = (event) => {
            if (!isActiveRecordingRun(runId)) return;
            const code = String(event.error || "").trim();
            const fatal = BROWSER_SPEECH_FATAL_ERRORS.has(code);
            const recoverable = !fatal && (BROWSER_SPEECH_RECOVERABLE_ERRORS.has(code) || !code);
            if (recoverable) {
              browserSpeechHadErrorRef.current = true;
              // Edge can surface remote Web Speech service churn as "network"
              // while the recognition object is still alive. Let that path end
              // naturally so onend can restart without a forced abort gap.
              const quietRecoverable = isQuietBrowserSpeechError(code);
              browserSpeechRecoveryRef.current.recordError(code);
              if (browserSpeechRecoveryRef.current.exhausted) {
                const message =
                  code === "audio-capture"
                    ? t("voiceSecretaryMicNotFound", {
                        defaultValue:
                          "No microphone was found or the selected microphone is unavailable.",
                      })
                    : code
                      ? t("voiceSecretarySpeechError", {
                          code,
                          defaultValue: "Speech recognition error: {{code}}",
                        })
                      : t("voiceSecretarySpeechErrorGeneric", {
                          defaultValue: "Speech recognition stopped unexpectedly.",
                        });
                stopAfterFatalSpeechFailure(
                  recognition,
                  message,
                  code !== "no-speech" && code !== "aborted",
                );
                return;
              }
              if (code && !quietRecoverable) {
                setBrowserSpeechRecoveryCode(code);
                setSpeechError(
                  t("voiceSecretarySpeechError", {
                    code,
                    defaultValue: "Speech recognition error: {{code}}",
                  }),
                );
              } else {
                setBrowserSpeechRecoveryCode("");
                setSpeechError("");
              }
              if (shouldScheduleBrowserSpeechErrorRestart(code)) {
                scheduleRecoverableSpeechRestart(
                  recognition,
                  browserSpeechRestartDelayMs(browserSpeechRecoveryRef.current.failures),
                );
              }
              return;
            }

            const message =
              code === "not-allowed" || code === "service-not-allowed"
                ? t("voiceSecretaryMicPermissionBlocked", {
                    defaultValue:
                      "Microphone permission is blocked. Allow microphone access for this site in the browser, then try again.",
                  })
                : code === "audio-capture"
                  ? t("voiceSecretaryMicNotFound", {
                      defaultValue:
                        "No microphone was found or the selected microphone is unavailable.",
                    })
                  : code
                    ? t("voiceSecretarySpeechError", {
                        code,
                        defaultValue: "Speech recognition error: {{code}}",
                      })
                    : t("voiceSecretarySpeechErrorGeneric", {
                        defaultValue: "Speech recognition stopped unexpectedly.",
                      });
            stopAfterFatalSpeechFailure(
              recognition,
              message,
              code !== "no-speech" && code !== "aborted",
            );
          };
          recognition.onend = () => {
            if (!isActiveRecordingRun(runId)) return;
            if (recognitionRef.current !== recognition) return;
            clearBrowserSpeechStopFinalizeTimer();
            const stoppedByUser = browserSpeechStopRequestedRef.current;
            if (
              !stoppedByUser &&
              browserSpeechRecoveryRef.current.endCycle(performance.now() - cycleStartedAt)
            ) {
              stopAfterFatalSpeechFailure(
                recognition,
                t("voiceSecretaryBrowserAsrEndedWithoutTranscript", {
                  defaultValue:
                    "Browser ASR stopped without returning transcript. Check the microphone connection, site permission, and system input device, then try again.",
                }),
              );
              return;
            }
            const shouldRestart = !stoppedByUser && browserBackendSelected;
            const restartDelay = browserSpeechHadErrorRef.current
              ? browserSpeechRestartDelayMs(browserSpeechRecoveryRef.current.failures)
              : 250;
            if (!stoppedByUser && !shouldRestart) {
              reportRecordingStopReason("browser_speech_ended", {
                backend: "browser_asr",
                groupId: gid,
                runId,
                detail: browserSpeechHadErrorRef.current ? "after_error" : "without_restart",
              });
            }
            if (recognitionRef.current === recognition) recognitionRef.current = null;
            if (shouldRestart) {
              setRecording(true);
              startRecognitionCycle(restartDelay);
              return;
            }
            clearBrowserSpeechMediaHandlers();
            stopMediaStream(mediaStreamRef.current);
            mediaStreamRef.current = null;
            recordingStoppingRef.current = true;
            setRecording(false);
            void finalizeBrowserRecordingRun(
              runId,
              stoppedByUser ? "push_to_talk_stop" : "meeting_window",
            );
            if (!browserSpeechReceivedFinalRef.current && !browserSpeechHadErrorRef.current) {
              setSpeechError(
                t("voiceSecretaryBrowserAsrEndedWithoutTranscript", {
                  defaultValue:
                    "Browser ASR stopped without returning transcript. Check the microphone connection, site permission, and system input device, then try again.",
                }),
              );
            }
          };

          try {
            if (!isActiveRecordingRun(runId)) return;
            recognitionRef.current = recognition;
            setRecording(true);
            finishRecordingStart(runId);
            refreshVoiceCaptureLock(voiceCaptureOwnerIdRef.current, gid);
            browserSpeechHadErrorRef.current = false;
            recognition.start();
            setSpeechError("");
          } catch {
            if (recognitionRef.current === recognition) recognitionRef.current = null;
            browserSpeechHadErrorRef.current = true;
            browserSpeechRecoveryRef.current.recordError("start-failed");
            setBrowserSpeechRecoveryCode("start-failed");
            setSpeechError(
              t("voiceSecretarySpeechError", {
                code: "start-failed",
                defaultValue: "Speech recognition error: {{code}}",
              }),
            );
            if (
              !browserSpeechRecoveryRef.current.exhausted &&
              !browserSpeechStopRequestedRef.current &&
              browserBackendSelected
            ) {
              startRecognitionCycle(
                browserSpeechRestartDelayMs(browserSpeechRecoveryRef.current.failures),
              );
              return;
            }
            stopAfterFatalSpeechFailure(
              recognition,
              t("voiceSecretarySpeechStartFailed", {
                defaultValue: "Could not start browser speech recognition.",
              }),
            );
          }
        };

        clearBrowserSpeechRestartTimer();
        if (delayMs > 0) {
          browserSpeechRestartTimerRef.current = window.setTimeout(runCycle, delayMs);
          return;
        }
        setSpeechError("");
        runCycle();
      }

      startRecognitionCycle();
    },
    [
      acquireDaemonVoiceRecordingLease,
      captureDispatchTarget,
      captureTransportMode,
      captureMode,
      composerContext,
      composerText,
      clearBrowserSpeechMediaHandlers,
      clearBrowserSpeechRestartTimer,
      clearBrowserSpeechStopFinalizeTimer,
      clearTranscriptFlushTimer,
      endRecordingRun,
      effectiveCaptureTargetDocumentPath,
      finishRecordingStart,
      finalizeBrowserRecordingRun,
      flushBrowserTranscriptWindow,
      getAudioCaptureErrorMessage,
      getAudioSupportIssueMessage,
      getBrowserSpeechIssueMessage,
      getRecordingRecognitionLanguage,
      isActiveRecordingRun,
      loadAudioDevices,
      queueBrowserFinalTranscript,
      releaseVoiceRecordingGuards,
      reportRecordingStopReason,
      selectedGroupId,
      showError,
      t,
      updateLiveTranscriptPreview,
      resetVoiceAudioLevel,
    ],
  );

  const handleServiceStreamingFinal = useCallback(
    async (text: string) => {
      const clean = normalizeBrowserTranscriptChunk(text);
      if (!clean) return;
      clearServicePartialCommitTimer();
      const committed = normalizeBrowserTranscriptChunk(serviceCommittedTranscriptRef.current);
      if (committed && (committed === clean || committed.endsWith(clean))) {
        serviceFinalTranscriptRef.current = committed;
        finalizeLiveTranscriptPreview();
        return;
      }
      const newText =
        committed && clean.startsWith(committed) ? clean.slice(committed.length).trim() : clean;
      serviceFinalTranscriptRef.current = mergeTranscriptChunks(
        serviceFinalTranscriptRef.current,
        clean,
      );
      if (captureDispatchTarget === "composer" || captureDispatchTarget === "prompt") {
        updateLiveTranscriptPreview(newText || clean, "final");
      } else if (captureDispatchTarget === "instruction") {
        updateLiveTranscriptPreview(newText || clean, "final");
      } else {
        const startMs = serviceCommittedEndMsRef.current;
        const endMs = Math.max(startMs, serviceAudioDurationMsRef.current);
        updateLiveTranscriptPreview(newText || clean, "final", { startMs, endMs });
        finalizeLiveTranscriptPreview();
        serviceCommittedEndMsRef.current = endMs;
      }
      serviceCommittedTranscriptRef.current = mergeTranscriptChunks(committed, newText);
      scheduleServiceDocumentCheckpoint();
    },
    [
      captureDispatchTarget,
      clearServicePartialCommitTimer,
      finalizeLiveTranscriptPreview,
      scheduleServiceDocumentCheckpoint,
      updateLiveTranscriptPreview,
    ],
  );

  const commitServiceLatestPartialTranscript = useCallback(async () => {
    if (captureDispatchTarget !== "document") return;
    const committed = normalizeBrowserTranscriptChunk(serviceCommittedTranscriptRef.current);
    const newText = nextUncommittedServiceTranscriptText(
      serviceLatestPartialTranscriptRef.current,
      committed,
    );
    if (!newText) return;
    const startMs = serviceCommittedEndMsRef.current;
    const endMs = Math.max(startMs, serviceAudioDurationMsRef.current);
    updateLiveTranscriptPreview(newText, "final", { startMs, endMs });
    serviceCommittedTranscriptRef.current = mergeTranscriptChunks(committed, newText);
    serviceCommittedEndMsRef.current = endMs;
  }, [captureDispatchTarget, updateLiveTranscriptPreview]);

  const startServiceAudio = useCallback(
    async (runId: number, latestAssistant: BuiltinAssistant | null) => {
      const gid = String(selectedGroupId || "").trim();
      const failStart = () => endRecordingRun(runId);
      const latestReadiness = resolveVoiceServiceReadiness({ assistant: latestAssistant });
      if (!latestReadiness.serviceAsrReady) {
        failStart();
        showError(
          t("voiceSecretaryServiceBackendRequired", {
            defaultValue:
              "Switch recognition to Assistant service local ASR in global Voice Secretary settings first.",
          }),
        );
        return;
      }
      if (!latestReadiness.serviceAsrConfigured) {
        failStart();
        showError(
          t(
            latestReadiness.recognitionBackend === "external_provider_asr"
              ? "voiceSecretaryExternalAsrNotReady"
              : "voiceSecretaryLocalAsrModelsNotReady",
            {
              defaultValue:
                "The local ASR models are not ready. Install or repair Local ASR in Settings > Voice > Voice Secretary, or switch to Browser ASR.",
            },
          ),
        );
        return;
      }
      const supportIssue = getBrowserAudioSupportIssue();
      if (supportIssue) {
        failStart();
        const message = getAudioSupportIssueMessage(supportIssue);
        setServiceAudioSupported(false);
        setSpeechError(message);
        showError(message);
        return;
      }
      const activeLock = await claimVoiceCaptureLock(voiceCaptureOwnerIdRef.current, gid);
      if (!isActiveRecordingRun(runId)) return;
      if (activeLock) {
        failStart();
        showError(
          t("voiceSecretaryAnotherRecording", {
            groupId: activeLock.groupId,
            defaultValue:
              "Voice Secretary is already recording in group {{groupId}} in another active tab. Stop that recording before starting another one.",
          }),
        );
        return;
      }
      try {
        const activeLease = await acquireDaemonVoiceRecordingLease(gid, {
          captureMode: captureTransportMode,
          recognitionBackend: latestReadiness.recognitionBackend,
          dispatchTarget: captureDispatchTarget,
        });
        if (!isActiveRecordingRun(runId)) return;
        if (activeLease) {
          failStart();
          releaseVoiceRecordingGuards(gid);
          showError(
            t("voiceSecretaryAnotherRecording", {
              groupId: activeLease.groupTitle || activeLease.groupId,
              defaultValue:
                "Voice Secretary is already recording in group {{groupId}}. Stop that recording before starting another one.",
            }),
          );
          return;
        }
      } catch (error) {
        failStart();
        releaseVoiceRecordingGuards(gid);
        const message = error instanceof Error ? error.message : String(error || "");
        showError(
          message ||
            t("voiceSecretaryRecordingLeaseFailed", {
              defaultValue: "Could not start Voice Secretary recording.",
            }),
        );
        return;
      }
      serviceAsrBackendRef.current = latestReadiness.recognitionBackend;
      let pendingStream: MediaStream | null = null;
      try {
        const audioConstraints: MediaTrackConstraints = {
          channelCount: { ideal: 1 },
          echoCancellation: { ideal: false },
          noiseSuppression: { ideal: false },
          autoGainControl: { ideal: true },
          sampleRate: { ideal: 48000 },
        };
        const inputDeviceId = capturedAudioPreferencesRef.current?.inputDeviceId || "";
        if (inputDeviceId) audioConstraints.deviceId = { exact: inputDeviceId };
        const constraints: MediaStreamConstraints = { audio: audioConstraints };
        const stream = await getUserMediaWithTimeout(constraints);
        pendingStream = stream;
        if (!isActiveRecordingRun(runId)) {
          stopMediaStream(stream);
          return;
        }
        const AudioContextConstructor =
          window.AudioContext ||
          (window as typeof window & { webkitAudioContext?: typeof AudioContext })
            .webkitAudioContext;
        if (!AudioContextConstructor) {
          throw new Error("AudioContext unavailable");
        }
        const audioContext = new AudioContextConstructor();
        const source = audioContext.createMediaStreamSource(stream);
        const processor = audioContext.createScriptProcessor(4096, 1, 1);
        const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
        const wsParams = new URLSearchParams({
          owner_id: voiceCaptureOwnerIdRef.current,
          lease_id: voiceRecordingLeaseIdRef.current,
        });
        const wsUrl = withAuthToken(
          `${protocol}//${window.location.host}/api/v1/groups/${encodeURIComponent(gid)}/assistants/voice_secretary/transcriptions/ws?${wsParams.toString()}`,
        );
        const ws = new WebSocket(wsUrl);
        mediaStreamRef.current = stream;
        pendingStream = null;
        serviceAudioContextRef.current = audioContext;
        serviceAudioSourceRef.current = source;
        serviceAudioProcessorRef.current = processor;
        serviceAudioWsRef.current = ws;
        browserSpeechMediaCleanupRef.current = attachMediaStreamDiagnostics(stream, {
          backend: "assistant_service_local_asr",
          groupId: gid,
          runId,
        });
        audioContext.onstatechange = () => {
          if (!isActiveRecordingRun(runId)) return;
          const state = String(audioContext.state || "").trim();
          if (!state || state === "running") return;
          reportRecordingStopReason("audio_context_state", {
            backend: "assistant_service_local_asr",
            groupId: gid,
            runId,
            detail: state,
          });
        };
        voiceRecordingSessionScopeRef.current = createVoiceRecordingSessionScope({
          runId,
          sessionId: voiceRecordingSessionIdRef.current,
          groupId: gid,
          documentPath: effectiveCaptureTargetDocumentPath,
          captureMode,
          dispatchTarget: captureDispatchTarget,
          composerText,
          composerContext,
        });
        setSpeechError("");
        recordingRef.current = true;
        setRecording(true);
        finishRecordingStart(runId);
        serviceAudioSeqRef.current = 0;
        serviceFinalTranscriptRef.current = "";
        serviceLatestPartialTranscriptRef.current = "";
        serviceCommittedTranscriptRef.current = "";
        serviceFinalAsrTextRef.current = "";
        serviceAudioPendingPcmRef.current = [];
        serviceAudioResamplerRef.current = new Pcm16Resampler(audioContext.sampleRate);
        serviceAudioDurationMsRef.current = 0;
        serviceCommittedEndMsRef.current = 0;
        serviceDocumentCommittedEndMsRef.current = 0;
        serviceDocumentCommittedTranscriptRef.current = "";
        serviceFinalSpeakerSegmentsRef.current = [];
        serviceProvisionalSpeakerSegmentsRef.current = [];
        clearServicePartialCommitTimer();
        clearServiceDocumentCheckpointTimer();
        processor.onaudioprocess = (event) => {
          const activeWs = serviceAudioWsRef.current;
          if (!recordingRef.current || !isActiveRecordingRun(runId)) return;
          const input = event.inputBuffer.getChannelData(0);
          updateVoiceAudioLevelsFromSamples(input);
          let resampler = serviceAudioResamplerRef.current;
          if (!resampler) {
            resampler = new Pcm16Resampler(audioContext.sampleRate);
            serviceAudioResamplerRef.current = resampler;
          }
          const pcm = resampler.push(input);
          if (!pcm.byteLength) return;
          const { droppedBytes } = voicePcmTransport.queueVoicePcmFrame(
            activeWs,
            serviceAudioPendingPcmRef.current,
            pcm,
          );
          if (droppedBytes > 0) {
            const message = t("voiceSecretaryAudioBackpressureFailed", {
              defaultValue: "Recording stopped because the audio stream could not keep up.",
            });
            if (isCurrentGroup(gid)) {
              setSpeechError(message);
              showError(message);
            }
            cleanupServiceAudio(runId, {
              code: "local_asr_pcm_backpressure",
              detail: `${droppedBytes} bytes dropped`,
              groupId: gid,
            });
            return;
          }
          serviceAudioDurationMsRef.current += Math.round((pcm.byteLength / 2 / 16000) * 1000);
        };
        ws.onopen = () => {
          if (!isActiveRecordingRun(runId)) return;
          serviceAudioSeqRef.current += 1;
          ws.send(
            JSON.stringify({
              type: "start",
              seq: serviceAudioSeqRef.current,
              session_id: voiceRecordingSessionIdRef.current,
              capture_mode: captureTransportMode,
              dispatch_target: captureDispatchTarget,
              document_path:
                captureDispatchTarget === "document" ? effectiveCaptureTargetDocumentPath : "",
              sample_rate: 16000,
              language: getRecordingRecognitionLanguage(),
              by: "user",
            }),
          );
          voicePcmTransport.flushVoicePcmQueue(ws, serviceAudioPendingPcmRef.current);
        };
        let serviceMessageQueue = Promise.resolve();
        ws.onmessage = (event) => {
          serviceMessageQueue = serviceMessageQueue
            .then(async () => {
              if (!isActiveRecordingRun(runId)) return;
              const payload = JSON.parse(String(event.data || "{}")) as Record<string, unknown>;
              const type = String(payload.type || "").trim();
              if (type === "ready") {
                serviceAsrBackendRef.current =
                  payload.backend === "external_provider_asr"
                    ? "external_provider_asr"
                    : latestReadiness.recognitionBackend;
                if (isCurrentGroup(gid)) setSpeechError("");
                return;
              }
              if (type === "recording_segment_saved") {
                showNotice({
                  message: t("voiceSecretaryRecordingSegmentSaved", {
                    segment: Number(payload.segment_index) || 1,
                  }),
                });
                return;
              }
              if (type === "partial") {
                const text = String(payload.text || "").trim();
                if (text && isMeaningfulVoiceDispatchText(text)) {
                  serviceLatestPartialTranscriptRef.current = text;
                  updateLiveTranscriptPreview(text, "interim", {
                    startMs: serviceCommittedEndMsRef.current,
                    endMs: serviceAudioDurationMsRef.current,
                  });
                }
                return;
              }
              if (type === "final") {
                const finalText = String(payload.text || "").trim();
                if (isMeaningfulVoiceDispatchText(finalText)) {
                  await handleServiceStreamingFinal(finalText);
                }
                return;
              }
              if (type === "final_asr_text") {
                if (payload.ok !== false) {
                  const documentDisposition =
                    captureDispatchTarget === "document"
                      ? documentFinalAsrDisposition(payload)
                      : null;
                  if (payload.partial === true) {
                    showNotice({
                      message: t("voiceSecretaryFinalAsrPartial", {
                        count: Math.max(1, Number(payload.failed_segment_count) || 1),
                      }),
                    });
                    if (documentDisposition === "preserve_live") {
                      const partialText = String(payload.text || "").trim();
                      if (isMeaningfulVoiceDispatchText(partialText)) {
                        serviceFinalTranscriptRef.current = mergeTranscriptChunks(
                          serviceFinalTranscriptRef.current,
                          partialText,
                        );
                      }
                      return;
                    }
                  }
                  const finalText = String(payload.text || "").trim();
                  if (isMeaningfulVoiceDispatchText(finalText)) {
                    if (captureDispatchTarget === "document") {
                      if (documentDisposition === "retry_persistence") {
                        serviceFinalTranscriptRef.current = mergeTranscriptChunks(
                          serviceFinalTranscriptRef.current,
                          finalText,
                        );
                        const retry = await retryVoiceAssistantTranscriptPersistence(gid, {
                          sessionId: voiceRecordingSessionIdRef.current,
                          documentPath: effectiveCaptureTargetDocumentPath,
                          text: finalText,
                          language: getRecordingRecognitionLanguage(),
                          modelId: String(payload.model_id || "").trim(),
                          recognitionBackend: String(payload.backend || ""),
                          partial: payload.partial === true,
                          pendingSegments: payload.transcript_pending_segments,
                        });
                        if (!retry.ok) {
                          const pending = recordFromUnknown(
                            retry.error.details,
                          ).transcript_pending_segments;
                          const recovered = Array.isArray(pending)
                            ? pending
                                .map((segment) =>
                                  String(recordFromUnknown(segment).text || "").trim(),
                                )
                                .filter(Boolean)
                                .join("\n")
                            : "";
                          if (recovered) {
                            // Recovery remains scoped to the recording's Group even
                            // if the user navigated or started another recording.
                            routeVoiceTextToComposerGroup({
                              groupId: gid,
                              text: recovered,
                              mode: "append",
                            });
                            showNotice({ message: t("voiceSecretaryCheckpointRecoveryFilled") });
                          }
                          if (isCurrentGroup(gid)) {
                            showError(
                              retry.error.message ||
                                t("voiceSecretaryTranscriptAppendFailed", {
                                  defaultValue:
                                    "Failed to save Voice Secretary transcript segment.",
                                }),
                            );
                          }
                          return;
                        }
                        if (!isActiveRecordingRun(runId)) return;
                        if (isCurrentGroup(gid)) applyTranscriptAppendResult(retry.result);
                      }
                      if (payload.partial === true) {
                        finalizeLiveTranscriptPreview();
                        if (isCurrentGroup(gid)) {
                          await restoreLatestVoiceMeetingSession({
                            replaceSession: true,
                            sessionId: voiceRecordingSessionIdRef.current,
                          });
                        }
                        return;
                      }
                      serviceFinalAsrTextRef.current = finalText;
                      serviceCommittedTranscriptRef.current = finalText;
                      serviceDocumentCommittedTranscriptRef.current = finalText;
                      finalizeLiveTranscriptPreview();
                      await restoreLatestVoiceMeetingSession({
                        replaceSession: true,
                        sessionId: voiceRecordingSessionIdRef.current,
                      });
                    } else {
                      serviceFinalAsrTextRef.current = finalText;
                      await handleServiceStreamingFinal(finalText);
                    }
                  }
                }
                return;
              }
              if (type === "diarization_started" || type === "diarization_status") {
                const status = String(payload.status || "").trim();
                if (status === "separating_speakers" && captureDispatchTarget === "document") {
                  pendingDiarizationSessionsRef.current.add(voiceRecordingSessionIdRef.current);
                  const now = Date.now();
                  const documentPath = String(
                    effectiveCaptureTargetDocumentPath ||
                      captureTargetDocumentPathRef.current ||
                      "",
                  ).trim();
                  if (documentPath) {
                    setVoiceTranscriptItems((prev) =>
                      upsertLiveVoiceTranscriptItem(prev, {
                        id: `${voiceRecordingSessionIdRef.current}-analysis`,
                        sessionId: voiceRecordingSessionIdRef.current,
                        phase: "interim",
                        text: t("voiceSecretaryTranscriptAnalyzingAudio", {
                          defaultValue: "Analyzing final audio...",
                        }),
                        mode: "document",
                        documentTitle: captureTargetDocumentTitle,
                        documentPath,
                        language: getRecordingRecognitionLanguage(),
                        processingPhase: "separating_speakers",
                        updatedAt: now,
                      }),
                    );
                    finalizeLiveTranscriptPreview();
                    setActivityClockMs(now);
                  }
                }
                return;
              }
              if (type === "diarization_skipped") {
                const reason = String(payload.reason || "").trim();
                const message =
                  reason === "worker_busy"
                    ? t("voiceSecretaryDiarizationBusy", {
                        defaultValue:
                          "Speaker separation is busy with another recording. This transcript was saved without speaker labels.",
                      })
                    : t("voiceSecretaryDiarizationUnavailable", {
                        defaultValue:
                          "Speaker separation is unavailable. This transcript was saved without speaker labels.",
                      });
                pendingDiarizationSessionsRef.current.delete(voiceRecordingSessionIdRef.current);
                if (isCurrentGroup(gid)) setSpeechError(message);
                if (captureDispatchTarget === "document") {
                  const now = Date.now();
                  const sessionId = voiceRecordingSessionIdRef.current;
                  const documentPath = String(
                    effectiveCaptureTargetDocumentPath ||
                      captureTargetDocumentPathRef.current ||
                      "",
                  ).trim();
                  if (sessionId && documentPath) {
                    setVoiceTranscriptItems((prev) =>
                      replaceVoiceTranscriptProcessingItem(prev, {
                        id: `${sessionId}-analysis-skipped`,
                        sessionId,
                        phase: "interim",
                        text: message,
                        mode: "document",
                        documentPath,
                        language: getRecordingRecognitionLanguage(),
                        processingPhase: "failed",
                        updatedAt: now,
                        createdAt: now,
                      }),
                    );
                  }
                }
                return;
              }
              if (type === "diarization_delta" || type === "diarization") {
                if (payload.ok === false) {
                  return;
                }
                const result = recordFromUnknown(payload.result);
                const rawSegments = (Array.isArray(result.segments) ? result.segments : [])
                  .map((item) => recordFromUnknown(item))
                  .filter((item) => Object.keys(item).length > 0);
                const provisional = Boolean(result.provisional);
                if (provisional) serviceProvisionalSpeakerSegmentsRef.current = rawSegments;
                else serviceFinalSpeakerSegmentsRef.current = rawSegments;
                setVoiceTranscriptItems((prev) =>
                  annotateVoiceTranscriptItemsWithSpeakers(prev, rawSegments),
                );
                return;
              }
              if (type === "closed") {
                serviceAudioExpectedCloseRunIdRef.current = runId;
                await commitServiceLatestPartialTranscript();
                if (captureDispatchTarget === "document") {
                  await flushServiceDocumentCheckpoint("service_transcript");
                }
                if (!isActiveRecordingRun(runId)) return;
                const dispatchText =
                  serviceFinalAsrTextRef.current ||
                  serviceCommittedTranscriptRef.current ||
                  serviceFinalTranscriptRef.current;
                if (captureDispatchTarget === "composer") {
                  appendDirectDictationToComposer(dispatchText);
                } else {
                  const dispatchKind = voiceServiceStopDispatchKind({
                    mode: captureTransportMode,
                    transcriptText: dispatchText,
                    pendingPromptRequestId: pendingPromptRequestIdRef.current,
                    pendingAskRequestId: pendingAskRequestIdRef.current,
                  });
                  if (dispatchKind === "prompt") {
                    await requestPromptRefine(dispatchText, "service_prompt_refine");
                  } else if (dispatchKind === "instruction") {
                    await sendInstructionTranscript(dispatchText, {
                      triggerKind: "service_voice_instruction",
                    });
                  }
                }
                cleanupServiceAudio(runId);
                window.setTimeout(() => {
                  if (open) void refreshAssistant({ quiet: true });
                }, 1800);
                return;
              }
              if (type === "error" || payload.ok === false) {
                const recovered = String(payload.recovered_text || "").trim();
                if (recovered && captureDispatchTarget !== "document")
                  appendDirectDictationToComposer(recovered);
                const error = recordFromUnknown(payload.error);
                const message =
                  String(error.message || "") ||
                  t("voiceSecretaryAudioTranscribeFailed", {
                    defaultValue: "Voice Secretary could not transcribe the recorded audio.",
                  });
                if (isCurrentGroup(gid)) {
                  setSpeechError(message);
                  showError(message);
                }
                cleanupServiceAudio(runId, {
                  code: "local_asr_backend_error",
                  detail: message,
                  groupId: gid,
                });
              }
            })
            .catch(() => {
              if (!isActiveRecordingRun(runId)) return;
              const message = t("voiceSecretaryAudioTranscribeFailed", {
                defaultValue: "Voice Secretary could not transcribe the recorded audio.",
              });
              if (isCurrentGroup(gid)) {
                setSpeechError(message);
                showError(message);
              }
              cleanupServiceAudio(runId, {
                code: "local_asr_message_handler_error",
                detail: message,
                groupId: gid,
              });
            });
        };
        ws.onerror = () => {
          serviceMessageQueue = queueVoiceSocketError(
            serviceMessageQueue,
            () =>
              !isActiveRecordingRun(runId) || serviceAudioExpectedCloseRunIdRef.current === runId,
            () => {
              const message = t(
                serviceAsrBackendRef.current === "external_provider_asr"
                  ? "voiceSecretaryExternalAsrConnectionFailed"
                  : "voiceSecretaryLocalAsrConnectionFailed",
                {
                  defaultValue:
                    "The local ASR connection failed. Refresh the page to initialize the authenticated session, and confirm the updated Web backend is running.",
                },
              );
              if (isCurrentGroup(gid)) {
                setSpeechError(message);
                showError(message);
              }
              cleanupServiceAudio(runId, {
                code: "local_asr_ws_error",
                detail: message,
                groupId: gid,
              });
            },
          );
        };
        ws.onclose = (event) => {
          serviceMessageQueue = serviceMessageQueue.then(() => {
            if (serviceAudioExpectedCloseRunIdRef.current === runId) {
              serviceAudioExpectedCloseRunIdRef.current = 0;
              return;
            }
            if (serviceAudioWsRef.current === ws) {
              cleanupServiceAudio(runId, {
                code: "local_asr_ws_closed",
                detail: `${event.code || 0}${event.reason ? ` ${event.reason}` : ""}`.trim(),
                groupId: gid,
              });
            }
          });
        };
        source.connect(processor);
        processor.connect(audioContext.destination);
        void loadAudioDevices();
      } catch (error) {
        if (pendingStream) stopMediaStream(pendingStream);
        // Cleanup must run before invalidating the run that owns these resources.
        cleanupServiceAudio(runId);
        if (!isCurrentGroup(gid)) return;
        const { message } = getAudioCaptureErrorMessage(error);
        setSpeechError(message);
        showError(message);
      }
    },
    [
      acquireDaemonVoiceRecordingLease,
      applyTranscriptAppendResult,
      cleanupServiceAudio,
      appendDirectDictationToComposer,
      captureDispatchTarget,
      captureMode,
      captureTransportMode,
      composerContext,
      composerText,
      commitServiceLatestPartialTranscript,
      flushServiceDocumentCheckpoint,
      endRecordingRun,
      getAudioCaptureErrorMessage,
      getAudioSupportIssueMessage,
      handleServiceStreamingFinal,
      getRecordingRecognitionLanguage,
      clearServiceDocumentCheckpointTimer,
      clearServicePartialCommitTimer,
      finishRecordingStart,
      loadAudioDevices,
      isActiveRecordingRun,
      isCurrentGroup,
      attachMediaStreamDiagnostics,
      requestPromptRefine,
      reportRecordingStopReason,
      refreshAssistant,
      releaseVoiceRecordingGuards,
      restoreLatestVoiceMeetingSession,
      sendInstructionTranscript,
      open,
      selectedGroupId,
      captureTargetDocumentTitle,
      effectiveCaptureTargetDocumentPath,
      finalizeLiveTranscriptPreview,
      showError,
      showNotice,
      t,
      updateLiveTranscriptPreview,
      updateVoiceAudioLevelsFromSamples,
    ],
  );

  useEffect(() => {
    if (!recording) return undefined;
    const gid = String(voiceRecordingLeaseGroupIdRef.current || selectedGroupId || "").trim();
    const interval = window.setInterval(() => {
      refreshVoiceCaptureLock(voiceCaptureOwnerIdRef.current, gid);
      if (!voiceRecordingLeaseAcquiredRef.current || !gid) return;
      const leaseId = voiceRecordingLeaseIdRef.current;
      const sessionScope = voiceRecordingSessionScopeRef.current;
      void updateVoiceAssistantRecordingLease(gid, {
        action: "heartbeat",
        ownerId: voiceCaptureOwnerIdRef.current,
        leaseId,
        ttlSeconds: VOICE_RECORDING_LEASE_TTL_SECONDS,
        captureMode: voiceCaptureTransportMode(
          voiceRecordingDispatchTarget(sessionScope, captureDispatchTarget),
        ),
        recognitionBackend,
        dispatchTarget: voiceRecordingDispatchTarget(sessionScope, captureDispatchTarget),
      })
        .then((resp) => {
          if (leaseId !== voiceRecordingLeaseIdRef.current) return;
          if (!recordingRef.current) return;
          if (voiceRecordingLeaseIsDefinitelyLost(resp)) {
            voiceRecordingLeaseAcquiredRef.current = false;
            voiceRecordingLeaseIdRef.current = "";
            const message = t("voiceSecretaryRecordingLeaseLost", {
              defaultValue:
                "Voice Secretary recording ownership was lost. Recording has been stopped; start it again when the microphone is available.",
            });
            reportRecordingStopReason("recording_lease_lost", {
              detail: message,
              backend: String(getActiveCaptureConfig()?.recognition_backend || recognitionBackend),
              groupId: gid,
            });
            if (isCurrentGroup(gid)) {
              setSpeechError(message);
              showError(message);
            }
            stopCurrentRecording();
          }
        })
        .catch(() => undefined);
    }, 5000);
    return () => window.clearInterval(interval);
  }, [
    captureDispatchTarget,
    captureMode,
    getActiveCaptureConfig,
    isCurrentGroup,
    recognitionBackend,
    recording,
    reportRecordingStopReason,
    selectedGroupId,
    showError,
    stopCurrentRecording,
    t,
  ]);

  const updateRecognitionLanguage = useCallback(
    (nextLanguage: string) => {
      useUIStore
        .getState()
        .setChatVoicePreferences(selectedGroupId, {
          voiceRecognitionLanguage: normalizeVoiceRecognitionLanguageForBackend(
            nextLanguage,
            recognitionBackend,
          ),
        });
    },
    [recognitionBackend, selectedGroupId],
  );

  const clearAskFeedbackHistory = useCallback(async () => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) {
      liveTranscriptPreviewRef.current = null;
      setLiveTranscriptPreview(null);
      voiceStreamItemIdRef.current = "";
      return;
    }
    if (!askFeedbackItems.length && !liveTranscriptPreview) return;
    const previousItems = askFeedbackItems;
    const previousLiveTranscriptPreview = liveTranscriptPreview;
    setAskFeedbackItems([]);
    liveTranscriptPreviewRef.current = null;
    setLiveTranscriptPreview(null);
    voiceStreamItemIdRef.current = "";
    if (voiceReplyBubbleRequestId) {
      setVoiceReplyBubbleRequestId("");
    }
    if (!askFeedbackItems.length) return;
    setActionBusy("clear_ask");
    try {
      const resp = await clearVoiceAssistantAskRequests(gid, { keepActive: false, by: "user" });
      if (!isCurrentGroup(gid)) return;
      if (!resp.ok) {
        setAskFeedbackItems(previousItems);
        liveTranscriptPreviewRef.current = previousLiveTranscriptPreview;
        setLiveTranscriptPreview(previousLiveTranscriptPreview);
        showError(resp.error.message);
        return;
      }
      const nextItems = resp.result.ask_requests || [];
      setAskFeedbackItems(nextItems);
      const currentAskRequestId = askNoticeRef.current.requestId;
      if (
        currentAskRequestId &&
        !nextItems.some((item) => item.request_id === currentAskRequestId)
      ) {
        pendingAskRequestIdRef.current = "";
        setPendingAskRequestId("");
        const notice = { requestId: "", hidden: false };
        askNoticeRef.current = notice;
        saveVoiceAskNotice(gid, notice);
        setAskNotice(notice);
        pendingAskTaskRef.current = null;
        setPendingAskTask(null);
        askObservationRevisionRef.current++;
      }
      const replyRequestId = String(voiceReplyBubbleRequestId || "").trim();
      if (
        replyRequestId &&
        !nextItems.some((item) => item.request_id === replyRequestId && hasFinalAskReply(item))
      ) {
        setVoiceReplyBubbleRequestId("");
      }
    } catch {
      if (!isCurrentGroup(gid)) return;
      setAskFeedbackItems(previousItems);
      liveTranscriptPreviewRef.current = previousLiveTranscriptPreview;
      setLiveTranscriptPreview(previousLiveTranscriptPreview);
      showError(
        t("voiceSecretaryClearRequestsFailed", {
          defaultValue: "Failed to clear Voice Secretary requests.",
        }),
      );
    } finally {
      if (isCurrentGroup(gid)) setActionBusy("");
    }
  }, [
    askFeedbackItems,
    liveTranscriptPreview,
    isCurrentGroup,
    selectedGroupId,
    showError,
    t,
    voiceReplyBubbleRequestId,
  ]);

  const persistCurrentDocument = useCallback(async (): Promise<AssistantVoiceDocument | null> => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return null;
    const resp = await saveVoiceAssistantDocument(gid, {
      documentPath: activeDocumentWritePath || viewedDocumentPath || captureTargetDocumentPath,
      content: documentDraft,
      status: activeDocument?.status || "active",
      by: "user",
    });
    if (!isCurrentGroup(gid)) return null;
    if (!resp.ok) {
      showError(resp.error.message);
      return null;
    }
    applyDocumentMutationResult(resp.result.document, resp.result.assistant);
    return resp.result.document || null;
  }, [
    activeDocumentWritePath,
    activeDocument?.status,
    viewedDocumentPath,
    applyDocumentMutationResult,
    captureTargetDocumentPath,
    documentDraft,
    isCurrentGroup,
    selectedGroupId,
    showError,
  ]);

  const saveDocument = useCallback(async () => {
    const gid = String(selectedGroupId || "").trim();
    setActionBusy("save_doc");
    try {
      const document = await persistCurrentDocument();
      if (!isCurrentGroup(gid)) return;
      if (!document) return;
      setDocumentEditing(false);
      showNotice({
        message: t("voiceSecretaryDocumentSaved", {
          defaultValue: "Voice Secretary working document saved.",
        }),
      });
    } catch {
      if (!isCurrentGroup(gid)) return;
      showError(
        t("voiceSecretaryDocumentSaveFailed", {
          defaultValue: "Failed to save Voice Secretary working document.",
        }),
      );
    } finally {
      if (isCurrentGroup(gid)) setActionBusy("");
    }
  }, [isCurrentGroup, persistCurrentDocument, selectedGroupId, showError, showNotice, t]);

  const startCreateDocument = useCallback(() => {
    if (documentHasUnsavedEdits) {
      const confirmed = window.confirm(
        t("voiceSecretaryNewDocumentConfirm", {
          defaultValue: "Create a new document and discard unsaved edits in this panel?",
        }),
      );
      if (!confirmed) return;
    }
    setNewDocumentTitleDraft(
      t("voiceSecretaryDefaultDocumentTitle", { defaultValue: "Untitled document" }),
    );
    setCreatingDocument(true);
  }, [documentHasUnsavedEdits, t]);

  const cancelCreateDocument = useCallback(() => {
    if (actionBusy === "new_doc") return;
    setCreatingDocument(false);
    setNewDocumentTitleDraft("");
  }, [actionBusy]);

  const createDocument = useCallback(async () => {
    const gid = String(selectedGroupId || "").trim();
    if (!gid) return;
    const title =
      newDocumentTitleDraft.trim() ||
      t("voiceSecretaryDefaultDocumentTitle", { defaultValue: "Untitled document" });
    setActionBusy("new_doc");
    try {
      const resp = await saveVoiceAssistantDocument(gid, {
        title,
        content: "",
        createNew: true,
        by: "user",
      });
      if (!isCurrentGroup(gid)) return;
      if (!resp.ok) {
        showError(resp.error.message);
        return;
      }
      applyDocumentMutationResult(resp.result.document, resp.result.assistant);
      const docPath = voiceDocumentPath(resp.result.document);
      if (docPath) {
        setViewedDocumentPath(docPath);
        setCaptureTargetDocumentPath(docPath);
        captureTargetDocumentPathRef.current = docPath;
        loadDocumentDraft(resp.result.document || null);
      }
      setCreatingDocument(false);
      setNewDocumentTitleDraft("");
      setDocumentEditing(true);
      showNotice({
        message: t("voiceSecretaryDocumentCreated", {
          defaultValue: "Voice Secretary working document created.",
        }),
      });
    } catch {
      if (!isCurrentGroup(gid)) return;
      showError(
        t("voiceSecretaryDocumentCreateFailed", {
          defaultValue: "Failed to create Voice Secretary working document.",
        }),
      );
    } finally {
      if (isCurrentGroup(gid)) setActionBusy("");
    }
  }, [
    applyDocumentMutationResult,
    isCurrentGroup,
    loadDocumentDraft,
    newDocumentTitleDraft,
    selectedGroupId,
    showError,
    showNotice,
    t,
  ]);

  const sendPanelRequest = useCallback(
    async (kind: "document" | "askDocument" | "ask") => {
      const gid = String(selectedGroupId || "").trim();
      const instruction = documentInstruction.trim();
      if (!gid || !instruction) return;
      const dispatchKey = `panel:${gid}`;
      if (!requestDispatchGateRef.current.tryAcquire(dispatchKey)) return;
      try {
        if (kind !== "document") {
          if (kind === "askDocument" && documentHasUnsavedEdits) {
            showError(t("voiceSecretaryDocumentUnsavedBeforeRequest"));
            return;
          }
          setActionBusy("instruct_ask");
          try {
            const sent = await sendInstructionTranscript(instruction, {
              triggerKind: "typed_voice_instruction",
              targetGroupId: gid,
              documentPath:
                kind === "askDocument"
                  ? activeDocumentWritePath || viewedDocumentPath
                  : captureTargetDocumentPath || activeDocumentWritePath,
              referenceDocument: kind === "askDocument",
            });
            if (!isCurrentGroup(gid)) return;
            if (sent) {
              setDocumentInstruction("");
              setPanelView("ask");
            }
          } finally {
            if (isCurrentGroup(gid)) setActionBusy("");
          }
          return;
        }
        if (documentHasUnsavedEdits) {
          showError(
            t("voiceSecretaryDocumentUnsavedBeforeRequest", {
              defaultValue:
                "Save or discard local document edits before sending a request to Voice Secretary.",
            }),
          );
          return;
        }
        const docPath = activeDocumentWritePath || viewedDocumentPath || captureTargetDocumentPath;
        const targetDocument = docPath ? findVoiceDocument(documents, docPath) : null;
        if (
          !docPath ||
          !targetDocument ||
          String(targetDocument.status || "active")
            .trim()
            .toLowerCase() === "archived"
        ) {
          showError(
            t("voiceSecretaryDocumentRequestStale", {
              defaultValue:
                "This document is no longer active. Refresh or choose another document before sending a request.",
            }),
          );
          await refreshAssistant({ quiet: true });
          return;
        }
        setActionBusy("instruct_doc");
        const nowMs = Date.now();
        const requestId = `voice-ask-${nowMs.toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
        try {
          const resp = await sendVoiceAssistantDocumentInstruction(gid, docPath, {
            instruction,
            documentPath: docPath,
            requestId,
            inputAppendId: requestId,
            trigger: {
              trigger_kind: "user_instruction",
              mode: "meeting",
              recognition_backend: String(
                getActiveCaptureConfig()?.recognition_backend || recognitionBackend,
              ),
              language: getRecordingRecognitionLanguage(),
            },
            by: "user",
          });
          if (!isCurrentGroup(gid)) return;
          if (!resp.ok) {
            showError(resp.error.message);
            return;
          }
          applyDocumentMutationResult(resp.result.document, resp.result.assistant);
          const effectiveRequestId = String(resp.result.request_id || requestId).trim();
          if (effectiveRequestId) {
            localVoiceReplyRequestIdsRef.current.add(effectiveRequestId);
            setAskFeedbackItems((prev) =>
              [
                {
                  request_id: effectiveRequestId,
                  status: "pending",
                  request_text: instruction,
                  request_preview: instruction.slice(0, 240),
                  document_path: docPath,
                  target_kind: "document",
                },
                ...prev.filter((item) => item.request_id !== effectiveRequestId),
              ].slice(0, 10),
            );
          }
          setDocumentInstruction("");
          setDocumentEditing(false);
          showNotice({
            message: t(
              resp.result.secretary_processing_deferred
                ? "voiceSecretaryInputSavedForLater"
                : "voiceSecretaryDocumentInstructionQueued",
              { defaultValue: "Request sent to Voice Secretary." },
            ),
          });
          void refreshAssistant({ quiet: true });
        } catch {
          if (!isCurrentGroup(gid)) return;
          showError(
            t("voiceSecretaryDocumentInstructionFailed", {
              defaultValue: "Failed to send the request to Voice Secretary.",
            }),
          );
        } finally {
          if (isCurrentGroup(gid)) setActionBusy("");
        }
      } finally {
        requestDispatchGateRef.current.release(dispatchKey);
      }
    },
    [
      activeDocumentWritePath,
      viewedDocumentPath,
      applyDocumentMutationResult,
      captureTargetDocumentPath,
      documentHasUnsavedEdits,
      documentInstruction,
      documents,
      getActiveCaptureConfig,
      getRecordingRecognitionLanguage,
      isCurrentGroup,
      recognitionBackend,
      refreshAssistant,
      selectedGroupId,
      sendInstructionTranscript,
      showError,
      showNotice,
      t,
    ],
  );

  const selectDocument = useCallback(
    async (document: AssistantVoiceDocument) => {
      const nextPath = voiceDocumentPath(document);
      const currentPath = activeDocumentWritePath || viewedDocumentPath;
      if (!nextPath) return;
      if (nextPath === currentPath) {
        setPanelView("document");
        setVoiceWorkspaceView("document");
        setMobileDocumentListOpen(false);
        return;
      }
      if (documentHasUnsavedEdits) {
        const confirmed = window.confirm(
          t("voiceSecretarySwitchDocumentConfirm", {
            defaultValue: "Switch documents and discard unsaved edits in this panel?",
          }),
        );
        if (!confirmed) return;
      }
      setPanelView("document");
      setVoiceWorkspaceView("document");
      setMobileDocumentListOpen(false);
      viewedDocumentPathRef.current = nextPath;
      setViewedDocumentPath(nextPath);
      invalidateWorkspaceRefresh();
      const contentLoad = documentContentLoadTracker.current.begin();
      const draftBeforeLoad = documentDraftRef.current;
      setDocumentContentLoadingPath("");
      let nextDocument = document;
      if (documentNeedsContentLoad(document)) {
        const gid = String(selectedGroupId || "").trim();
        setDocumentContentLoadingPath(nextPath);
        try {
          const resp = gid ? await fetchVoiceAssistantDocumentContent(gid, nextPath) : null;
          if (
            !isCurrentGroup(gid) ||
            viewedDocumentPathRef.current !== nextPath ||
            !documentContentLoadTracker.current.end(contentLoad)
          )
            return;
          if (resp?.ok && resp.result.document) {
            nextDocument = resp.result.document;
            setDocuments((prev) =>
              prev.map((item) => (voiceDocumentMatches(item, nextPath) ? nextDocument : item)),
            );
          }
        } finally {
          if (documentContentLoadTracker.current.end(contentLoad) && isCurrentGroup(gid)) {
            setDocumentContentLoadingPath("");
          }
        }
      }
      if (documentDraftRef.current !== draftBeforeLoad) {
        setDocumentRemoteChanged(true);
        return;
      }
      loadDocumentDraft(nextDocument);
      setDocumentEditing(false);
      setCreatingDocument(false);
      setNewDocumentTitleDraft("");
    },
    [
      activeDocumentWritePath,
      viewedDocumentPath,
      documentHasUnsavedEdits,
      invalidateWorkspaceRefresh,
      isCurrentGroup,
      loadDocumentDraft,
      selectedGroupId,
      t,
    ],
  );

  const setCaptureTargetDocument = useVoiceCaptureTargetDocumentSelection({
    selectedGroupId,
    recording,
    captureTargetDocumentPathRef,
    getDocumentPath: voiceDocumentPath,
    setCaptureTargetDocumentPath,
    clearTranscriptFlushTimer,
    clearTranscriptMaxFlushTimer,
    flushBrowserTranscriptWindow,
    refreshAssistant,
    showError,
    showNotice,
    t,
  });

  const archiveDocument = useVoiceDocumentArchive({
    selectedGroupId,
    activeDocumentWritePath,
    viewedDocumentPath,
    documents,
    archivedDocumentPathsRef,
    captureTargetDocumentPathRef,
    isCurrentGroup,
    loadDocumentDraft,
    clearReferences: clearVoiceDocumentReferences,
    refreshAssistant,
    setActionBusy,
    setDocuments,
    setViewedDocumentPath,
    setDocumentEditing,
    setCaptureTargetDocumentPath,
    showError,
    showNotice,
    t,
  });

  const downloadCurrentDocument = () =>
    downloadVoiceDocument(activeDocument, documentDisplayTitle, documentDraft, showNotice, t);

  const captureStartTitle = !composerNeedsSecretary
    ? t("voiceDirectDictationStartHint", {
        defaultValue: "Record speech and append the transcript directly to the composer",
      })
    : composerCaptureMode === "prompt"
      ? t("voiceSecretaryPromptModeStartHint", {
          defaultValue: "Click to quickly polish speech into a ready-to-send prompt",
        })
      : composerCaptureMode === "instruction"
        ? t("voiceSecretaryInstructionModeStartHint", {
            defaultValue: "Record a request for Voice Secretary to handle directly",
          })
        : t("voiceSecretaryStartDictation", { defaultValue: "Start recording" });
  const assistantRowModeOptions: Array<{
    key: VoiceSecretaryCaptureMode;
    label: string;
    description: string;
  }> = useMemo(
    () => [
      {
        key: "prompt",
        label: t("voiceSecretaryModePrompt", { defaultValue: "Prompt" }),
        description: t("voiceSecretaryModePromptDesc"),
      },
      {
        key: "document",
        label: t("voiceSecretaryModeDocument", { defaultValue: "Doc" }),
        description: t("voiceSecretaryModeDocumentDesc", {
          defaultValue: "Record into working docs",
        }),
      },
      {
        key: "instruction",
        label: t("voiceSecretaryModeInstruction", { defaultValue: "Ask" }),
        description: t("voiceSecretaryModeInstructionDesc", { defaultValue: "Handle directly" }),
      },
    ],
    [t],
  );
  const currentSelectedGroupId = String(selectedGroupId || "").trim();
  const activeRecordingGroupId = String(
    recordingGroupId || voiceRecordingLeaseGroupIdRef.current || "",
  ).trim();
  const activeRecordingGroupLabel = String(recordingGroupTitle || activeRecordingGroupId).trim();
  const recordingInOtherGroup = Boolean(
    recording &&
    activeRecordingGroupId &&
    currentSelectedGroupId &&
    activeRecordingGroupId !== currentSelectedGroupId,
  );
  const recordingGroupNoticeText = recordingInOtherGroup
    ? t("voiceSecretaryRecordingInOtherGroup", {
        group: activeRecordingGroupLabel,
        defaultValue: "Recording in {{group}}",
      })
    : "";
  const recordingSettingsLockedTitle = recordingInOtherGroup
    ? t("voiceSecretaryRecordingSettingsLockedOtherGroup", {
        group: activeRecordingGroupLabel,
        defaultValue: "Stop the recording in {{group}} before changing Voice Secretary settings.",
      })
    : t("voiceSecretaryModeChangeDisabledRecording", {
        defaultValue: "Stop recording before changing mode.",
      });

  const dictationSupported = browserSpeechReady
    ? !browserSpeechSupportIssue && speechSupported
    : serviceAsrReady
      ? serviceAudioSupportIssue
        ? false
        : serviceAudioSupported
      : false;
  const startDictation = useCallback(async () => {
    const gid = String(selectedGroupId || "").trim();
    const runId = beginRecordingRun();
    if (!runId) return;
    setRecordingTarget({
      mode: captureMode,
      documentPath: effectiveCaptureTargetDocumentPath,
      dispatchTarget: captureDispatchTarget,
    });
    try {
      // Resolve shared settings before choosing the transport; already-active
      // recordings continue to use their captured configuration.
      const resp = await fetchVoiceAssistantStatus(gid);
      if (!isActiveRecordingRun(runId)) return;
      if (!isCurrentGroup(gid)) {
        endRecordingRun(runId);
        return;
      }
      if (!resp.ok) throw new Error(resp.error.message);
      const latestAssistant = resp.result.assistant || null;
      setAssistant(latestAssistant);
      capturedConfigRef.current = {
        ...(latestAssistant?.config || {}),
        recognition_language:
          recognitionLanguageOverride || latestAssistant?.config?.recognition_language || "auto",
      };
      if (latestAssistant?.config?.recognition_backend === "browser_asr") {
        await startBrowserSpeech(runId, latestAssistant);
      } else {
        await startServiceAudio(runId, latestAssistant);
      }
    } catch (error) {
      if (!isActiveRecordingRun(runId)) return;
      endRecordingRun(runId);
      const message = error instanceof Error ? error.message : String(error || "");
      setSpeechError(message);
      showError(message);
    }
  }, [
    selectedGroupId,
    beginRecordingRun,
    captureMode,
    captureDispatchTarget,
    effectiveCaptureTargetDocumentPath,
    endRecordingRun,
    isActiveRecordingRun,
    isCurrentGroup,
    recognitionLanguageOverride,
    startBrowserSpeech,
    startServiceAudio,
    showError,
  ]);
  const activeDocumentPath = voiceDocumentPath(activeDocument);
  const transcriptDocumentPath = String(
    activeDocumentWritePath ||
      activeDocumentPath ||
      viewedDocumentPath ||
      captureTargetDocumentPath ||
      "",
  ).trim();
  transcriptDocumentPathRef.current = transcriptDocumentPath;
  useEffect(() => {
    const gid = String(selectedGroupId || "").trim();
    if (!open || !gid || !transcriptDocumentPath) return undefined;
    void restoreLatestVoiceMeetingSession().catch(() => {
      // Best-effort UI recovery; normal assistant refresh still owns error display.
    });
    return undefined;
  }, [open, restoreLatestVoiceMeetingSession, selectedGroupId, transcriptDocumentPath]);
  const visibleVoiceTranscriptItems = useMemo(
    () => filterVoiceTranscriptItemsForDocument(voiceTranscriptItems, transcriptDocumentPath),
    [transcriptDocumentPath, voiceTranscriptItems],
  );
  const openButtonLabel = open
    ? t("voiceSecretaryClose", { defaultValue: "Close Voice Secretary" })
    : recording
      ? t("voiceSecretaryOpenRecordingWorkspace", {
          defaultValue: "Expand Voice Secretary workspace - recording",
        })
      : t("voiceSecretaryOpenWorkspace", { defaultValue: "Expand Voice Secretary workspace" });
  const openButtonIconSizePx = buttonClassName
    ? buttonSizePx
    : Math.max(20, Math.min(26, Math.round(buttonSizePx - 14)));
  const pendingPromptInOtherGroup = Boolean(
    pendingPromptRequestId &&
    pendingPromptGroupIdRef.current &&
    currentSelectedGroupId &&
    pendingPromptGroupIdRef.current !== currentSelectedGroupId,
  );
  const pendingPromptBlocksOtherGroup =
    voicePromptRequestOwnership({
      requestId: pendingPromptRequestId,
      pendingGroupId: pendingPromptGroupIdRef.current,
      targetGroupId: currentSelectedGroupId,
      task: pendingPromptTask,
    }) === "other_group";
  const promptDraftWaiting = Boolean(
    pendingPromptRequestId &&
    !pendingPromptDraft &&
    !pendingPromptInOtherGroup &&
    !pendingPromptProblem,
  );
  const promptDraftWaitingTitle = t("voiceSecretaryPromptDraftWaitingShort", {
    defaultValue: "Polishing prompt...",
  });
  const promptDraftReadyTitle = t(
    promptNeedsReview
      ? "voiceSecretaryPromptDraftNeedsReview"
      : "voiceSecretaryPromptDraftReadyShort",
  );
  const askFeedbackStatusLabel = useCallback(
    (status: string) => {
      const key = String(status || "pending")
        .trim()
        .toLowerCase();
      if (key === "working")
        return t("voiceSecretaryAskStatusWorking", { defaultValue: "Working" });
      if (key === "starting") return t("settings:voiceSettings.phases.starting");
      if (key === "submitted")
        return t("voiceSecretaryAskStatusSaved", { defaultValue: "Request saved" });
      if (key === "result_pending")
        return t("voiceSecretaryAskStatusResultPending", { defaultValue: "Receiving result" });
      if (["cancelled", "conflict", "unconfirmed"].includes(key))
        return t(`settings:voiceSettings.phases.${key}`);
      if (key === "needs_user")
        return t("voiceSecretaryAskStatusNeedsUser", { defaultValue: "Needs input" });
      if (key === "failed") return t("voiceSecretaryAskStatusFailed", { defaultValue: "Failed" });
      if (key === "handed_off")
        return t("voiceSecretaryAskStatusHandedOff", { defaultValue: "Handed off" });
      return t("voiceSecretaryAskStatusPending", { defaultValue: "Queued" });
    },
    [t],
  );
  const askFeedbackStatusClassName = useCallback(
    (status: string) => {
      const key = String(status || "pending")
        .trim()
        .toLowerCase();
      if (key === "needs_user")
        return isDark ? "bg-amber-400/14 text-amber-100" : "bg-amber-50 text-amber-800";
      if (key === "failed")
        return isDark ? "bg-rose-400/14 text-rose-100" : "bg-rose-50 text-rose-800";
      if (key === "handed_off")
        return isDark ? "bg-sky-400/14 text-sky-100" : "bg-sky-50 text-sky-800";
      return isDark ? "bg-white/10 text-slate-200" : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]";
    },
    [isDark],
  );
  const voiceModeLabel = useCallback(
    (mode: VoiceSecretaryCaptureMode) => {
      return assistantRowModeOptions.find((option) => option.key === mode)?.label || mode;
    },
    [assistantRowModeOptions],
  );
  const currentLiveTranscript =
    liveTranscriptPreview &&
    voiceTranscriptPreviewBelongsToGroup(liveTranscriptPreview, currentSelectedGroupId) &&
    (recording ||
      activityClockMs - liveTranscriptPreview.updatedAt <= VOICE_LIVE_TRANSCRIPT_VISIBLE_MS)
      ? liveTranscriptPreview
      : null;
  const liveTranscriptDisplayText = currentLiveTranscript
    ? normalizeBrowserTranscriptChunk(currentLiveTranscript.text)
    : "";
  const liveTranscriptSummaryPreview = currentLiveTranscript
    ? compactVoiceTranscriptSummaryText(liveTranscriptDisplayText)
    : "";
  const liveTranscriptSummaryText =
    currentLiveTranscript && liveTranscriptSummaryPreview
      ? `${voiceModeLabel(currentLiveTranscript.mode)} · ${liveTranscriptSummaryPreview}`
      : "";
  const promptOptimizePending = Boolean(
    pendingPromptRequestId && !pendingPromptDraft && !pendingPromptProblem,
  );
  const canOptimizeComposerPrompt =
    composerCaptureMode === "prompt" &&
    !secretaryNotReadyReason &&
    !pendingPromptBlocksOtherGroup &&
    !!composerText.trim() &&
    !promptOptimizePending &&
    !pendingPromptDraft;
  const pendingAskFeedback = askNotice.requestId
    ? askFeedbackItems.find((item) => item.request_id === askNotice.requestId) || null
    : null;
  const pendingAskFeedbackHasFinalReply = voiceAskReplyIsCurrent(
    pendingAskTask,
    pendingAskFeedback,
  );
  const pendingAskFeedbackStatus = pendingAskTask
    ? pendingAskFeedbackHasFinalReply && secretaryTaskRunning(pendingAskTask)
      ? askFeedbackStatusKey(pendingAskFeedback!.status)
      : pendingAskTask.phase === "running"
        ? "working"
        : pendingAskTask.phase === "queued"
          ? "pending"
          : pendingAskTask.phase === "done" && !pendingAskFeedbackHasFinalReply
            ? "result_pending"
            : pendingAskTask.phase
    : pendingAskFeedback && !hasFinalAskReply(pendingAskFeedback)
      ? "submitted"
      : pendingAskFeedback
        ? askFeedbackStatusKey(pendingAskFeedback.status)
        : "";
  const pendingAskFeedbackText = pendingAskFeedback
    ? pendingAskFeedbackHasFinalReply
      ? askFeedbackDisplayText(pendingAskFeedback)
      : String(
          pendingAskTask?.preview ||
            pendingAskFeedback.request_preview ||
            pendingAskFeedback.request_text ||
            "",
        ).trim()
    : "";
  const pendingAskFeedbackStatusText = pendingAskFeedback
    ? askStatusReadError
      ? t("voiceSecretaryAskStatusReadFailed", { defaultValue: "Status not updated" })
      : pendingAskFeedbackStatus
        ? askFeedbackStatusLabel(pendingAskFeedbackStatus)
        : ""
    : "";
  const pendingAskFeedbackSummaryText = pendingAskFeedback
    ? pendingAskFeedbackHasFinalReply
      ? pendingAskFeedbackStatus !== "done"
        ? askFeedbackStatusLabel(pendingAskFeedbackStatus)
        : t("voiceSecretaryReplyReadyShort", { defaultValue: "Reply ready" })
      : pendingAskFeedbackStatusText
        ? pendingAskFeedbackText
          ? `${pendingAskFeedbackStatusText} · ${pendingAskFeedbackText}`
          : pendingAskFeedbackStatusText
        : ""
    : "";
  const askThreadItems = useMemo(
    () =>
      askFeedbackItems
        .filter((item) => !ASK_THREAD_EXCLUDED_TARGETS.has(String(item.target_kind || "")))
        .map((item) => ({
          item,
          sortAt:
            assistantVoiceTimestampMs(item.updated_at) ||
            assistantVoiceTimestampMs(item.created_at) ||
            0,
          status: displayAskFeedbackStatus(item),
        }))
        .reverse(),
    [askFeedbackItems],
  );
  const latestVoiceReplyFeedback = useMemo(
    () =>
      askFeedbackItems.find(
        (item) =>
          hasFinalAskReply(item) &&
          (item.request_id !== askNotice.requestId || voiceAskReplyIsCurrent(pendingAskTask, item)),
      ) || null,
    [askFeedbackItems, askNotice.requestId, pendingAskTask],
  );
  const voiceReplyBubbleFeedback = useMemo(() => {
    const targetId = String(voiceReplyBubbleRequestId || "").trim();
    if (!targetId || !askFeedbackItems.length) return null;
    return (
      askFeedbackItems.find(
        (item) =>
          item.request_id === targetId &&
          hasFinalAskReply(item) &&
          (item.request_id !== askNotice.requestId || voiceAskReplyIsCurrent(pendingAskTask, item)),
      ) || null
    );
  }, [askFeedbackItems, voiceReplyBubbleRequestId, askNotice.requestId, pendingAskTask]);
  const voiceReplyBubbleText = String(voiceReplyBubbleFeedback?.reply_text || "").trim();
  const openVoiceReplyBubble = useCallback((item?: AssistantVoiceAskFeedback | null) => {
    const requestId = String(item?.request_id || "").trim();
    const dismissKey = voiceReplyDismissKey(item);
    if (!requestId || !dismissKey) return;
    dismissedVoiceReplyKeysRef.current.delete(dismissKey);
    setVoiceReplyBubbleRequestId(requestId);
  }, []);
  const closeVoiceReplyBubble = useCallback(() => {
    const dismissKey = voiceReplyDismissKey(voiceReplyBubbleFeedback);
    if (dismissKey) dismissedVoiceReplyKeysRef.current.add(dismissKey);
    setVoiceReplyBubbleRequestId("");
  }, [voiceReplyBubbleFeedback]);
  const hideAskNotice = useCallback(() => {
    const notice = { ...askNoticeRef.current, hidden: true };
    askNoticeRef.current = notice;
    saveVoiceAskNotice(selectedGroupId, notice);
    setAskNotice(notice);
    onFocusComposer?.();
  }, [onFocusComposer, selectedGroupId]);
  const copyVoiceReplyBubble = useCallback(async () => {
    const requestId = String(voiceReplyBubbleFeedback?.request_id || "").trim();
    if (!voiceReplyBubbleText || !requestId) return;
    const ok = await copyTextToClipboard(voiceReplyBubbleText);
    if (!ok) {
      showError(
        t("voiceSecretaryReplyCopyFailed", {
          defaultValue: "Failed to copy Voice Secretary reply.",
        }),
      );
      return;
    }
    setCopiedVoiceReplyRequestId(requestId);
    showNotice({
      message: t("voiceSecretaryReplyCopied", { defaultValue: "Voice Secretary reply copied." }),
    });
    if (typeof window !== "undefined") {
      window.setTimeout(() => {
        setCopiedVoiceReplyRequestId((current) => (current === requestId ? "" : current));
      }, 1800);
    }
  }, [showError, showNotice, t, voiceReplyBubbleFeedback?.request_id, voiceReplyBubbleText]);
  useEffect(() => {
    const tracker = {
      replyKeyByRequestId: askFeedbackReplyKeyByRequestIdRef.current,
      localRequestIds: localVoiceReplyRequestIdsRef.current,
      dismissedReplyKeys: dismissedVoiceReplyKeysRef.current,
    };
    const requestId = resolveAutoOpenVoiceReplyBubbleRequestId(tracker, latestVoiceReplyFeedback);
    if (!requestId) return;
    setVoiceReplyBubbleRequestId(requestId);
  }, [latestVoiceReplyFeedback]);
  useEffect(() => {
    trackActiveVoiceReplyRequests(
      {
        replyKeyByRequestId: askFeedbackReplyKeyByRequestIdRef.current,
        localRequestIds: localVoiceReplyRequestIdsRef.current,
        dismissedReplyKeys: dismissedVoiceReplyKeysRef.current,
      },
      askFeedbackItems,
    );
  }, [askFeedbackItems]);
  const lastRecordingStopReasonText = useMemo(() => {
    if (!lastRecordingStopReason || recording || recordingStarting) return "";
    const detail = String(lastRecordingStopReason.detail || "").trim();
    if (
      lastRecordingStopReason.code === "media_track_muted" ||
      lastRecordingStopReason.code === "media_track_unmuted" ||
      lastRecordingStopReason.code === "audio_context_state"
    ) {
      return "";
    }
    if (lastRecordingStopReason.code === "media_track_ended") {
      return t("voiceSecretaryRecordingStoppedTrackEnded", {
        detail,
        defaultValue: detail
          ? "Recording stopped because microphone input ended: {{detail}}."
          : "Recording stopped because microphone input ended.",
      });
    }
    if (lastRecordingStopReason.code === "local_asr_ws_closed") {
      return t("voiceSecretaryRecordingStoppedLocalAsrClosed", {
        detail,
        defaultValue: detail
          ? "Recording stopped because the local ASR stream closed: {{detail}}."
          : "Recording stopped because the local ASR stream closed.",
      });
    }
    if (lastRecordingStopReason.code === "local_asr_ws_error") {
      return t("voiceSecretaryRecordingStoppedLocalAsrError", {
        defaultValue: "Recording stopped because the local ASR stream failed.",
      });
    }
    if (lastRecordingStopReason.code === "local_asr_backend_error") {
      return t("voiceSecretaryRecordingStoppedLocalAsrBackend", {
        detail,
        defaultValue: detail
          ? "Recording stopped because local ASR returned an error: {{detail}}."
          : "Recording stopped because local ASR returned an error.",
      });
    }
    if (lastRecordingStopReason.code === "browser_speech_ended") {
      return t("voiceSecretaryRecordingStoppedBrowserSpeechEnded", {
        defaultValue: "Browser ASR stopped and could not restart.",
      });
    }
    if (lastRecordingStopReason.code === "browser_speech_fatal_error") {
      return t("voiceSecretaryRecordingStoppedBrowserSpeechError", {
        detail,
        defaultValue: detail
          ? "Browser ASR stopped: {{detail}}"
          : "Browser ASR stopped unexpectedly.",
      });
    }
    return t("voiceSecretaryRecordingStoppedUnexpectedly", {
      reason: lastRecordingStopReason.code,
      defaultValue: "Recording stopped unexpectedly: {{reason}}.",
    });
  }, [lastRecordingStopReason, recording, recordingStarting, t]);
  const speechErrorHint =
    speechError.trim() && browserSpeechRecoveryCode && (recording || recordingStarting)
      ? t("voiceSecretarySpeechRecovering", {
          code: browserSpeechRecoveryCode,
          defaultValue: "Browser speech recognition encountered {{code}} and is reconnecting.",
        })
      : speechError.trim();
  const panelNoticeText = recordingGroupNoticeText
    ? recordingSettingsLockedTitle
    : speechErrorHint || lastRecordingStopReasonText;
  const recordingTargetText = (() => {
    if (recordingGroupNoticeText) return recordingGroupNoticeText;
    if (!recordingTarget) return t("voiceSecretaryRecording");
    if (recordingTarget.mode === "document") {
      const title =
        String(findVoiceDocument(documents, recordingTarget.documentPath)?.title || "").trim() ||
        recordingTarget.documentPath ||
        documentDisplayTitle;
      return t("voiceSecretaryRecordingTargetDocument", { title });
    }
    if (recordingTarget.mode === "instruction") return t("voiceSecretaryRecordingTargetAsk");
    return recordingTarget.dispatchTarget === "composer"
      ? t("voiceSecretaryRecordingTargetDictation")
      : t("voiceSecretaryRecordingTargetPrompt");
  })();
  const panelRequestDisabled = !!actionBusy || !documentInstruction.trim();
  const composerErrorText =
    (!pendingPromptInOtherGroup && pendingPromptProblem) || composerSecretaryUnavailableReason;
  const composerActivity = resolveVoiceComposerActivity({
    recording: recording || recordingStarting,
    promptWaiting: !pendingPromptInOtherGroup && promptDraftWaiting,
    promptReady: !pendingPromptInOtherGroup && !!pendingPromptDraft,
    askSummary: pendingAskFeedback && !askNotice.hidden ? pendingAskFeedbackSummaryText : "",
    transcriptSummary: liveTranscriptSummaryText,
  });
  const composerAnnouncement =
    composerActivity === "recording"
      ? t("voiceSecretaryRecording")
      : composerActivity === "promptWaiting"
        ? promptDraftWaitingTitle
        : composerActivity === "promptReady"
          ? promptDraftReadyTitle
          : composerActivity === "ask" || (askNotice.hidden && pendingAskFeedbackHasFinalReply)
            ? pendingAskFeedbackHasFinalReply
              ? t("voiceSecretaryReplyReadyShort")
              : pendingAskFeedbackStatusText
            : "";
  const assistantRowCurrentMode =
    assistantRowModeOptions.find((option) => option.key === composerCaptureMode) ||
    assistantRowModeOptions[0];
  useEffect(() => {
    setExecutionOpen(false);
    setExecutionTaskId("");
    setMobileDocumentListOpen(false);
  }, [selectedGroupId]);
  useEffect(() => {
    if (!open || !isSmallScreen) return undefined;
    const node = workspaceScrollRef.current;
    if (!node) return undefined;
    const resetScroll = () => {
      node.scrollTop = 0;
      node.scrollLeft = 0;
    };
    resetScroll();
    let secondFrame = 0;
    const firstFrame = window.requestAnimationFrame(() => {
      resetScroll();
      secondFrame = window.requestAnimationFrame(resetScroll);
    });
    const lateReset = window.setTimeout(resetScroll, 160);
    return () => {
      window.cancelAnimationFrame(firstFrame);
      if (secondFrame) window.cancelAnimationFrame(secondFrame);
      window.clearTimeout(lateReset);
    };
  }, [activeDocumentPath, executionOpen, isSmallScreen, open, panelView, voiceWorkspaceView]);
  const modeChangeDisabledReason = recording ? recordingSettingsLockedTitle : "";
  const assistantRowControlLabel = recording
    ? t("voiceSecretaryStopAndSave", { defaultValue: "Stop and save recording" })
    : !composerNeedsSecretary
      ? t("voiceDirectDictationStart", { defaultValue: "Start direct dictation" })
      : captureStartTitle;
  const promptCaptureStateLabel = promptAutoRefine
    ? t("voiceSecretaryPromptAutoRefineState")
    : t("voiceSecretaryModeDictation");
  const promptCaptureBlocked =
    !recording &&
    composerCaptureMode === "prompt" &&
    promptAutoRefine &&
    pendingPromptBlocksOtherGroup;
  const promptOptimizeTitle = secretaryNotReadyReason
    ? secretaryNotReadyReason
    : pendingPromptBlocksOtherGroup
      ? t("voiceSecretaryPromptPendingOtherGroup", {
          defaultValue:
            "Another Group already has a prompt refinement in progress. Wait for it to finish before starting another.",
        })
      : promptOptimizePending
        ? t("voiceSecretaryPromptOptimizingButton", { defaultValue: "Optimizing..." })
        : !composerText.trim()
          ? t("voiceSecretaryPromptOptimizeNeedsText", { defaultValue: "Type a prompt first" })
          : t("voiceSecretaryPromptOptimizeButton", { defaultValue: "Optimize current prompt" });
  const handleAssistantRowModeChange = useCallback(
    (nextMode: VoiceSecretaryCaptureMode) => {
      if (recording || recordingStarting) return;
      onCaptureModeChange?.(nextMode);
      setShowAssistantModeMenu(false);
    },
    [onCaptureModeChange, recording, recordingStarting],
  );
  const updatePromptAutoRefine = useCallback(
    (enabled: boolean) => {
      if (recording || recordingStarting) return;
      useUIStore
        .getState()
        .setChatVoicePreferences(selectedGroupId, { voicePromptAutoRefine: enabled });
    },
    [recording, recordingStarting, selectedGroupId],
  );
  const toggleRecording = useCallback(() => {
    if (recordingStarting) return;
    if (recording) stopCurrentRecording();
    else void startDictation();
  }, [recording, recordingStarting, startDictation, stopCurrentRecording]);
  const handleAssistantRowRecordClick = useCallback(
    (event?: ReactMouseEvent<HTMLButtonElement>) => {
      event?.preventDefault();
      toggleRecording();
    },
    [toggleRecording],
  );
  const openPanel = useCallback(
    (view?: VoicePanelView) => {
      setPanelView(
        (current) => view ?? voicePanelViewForComposerMode(composerCaptureMode, current),
      );
      setExecutionOpen(false);
      setExecutionTaskId("");
      setOpen(true);
    },
    [composerCaptureMode],
  );
  const showExecution = useCallback((task?: SecretaryTaskSummary | null) => {
    setExecutionTaskId(task?.task_id || "");
    setExecutionOpen(true);
    setOpen(true);
  }, []);
  const openDocumentInPanel = useCallback(
    (path: string) => {
      const document = path ? findVoiceDocument(documents, path) : null;
      openPanel("document");
      if (document) void selectDocument(document);
    },
    [documents, openPanel, selectDocument],
  );
  const focusPanelInput = useCallback(() => {
    window.requestAnimationFrame(() =>
      panelInputRef.current?.querySelector<HTMLTextAreaElement>("textarea")?.focus(),
    );
  }, []);
  const followUpAsk = useCallback(
    (item: AssistantVoiceAskFeedback) => {
      const question = String(item.request_text || item.request_preview || "").trim();
      const answer = String(item.reply_text || "").trim();
      const quote = t("voiceSecretaryFollowUpQuote", {
        question: question.slice(0, 600),
        answer: answer.slice(0, 1200),
      });
      setDocumentInstruction((current) => (current.trim() ? `${quote}\n${current}` : `${quote}\n`));
      focusPanelInput();
    },
    [focusPanelInput, t],
  );
  const cancelPendingPrompt = useCallback(async () => {
    const task = pendingPromptTask;
    const gid = String(pendingPromptGroupIdRef.current || selectedGroupId || "").trim();
    if (!task || !gid) return;
    setPromptCancelBusy(true);
    try {
      const resp = await cancelSecretaryTask(gid, task.task_id);
      if (!resp.ok) {
        showError(resp.error.message);
        return;
      }
      if (pendingPromptRequestIdRef.current === task.target.request_id) {
        clearPendingPromptRequest(true);
      }
    } catch {
      showError(t("settings:voiceSettings.actionFailed"));
    } finally {
      setPromptCancelBusy(false);
    }
  }, [clearPendingPromptRequest, pendingPromptTask, selectedGroupId, showError, t]);
  const copyAskReply = useCallback(
    async (item: AssistantVoiceAskFeedback) => {
      const ok = await copyTextToClipboard(String(item.reply_text || "").trim());
      if (ok) showNotice({ message: t("voiceSecretaryReplyCopied") });
      else showError(t("voiceSecretaryReplyCopyFailed"));
    },
    [showError, showNotice, t],
  );
  const handlePromptOptimizeClick = useCallback(
    (event?: ReactMouseEvent<HTMLButtonElement>) => {
      event?.preventDefault();
      if (!canOptimizeComposerPrompt || controlDisabled || !!actionBusy) return;
      void requestPromptRefine("", "composer_prompt_refine", {
        operation: "replace_with_refined_prompt",
        targetGroupId: selectedGroupId,
        composerText,
        composerContext,
      });
    },
    [
      actionBusy,
      canOptimizeComposerPrompt,
      composerContext,
      composerText,
      controlDisabled,
      requestPromptRefine,
      selectedGroupId,
    ],
  );
  return (
    <div className={classNames("relative", isAssistantRow ? "w-auto shrink-0" : "self-end")}>
      {open && typeof document !== "undefined"
        ? createPortal(
            <div
              className="fixed inset-0 z-[180] flex items-end justify-center p-0 sm:items-center sm:p-4"
              style={{
                ...VISUAL_VIEWPORT_BOX,
                bottom: "auto",
                display: settingsOpen ? "none" : undefined,
              }}
              aria-hidden={settingsOpen || undefined}
            >
              <div
                className="absolute inset-0 glass-overlay"
                onPointerDown={(event) => {
                  if (event.target === event.currentTarget) closePanel();
                }}
                aria-hidden="true"
              />
              <section
                ref={modalRef}
                data-voice-mobile-sheet
                data-voice-sheet-mode={executionOpen ? "execution" : panelView}
                role="dialog"
                aria-modal="true"
                aria-labelledby="voice-secretary-sheet-title"
                aria-describedby="voice-secretary-sheet-description"
                className={classNames(
                  "relative z-[181] flex h-[min(92dvh,58rem)] max-h-[calc(var(--app-viewport-height,100dvh)-0.75rem)] w-full max-w-[88rem] flex-col overflow-hidden rounded-t-[28px] border p-3 pt-6 shadow-2xl glass-modal sm:w-[min(94vw,88rem)] sm:rounded-[30px]",
                  isDark ? "border-white/10 bg-slate-950/96" : "border-black/10 bg-white/96",
                )}
                onPointerDown={(event) => event.stopPropagation()}
              >
                <div id="voice-secretary-sheet-description" className="sr-only">
                  {t("voiceSecretaryWorkspaceHint")}
                </div>
                <div
                  data-voice-sheet-header
                  className={classNames(
                    "shrink-0 border-b px-4 pb-3 pt-1 sm:px-5",
                    isDark ? "border-white/10" : "border-black/10",
                  )}
                >
                  <div className="flex flex-wrap items-center gap-x-4 gap-y-2 pr-10">
                    <h2
                      id="voice-secretary-sheet-title"
                      className="min-w-0 text-lg font-semibold tracking-[-0.02em] text-[var(--color-text-primary)]"
                    >
                      {t("voiceSecretaryTitle")}
                      <span className="ml-2 text-sm font-normal text-[var(--color-text-muted)]">
                        {executionOpen
                          ? t("settings:voiceSettings.resident.scope")
                          : executionGroupLabel}
                        {loading ? ` · ${t("loadingContext")}` : ""}
                      </span>
                    </h2>
                    <div className="flex min-w-0 flex-1 items-center gap-2 max-sm:basis-full">
                      {!executionOpen ? (
                        <div
                          data-voice-panel-views
                          role="group"
                          aria-label={t("voiceSecretaryPanelViews")}
                          className={classNames(
                            "inline-flex min-h-[38px] items-center rounded-full border p-0.5",
                            isDark ? "border-white/10 bg-white/[0.04]" : "border-black/10 bg-white",
                          )}
                        >
                          {(["document", "ask"] as const).map((view) => {
                            const active = panelView === view;
                            return (
                              <button
                                key={view}
                                type="button"
                                data-voice-panel-view={view}
                                aria-pressed={active}
                                onClick={() => setPanelView(view)}
                                className={classNames(
                                  "min-h-[34px] rounded-full px-3.5 text-xs font-semibold transition-colors focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]",
                                  active
                                    ? isDark
                                      ? "bg-white text-slate-950 shadow-sm"
                                      : "bg-[rgb(35,36,37)] text-white shadow-sm"
                                    : "text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-tertiary)]",
                                )}
                              >
                                {t(
                                  view === "document"
                                    ? "voiceSecretaryPanelViewDocument"
                                    : "voiceSecretaryPanelViewAsk",
                                )}
                              </button>
                            );
                          })}
                        </div>
                      ) : null}
                      <div className="ml-auto flex items-center gap-1">
                        <button
                          type="button"
                          data-voice-workstage-toggle
                          aria-pressed={executionOpen}
                          disabled={controlDisabled}
                          onClick={() => {
                            setExecutionTaskId("");
                            setExecutionOpen((value) => !value);
                          }}
                          className="inline-flex min-h-[38px] items-center gap-1.5 rounded-lg px-2 text-xs font-medium text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-tertiary)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
                        >
                          {executionOpen ? (
                            <ChevronLeftIcon size={15} aria-hidden="true" />
                          ) : (
                            <TerminalIcon size={15} aria-hidden="true" />
                          )}
                          <span className="max-sm:sr-only">
                            {t(
                              executionOpen
                                ? "voiceSecretaryBackToWorkspace"
                                : "voiceSecretaryViewExecution",
                            )}
                          </span>
                        </button>
                        <button
                          type="button"
                          data-voice-settings
                          className="inline-flex h-[38px] w-[38px] shrink-0 items-center justify-center rounded-lg text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-tertiary)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
                          aria-label={t("settings:voiceSettings.title")}
                          title={t("settings:voiceSettings.title")}
                          onClick={openVoiceSettings}
                        >
                          <SettingsIcon size={17} aria-hidden="true" />
                        </button>
                      </div>
                    </div>
                  </div>
                  {panelNoticeText ? (
                    <p
                      role="status"
                      data-voice-header-notice
                      className={classNames(
                        "mt-2 line-clamp-2 break-words text-xs leading-5",
                        recordingGroupNoticeText
                          ? isDark
                            ? "text-rose-200"
                            : "text-rose-700"
                          : "text-[var(--color-text-secondary)]",
                      )}
                      title={panelNoticeText}
                    >
                      {panelNoticeText}
                    </p>
                  ) : null}
                </div>

                {secretaryNotReadyReason ? (
                  <div
                    role="status"
                    data-voice-readiness={secretaryIssue?.code}
                    className={classNames(
                      "mx-4 mt-3 flex shrink-0 flex-wrap items-center gap-2 rounded-lg border px-3 py-2 text-xs leading-5 sm:mx-5",
                      isDark
                        ? "border-amber-300/25 bg-amber-400/10 text-amber-100"
                        : "border-amber-200 bg-amber-50 text-amber-900",
                    )}
                  >
                    <span className="min-w-0 flex-1 break-words">
                      <span className="font-semibold">{secretaryNotReadyReason}</span>
                      <span className="opacity-90 max-sm:hidden">
                        {" · "}
                        {secretaryIssue && t(`settings:${secretaryIssue.hintKey}`)}
                      </span>
                      {secretaryReadinessDiagnostic && !executionOpen ? (
                        <span className="block opacity-75">{secretaryReadinessDiagnostic}</span>
                      ) : null}
                    </span>
                    {canAccessGlobalSettings === true &&
                    (secretaryIssue?.action === "settings" || !executionOpen) ? (
                      <button
                        type="button"
                        data-voice-readiness-action
                        onClick={() => {
                          if (secretaryIssue?.action === "execution") {
                            setExecutionTaskId("");
                            setExecutionOpen(true);
                          } else openVoiceSettings();
                        }}
                        className="min-h-9 shrink-0 rounded-lg border border-current/30 px-3 font-semibold hover:bg-black/5 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] dark:hover:bg-white/10"
                      >
                        {t(
                          secretaryIssue?.action === "execution"
                            ? "voiceSecretaryViewExecution"
                            : "voiceSecretaryConfigure",
                        )}
                      </button>
                    ) : canAccessGlobalSettings === false ? (
                      <span className="shrink-0 opacity-80">
                        {t(
                          secretaryIssue?.action === "execution"
                            ? "settings:voiceSettings.readiness.askAdminToCheck"
                            : "voiceSecretaryAskAdminToConfigure",
                        )}
                      </span>
                    ) : null}
                  </div>
                ) : null}

                <div
                  ref={workspaceScrollRef}
                  data-voice-workspace-body
                  data-voice-body-mode={executionOpen ? "execution" : panelView}
                  className="flex min-h-0 flex-1 flex-col overflow-hidden px-4 py-4 [overflow-anchor:none] sm:px-5 sm:py-5"
                >
                  {executionOpen ? (
                    <SecretaryTasks
                      key={executionTaskId || "latest"}
                      groupId={selectedGroupId}
                      groupTitle={executionGroupLabel}
                      active={open && !settingsOpen}
                      isDark={isDark}
                      workspace
                      documents={documents}
                      initialTaskId={executionTaskId || undefined}
                      onRetryAccepted={acceptAskRetry}
                      canOpenTarget={(task) =>
                        task.target.kind !== "document" ||
                        documents.some(
                          (doc) =>
                            doc.document_id === task.target.document_id &&
                            doc.document_path === task.target.document_path,
                        )
                      }
                      onOpenTarget={(task) => {
                        if (task.target.group_id !== selectedGroupId) return;
                        if (task.target.kind === "prompt") closePanel();
                        else if (task.target.kind === "document")
                          openDocumentInPanel(task.target.document_path);
                        else {
                          openPanel("ask");
                          window.requestAnimationFrame(() => {
                            const items =
                              modalRef.current?.querySelectorAll<HTMLElement>(
                                "[data-voice-ask-item]",
                              );
                            const item = Array.from(items || []).find(
                              (node) => node.dataset.voiceAskItem === task.target.request_id,
                            );
                            item?.scrollIntoView({ block: "nearest" });
                          });
                        }
                      }}
                    />
                  ) : panelView === "document" ? (
                    <>
                      <button
                        type="button"
                        data-voice-document-list-toggle
                        aria-expanded={mobileDocumentListOpen}
                        onClick={() => setMobileDocumentListOpen((value) => !value)}
                        className="mb-3 inline-flex min-h-11 shrink-0 items-center gap-1 self-start rounded-lg px-2 text-sm font-medium text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-tertiary)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] lg:hidden"
                      >
                        {mobileDocumentListOpen ? (
                          <>
                            <ChevronLeftIcon size={16} aria-hidden="true" />
                            {t("voiceSecretaryBackToDocument")}
                          </>
                        ) : (
                          <>
                            {t("voiceSecretaryDocumentList", { count: documents.length })}
                            <ChevronDownIcon size={14} aria-hidden="true" />
                          </>
                        )}
                      </button>
                      <div className="grid min-h-0 w-full flex-1 grid-cols-1 grid-rows-[minmax(0,1fr)] overflow-hidden rounded-xl border border-[var(--glass-panel-border)] lg:grid-cols-[16rem_minmax(0,1fr)]">
                        <div
                          data-voice-document-list-wrap
                          className={classNames(
                            "flex min-h-0 flex-col overflow-y-auto bg-[var(--color-bg-secondary)] scrollbar-subtle lg:border-r lg:border-[var(--glass-border-subtle)]",
                            mobileDocumentListOpen ? "" : "max-lg:hidden",
                          )}
                        >
                          <VoiceSecretaryDocumentListPanel
                            groupId={selectedGroupId}
                            onRestored={(document) => {
                              archivedDocumentPathsRef.current.delete(voiceDocumentPath(document));
                              void refreshAssistant({ quiet: true });
                            }}
                            onRenamed={() => void refreshAssistant({ quiet: true })}
                            actionBusy={actionBusy}
                            recording={recording}
                            onArchiveDocument={(document) => void archiveDocument(document)}
                            onDeleteDocument={(document) => void archiveDocument(document, true)}
                            activeDocumentPath={String(
                              activeDocumentWritePath || viewedDocumentPath || "",
                            ).trim()}
                            captureTargetDocumentPath={String(
                              captureTargetDocumentPath || "",
                            ).trim()}
                            creatingDocument={creatingDocument}
                            documents={documents}
                            isDark={isDark}
                            newDocumentTitleDraft={newDocumentTitleDraft}
                            t={t}
                            documentKey={voiceDocumentKey}
                            documentPath={voiceDocumentPath}
                            onCancelCreateDocument={cancelCreateDocument}
                            onCreateDocument={() => void createDocument()}
                            onNewDocumentTitleChange={setNewDocumentTitleDraft}
                            onSelectDocument={(document) => void selectDocument(document)}
                            onSetCaptureTargetDocument={(document) =>
                              void setCaptureTargetDocument(document)
                            }
                            onStartCreateDocument={startCreateDocument}
                          />
                        </div>
                        <div
                          className={classNames(
                            "flex min-h-0 min-w-0 flex-col",
                            mobileDocumentListOpen ? "max-lg:hidden" : "",
                          )}
                        >
                          <VoiceSecretaryWorkspacePanel
                            statusLine={
                              <SecretaryTasks
                                groupId={selectedGroupId}
                                active={open && !settingsOpen}
                                isDark={isDark}
                                kinds={["document"]}
                                hideReadinessError={!!secretaryNotReadyReason}
                                onShowExecution={showExecution}
                              />
                            }
                            footer={
                              <div ref={panelInputRef} className="voice-document-footer">
                                <VoicePanelInputBar
                                  isDark={isDark}
                                  recordLabel={t("voiceSecretaryRecordIntoDocument")}
                                  recordTitle={captureTargetDocumentTitle}
                                  recordDisabled={
                                    controlDisabled || !!actionBusy || !dictationSupported
                                  }
                                  recording={recording}
                                  recordingStarting={recordingStarting}
                                  recordingTargetLabel={recordingTargetText}
                                  stopLabel={t("voiceSecretaryStopAndSaveShort")}
                                  liveSnippet={liveTranscriptSummaryPreview}
                                  onToggleRecord={toggleRecording}
                                  value={documentInstruction}
                                  onChange={setDocumentInstruction}
                                  placeholder={t("voiceSecretaryDocumentInputPlaceholder")}
                                  inputLabel={t("voiceSecretaryDocumentInputLabel")}
                                  notice={
                                    documentHasUnsavedEdits
                                      ? t("voiceSecretaryDocumentUnsavedBeforeRequest")
                                      : ""
                                  }
                                  actions={[
                                    {
                                      key: "ask",
                                      label: t("voiceSecretaryAskDocumentButton"),
                                      onClick: () => void sendPanelRequest("askDocument"),
                                      disabled: panelRequestDisabled || documentHasUnsavedEdits,
                                    },
                                    {
                                      key: "update",
                                      label:
                                        actionBusy === "instruct_doc"
                                          ? t("voiceSecretaryApplyingInstruction")
                                          : t("voiceSecretaryUpdateDocumentButton"),
                                      onClick: () => void sendPanelRequest("document"),
                                      disabled:
                                        panelRequestDisabled ||
                                        documentHasUnsavedEdits ||
                                        !activeDocumentWritePath,
                                      primary: true,
                                    },
                                  ]}
                                />
                              </div>
                            }
                            activeDocumentPath={activeDocumentPath}
                            activeDocumentWritePath={activeDocumentWritePath}
                            actionBusy={actionBusy}
                            captureTargetDocumentPath={captureTargetDocumentPath}
                            documentDisplayTitle={documentDisplayTitle}
                            documentDraft={documentDraft}
                            documentEditing={documentEditing}
                            documentHasUnsavedEdits={documentHasUnsavedEdits}
                            documentLoading={documentContentLoading}
                            documentRemoteChanged={documentRemoteChanged}
                            isDark={isDark}
                            recording={
                              recording &&
                              recordingTarget?.mode === "document" &&
                              recordingTarget.documentPath === activeDocumentWritePath
                            }
                            recordingAudioLevel={getVoiceAudioLevel}
                            t={t}
                            transcriptItems={visibleVoiceTranscriptItems}
                            livePreview={currentLiveTranscript}
                            view={voiceWorkspaceView}
                            onChangeView={setVoiceWorkspaceView}
                            onArchiveDocument={() => void archiveDocument(activeDocument)}
                            onClearTranscript={() => {
                              liveTranscriptPreviewRef.current = null;
                              setLiveTranscriptPreview(null);
                              setVoiceTranscriptItems([]);
                              voiceStreamItemIdRef.current = "";
                            }}
                            onDownloadDocument={downloadCurrentDocument}
                            onEditDocumentChange={updateDocumentDraft}
                            onLoadLatestDocument={() => loadDocumentDraft(activeDocument)}
                            onQuoteDocument={() => {
                              if (!activeDocument) return;
                              const ref = buildVoiceDocumentMessageRef(
                                selectedGroupId,
                                activeDocument,
                              );
                              if (!ref || !onQuoteDocument) return;
                              onQuoteDocument(ref);
                              setOpen(false);
                            }}
                            onSaveDocument={() => void saveDocument()}
                            onToggleDocumentEditing={() => setDocumentEditing((value) => !value)}
                            formatTime={formatVoiceActivityTimeMs}
                            formatFullTime={formatVoiceActivityFullTimeMs}
                            normalizeTranscriptText={normalizeBrowserTranscriptChunk}
                          />
                        </div>
                      </div>
                    </>
                  ) : (
                    <div className="mx-auto flex min-h-0 w-full max-w-3xl flex-1 flex-col">
                      <VoiceAskThread
                        items={askThreadItems}
                        isDark={isDark}
                        t={t}
                        documents={documents}
                        documentKey={voiceDocumentKey}
                        tasks={askTasks}
                        statusLabel={askFeedbackStatusLabel}
                        statusClassName={askFeedbackStatusClassName}
                        formatTime={formatVoiceActivityTimeMs}
                        formatFullTime={formatVoiceActivityFullTimeMs}
                        onOpenDocument={(document) => void selectDocument(document)}
                        onFollowUp={followUpAsk}
                        onCopyReply={(item) => void copyAskReply(item)}
                        onShowExecution={showExecution}
                        onClear={() => void clearAskFeedbackHistory()}
                        clearing={actionBusy === "clear_ask"}
                      />
                      <div ref={panelInputRef}>
                        <VoicePanelInputBar
                          isDark={isDark}
                          recordLabel={t("voiceSecretaryRecordQuestion")}
                          recordDisabled={controlDisabled || !!actionBusy || !dictationSupported}
                          recording={recording}
                          recordingStarting={recordingStarting}
                          recordingTargetLabel={recordingTargetText}
                          stopLabel={t("voiceSecretaryStopAndSaveShort")}
                          liveSnippet={liveTranscriptSummaryPreview}
                          onToggleRecord={toggleRecording}
                          value={documentInstruction}
                          onChange={setDocumentInstruction}
                          placeholder={t("voiceSecretaryAskRequestPlaceholder")}
                          inputLabel={t("voiceSecretaryAskInputLabel")}
                          actions={[
                            {
                              key: "ask",
                              label:
                                actionBusy === "instruct_ask"
                                  ? t("voiceSecretaryApplyingInstruction")
                                  : t("voiceSecretaryAskDocumentButton"),
                              onClick: () => void sendPanelRequest("ask"),
                              disabled: panelRequestDisabled,
                              primary: true,
                            },
                          ]}
                        />
                      </div>
                    </div>
                  )}
                </div>
                <button
                  data-voice-sheet-close
                  type="button"
                  onClick={closePanel}
                  className="absolute right-3 top-3 inline-flex h-9 w-9 items-center justify-center rounded-lg text-[var(--color-text-muted)] transition-colors hover:bg-[var(--color-bg-tertiary)] hover:text-[var(--color-text-primary)] focus:outline-none focus-visible:ring-2 focus-visible:ring-[var(--color-border-focus)]/45 sm:right-4 sm:top-4"
                  aria-label={t("voiceSecretaryClose")}
                >
                  <CloseIcon size={16} aria-hidden="true" />
                </button>
              </section>
            </div>,
            document.body,
          )
        : null}

      {isAssistantRow ? (
        <div className="relative flex min-w-0 max-w-full items-center gap-1.5">
          {voiceReplyBubbleFeedback && voiceReplyBubbleText ? (
            <div
              className={classNames(
                "absolute bottom-full left-0 z-[80] mb-2 w-[min(28rem,calc(100vw-1.5rem))] overflow-hidden rounded-2xl border shadow-2xl",
                isDark
                  ? "border-white/10 bg-[rgb(18,22,28)] text-slate-100 shadow-black/40"
                  : "border-black/10 bg-white text-gray-900 shadow-black/15",
              )}
              role="dialog"
              aria-label={t("voiceSecretaryReplyBubbleTitle", {
                defaultValue: "Voice Secretary reply",
              })}
            >
              <div
                className={classNames(
                  "flex items-center justify-between gap-2 border-b px-3 py-2",
                  isDark ? "border-white/[0.08]" : "border-black/[0.06]",
                )}
              >
                <div className="min-w-0">
                  <div className="truncate text-[11px] font-semibold">
                    {t("voiceSecretaryReplyBubbleTitle", { defaultValue: "Voice Secretary reply" })}
                  </div>
                  <div
                    className={classNames(
                      "mt-0.5 text-[10px]",
                      isDark ? "text-slate-400" : "text-gray-500",
                    )}
                  >
                    {displayAskFeedbackStatus(voiceReplyBubbleFeedback)
                      ? askFeedbackStatusLabel(displayAskFeedbackStatus(voiceReplyBubbleFeedback))
                      : t("voiceSecretaryReplyReadyShort", { defaultValue: "Reply ready" })}
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1">
                  <button
                    type="button"
                    onClick={() => void copyVoiceReplyBubble()}
                    className={classNames(
                      "inline-flex h-7 w-7 items-center justify-center rounded-md transition-colors",
                      isDark
                        ? "text-slate-300 hover:bg-white/10 hover:text-white"
                        : "text-gray-500 hover:bg-black/5 hover:text-gray-900",
                    )}
                    aria-label={t("common:copy")}
                    title={t("common:copy")}
                  >
                    {copiedVoiceReplyRequestId === voiceReplyBubbleFeedback.request_id ? (
                      <span className="text-[11px] font-bold" aria-hidden="true">
                        ✓
                      </span>
                    ) : (
                      <CopyIcon size={14} aria-hidden="true" />
                    )}
                  </button>
                  <button
                    type="button"
                    onClick={closeVoiceReplyBubble}
                    className={classNames(
                      "inline-flex h-7 w-7 items-center justify-center rounded-md transition-colors",
                      isDark
                        ? "text-slate-300 hover:bg-white/10 hover:text-white"
                        : "text-gray-500 hover:bg-black/5 hover:text-gray-900",
                    )}
                    aria-label={t("common:close")}
                    title={t("common:close")}
                  >
                    <CloseIcon size={14} aria-hidden="true" />
                  </button>
                </div>
              </div>
              <div className="max-h-[18rem] overflow-auto px-3 py-2.5 text-[12px] leading-5">
                <LazyMarkdownRenderer
                  content={voiceReplyBubbleText}
                  isDark={isDark}
                  className="break-words [overflow-wrap:anywhere] max-w-full [&_p]:my-1 [&_li]:my-0.5 [&_pre]:my-2"
                  fallback={
                    <div className="whitespace-pre-wrap break-words">{voiceReplyBubbleText}</div>
                  }
                />
              </div>
            </div>
          ) : null}
          <div
            className={classNames(
              "inline-flex h-11 min-w-0 max-w-full shrink items-center gap-0.5 rounded-lg border transition-colors sm:h-9 sm:shrink-0 sm:p-0.5",
              recording
                ? isDark
                  ? "border-rose-400/30 bg-rose-500/10"
                  : "border-rose-200 bg-rose-50/70"
                : "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)]",
            )}
            role="group"
            aria-label={t("voiceSecretaryTitle", { defaultValue: "Voice Secretary" })}
          >
            <button
              type="button"
              className={classNames(
                "relative inline-flex h-11 w-11 shrink-0 items-center justify-center rounded-md transition-colors disabled:cursor-not-allowed disabled:opacity-50 sm:h-8 sm:w-8",
                recording
                  ? isDark
                    ? "bg-rose-500/20 text-rose-200 hover:bg-rose-500/28"
                    : "bg-rose-500 text-white shadow-sm hover:bg-rose-600"
                  : !dictationSupported
                    ? "text-[var(--color-text-tertiary)]"
                    : isDark
                      ? "text-[var(--color-text-secondary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                      : "text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-gray-900",
                !controlDisabled && !actionBusy && "active:scale-[0.96]",
              )}
              onClick={(event) => void handleAssistantRowRecordClick(event)}
              disabled={
                !!actionBusy ||
                recordingStarting ||
                controlDisabled ||
                promptCaptureBlocked ||
                (!recording && !dictationSupported)
              }
              aria-pressed={recording}
              aria-label={promptCaptureBlocked ? promptOptimizeTitle : assistantRowControlLabel}
              title={
                promptCaptureBlocked
                  ? promptOptimizeTitle
                  : `${assistantRowControlLabel} · ${assistantRowCurrentMode.label}`
              }
            >
              {recording ? (
                <StopIcon size={13} aria-hidden="true" />
              ) : (
                <MicrophoneIcon size={15} aria-hidden="true" />
              )}
            </button>
            <VoiceMobileMenu
              key={selectedGroupId}
              disabled={controlDisabled}
              settingsLocked={recording || recordingStarting}
              promptAutoRefine={promptAutoRefine}
              onPromptAutoRefineChange={updatePromptAutoRefine}
              mode={composerCaptureMode}
              modes={assistantRowModeOptions}
              onModeChange={onCaptureModeChange ? handleAssistantRowModeChange : undefined}
              language={configuredRecognitionLanguage}
              languageDisabled={recognitionLanguageDisabled}
              languages={voiceLanguageOptions.map((value) => ({
                value,
                label: voiceLanguageLabel(value),
              }))}
              onLanguageChange={(value) => {
                void updateRecognitionLanguage(value);
              }}
              optimizeLabel={promptOptimizeTitle}
              optimizeDisabled={controlDisabled || !!actionBusy || !canOptimizeComposerPrompt}
              onOptimize={handlePromptOptimizeClick}
              workspaceLabel={openButtonLabel}
              onWorkspace={() => {
                if (open) closePanel();
                else openPanel();
              }}
            />
            <div className="voice-desktop-controls">
              {onCaptureModeChange ? (
                <Popover open={showAssistantModeMenu} onOpenChange={setShowAssistantModeMenu}>
                  <PopoverTrigger asChild>
                    <button
                      ref={assistantModeTriggerRef}
                      type="button"
                      className={classNames(
                        "inline-flex h-11 min-w-0 shrink items-center justify-center gap-1 rounded-md px-2 text-[11px] font-semibold transition-colors disabled:cursor-not-allowed disabled:opacity-50 sm:h-8 sm:shrink-0",
                        isDark
                          ? "text-[var(--color-text-secondary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                          : "text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-gray-900",
                      )}
                      disabled={controlDisabled || recording || recordingStarting}
                      title={
                        modeChangeDisabledReason ||
                        `${t("voiceSecretaryModeSelector", { defaultValue: "Voice Secretary capture mode" })}: ${assistantRowCurrentMode.label}`
                      }
                      aria-label={t("voiceSecretaryModeSelector", {
                        defaultValue: "Voice Secretary capture mode",
                      })}
                    >
                      <span className="min-w-0 truncate">{assistantRowCurrentMode.label}</span>
                      {composerCaptureMode === "prompt" ? (
                        <span className="text-[var(--color-text-muted)]">
                          · {promptCaptureStateLabel}
                        </span>
                      ) : null}
                      <ChevronDownIcon size={12} aria-hidden="true" />
                    </button>
                  </PopoverTrigger>
                  <PopoverContent align="start" sideOffset={6} className="w-64 rounded-2xl p-1.5">
                    <div
                      role="group"
                      aria-label={t("voiceSecretaryModeSelector", {
                        defaultValue: "Voice Secretary capture mode",
                      })}
                    >
                      {assistantRowModeOptions.map((option) => {
                        const active = option.key === composerCaptureMode;
                        return (
                          <div
                            key={option.key}
                            role="group"
                            aria-label={option.label}
                            className={classNames(
                              "rounded-xl transition-colors",
                              active
                                ? isDark
                                  ? "bg-white/10"
                                  : "bg-black/5"
                                : isDark
                                  ? "hover:bg-white/5"
                                  : "hover:bg-black/5",
                            )}
                          >
                            <button
                              type="button"
                              className={classNames(
                                "w-full rounded-xl px-3 text-left flex items-center gap-2.5",
                                option.key === "prompt" ? "pt-2.5 pb-0" : "py-2.5",
                              )}
                              aria-pressed={active}
                              disabled={recording || recordingStarting}
                              title={modeChangeDisabledReason || option.description}
                              onClick={() => {
                                handleAssistantRowModeChange(option.key);
                              }}
                            >
                              <span
                                className={classNames(
                                  "w-6 h-6 rounded-md flex items-center justify-center flex-shrink-0",
                                  option.key === "document"
                                    ? isDark
                                      ? "bg-slate-700 text-slate-200"
                                      : "bg-gray-100 text-gray-700"
                                    : option.key === "instruction"
                                      ? isDark
                                        ? "bg-emerald-500/20 text-emerald-100"
                                        : "bg-emerald-50 text-emerald-700"
                                      : isDark
                                        ? "bg-indigo-500/25 text-indigo-200"
                                        : "bg-indigo-100 text-indigo-700",
                                )}
                              >
                                {option.key === "document" ? (
                                  <MicrophoneIcon size={13} />
                                ) : option.key === "instruction" ? (
                                  <span className="text-[12px] font-black leading-none">?</span>
                                ) : (
                                  <span className="text-[11px] font-black italic leading-none">
                                    P
                                  </span>
                                )}
                              </span>
                              <span className="min-w-0 flex-1">
                                <span
                                  className={classNames(
                                    "block text-sm font-semibold",
                                    isDark ? "text-slate-100" : "text-gray-900",
                                  )}
                                >
                                  {option.label}
                                </span>
                                <span
                                  className={classNames(
                                    "block text-[11px]",
                                    isDark ? "text-[var(--color-text-tertiary)]" : "text-gray-500",
                                  )}
                                >
                                  {option.description}
                                </span>
                              </span>
                              {active ? (
                                <span
                                  className={classNames(
                                    "text-xs font-semibold",
                                    isDark ? "text-slate-200" : "text-[rgb(35,36,37)]",
                                  )}
                                >
                                  ✓
                                </span>
                              ) : null}
                            </button>
                            {option.key === "prompt" ? (
                              <div className="pb-0.5 pl-[46px] pr-3">
                                <VoicePromptAutoRefineOption
                                  checked={promptAutoRefine}
                                  disabled={controlDisabled || recording || recordingStarting}
                                  onChange={updatePromptAutoRefine}
                                />
                              </div>
                            ) : null}
                          </div>
                        );
                      })}
                    </div>
                  </PopoverContent>
                </Popover>
              ) : (
                <span className="inline-flex h-11 items-center px-2 text-[11px] font-semibold text-[var(--color-text-secondary)] sm:h-8">
                  {assistantRowCurrentMode.label}
                </span>
              )}
              {composerCaptureMode === "prompt" ? (
                <button
                  type="button"
                  className={classNames(
                    "inline-flex h-11 w-11 shrink-0 items-center justify-center rounded-md transition-colors disabled:cursor-not-allowed disabled:opacity-50 sm:h-8 sm:w-8",
                    promptOptimizePending
                      ? isDark
                        ? "bg-amber-400/12 text-amber-100"
                        : "bg-amber-50 text-amber-800"
                      : isDark
                        ? "text-[var(--color-text-secondary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                        : "text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-gray-900",
                    !controlDisabled &&
                      !actionBusy &&
                      canOptimizeComposerPrompt &&
                      "active:scale-[0.96]",
                  )}
                  onClick={(event) => handlePromptOptimizeClick(event)}
                  disabled={controlDisabled || !!actionBusy || !canOptimizeComposerPrompt}
                  aria-label={promptOptimizeTitle}
                  title={promptOptimizeTitle}
                >
                  <SparklesIcon size={15} aria-hidden="true" />
                </button>
              ) : null}
              <Popover open={showAssistantLanguageMenu} onOpenChange={setShowAssistantLanguageMenu}>
                <PopoverTrigger asChild>
                  <button
                    ref={assistantLanguageTriggerRef}
                    type="button"
                    className={classNames(
                      "inline-flex h-11 shrink-0 items-center justify-center rounded-md px-1.5 text-[10px] font-bold tracking-[0.08em] transition-colors disabled:cursor-not-allowed disabled:opacity-50 sm:h-8",
                      isDark
                        ? "text-[var(--color-text-secondary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                        : "text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-gray-900",
                    )}
                    disabled={recognitionLanguageDisabled}
                    title={
                      recording
                        ? recordingSettingsLockedTitle
                        : `${t("voiceSecretaryLanguage", { defaultValue: "Language" })}: ${configuredRecognitionLanguageLabel}`
                    }
                    aria-label={`${t("voiceSecretaryLanguage", { defaultValue: "Language" })}: ${configuredRecognitionLanguageLabel}`}
                  >
                    {configuredRecognitionLanguageShortLabel}
                  </button>
                </PopoverTrigger>
                <PopoverContent align="start" sideOffset={6} className="w-52 rounded-2xl p-1.5">
                  <div
                    role="group"
                    aria-label={t("voiceSecretaryLanguage", { defaultValue: "Language" })}
                  >
                    {voiceLanguageOptions.map((optionValue) => {
                      const active = optionValue === configuredRecognitionLanguage;
                      return (
                        <button
                          key={optionValue}
                          type="button"
                          className={classNames(
                            "flex w-full items-center gap-2 rounded-xl px-3 py-2 text-left transition-colors",
                            active
                              ? isDark
                                ? "bg-white/10"
                                : "bg-black/5"
                              : isDark
                                ? "hover:bg-white/5"
                                : "hover:bg-black/5",
                          )}
                          aria-pressed={active}
                          onClick={() => {
                            setShowAssistantLanguageMenu(false);
                            void updateRecognitionLanguage(optionValue);
                          }}
                        >
                          <span
                            className={classNames(
                              "flex h-6 w-8 shrink-0 items-center justify-center rounded-md text-[10px] font-bold tracking-[0.08em]",
                              isDark ? "bg-white/10 text-slate-200" : "bg-gray-100 text-gray-700",
                            )}
                          >
                            {voiceLanguageShortLabel(optionValue)}
                          </span>
                          <span
                            className={classNames(
                              "min-w-0 flex-1 truncate text-sm font-semibold",
                              isDark ? "text-slate-100" : "text-gray-900",
                            )}
                          >
                            {voiceLanguageLabel(optionValue)}
                          </span>
                          {active ? (
                            <span
                              className={classNames(
                                "text-xs font-semibold",
                                isDark ? "text-slate-200" : "text-[rgb(35,36,37)]",
                              )}
                            >
                              ✓
                            </span>
                          ) : null}
                        </button>
                      );
                    })}
                  </div>
                </PopoverContent>
              </Popover>

              <button
                type="button"
                className={classNames(
                  "inline-flex h-11 w-11 shrink-0 items-center justify-center rounded-md transition-colors disabled:cursor-not-allowed disabled:opacity-50 sm:h-8 sm:w-8",
                  open
                    ? isDark
                      ? "bg-white/10 text-[var(--color-text-primary)]"
                      : "bg-black/5 text-gray-900"
                    : isDark
                      ? "text-[var(--color-text-secondary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                      : "text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-gray-900",
                  !controlDisabled && "active:scale-[0.96]",
                )}
                onClick={() => {
                  if (open) closePanel();
                  else openPanel();
                }}
                disabled={controlDisabled}
                aria-pressed={open}
                aria-label={openButtonLabel}
                title={openButtonLabel}
              >
                <MaximizeIcon size={15} />
              </button>
            </div>
          </div>
          <VoiceComposerStatus target={statusPortalTarget}>
            {composerErrorText ? (
              <div
                data-voice-secretary-error
                role="status"
                className={classNames(
                  "max-w-[min(34rem,calc(100vw-6rem))] break-words rounded-lg px-2.5 py-1 text-[11px]",
                  isDark ? "bg-rose-400/12 text-rose-100" : "bg-rose-50 text-rose-800",
                )}
              >
                {composerErrorText}
                {!pendingPromptInOtherGroup && pendingPromptProblem && pendingPromptTask ? (
                  <button
                    type="button"
                    data-voice-prompt-details
                    className="ml-2 rounded px-1 font-semibold underline underline-offset-2 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
                    onClick={() => showExecution(pendingPromptTask)}
                  >
                    {t("settings:voiceSettings.viewExecution")}
                  </button>
                ) : null}
              </div>
            ) : null}
            {speechErrorHint ? (
              <div
                data-voice-recognition-error
                role="status"
                className={classNames(
                  "max-w-[min(34rem,calc(100vw-6rem))] rounded-lg px-2.5 py-1 text-[11px]",
                  isDark ? "bg-rose-400/12 text-rose-100" : "bg-rose-50 text-rose-800",
                )}
              >
                {speechErrorHint}
              </div>
            ) : null}
            {composerActivity === "recording" ? (
              <div
                data-voice-composer-activity="recording"
                className={classNames(
                  "inline-flex max-w-[min(40rem,calc(100vw-6rem))] items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px]",
                  isDark ? "bg-rose-400/12 text-rose-100" : "bg-rose-50 text-rose-800",
                )}
                title={recordingGroupNoticeText ? recordingSettingsLockedTitle : undefined}
              >
                <span className="shrink-0 font-semibold">{recordingTargetText}</span>
                {liveTranscriptSummaryPreview ? (
                  <span className="min-w-0 truncate opacity-80">
                    {liveTranscriptSummaryPreview}
                  </span>
                ) : null}
                {recordingTarget?.mode === "document" && !recordingGroupNoticeText ? (
                  <button
                    type="button"
                    className="shrink-0 rounded px-1 font-semibold underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
                    onClick={() => openDocumentInPanel(recordingTarget.documentPath)}
                  >
                    {t("voiceSecretaryOpenDocument")}
                  </button>
                ) : null}
              </div>
            ) : composerActivity === "promptWaiting" || composerActivity === "promptReady" ? (
              <div
                data-voice-composer-activity={composerActivity}
                className={classNames(
                  "inline-flex max-w-[min(34rem,calc(100vw-6rem))] items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px]",
                  composerActivity === "promptReady" && !promptNeedsReview
                    ? isDark
                      ? "bg-emerald-400/12 text-emerald-100"
                      : "bg-emerald-50 text-emerald-900"
                    : isDark
                      ? "bg-amber-400/12 text-amber-100"
                      : "bg-amber-50 text-amber-900",
                )}
              >
                <span className="min-w-0 truncate font-semibold leading-4">
                  {composerActivity === "promptWaiting" ? (
                    <AnimatedShinyText
                      className={classNames(
                        isDark
                          ? "bg-[linear-gradient(110deg,rgba(254,243,199,0.82)_18%,rgba(255,255,255,0.98)_48%,rgba(251,191,36,0.94)_68%,rgba(254,243,199,0.82)_84%)]"
                          : "bg-[linear-gradient(110deg,rgb(120,53,15)_18%,rgb(217,119,6)_42%,rgb(255,255,255)_52%,rgb(146,64,14)_66%,rgb(120,53,15)_84%)]",
                      )}
                    >
                      {promptDraftWaitingTitle}
                    </AnimatedShinyText>
                  ) : (
                    promptDraftReadyTitle
                  )}
                </span>
                {composerActivity === "promptReady" && pendingPromptDraft && promptNeedsReview ? (
                  <VoicePromptDraftReview
                    key={pendingPromptDraft.request_id}
                    text={pendingPromptDraft.draft_text}
                    replace={promptDraftApplyMode(pendingPromptDraft) === "replace"}
                    onApply={() => applyPromptDraft(pendingPromptDraft, true)}
                    onDismiss={dismissPromptDraft}
                    onReturnToComposer={onFocusComposer}
                  />
                ) : null}
                {composerActivity === "promptWaiting" && pendingPromptTask ? (
                  <>
                    <button
                      type="button"
                      data-voice-prompt-details
                      className="shrink-0 rounded px-1 font-semibold underline-offset-2 hover:underline focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
                      onClick={() => showExecution(pendingPromptTask)}
                    >
                      {t("settings:voiceSettings.viewExecution")}
                    </button>
                    {secretaryTaskRunning(pendingPromptTask) ? (
                      <button
                        type="button"
                        data-voice-prompt-cancel
                        className="shrink-0 rounded px-1 font-semibold underline-offset-2 hover:underline disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
                        disabled={promptCancelBusy}
                        onClick={() => void cancelPendingPrompt()}
                      >
                        {t("settings:voiceSettings.cancelTask")}
                      </button>
                    ) : null}
                  </>
                ) : null}
              </div>
            ) : composerActivity === "ask" && pendingAskFeedback ? (
              <div
                data-voice-composer-activity="ask"
                className={classNames(
                  "inline-flex max-w-[min(34rem,calc(100vw-6rem))] items-center rounded-full pl-2.5 pr-1 text-left text-[11px]",
                  askFeedbackStatusClassName(pendingAskFeedbackStatus),
                )}
              >
                <button
                  type="button"
                  className="min-h-11 min-w-0 rounded-full py-1 text-left hover:opacity-85 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] sm:min-h-6"
                  onClick={() =>
                    pendingAskFeedbackHasFinalReply
                      ? openVoiceReplyBubble(pendingAskFeedback)
                      : pendingAskTask
                        ? showExecution(pendingAskTask)
                        : openPanel("ask")
                  }
                  title={
                    askStatusReadError ||
                    (pendingAskFeedbackHasFinalReply
                      ? t("voiceSecretaryOpenReply")
                      : t("settings:voiceSettings.viewExecution"))
                  }
                >
                  <span
                    className="min-w-0 whitespace-normal break-words font-semibold leading-4"
                    style={TWO_LINE_STATUS_STYLE}
                  >
                    {pendingAskFeedbackSummaryText}
                  </span>
                </button>
                <button
                  type="button"
                  data-voice-ask-hide
                  onClick={hideAskNotice}
                  className="inline-flex h-11 w-11 shrink-0 items-center justify-center rounded-full hover:bg-black/5 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)] dark:hover:bg-white/10 sm:h-6 sm:w-6"
                  aria-label={t("voiceSecretaryHideHint")}
                  title={t("voiceSecretaryHideHint")}
                >
                  <CloseIcon size={12} aria-hidden="true" />
                </button>
              </div>
            ) : composerActivity === "transcript" ? (
              <div
                data-voice-composer-activity="transcript"
                className={classNames(
                  "inline-flex max-w-[min(40rem,calc(100vw-6rem))] items-start rounded-full px-2.5 py-1 text-left text-[11px]",
                  isDark ? "bg-cyan-400/12 text-cyan-100" : "bg-cyan-50 text-cyan-900",
                )}
              >
                <span
                  className="min-w-0 whitespace-normal break-words font-semibold leading-4"
                  style={TWO_LINE_STATUS_STYLE}
                >
                  {liveTranscriptSummaryText}
                </span>
              </div>
            ) : null}
          </VoiceComposerStatus>
          <span className="sr-only" aria-live="polite" aria-atomic="true">
            {composerAnnouncement}
          </span>
        </div>
      ) : (
        <button
          type="button"
          className={classNames(
            buttonClassName ||
              classNames(
                "glass-btn flex items-center justify-center rounded-lg transition-colors disabled:cursor-not-allowed disabled:opacity-60",
                controlDisabled
                  ? "text-[var(--color-text-tertiary)]"
                  : recording
                    ? isDark
                      ? "border-rose-400/25 bg-rose-500/15 text-rose-100 hover:bg-rose-500/22"
                      : "border-rose-200 bg-rose-50 text-rose-700 hover:bg-rose-100"
                    : isDark
                      ? "text-[var(--color-text-secondary)] hover:bg-white/10 hover:text-[var(--color-text-primary)]"
                      : "text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-gray-800",
              ),
            "relative",
            recording && buttonClassName ? (isDark ? "!text-rose-300" : "!text-rose-600") : "",
          )}
          style={
            buttonClassName
              ? undefined
              : { width: `${buttonSizePx}px`, height: `${buttonSizePx}px` }
          }
          onClick={() => {
            if (open) closePanel();
            else openPanel();
          }}
          disabled={controlDisabled}
          aria-pressed={open}
          aria-label={openButtonLabel}
          title={openButtonLabel}
        >
          <MicrophoneIcon size={openButtonIconSizePx} className="transition-transform" />
          {recording ? (
            <span
              aria-hidden="true"
              className={classNames(
                "absolute right-1.5 top-1.5 h-2 w-2 rounded-full",
                "bg-rose-500 shadow-[0_0_0_3px_rgba(244,63,94,0.16)]",
              )}
            />
          ) : null}
        </button>
      )}
    </div>
  );
}
