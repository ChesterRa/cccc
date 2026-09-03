import type {
  BrowserAudioSupportIssue,
  BrowserMicrophoneSupportIssue,
  BrowserSpeechRecognition,
  BrowserSpeechRecognitionConstructor,
  BrowserSpeechSupportIssue,
} from "./voiceBrowserSpeechTypes";

export function getBrowserSpeechRecognitionConstructor(): BrowserSpeechRecognitionConstructor | null {
  if (typeof window === "undefined") return null;
  const speechWindow = window as typeof window & {
    SpeechRecognition?: BrowserSpeechRecognitionConstructor;
    webkitSpeechRecognition?: BrowserSpeechRecognitionConstructor;
  };
  return speechWindow.SpeechRecognition || speechWindow.webkitSpeechRecognition || null;
}

export function getBrowserSpeechSupportIssue(): BrowserSpeechSupportIssue {
  return getBrowserSpeechRecognitionConstructor() ? "" : "unsupported";
}

export function getBrowserMicrophoneSupportIssue(): BrowserMicrophoneSupportIssue {
  if (typeof window !== "undefined" && window.isSecureContext === false) return "secure_context";
  if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia)
    return "get_user_media";
  return "";
}

export function getBrowserAudioSupportIssue(): BrowserAudioSupportIssue {
  return getBrowserMicrophoneSupportIssue();
}

export function mediaRecorderSupported(): boolean {
  return !getBrowserAudioSupportIssue();
}

export function stopMediaStream(stream: MediaStream | null): void {
  if (!stream) return;
  try {
    stream.getTracks().forEach((track) => track.stop());
  } catch {
    // Ignore browser cleanup failure.
  }
}

export function mediaStreamHasLiveAudio(stream: MediaStream | null): boolean {
  if (!stream) return false;
  try {
    return stream.getAudioTracks().some((track) => track.readyState === "live");
  } catch {
    return false;
  }
}

export function abortBrowserSpeechRecognition(recognition: BrowserSpeechRecognition | null): void {
  if (!recognition) return;
  recognition.onend = null;
  recognition.onerror = null;
  recognition.onresult = null;
  recognition.onspeechstart = null;
  recognition.onspeechend = null;
  try {
    recognition.abort();
  } catch {
    // Ignore browser cleanup failure.
  }
}
