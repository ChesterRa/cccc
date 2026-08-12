export type VoiceCaptureStopAction = {
  releaseLocalMicrophoneNow: boolean;
  waitForRemoteFinalization: boolean;
};

export function voiceCaptureStopAction(): VoiceCaptureStopAction {
  return { releaseLocalMicrophoneNow: true, waitForRemoteFinalization: true };
}

export function shouldCloseVoiceCaptureSocket(readyState: number): boolean {
  return readyState === 0 || readyState === 1;
}
