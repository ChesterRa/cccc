import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  cancelCodexVoiceAnalyst,
  fetchActiveCodexVoiceCall,
  resetCodexVoiceAnalyst,
  stopCodexVoiceCall,
  type CodexVoiceAnalystInfo,
  type CodexVoiceCallInfo,
  type CodexVoiceReadiness,
} from "../../services/api";
import { CodexVoiceBrowserSession, type CodexVoicePhase } from "./codexVoiceSession";
import {
  codexVoiceErrorText,
  codexVoiceReadinessProblem,
  codexVoiceWarningText,
} from "./codexVoiceControllerText";
import { useCodexVoicePolling } from "./useCodexVoicePolling";
import { useCodexVoicePreferencesState } from "./useCodexVoicePreferencesState";
import { voiceAudioSnapshot } from "../../stores/useVoiceAudioStore";
import { useCodexVoiceWindowLifecycle } from "./useCodexVoiceWindowLifecycle";
import type { VoiceConversationTurn } from "./codexVoiceProtocol";
import type { CodexVoiceOutputStatus } from "./codexVoiceProviderChannel";

const ENGAGED_PHASES: CodexVoicePhase[] = ["preparing", "connecting", "stopping"];

export function useCodexVoiceSessionController(enabled = true) {
  const { t } = useTranslation("modals");
  const audioRef = useRef<HTMLAudioElement>(null);
  const sessionRef = useRef<CodexVoiceBrowserSession | null>(null);
  const mountedRef = useRef(true);
  const refreshGenerationRef = useRef(0);
  const enabledRef = useRef(enabled);
  const refreshFlightRef = useRef<{
    promise: Promise<void>;
    trailingGeneration: number | null;
  } | null>(null);
  const hasSnapshotRef = useRef(false);
  useLayoutEffect(() => {
    enabledRef.current = enabled;
  }, [enabled]);
  const [phase, setPhase] = useState<CodexVoicePhase>("idle");
  const [call, setCall] = useState<CodexVoiceCallInfo | null>(null);
  const [analyst, setAnalyst] = useState<CodexVoiceAnalystInfo | null>(null);
  const analystGenerationRef = useRef<string | null>(null);
  useLayoutEffect(() => {
    analystGenerationRef.current = analyst?.generation || null;
  }, [analyst?.generation]);
  const [owned, setOwned] = useState(false);
  const [checking, setChecking] = useState(enabled);
  const [conversation, setConversation] = useState<VoiceConversationTurn[]>([]);
  const [notificationPaused, setNotificationPaused] = useState(false);
  const [microphoneMuted, setMicrophoneMuted] = useState(false);
  const [playbackBlocked, setPlaybackBlocked] = useState(false);
  const [error, setError] = useState("");
  const [refreshError, setRefreshError] = useState("");
  const [outputStatus, setOutputStatus] = useState<CodexVoiceOutputStatus>({
    queued: 0,
    blocked: null,
  });
  const { preferences, supportedVoices, updatePreferences, acceptSupportedVoices } =
    useCodexVoicePreferencesState();
  const [readiness, setReadiness] = useState<CodexVoiceReadiness | null>(null);
  const analystWorking =
    analyst?.phase === "working" ||
    Boolean(
      analyst?.structured &&
      ((analyst.queued_inputs || 0) > 0 ||
        analyst.manual_tasks?.some((task) => ["queued", "working"].includes(task.status))),
    );

  const updateAnalystSnapshot = useCallback(
    (next: CodexVoiceAnalystInfo, confirmedControl = false) => {
      if (!mountedRef.current) return;
      const session = sessionRef.current;
      if (session) session.updateAnalystSnapshot(next);
      else if (analystGenerationRef.current === next.generation) {
        if (confirmedControl) {
          // Accepted controls supersede earlier reads. Ordinary ACP polling must
          // not retire the controller's slower call/readiness snapshots.
          refreshGenerationRef.current += 1;
          setChecking(false);
        }
        setAnalyst(next);
      }
    },
    [],
  );

  const refresh = useCallback(
    async (showChecking = true) => {
      if (!enabled) {
        if (showChecking) setChecking(false);
        return;
      }
      if (sessionRef.current) return;
      if (showChecking) setChecking(true);
      const existing = refreshFlightRef.current;
      if (existing) {
        // Focus, visibility and polling request one later snapshot without
        // invalidating a useful slow read already in flight.
        existing.trailingGeneration = refreshGenerationRef.current;
        return existing.promise;
      }
      const flight = { promise: Promise.resolve(), trailingGeneration: null as number | null };
      refreshFlightRef.current = flight;
      flight.promise = (async () => {
        do {
          flight.trailingGeneration = null;
          const generation = ++refreshGenerationRef.current;
          const current = () =>
            mountedRef.current &&
            enabledRef.current &&
            generation === refreshGenerationRef.current &&
            !sessionRef.current;
          const readFailed = () => {
            if (current()) {
              setRefreshError(
                t(hasSnapshotRef.current ? "codexVoiceStatusStale" : "codexVoiceStatusUnavailable"),
              );
            }
          };
          try {
            const response = await fetchActiveCodexVoiceCall();
            if (current()) {
              if (response.ok) {
                hasSnapshotRef.current = true;
                setRefreshError("");
                setCall(response.result.call);
                setAnalyst(response.result.analyst);
                setPhase((value) => (value === "failed" ? value : "idle"));
                setReadiness(response.result.readiness);
                acceptSupportedVoices(response.result.voices);
              } else readFailed();
            }
          } catch {
            readFailed();
          }
          if (current()) setChecking(false);
          // An explicit Start/Stop/Cancel retires earlier pending refreshes.
          // Only a refresh requested after that action may continue reading.
        } while (
          mountedRef.current &&
          enabledRef.current &&
          !sessionRef.current &&
          flight.trailingGeneration === refreshGenerationRef.current
        );
      })().finally(() => {
        if (refreshFlightRef.current === flight) refreshFlightRef.current = null;
      });
      return flight.promise;
    },
    [acceptSupportedVoices, enabled, t],
  );

  useCodexVoiceWindowLifecycle({ refresh, mountedRef, refreshGenerationRef, sessionRef });
  useCodexVoicePolling({ enabled, analyst, owned, sessionRef, refresh });

  const start = useCallback(async () => {
    const audio = audioRef.current;
    if (!audio || sessionRef.current) return;
    if (call) {
      setError(t("codexVoiceExistingCallStartBlocked"));
      return;
    }
    const readinessProblem = codexVoiceReadinessProblem(t, readiness);
    if (readinessProblem) {
      setPhase("failed");
      setError(readinessProblem);
      return;
    }

    refreshGenerationRef.current += 1;
    setChecking(false);
    setError("");
    setRefreshError("");
    setConversation([]);
    setNotificationPaused(false);
    setMicrophoneMuted(false);
    setPlaybackBlocked(false);
    setOutputStatus({ queued: 0, blocked: null });

    const session = new CodexVoiceBrowserSession({
      audio,
      preferences: { ...preferences, ...voiceAudioSnapshot() },
      callbacks: {
        onPhase: (next) => {
          if (mountedRef.current) setPhase(next);
        },
        onCall: (next) => {
          if (!mountedRef.current) return;
          setCall(next);
          if (!next) {
            if (sessionRef.current === session) sessionRef.current = null;
            setOwned(false);
          }
        },
        onAnalyst: (next) => {
          if (!mountedRef.current) return;
          setAnalyst(next);
        },
        onUserTranscript: () => undefined,
        onAssistantTranscript: () => undefined,
        onConversation: (turns) => {
          if (mountedRef.current) setConversation(turns);
        },
        onNotificationPaused: (paused) => {
          if (mountedRef.current) setNotificationPaused(paused);
        },
        onAnalystProgress: () => undefined,
        onAnalystResult: () => undefined,
        onPlaybackBlocked: (blocked) => {
          if (mountedRef.current) setPlaybackBlocked(blocked);
        },
        onOutputStatus: (status) => {
          if (mountedRef.current) setOutputStatus(status);
        },
        onError: (code, providerCode) => {
          if (mountedRef.current) setError(codexVoiceErrorText(t, code, providerCode));
        },
      },
    });
    sessionRef.current = session;
    setOwned(true);
    try {
      await session.start();
    } catch {
      if (sessionRef.current === session) sessionRef.current = null;
      if (mountedRef.current) {
        setOwned(false);
        void refresh();
      }
    }
  }, [call, preferences, readiness, refresh, t]);

  const disconnect = useCallback(async () => {
    refreshGenerationRef.current += 1;
    setError("");
    const session = sessionRef.current;
    sessionRef.current = null;
    if (session) {
      setOwned(false);
      await session.stop();
      return;
    }
    if (!call) return;
    setPhase("stopping");
    const response = await stopCodexVoiceCall(call.generation);
    if (!mountedRef.current) return;
    if (!response.ok) {
      setError(codexVoiceErrorText(t, response.error.code));
      setPhase("failed");
      return;
    }
    setCall(null);
    setPhase("idle");
  }, [call, t]);

  const cancelInvestigation = useCallback(async () => {
    if (!analyst || !analystWorking) return false;
    refreshGenerationRef.current += 1;
    setChecking(false);
    setError("");
    if (sessionRef.current?.cancelInvestigation()) return true;
    const response = await cancelCodexVoiceAnalyst(analyst.generation);
    if (!mountedRef.current) return false;
    if (!response.ok) {
      setError(codexVoiceErrorText(t, response.error.code));
      return false;
    }
    if (!response.result.cancelled) {
      await refresh();
      return false;
    }
    return true;
  }, [analyst, analystWorking, refresh, t]);

  const toggleMicrophone = useCallback(() => {
    const next = !microphoneMuted;
    if (!sessionRef.current?.setMicrophoneMuted(next)) return;
    setMicrophoneMuted(next);
  }, [microphoneMuted]);

  const resumeAudio = useCallback(async () => {
    if (await sessionRef.current?.resumeAudio()) setPlaybackBlocked(false);
  }, []);

  const startNewAnalyst = useCallback(async () => {
    if (!analyst || call || analystWorking) return false;
    refreshGenerationRef.current += 1;
    setChecking(false);
    setError("");
    const response = await resetCodexVoiceAnalyst(analyst.generation);
    if (!mountedRef.current) return false;
    if (!response.ok) {
      setError(codexVoiceErrorText(t, response.error.code));
      return false;
    }
    setAnalyst(response.result.analyst);
    return true;
  }, [analyst, analystWorking, call, t]);

  const clearError = useCallback(() => {
    setError("");
    setPhase((current) => (current === "failed" ? "idle" : current));
  }, []);

  return useMemo(
    () => ({
      audioRef,
      phase,
      call,
      analyst,
      owned,
      checking,
      conversation,
      notificationPaused,
      microphoneMuted,
      playbackBlocked,
      outputStatus,
      error,
      refreshError,
      preferences,
      audioDeviceSnapshot: sessionRef.current?.audioPreferences() || null,
      supportedVoices,
      isStarting: phase === "preparing" || phase === "connecting",
      isEngaged: call !== null || owned || ENGAGED_PHASES.includes(phase),
      externalCall: call !== null && !owned,
      analystWorking,
      analystWarning: analyst?.warning ? codexVoiceWarningText(t, analyst.warning) : "",
      refresh,
      readiness,
      start,
      disconnect,
      cancelInvestigation,
      toggleMicrophone,
      resumeAudio,
      startNewAnalyst,
      updatePreferences,
      updateAnalystSnapshot,
      clearError,
    }),
    [
      analyst,
      analystWorking,
      conversation,
      notificationPaused,
      call,
      checking,
      cancelInvestigation,
      clearError,
      disconnect,
      error,
      refreshError,
      microphoneMuted,
      owned,
      phase,
      preferences,
      playbackBlocked,
      outputStatus,
      refresh,
      readiness,
      resumeAudio,
      start,
      startNewAnalyst,
      supportedVoices,
      toggleMicrophone,
      updatePreferences,
      updateAnalystSnapshot,
      t,
    ],
  );
}

export type CodexVoiceSessionController = ReturnType<typeof useCodexVoiceSessionController>;
