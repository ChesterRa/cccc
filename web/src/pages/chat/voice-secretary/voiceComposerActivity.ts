export type VoiceComposerActivity =
  | "recording"
  | "finishing"
  | "promptWaiting"
  | "promptReady"
  | "ask"
  | "transcript"
  | "";

/** The composer shows one activity line; errors are rendered separately and never hidden by it. */
export function resolveVoiceComposerActivity(input: {
  recording: boolean;
  finishing?: boolean;
  promptWaiting: boolean;
  promptReady: boolean;
  askSummary: string;
  transcriptSummary: string;
}): VoiceComposerActivity {
  if (input.recording) return "recording";
  if (input.finishing) return "finishing";
  if (input.promptWaiting) return "promptWaiting";
  if (input.promptReady) return "promptReady";
  if (input.askSummary) return "ask";
  if (input.transcriptSummary) return "transcript";
  return "";
}
