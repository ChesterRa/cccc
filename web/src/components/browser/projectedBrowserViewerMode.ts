export type ProjectedBrowserViewerMode = "page" | "browser";

export function projectedBrowserSocketViewerMode(
  viewerMode: ProjectedBrowserViewerMode,
  vncFailed: boolean,
): "auto" | "screencast" {
  return viewerMode === "page" || vncFailed ? "screencast" : "auto";
}

export function projectedBrowserEffectiveViewerMode(
  viewerMode: ProjectedBrowserViewerMode,
  browserViewAvailable: boolean,
  vncFailed: boolean,
): ProjectedBrowserViewerMode {
  return viewerMode === "browser" && browserViewAvailable && !vncFailed ? "browser" : "page";
}

export function shouldReuseProjectedBrowserSession(
  reuseActiveSession: boolean,
  activeLifecycleKey: string,
  nextLifecycleKey: string,
): boolean {
  return reuseActiveSession || (!!nextLifecycleKey && activeLifecycleKey === nextLifecycleKey);
}
