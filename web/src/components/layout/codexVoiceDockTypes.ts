import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";

export type CodexVoiceDockProps = {
  controller: CodexVoiceSessionController;
  collapsed?: boolean;
  variant?: "sidebar" | "mobile";
  onOpen: () => void;
  onStart: () => void;
};
