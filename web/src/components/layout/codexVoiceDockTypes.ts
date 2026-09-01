import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";

export type CodexVoiceDockProps = {
  controller: CodexVoiceSessionController;
  selectedGroupId: string;
  collapsed?: boolean;
  variant?: "sidebar" | "mobile";
  onOpen: () => void;
  onStart: () => void;
};
