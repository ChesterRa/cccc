import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";

export type CodexVoiceDockProps = {
  controller: CodexVoiceSessionController;
  /** Chet has requests that live status may still change. */
  chetWorking?: boolean;
  collapsed?: boolean;
  variant?: "sidebar" | "mobile";
  onOpen: () => void;
  onStart: () => void;
};
