export type VoiceCaptureStopAction = {
  releaseLocalMicrophoneNow: boolean;
  waitForRemoteFinalization: boolean;
};

export function voiceCaptureStopAction(): VoiceCaptureStopAction {
  return {
    releaseLocalMicrophoneNow: true,
    waitForRemoteFinalization: true,
  };
}
