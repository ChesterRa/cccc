export const GROUP_STREAMS_HIDDEN_DISCONNECT_GRACE_MS = 90_000;

export function shouldStartGroupStreams(documentHidden: boolean): boolean {
  return !documentHidden;
}

export function getGroupStreamsHiddenDisconnectDelayMs(documentHidden: boolean): number | null {
  return documentHidden ? GROUP_STREAMS_HIDDEN_DISCONNECT_GRACE_MS : null;
}
