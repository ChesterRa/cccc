export function shouldCloseVoiceCaptureSocket(readyState: number): boolean {
  return readyState === 0 || readyState === 1;
}
