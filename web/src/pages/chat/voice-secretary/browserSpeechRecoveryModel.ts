const PASSIVE_RECOVERABLE_BROWSER_SPEECH_ERRORS = new Set(["network"]);

export function shouldScheduleBrowserSpeechErrorRestart(errorCode: string): boolean {
  return !PASSIVE_RECOVERABLE_BROWSER_SPEECH_ERRORS.has(String(errorCode || "").trim());
}
