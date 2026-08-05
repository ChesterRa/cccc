import type { PresentationBrowserSurfaceState } from "../../types";

export type BrowserDisconnectResolution = {
  state: PresentationBrowserSurfaceState;
  shouldReconnect: boolean;
};

export function resolveBrowserObserverDisconnect(args: {
  surface: PresentationBrowserSurfaceState;
  reconnectAttempts: number;
  maxReconnectAttempts: number;
  reconnectingMessage: string;
  closedMessage: string;
}): BrowserDisconnectResolution {
  const state = String(args.surface.state || "")
    .trim()
    .toLowerCase();
  const live = args.surface.active && (state === "ready" || state === "starting");
  if (live) {
    const shouldReconnect = args.reconnectAttempts < args.maxReconnectAttempts;
    return {
      state: {
        ...args.surface,
        active: true,
        state: shouldReconnect ? state : "disconnected",
        message: args.reconnectingMessage,
      },
      shouldReconnect,
    };
  }
  return {
    state: {
      ...args.surface,
      active: false,
      state: state === "failed" ? "failed" : "closed",
      message: state === "failed" ? args.surface.message : args.closedMessage,
    },
    shouldReconnect: false,
  };
}
