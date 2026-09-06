import type { CodexVoiceAnalystInfo, CodexVoiceCallInfo } from "../../services/api";
import type { CodexVoiceOutputStatus } from "./codexVoiceProviderChannel";
import type { VoiceConversationTurn } from "./codexVoiceProtocol";

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
  onOutputStatus?(status: CodexVoiceOutputStatus): void;
  onConversation?(turns: VoiceConversationTurn[]): void;
  onNotificationPaused?(paused: boolean): void;
  onError(code: string, providerCode?: string): void;
};
