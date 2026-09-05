import { useCallback, useMemo, useRef, useState } from "react";
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
import { codexVoiceErrorText, codexVoiceWarningText } from "./codexVoiceControllerText";
import { useCodexVoicePolling } from "./useCodexVoicePolling";
import { useCodexVoicePreferencesState } from "./useCodexVoicePreferencesState";
import { useCodexVoiceWindowLifecycle } from "./useCodexVoiceWindowLifecycle";

const ENGAGED_PHASES: CodexVoicePhase[] = ["preparing", "connecting", "stopping"];

export function useCodexVoiceSessionController(enabled = true) {
  const { t } = useTranslation("modals");
  const audioRef = useRef<HTMLAudioElement>(null);
  const sessionRef = useRef<CodexVoiceBrowserSession | null>(null);
  const mountedRef = useRef(true);
  const refreshGenerationRef = useRef(0);
  const [phase, setPhase] = useState<CodexVoicePhase>("idle");
  const [call, setCall] = useState<CodexVoiceCallInfo | null>(null);
  const [analyst, setAnalyst] = useState<CodexVoiceAnalystInfo | null>(null);
  const [owned, setOwned] = useState(false);
  const [checking, setChecking] = useState(enabled);
  const [userTranscript, setUserTranscript] = useState("");
  const [assistantTranscript, setAssistantTranscript] = useState("");
  const [microphoneMuted, setMicrophoneMuted] = useState(false);
  const [playbackBlocked, setPlaybackBlocked] = useState(false);
  const [error, setError] = useState("");
  const { preferences, supportedVoices, updatePreferences, acceptSupportedVoices } =
    useCodexVoicePreferencesState();
  const [readiness, setReadiness] = useState<CodexVoiceReadiness | null>(null);

  const refresh = useCallback(
    async (showChecking = true) => {
      if (!enabled) {
        if (showChecking) setChecking(false);
        return;
      }
      if (sessionRef.current) return;
      const generation = ++refreshGenerationRef.current;
      if (showChecking) setChecking(true);
      const response = await fetchActiveCodexVoiceCall();
      const refreshIsStale =
        !mountedRef.current || generation !== refreshGenerationRef.current || sessionRef.current;
      if (refreshIsStale) return;
      if (response.ok) {
        setCall(response.result.call);
        setAnalyst(response.result.analyst);
        setPhase((current) => (current === "failed" ? current : "idle"));
        setReadiness(response.result.readiness);
        acceptSupportedVoices(response.result.voices);
      } else {
        setError(codexVoiceErrorText(t, response.error.code));
      }
      if (showChecking) setChecking(false);
    },
    [acceptSupportedVoices, enabled, t],
  );

  useCodexVoiceWindowLifecycle({ refresh, mountedRef, refreshGenerationRef, sessionRef });
  useCodexVoicePolling({ enabled, analyst, sessionRef, refresh });

  const start = useCallback(async () => {
    const audio = audioRef.current;
    if (!audio || sessionRef.current) return;
    if (call) {
      setError(t("codexVoiceExistingCallStartBlocked"));
      return;
    }
    if (readiness && !readiness.analyst_runtime_available) {
      setPhase("failed");
      setError(t("codexVoiceAnalystRuntimeMissing", { runtime: readiness.analyst_runtime }));
      return;
    }
    if (readiness && !readiness.realtime_credentials_available) {
      setPhase("failed");
      setError(t("codexVoiceCodexLoginRequired"));
      return;
    }

    refreshGenerationRef.current += 1;
    setChecking(false);
    setError("");
    setUserTranscript("");
    setAssistantTranscript("");
    setMicrophoneMuted(false);
    setPlaybackBlocked(false);

    const session = new CodexVoiceBrowserSession({
      audio,
      preferences,
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
        onUserTranscript: (text) => {
          if (mountedRef.current) setUserTranscript(text);
        },
        onAssistantTranscript: (text) => {
          if (mountedRef.current) setAssistantTranscript(text);
        },
        onAnalystProgress: () => undefined,
        onAnalystResult: () => undefined,
        onPlaybackBlocked: (blocked) => {
          if (mountedRef.current) setPlaybackBlocked(blocked);
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
    if (!analyst || analyst.phase !== "working") return false;
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
  }, [analyst, refresh, t]);

  const toggleMicrophone = useCallback(() => {
    const next = !microphoneMuted;
    if (!sessionRef.current?.setMicrophoneMuted(next)) return;
    setMicrophoneMuted(next);
  }, [microphoneMuted]);

  const resumeAudio = useCallback(async () => {
    if (await sessionRef.current?.resumeAudio()) setPlaybackBlocked(false);
  }, []);

  const startNewAnalyst = useCallback(async () => {
    if (!analyst || call || analyst.phase === "working") return false;
    setError("");
    const response = await resetCodexVoiceAnalyst(analyst.generation);
    if (!mountedRef.current) return false;
    if (!response.ok) {
      setError(codexVoiceErrorText(t, response.error.code));
      return false;
    }
    setAnalyst(response.result.analyst);
    return true;
  }, [analyst, call, t]);

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
      userTranscript,
      assistantTranscript,
      microphoneMuted,
      playbackBlocked,
      error,
      preferences,
      supportedVoices,
      isStarting: phase === "preparing" || phase === "connecting",
      isEngaged: call !== null || owned || ENGAGED_PHASES.includes(phase),
      externalCall: call !== null && !owned,
      analystWorking: analyst?.phase === "working",
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
      clearError,
    }),
    [
      analyst,
      assistantTranscript,
      call,
      checking,
      cancelInvestigation,
      clearError,
      disconnect,
      error,
      microphoneMuted,
      owned,
      phase,
      preferences,
      playbackBlocked,
      refresh,
      readiness,
      resumeAudio,
      start,
      startNewAnalyst,
      supportedVoices,
      toggleMicrophone,
      userTranscript,
      updatePreferences,
      t,
    ],
  );
}

export type CodexVoiceSessionController = ReturnType<typeof useCodexVoiceSessionController>;
