import type { CodexVoiceSessionController } from "../../features/codexVoice/useCodexVoiceSessionController";

export type CodexVoiceDockProps = {
  controller: CodexVoiceSessionController;
  /** Chet has requests that live status may still change. */
  chetWorking?: boolean;
  /** Chet's history holds something new since its conversation was last open here. */
  chetUnread?: boolean;
  collapsed?: boolean;
  variant?: "sidebar" | "mobile";
  onOpen: () => void;
  onStart: () => void;
};
