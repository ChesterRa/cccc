import type { CodexVoiceAnalystInfo, CodexVoiceCallInfo } from "../../services/api";

export type CodexVoicePhase =
  | "idle"
  | "preparing"
  | "connecting"
  | "listening"
  | "responding"
  | "speaking"
  | "analysing"
  | "stopping"
  | "failed";

export type CodexVoiceSessionCallbacks = {
  onPhase(phase: CodexVoicePhase): void;
  onCall(call: CodexVoiceCallInfo | null): void;
  onAnalyst(analyst: CodexVoiceAnalystInfo): void;
  onUserTranscript(text: string): void;
  onAssistantTranscript(text: string): void;
  onAnalystProgress(text: string): void;
  onAnalystResult(text: string): void;
  onPlaybackBlocked(blocked: boolean): void;
  onError(code: string, providerCode?: string): void;
};
