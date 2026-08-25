// A hidden tab otherwise retains three HTTP/1.1 SSE connections. Two freshly
// opened CCCC tabs can exhaust Chromium's per-origin connection pool and delay
// the new document request by seconds. Reconnect catch-up already covers the
// visibility gap, so release hidden-tab streams on the next task.
export const GROUP_STREAMS_HIDDEN_DISCONNECT_GRACE_MS = 0;

export function shouldStartGroupStreams(documentHidden: boolean): boolean {
  return !documentHidden;
}

export function getGroupStreamsHiddenDisconnectDelayMs(documentHidden: boolean): number | null {
  return documentHidden ? GROUP_STREAMS_HIDDEN_DISCONNECT_GRACE_MS : null;
}
