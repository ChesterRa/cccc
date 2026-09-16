// Hidden tabs release their logical subscriptions promptly. Returning to the
// tab restores durable events and headless state through the shared transport.
export const GROUP_STREAMS_HIDDEN_DISCONNECT_GRACE_MS = 0;

export function shouldStartGroupStreams(documentHidden: boolean): boolean {
  return !documentHidden;
}

export function getGroupStreamsHiddenDisconnectDelayMs(documentHidden: boolean): number | null {
  return documentHidden ? GROUP_STREAMS_HIDDEN_DISCONNECT_GRACE_MS : null;
}
