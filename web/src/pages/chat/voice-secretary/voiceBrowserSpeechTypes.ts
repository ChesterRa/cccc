export type BrowserSpeechRecognitionAlternative = { transcript: string };
export type BrowserSpeechRecognitionResult = {
  isFinal: boolean;
  length: number;
  [index: number]: BrowserSpeechRecognitionAlternative;
};
export type BrowserSpeechRecognitionResultList = {
  length: number;
  [index: number]: BrowserSpeechRecognitionResult;
};
export type BrowserSpeechRecognitionEvent = {
  resultIndex: number;
  results: BrowserSpeechRecognitionResultList;
};
export type BrowserSpeechRecognitionErrorEvent = { error?: string; message?: string };
export type BrowserSpeechRecognition = {
  continuous: boolean;
  interimResults: boolean;
  lang: string;
  maxAlternatives?: number;
  onresult: ((event: BrowserSpeechRecognitionEvent) => void) | null;
  onerror: ((event: BrowserSpeechRecognitionErrorEvent) => void) | null;
  onend: (() => void) | null;
  onspeechstart?: (() => void) | null;
  onspeechend?: (() => void) | null;
  start: () => void;
  stop: () => void;
  abort: () => void;
};
export type BrowserSpeechRecognitionConstructor = new () => BrowserSpeechRecognition;

export type VoiceRecordingStopReason = {
  code: string;
  detail?: string;
  backend?: string;
  groupId?: string;
  runId?: number;
  at: number;
};

export type BrowserMicrophoneSupportIssue = "" | "secure_context" | "get_user_media";
export type BrowserAudioSupportIssue = BrowserMicrophoneSupportIssue;
export type BrowserSpeechSupportIssue = "" | "unsupported";
