import { useCallback, useRef, useState } from "react";
import type { VoiceAudioPreferences } from "../../../stores/useVoiceAudioStore";
import { createVoiceRecordingSessionId } from "./voiceCaptureLock";
import {
  createVoiceRecordingSessionScope,
  type VoiceRecordingSessionScope,
} from "./voiceRecordingSessionScope";

export type VoiceRecordingPhase = "idle" | "starting" | "recording" | "finishing";

/** Mutable data belongs to one capture run, never to the next recording. */
export type VoiceRecordingSession = {
  readonly id: number;
  readonly scope: VoiceRecordingSessionScope;
  readonly audioPreferences: VoiceAudioPreferences;
  config: Record<string, unknown>;
  phase: Exclude<VoiceRecordingPhase, "idle">;
  browserFinalText: string;
  browserInterimText: string;
  browserPendingDispatches: Set<{ text: string }>;
  service: {
    finalText: string;
    latestPartial: string;
    committed: string;
    documentHandled: string;
    documentCheckpointTail: Promise<boolean> | null;
    finalAsrText: string;
    seenSegmentIds: Set<string>;
    finalError: { code: string; message: string } | null;
    finalIncomplete: boolean;
    interrupted: boolean;
    recovered: boolean;
    stopRequested: boolean;
    socketStarted: boolean;
    awaitingPcmFlush: boolean;
    connectStopTimer: number | null;
    awaitingFinal: boolean;
  };
};

type BeginRecording = {
  scope: Omit<VoiceRecordingSessionScope, "runId" | "sessionId">;
  config: Record<string, unknown>;
  audioPreferences: VoiceAudioPreferences;
};

/**
 * One synchronous owner for a run's identity and lifecycle. The phase state is
 * its renderable projection; asynchronous handlers check the same current run.
 */
export function useVoiceRecordingSession() {
  const current = useRef<VoiceRecordingSession | null>(null);
  const nextRunId = useRef(0);
  const [phase, setPhase] = useState<VoiceRecordingPhase>("idle");
  const [waitingForFinal, setWaitingForFinal] = useState(false);
  const getCurrent = useCallback(() => current.current, []);

  const begin = useCallback((input: BeginRecording): VoiceRecordingSession | null => {
    if (current.current) return null;
    const id = ++nextRunId.current;
    const run: VoiceRecordingSession = {
      id,
      scope: createVoiceRecordingSessionScope({
        ...input.scope,
        runId: id,
        sessionId: createVoiceRecordingSessionId(),
      }),
      audioPreferences: input.audioPreferences,
      config: input.config,
      phase: "starting",
      browserFinalText: "",
      browserInterimText: "",
      browserPendingDispatches: new Set(),
      service: {
        finalText: "",
        latestPartial: "",
        committed: "",
        documentHandled: "",
        documentCheckpointTail: null,
        finalAsrText: "",
        seenSegmentIds: new Set(),
        finalError: null,
        finalIncomplete: false,
        interrupted: false,
        recovered: false,
        stopRequested: false,
        socketStarted: false,
        awaitingPcmFlush: false,
        connectStopTimer: null,
        awaitingFinal: false,
      },
    };
    current.current = run;
    setPhase("starting");
    return run;
  }, []);

  const isActive = useCallback((runId: number) => current.current?.id === runId, []);

  const markRecording = useCallback((runId: number) => {
    const run = current.current;
    if (!run || run.id !== runId || run.phase === "finishing") return false;
    if (run.phase !== "recording") {
      run.phase = "recording";
      setPhase("recording");
    }
    return true;
  }, []);

  const markFinishing = useCallback((runId: number) => {
    const run = current.current;
    if (!run || run.id !== runId) return false;
    if (run.phase !== "finishing") {
      run.phase = "finishing";
      setPhase("finishing");
    }
    return true;
  }, []);

  const markAwaitingFinal = useCallback((runId: number, waiting: boolean) => {
    const run = current.current;
    if (!run || run.id !== runId) return false;
    run.service.awaitingFinal = waiting;
    setWaitingForFinal(waiting);
    return true;
  }, []);

  const end = useCallback((runId: number): VoiceRecordingSession | null => {
    const run = current.current;
    if (!run || run.id !== runId) return null;
    current.current = null;
    setPhase("idle");
    setWaitingForFinal(false);
    return run;
  }, []);

  return {
    current,
    getCurrent,
    phase,
    waitingForFinal,
    begin,
    isActive,
    markRecording,
    markFinishing,
    markAwaitingFinal,
    end,
  };
}
