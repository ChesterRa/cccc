import {
  useEffect,
  useRef,
  useState,
  type FormEvent,
  type KeyboardEvent,
  type PointerEvent,
  type WheelEvent,
} from "react";
import { createPortal } from "react-dom";
import { useTranslation } from "react-i18next";
import type { ApiResponse } from "../../services/api";
import type { PresentationBrowserSurfaceState } from "../../types";
import { classNames } from "../../utils/classNames";
import { CollapseIcon, ExpandIcon } from "../Icons";
import { mapContainedImagePoint } from "./projectedBrowserCoordinates";
import { resolveBrowserObserverDisconnect } from "./projectedBrowserConnection";
import {
  projectedBrowserEffectiveViewerMode,
  projectedBrowserSocketViewerMode,
  shouldReuseProjectedBrowserSession,
  type ProjectedBrowserViewerMode,
} from "./projectedBrowserViewerMode";

type RfbInstance = {
  viewOnly: boolean;
  scaleViewport: boolean;
  resizeSession: boolean;
  clipViewport: boolean;
  background: string;
  disconnect: () => void;
  addEventListener: (type: string, listener: (event: Event) => void) => void;
};

type RfbConstructor = new (
  target: HTMLElement,
  url: string,
  options?: Record<string, unknown>,
) => RfbInstance;

export type ProjectedBrowserFrame = {
  seq: number;
  dataUrl: string;
  width: number;
  height: number;
  capturedAt: string;
  url: string;
};

type ProjectedBrowserSurfacePanelProps = {
  isDark: boolean;
  refreshNonce: number;
  reuseActiveSession?: boolean;
  sessionIdentity?: string;
  defaultViewerMode?: ProjectedBrowserViewerMode;
  chromeMode?: "standalone" | "embedded";
  viewportClassName?: string;
  onFrameUpdate?: (frame: ProjectedBrowserFrame | null) => void;
  loadSession: () => Promise<ApiResponse<{ browser_surface: PresentationBrowserSurfaceState }>>;
  startSession?: (size: {
    width: number;
    height: number;
  }) => Promise<ApiResponse<{ browser_surface: PresentationBrowserSurfaceState }>>;
  webSocketUrl: string;
  fallbackUrl?: string;
  labels?: Partial<{
    starting: string;
    waiting: string;
    ready: string;
    failed: string;
    closed: string;
    reconnecting: string;
    connectionFailed: string;
    reconnect: string;
    refreshPending: string;
    refreshing: string;
    refreshed: string;
    refreshFailed: string;
    back: string;
    frameAlt: string;
    fullScreen: string;
    exitFullScreen: string;
    viewerLabel: string;
    viewerPage: string;
    viewerBrowser: string;
    viewerPageTooltip: string;
    viewerBrowserTooltip: string;
    viewerBrowserUnavailable: string;
    viewerFallbackReason: string;
    viewerReasonX11vncNotFound: string;
    viewerReasonWaylandEnv: string;
    viewerReasonMissingDisplay: string;
    viewerReasonDisplayNotOwned: string;
    viewerReasonDisabled: string;
    viewerReasonUnsupportedPlatform: string;
    viewerReasonStartupTimeout: string;
    viewerIsolationXvfb: string;
    viewerTooltipIsolationXvfb: string;
  }>;
};

type BrowserEventPayload =
  | ({ t: "state" } & PresentationBrowserSurfaceState)
  | {
      t: "frame";
      seq?: number;
      data_base64?: string | null;
      width?: number;
      height?: number;
      captured_at?: string | null;
      url?: string | null;
      mime?: string | null;
    }
  | { t: "command_result"; id?: string | null; ok?: boolean; message?: string | null }
  | { t: "error"; code?: string | null; message?: string | null };

const SPECIAL_KEY_MAP: Record<string, string> = {
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Escape: "Escape",
  Delete: "Delete",
  ArrowUp: "ArrowUp",
  ArrowDown: "ArrowDown",
  ArrowLeft: "ArrowLeft",
  ArrowRight: "ArrowRight",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
};

function normalizeState(
  raw: PresentationBrowserSurfaceState | null | undefined,
): PresentationBrowserSurfaceState {
  return {
    active: !!raw?.active,
    state: String(raw?.state || "idle").trim() || "idle",
    message: String(raw?.message || "").trim(),
    error: raw?.error
      ? {
          code: String(raw.error.code || "").trim(),
          message: String(raw.error.message || "").trim(),
        }
      : null,
    strategy: String(raw?.strategy || "").trim(),
    url: String(raw?.url || "").trim(),
    width: Number.isFinite(Number(raw?.width)) ? Number(raw?.width) : 0,
    height: Number.isFinite(Number(raw?.height)) ? Number(raw?.height) : 0,
    started_at: String(raw?.started_at || "").trim(),
    updated_at: String(raw?.updated_at || "").trim(),
    last_frame_seq: Number.isFinite(Number(raw?.last_frame_seq)) ? Number(raw?.last_frame_seq) : 0,
    last_frame_at: String(raw?.last_frame_at || "").trim(),
    controller_attached: !!raw?.controller_attached,
    metadata: raw?.metadata || null,
    viewer: raw?.viewer || null,
  };
}

function urlWithViewerParam(rawUrl: string, key: string, value: string): string {
  try {
    const url = new URL(rawUrl, window.location.href);
    url.searchParams.set(key, value);
    return url.toString();
  } catch {
    const separator = rawUrl.includes("?") ? "&" : "?";
    return `${rawUrl}${separator}${encodeURIComponent(key)}=${encodeURIComponent(value)}`;
  }
}

function formatVncFallbackReason(raw: string, texts: { [key: string]: string }): string {
  const value = String(raw || "").trim();
  if (!value) return "";
  const lower = value.toLowerCase();
  if (lower.includes("x11vnc_not_found")) return texts.viewerReasonX11vncNotFound;
  if (lower.includes("wayland")) return texts.viewerReasonWaylandEnv;
  if (lower.includes("missing_display")) return texts.viewerReasonMissingDisplay;
  if (lower.includes("display_not_cccc_owned")) return texts.viewerReasonDisplayNotOwned;
  if (lower.includes("disabled")) return texts.viewerReasonDisabled;
  if (lower.includes("unsupported_platform")) return texts.viewerReasonUnsupportedPlatform;
  if (lower.includes("endpoint did not become ready") || lower.includes("startup_timeout")) {
    return texts.viewerReasonStartupTimeout;
  }
  return value.length > 120 ? `${value.slice(0, 117)}...` : value;
}

function buttonFromMouseEvent(button: number): "left" | "middle" | "right" {
  if (button === 1) return "middle";
  if (button === 2) return "right";
  return "left";
}

function ProjectedBrowserExpandIcon({ expanded }: { expanded: boolean }) {
  const Icon = expanded ? CollapseIcon : ExpandIcon;
  return <Icon aria-hidden="true" className="h-4 w-4" strokeWidth={1.6} />;
}

export function ProjectedBrowserSurfacePanel({
  isDark,
  refreshNonce,
  reuseActiveSession = true,
  sessionIdentity = "",
  defaultViewerMode = "page",
  chromeMode = "standalone",
  viewportClassName,
  onFrameUpdate,
  loadSession,
  startSession,
  webSocketUrl,
  fallbackUrl,
  labels,
}: ProjectedBrowserSurfacePanelProps) {
  const { t } = useTranslation("chat");
  const containerRef = useRef<HTMLDivElement | null>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const inputRelayRef = useRef<HTMLTextAreaElement | null>(null);
  const vncTargetRef = useRef<HTMLDivElement | null>(null);
  const rfbRef = useRef<RfbInstance | null>(null);
  const frameRef = useRef<ProjectedBrowserFrame | null>(null);
  const renderedFrameRef = useRef<ProjectedBrowserFrame | null>(null);
  const frameRenderTokenRef = useRef(0);
  const lastFrameCallbackAtRef = useRef(0);
  const wsRef = useRef<WebSocket | null>(null);
  const loadSessionRef = useRef(loadSession);
  const startSessionRef = useRef(startSession);
  const resizeTimerRef = useRef<number | null>(null);
  const reconnectTimerRef = useRef<number | null>(null);
  const reconnectAttemptsRef = useRef(0);
  const runIdRef = useRef(0);
  const activeLifecycleKeyRef = useRef("");
  const lastRefreshNonceRef = useRef(refreshNonce);
  const pendingRefreshIdRef = useRef("");
  const refreshFeedbackTimerRef = useRef<number | null>(null);
  const relayComposingRef = useRef(false);
  const touchGestureRef = useRef<{
    pointerId: number;
    start: { x: number; y: number };
    last: { x: number; y: number };
    moved: boolean;
  } | null>(null);

  const texts = {
    starting:
      labels?.starting ||
      t("presentationBrowserStarting", { defaultValue: "Preparing interactive view..." }),
    waiting:
      labels?.waiting ||
      t("presentationBrowserWaiting", { defaultValue: "Waiting for interactive view..." }),
    ready:
      labels?.ready || t("presentationBrowserReady", { defaultValue: "Interactive view ready" }),
    failed:
      labels?.failed || t("presentationBrowserFailed", { defaultValue: "Interactive view failed" }),
    closed:
      labels?.closed ||
      t("presentationBrowserClosed", { defaultValue: "Interactive view closed." }),
    reconnecting:
      labels?.reconnecting ||
      t("presentationBrowserReconnecting", { defaultValue: "Reconnecting interactive view..." }),
    connectionFailed:
      labels?.connectionFailed ||
      t("presentationBrowserConnectionFailed", {
        defaultValue:
          "The interactive connection was rejected. Check the Web session and reverse-proxy Origin headers.",
      }),
    reconnect:
      labels?.reconnect || t("presentationBrowserReconnect", { defaultValue: "Reconnect" }),
    refreshPending:
      labels?.refreshPending ||
      t("presentationBrowserRefreshPending", { defaultValue: "Refresh queued" }),
    refreshing:
      labels?.refreshing || t("presentationBrowserRefreshing", { defaultValue: "Refreshing..." }),
    refreshed:
      labels?.refreshed || t("presentationBrowserRefreshed", { defaultValue: "Refreshed" }),
    refreshFailed:
      labels?.refreshFailed ||
      t("presentationBrowserRefreshFailed", { defaultValue: "Refresh failed" }),
    back: labels?.back || t("presentationBrowserBack", { defaultValue: "Back" }),
    frameAlt:
      labels?.frameAlt ||
      t("presentationBrowserFrameAlt", { defaultValue: "Interactive view frame" }),
    fullScreen:
      labels?.fullScreen || t("presentationFullScreenAction", { defaultValue: "Full screen" }),
    exitFullScreen:
      labels?.exitFullScreen ||
      t("presentationExitFullScreenAction", { defaultValue: "Exit full screen" }),
    viewerLabel:
      labels?.viewerLabel || t("presentationBrowserViewerLabel", { defaultValue: "Viewer" }),
    viewerPage: labels?.viewerPage || t("presentationBrowserViewerPage", { defaultValue: "Page" }),
    viewerBrowser:
      labels?.viewerBrowser || t("presentationBrowserViewerBrowser", { defaultValue: "Browser" }),
    viewerPageTooltip:
      labels?.viewerPageTooltip ||
      t("presentationBrowserViewerPageTooltip", {
        defaultValue:
          "Show the website content directly. The site still runs in the same daemon-controlled browser.",
      }),
    viewerBrowserTooltip:
      labels?.viewerBrowserTooltip ||
      t("presentationBrowserViewerBrowserTooltip", {
        defaultValue:
          "Show the complete browser window for browser UI, sign-in prompts, and native interaction.",
      }),
    viewerBrowserUnavailable:
      labels?.viewerBrowserUnavailable ||
      t("presentationBrowserViewerBrowserUnavailable", {
        defaultValue: "Complete browser view is unavailable; page view remains active.",
      }),
    viewerFallbackReason:
      labels?.viewerFallbackReason ||
      t("presentationBrowserViewerFallbackReason", { defaultValue: "Fallback reason" }),
    viewerReasonX11vncNotFound:
      labels?.viewerReasonX11vncNotFound ||
      t("presentationBrowserViewerReasonX11vncNotFound", { defaultValue: "x11vnc not installed" }),
    viewerReasonWaylandEnv:
      labels?.viewerReasonWaylandEnv ||
      t("presentationBrowserViewerReasonWaylandEnv", { defaultValue: "Wayland env inherited" }),
    viewerReasonMissingDisplay:
      labels?.viewerReasonMissingDisplay ||
      t("presentationBrowserViewerReasonMissingDisplay", { defaultValue: "No X display" }),
    viewerReasonDisplayNotOwned:
      labels?.viewerReasonDisplayNotOwned ||
      t("presentationBrowserViewerReasonDisplayNotOwned", {
        defaultValue: "Display is not CCCC-owned",
      }),
    viewerReasonDisabled:
      labels?.viewerReasonDisabled ||
      t("presentationBrowserViewerReasonDisabled", { defaultValue: "VNC disabled" }),
    viewerReasonUnsupportedPlatform:
      labels?.viewerReasonUnsupportedPlatform ||
      t("presentationBrowserViewerReasonUnsupportedPlatform", {
        defaultValue: "Unsupported platform",
      }),
    viewerReasonStartupTimeout:
      labels?.viewerReasonStartupTimeout ||
      t("presentationBrowserViewerReasonStartupTimeout", { defaultValue: "VNC startup timeout" }),
    viewerIsolationXvfb:
      labels?.viewerIsolationXvfb ||
      t("presentationBrowserViewerIsolationXvfb", { defaultValue: "Xvfb isolated" }),
    viewerTooltipIsolationXvfb:
      labels?.viewerTooltipIsolationXvfb ||
      t("presentationBrowserViewerTooltipIsolationXvfb", {
        defaultValue:
          "The browser is running on a CCCC-owned virtual display, not the host desktop.",
      }),
  };

  const [runNonce, setRunNonce] = useState(0);
  const [sessionState, setSessionState] = useState<PresentationBrowserSurfaceState>(() =>
    normalizeState({ active: true, state: "starting", message: texts.starting }),
  );
  const [renderedFrame, setRenderedFrame] = useState<ProjectedBrowserFrame | null>(null);
  const [panelError, setPanelError] = useState("");
  const [isExpanded, setIsExpanded] = useState(false);
  const [viewerMode, setViewerMode] = useState<ProjectedBrowserViewerMode>(defaultViewerMode);
  const [vncConnected, setVncConnected] = useState(false);
  const [vncFailed, setVncFailed] = useState(false);
  const [refreshFeedback, setRefreshFeedback] = useState<
    "" | "pending" | "refreshing" | "refreshed" | "failed"
  >("");

  useEffect(() => {
    loadSessionRef.current = loadSession;
  }, [loadSession]);

  useEffect(() => {
    startSessionRef.current = startSession;
  }, [startSession]);

  useEffect(() => {
    setVncConnected(false);
    setVncFailed(false);
    if (rfbRef.current) {
      rfbRef.current.disconnect();
      rfbRef.current = null;
    }
    vncTargetRef.current?.replaceChildren();
  }, [webSocketUrl]);

  useEffect(() => {
    return () => {
      onFrameUpdate?.(null);
      if (rfbRef.current) {
        rfbRef.current.disconnect();
        rfbRef.current = null;
      }
      frameRef.current = null;
      renderedFrameRef.current = null;
      frameRenderTokenRef.current += 1;
      const ws = wsRef.current;
      wsRef.current = null;
      if (ws && (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING)) {
        ws.close(1000, "Browser surface cleanup");
      }
      if (resizeTimerRef.current !== null) {
        window.clearTimeout(resizeTimerRef.current);
        resizeTimerRef.current = null;
      }
      if (reconnectTimerRef.current !== null) {
        window.clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
      if (refreshFeedbackTimerRef.current !== null) {
        window.clearTimeout(refreshFeedbackTimerRef.current);
        refreshFeedbackTimerRef.current = null;
      }
    };
  }, [onFrameUpdate]);

  useEffect(() => {
    if (!isExpanded) return;
    const onWindowKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") {
        setIsExpanded(false);
      }
    };
    window.addEventListener("keydown", onWindowKeyDown);
    return () => window.removeEventListener("keydown", onWindowKeyDown);
  }, [isExpanded]);

  const sendCommand = (payload: Record<string, unknown>): boolean => {
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return false;
    ws.send(JSON.stringify(payload));
    return true;
  };

  const scheduleRefreshFeedbackReset = () => {
    if (refreshFeedbackTimerRef.current !== null) {
      window.clearTimeout(refreshFeedbackTimerRef.current);
    }
    refreshFeedbackTimerRef.current = window.setTimeout(() => {
      refreshFeedbackTimerRef.current = null;
      setRefreshFeedback("");
    }, 2000);
  };

  useEffect(() => {
    const lifecycleKey = `${sessionIdentity}\u0000${webSocketUrl}\u0000${runNonce}`;
    const reuseCurrentSession = shouldReuseProjectedBrowserSession(
      reuseActiveSession,
      activeLifecycleKeyRef.current,
      lifecycleKey,
    );
    const runId = runIdRef.current + 1;
    runIdRef.current = runId;
    let disposed = false;

    const cleanupTransport = () => {
      const ws = wsRef.current;
      if (ws) {
        wsRef.current = null;
        if (ws.readyState === WebSocket.OPEN || ws.readyState === WebSocket.CONNECTING) {
          ws.close(1000, "Browser surface cleanup");
        }
      }
      if (resizeTimerRef.current !== null) {
        window.clearTimeout(resizeTimerRef.current);
        resizeTimerRef.current = null;
      }
      if (reconnectTimerRef.current !== null) {
        window.clearTimeout(reconnectTimerRef.current);
        reconnectTimerRef.current = null;
      }
    };

    const attachSocket = () => {
      const socketViewerMode = projectedBrowserSocketViewerMode(viewerMode, vncFailed);
      const ws = new WebSocket(urlWithViewerParam(webSocketUrl, "viewer_mode", socketViewerMode));
      wsRef.current = ws;

      ws.onopen = () => {
        setPanelError("");
        const pendingId = pendingRefreshIdRef.current;
        if (!pendingId) return;
        if (sendCommand({ t: "refresh", id: pendingId })) {
          setRefreshFeedback("refreshing");
        }
      };

      ws.onmessage = (event) => {
        if (disposed || runIdRef.current !== runId) return;
        try {
          const payload = JSON.parse(String(event.data || "")) as BrowserEventPayload;
          if (payload.t === "state") {
            setSessionState(normalizeState(payload));
            if (payload.state === "failed") {
              const message = String(payload.error?.message || payload.message || "").trim();
              if (message) setPanelError(message);
            }
            return;
          }
          if (payload.t === "frame") {
            const rawBase64 = String(payload.data_base64 || "").trim();
            if (!rawBase64) return;
            reconnectAttemptsRef.current = 0;
            const mime = String(payload.mime || "image/jpeg").trim() || "image/jpeg";
            const nextFrame = {
              seq: Number(payload.seq || 0) || 0,
              dataUrl: `data:${mime};base64,${rawBase64}`,
              width: Number(payload.width || 0) || 0,
              height: Number(payload.height || 0) || 0,
              capturedAt: String(payload.captured_at || "").trim(),
              url: String(payload.url || "").trim(),
            };
            frameRef.current = nextFrame;
            const renderToken = frameRenderTokenRef.current + 1;
            frameRenderTokenRef.current = renderToken;
            const decoded = new Image();
            decoded.onload = () => {
              if (
                disposed ||
                runIdRef.current !== runId ||
                frameRenderTokenRef.current !== renderToken
              ) {
                return;
              }
              renderedFrameRef.current = nextFrame;
              if (imageRef.current) {
                // Keep the previous frame visible until the replacement JPEG is decoded.
                imageRef.current.src = nextFrame.dataUrl;
              } else {
                setRenderedFrame(nextFrame);
              }
            };
            decoded.src = nextFrame.dataUrl;
            if (onFrameUpdate) {
              const now = window.performance?.now?.() ?? Date.now();
              if (!lastFrameCallbackAtRef.current || now - lastFrameCallbackAtRef.current >= 250) {
                lastFrameCallbackAtRef.current = now;
                onFrameUpdate(nextFrame);
              }
            }
            return;
          }
          if (payload.t === "command_result") {
            const commandId = String(payload.id || "").trim();
            if (!commandId || commandId !== pendingRefreshIdRef.current) return;
            pendingRefreshIdRef.current = "";
            if (payload.ok) {
              setRefreshFeedback("refreshed");
              scheduleRefreshFeedbackReset();
            } else {
              setRefreshFeedback("failed");
              setPanelError(String(payload.message || texts.refreshFailed).trim());
            }
            return;
          }
          if (payload.t === "error") {
            const message = String(payload.message || "").trim();
            if (message) setPanelError(message);
          }
        } catch {
          // Ignore malformed websocket payloads.
        }
      };

      ws.onerror = () => {
        if (disposed || runIdRef.current !== runId) return;
        setPanelError(texts.connectionFailed);
      };

      ws.onclose = () => {
        if (disposed || runIdRef.current !== runId) return;
        wsRef.current = null;
        void (async () => {
          const info = await loadSessionRef.current();
          if (disposed || runIdRef.current !== runId) return;
          if (info.ok) {
            const resolution = resolveBrowserObserverDisconnect({
              surface: info.result.browser_surface,
              reconnectAttempts: reconnectAttemptsRef.current,
              maxReconnectAttempts: 3,
              reconnectingMessage: texts.reconnecting,
              closedMessage: texts.closed,
            });
            setSessionState(normalizeState(resolution.state));
            if (!resolution.shouldReconnect) {
              const message = String(
                info.result.browser_surface.error?.message ||
                  info.result.browser_surface.message ||
                  "",
              ).trim();
              if (message && info.result.browser_surface.state === "failed") {
                setPanelError(message);
              }
              return;
            }
            reconnectAttemptsRef.current += 1;
            reconnectTimerRef.current = window.setTimeout(() => {
              reconnectTimerRef.current = null;
              attachSocket();
            }, 800);
            return;
          }
          setSessionState((current) =>
            normalizeState({
              ...current,
              active: current.active,
              state: current.active ? "disconnected" : "closed",
              message: current.active ? texts.reconnecting : texts.closed,
              error: { code: info.error.code, message: info.error.message },
            }),
          );
          setPanelError(`${info.error.code}: ${info.error.message}`);
        })();
      };
    };

    const open = async () => {
      reconnectAttemptsRef.current = 0;
      setPanelError("");
      setVncConnected(false);
      if (rfbRef.current) {
        rfbRef.current.disconnect();
        rfbRef.current = null;
      }
      vncTargetRef.current?.replaceChildren();
      frameRef.current = null;
      renderedFrameRef.current = null;
      frameRenderTokenRef.current += 1;
      lastFrameCallbackAtRef.current = 0;
      setRenderedFrame(null);
      onFrameUpdate?.(null);
      const container = containerRef.current;
      const width = Math.max(960, Math.round(container?.clientWidth || 1280));
      const height = Math.max(640, Math.round(container?.clientHeight || 800));
      const existing = await loadSessionRef.current();
      if (disposed) return;

      if (
        reuseCurrentSession &&
        existing.ok &&
        existing.result.browser_surface.active &&
        ["starting", "ready"].includes(String(existing.result.browser_surface.state || "").trim())
      ) {
        activeLifecycleKeyRef.current = lifecycleKey;
        setSessionState(normalizeState(existing.result.browser_surface));
        attachSocket();
        return;
      }

      if (!startSessionRef.current) {
        if (existing.ok) {
          setSessionState(normalizeState(existing.result.browser_surface));
          return;
        }
        const message = `${existing.error.code}: ${existing.error.message}`;
        setPanelError(message);
        setSessionState(
          normalizeState({
            active: false,
            state: "failed",
            message,
            error: { code: existing.error.code, message: existing.error.message },
          }),
        );
        return;
      }

      const started = await startSessionRef.current({ width, height });
      if (disposed) {
        return;
      }
      if (!started.ok) {
        const message = `${started.error.code}: ${started.error.message}`;
        setPanelError(message);
        setSessionState(
          normalizeState({
            active: false,
            state: "failed",
            message,
            error: { code: started.error.code, message: started.error.message },
          }),
        );
        return;
      }

      activeLifecycleKeyRef.current = lifecycleKey;
      setSessionState(normalizeState(started.result.browser_surface));
      attachSocket();
    };

    void open();

    return () => {
      disposed = true;
      cleanupTransport();
    };
  }, [
    onFrameUpdate,
    runNonce,
    texts.closed,
    texts.connectionFailed,
    texts.reconnecting,
    texts.refreshFailed,
    reuseActiveSession,
    sessionIdentity,
    vncFailed,
    viewerMode,
    webSocketUrl,
  ]);

  const browserViewAvailable =
    String(sessionState.viewer?.kind || "")
      .trim()
      .toLowerCase() === "vnc" && !!sessionState.viewer?.vnc?.available;
  const vncAvailable = viewerMode === "browser" && browserViewAvailable && !vncFailed;
  const displayIsolated =
    !!sessionState.metadata?.display_owned &&
    String(sessionState.metadata?.display_owner || "").trim() === "cccc_xvfb";
  const vncFallbackReason = String(sessionState.viewer?.vnc?.error || "").trim();
  const vncFallbackReasonLabel = browserViewAvailable
    ? ""
    : formatVncFallbackReason(vncFallbackReason, texts);
  const isolationTooltip = displayIsolated ? ` ${texts.viewerTooltipIsolationXvfb}` : "";
  const pageModeTooltip = `${texts.viewerPageTooltip}${isolationTooltip}`;
  const browserModeTooltip = browserViewAvailable
    ? `${texts.viewerBrowserTooltip}${isolationTooltip}`
    : `${texts.viewerBrowserUnavailable}${
        vncFallbackReasonLabel ? ` ${texts.viewerFallbackReason}: ${vncFallbackReasonLabel}.` : ""
      }${isolationTooltip}`;
  const effectiveViewerMode = projectedBrowserEffectiveViewerMode(
    viewerMode,
    browserViewAvailable,
    vncFailed,
  );

  useEffect(() => {
    if (!vncAvailable || sessionState.state !== "ready") {
      if (rfbRef.current) {
        rfbRef.current.disconnect();
        rfbRef.current = null;
      }
      vncTargetRef.current?.replaceChildren();
      setVncConnected(false);
      return;
    }
    const target = vncTargetRef.current;
    if (!target || rfbRef.current) return;
    let cancelled = false;
    void (async () => {
      try {
        const mod = await import("@novnc/novnc");
        if (cancelled || rfbRef.current) return;
        const RFB = mod.default as RfbConstructor;
        target.replaceChildren();
        const rfb = new RFB(target, urlWithViewerParam(webSocketUrl, "mode", "vnc"), {});
        rfb.viewOnly = false;
        rfb.scaleViewport = true;
        rfb.clipViewport = false;
        rfb.resizeSession = false;
        rfb.background = "#ffffff";
        rfb.addEventListener("connect", () => {
          if (!cancelled) setVncConnected(true);
        });
        rfb.addEventListener("disconnect", () => {
          if (!cancelled) {
            setVncConnected(false);
            setVncFailed(true);
          }
        });
        rfbRef.current = rfb;
      } catch (error) {
        if (!cancelled) {
          setVncFailed(true);
          const message = error instanceof Error ? error.message : String(error || "");
          if (message) setPanelError(message);
        }
      }
    })();
    return () => {
      cancelled = true;
      if (rfbRef.current) {
        rfbRef.current.disconnect();
        rfbRef.current = null;
      }
      target.replaceChildren();
      setVncConnected(false);
    };
  }, [sessionState.state, vncAvailable, webSocketUrl]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container || typeof ResizeObserver === "undefined") return;

    const sendResize = () => {
      if (vncAvailable) return;
      const width = Math.max(640, Math.round(container.clientWidth || 0));
      const height = Math.max(480, Math.round(container.clientHeight || 0));
      if (!width || !height) return;
      sendCommand({ t: "resize", width, height });
    };

    const scheduleResize = () => {
      if (resizeTimerRef.current !== null) {
        window.clearTimeout(resizeTimerRef.current);
      }
      resizeTimerRef.current = window.setTimeout(() => {
        resizeTimerRef.current = null;
        sendResize();
      }, 120);
    };

    const observer = new ResizeObserver(() => {
      scheduleResize();
    });
    observer.observe(container);
    scheduleResize();
    return () => {
      observer.disconnect();
      if (resizeTimerRef.current !== null) {
        window.clearTimeout(resizeTimerRef.current);
        resizeTimerRef.current = null;
      }
    };
  }, [isExpanded, vncAvailable]);

  const handleBack = () => {
    setPanelError("");
    sendCommand({ t: "back" });
  };

  const handleViewerModeChange = (nextMode: ProjectedBrowserViewerMode) => {
    if (nextMode === "browser") {
      setVncFailed(false);
    }
    setPanelError("");
    setViewerMode(nextMode);
  };

  const handleReconnect = () => {
    frameRef.current = null;
    renderedFrameRef.current = null;
    frameRenderTokenRef.current += 1;
    if (rfbRef.current) {
      rfbRef.current.disconnect();
      rfbRef.current = null;
    }
    lastFrameCallbackAtRef.current = 0;
    setRenderedFrame(null);
    setVncConnected(false);
    setVncFailed(false);
    setPanelError("");
    reconnectAttemptsRef.current = 0;
    setSessionState(normalizeState({ active: true, state: "starting", message: texts.starting }));
    setRunNonce((value) => value + 1);
  };

  useEffect(() => {
    if (refreshNonce === lastRefreshNonceRef.current) return;
    lastRefreshNonceRef.current = refreshNonce;
    const commandId = `refresh-${refreshNonce}`;
    pendingRefreshIdRef.current = commandId;
    setRefreshFeedback("pending");
    if (
      sessionState.state === "failed" ||
      sessionState.state === "closed" ||
      sessionState.state === "disconnected"
    ) {
      const timer = window.setTimeout(() => {
        frameRef.current = null;
        renderedFrameRef.current = null;
        frameRenderTokenRef.current += 1;
        if (rfbRef.current) {
          rfbRef.current.disconnect();
          rfbRef.current = null;
        }
        lastFrameCallbackAtRef.current = 0;
        setRenderedFrame(null);
        setVncConnected(false);
        setVncFailed(false);
        setPanelError("");
        reconnectAttemptsRef.current = 0;
        setSessionState(
          normalizeState({ active: true, state: "starting", message: texts.starting }),
        );
        setRunNonce((value) => value + 1);
      }, 0);
      return () => window.clearTimeout(timer);
    }
    const timer = window.setTimeout(() => {
      setPanelError("");
      if (sendCommand({ t: "refresh", id: commandId })) {
        setRefreshFeedback("refreshing");
      } else {
        setRefreshFeedback("pending");
      }
    }, 0);
    return () => window.clearTimeout(timer);
  }, [refreshNonce, sessionState.state, texts.starting]);

  const framePoint = (clientX: number, clientY: number) => {
    if (vncAvailable) return;
    const frame = frameRef.current || renderedFrame;
    if (!frame || !imageRef.current) return;
    const rect = imageRef.current.getBoundingClientRect();
    return mapContainedImagePoint({ x: clientX, y: clientY }, rect, frame);
  };

  const handlePointerDown = (event: PointerEvent<HTMLImageElement>) => {
    const point = framePoint(event.clientX, event.clientY);
    if (!point) return;
    if (event.pointerType === "touch") {
      touchGestureRef.current = {
        pointerId: event.pointerId,
        start: point,
        last: point,
        moved: false,
      };
      event.currentTarget.setPointerCapture?.(event.pointerId);
    } else {
      sendCommand({
        t: "click",
        x: point.x,
        y: point.y,
        button: buttonFromMouseEvent(event.button),
      });
    }
    inputRelayRef.current?.focus({ preventScroll: true });
    event.preventDefault();
  };

  const handlePointerMove = (event: PointerEvent<HTMLImageElement>) => {
    const gesture = touchGestureRef.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    const point = framePoint(event.clientX, event.clientY);
    if (!point) return;
    const moved =
      gesture.moved ||
      Math.abs(point.x - gesture.start.x) + Math.abs(point.y - gesture.start.y) >= 4;
    if (!moved) {
      event.preventDefault();
      return;
    }
    const previous = gesture.moved ? gesture.last : gesture.start;
    sendCommand({
      t: "scroll",
      x: point.x,
      y: point.y,
      dx: previous.x - point.x,
      dy: previous.y - point.y,
    });
    gesture.last = point;
    gesture.moved = true;
    event.preventDefault();
  };

  const finishTouchGesture = (event: PointerEvent<HTMLImageElement>, cancelled: boolean) => {
    const gesture = touchGestureRef.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    if (!cancelled && !gesture.moved) {
      const point = framePoint(event.clientX, event.clientY);
      if (point) {
        sendCommand({ t: "click", x: point.x, y: point.y, button: "left" });
      }
    }
    touchGestureRef.current = null;
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    event.preventDefault();
  };

  const handleWheel = (event: WheelEvent<HTMLDivElement>) => {
    if (vncAvailable) return;
    const frame = frameRef.current || renderedFrame;
    if (!frame || !imageRef.current) return;
    const rect = imageRef.current.getBoundingClientRect();
    const point = mapContainedImagePoint({ x: event.clientX, y: event.clientY }, rect, frame);
    if (!point) return;
    sendCommand({
      t: "scroll",
      x: point.x,
      y: point.y,
      dx: Math.round(event.deltaX),
      dy: Math.round(event.deltaY),
    });
    event.preventDefault();
  };

  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (vncAvailable) return;
    if (event.nativeEvent.isComposing || relayComposingRef.current) return;
    if (event.metaKey || event.ctrlKey) return;
    const fromInputRelay = event.target === inputRelayRef.current;
    if (!fromInputRelay && !event.altKey && event.key.length === 1) {
      sendCommand({ t: "text", text: event.key });
      event.preventDefault();
      return;
    }
    const special = SPECIAL_KEY_MAP[event.key];
    if (!special) return;
    sendCommand({ t: "key", key: special });
    event.preventDefault();
  };

  const handleInputRelay = (event: FormEvent<HTMLTextAreaElement>) => {
    if (relayComposingRef.current || (event.nativeEvent as InputEvent).isComposing) return;
    const text = event.currentTarget.value;
    if (!text) return;
    sendCommand({ t: "text", text });
    event.currentTarget.value = "";
  };

  const showReconnect =
    sessionState.state === "failed" ||
    sessionState.state === "closed" ||
    sessionState.state === "disconnected";
  const fullScreenLabel = isExpanded ? texts.exitFullScreen : texts.fullScreen;
  const refreshFeedbackLabel =
    refreshFeedback === "pending"
      ? texts.refreshPending
      : refreshFeedback === "refreshing"
        ? texts.refreshing
        : refreshFeedback === "refreshed"
          ? texts.refreshed
          : refreshFeedback === "failed"
            ? texts.refreshFailed
            : "";
  const panelClassName = classNames(
    "relative flex flex-col overflow-hidden outline-none",
    chromeMode === "embedded"
      ? "rounded-none"
      : classNames(
          "rounded-3xl border",
          isDark
            ? "border-white/10 bg-slate-950/80"
            : "border-black/10 bg-[linear-gradient(180deg,#ffffff_0%,#f6f8fb_100%)]",
        ),
    isExpanded
      ? "h-full w-full shadow-2xl sm:h-[min(92dvh,980px)] sm:w-[min(96vw,1600px)]"
      : viewportClassName || "flex-1 min-h-0",
  );

  const panelBody = (
    <div
      ref={containerRef}
      tabIndex={0}
      onWheel={handleWheel}
      onKeyDown={handleKeyDown}
      className={panelClassName}
    >
      <textarea
        ref={inputRelayRef}
        data-browser-input-relay
        tabIndex={-1}
        aria-label="Browser input relay"
        autoCapitalize="none"
        autoCorrect="off"
        spellCheck={false}
        onCompositionStart={() => {
          relayComposingRef.current = true;
        }}
        onCompositionEnd={() => {
          relayComposingRef.current = false;
        }}
        onInput={handleInputRelay}
        onBlur={(event) => {
          relayComposingRef.current = false;
          event.currentTarget.value = "";
        }}
        className="pointer-events-none fixed h-px w-px opacity-0"
      />
      <div
        className={classNames(
          "flex flex-wrap items-center gap-2 text-xs",
          chromeMode === "embedded"
            ? classNames(
                "border-b px-2 py-1.5",
                isDark
                  ? "border-white/[0.06] bg-slate-950/50 text-slate-400"
                  : "border-black/[0.05] bg-black/[0.03] text-gray-600",
              )
            : classNames(
                "border-b px-4 py-3",
                isDark
                  ? "border-white/10 bg-slate-950/70 text-slate-300"
                  : "border-black/10 bg-white/75 text-gray-700",
              ),
        )}
      >
        <span
          className={classNames(
            "rounded-full px-2.5 py-1 font-medium",
            sessionState.state === "ready"
              ? isDark
                ? "bg-emerald-500/15 text-emerald-200"
                : "bg-emerald-50 text-emerald-700"
              : sessionState.state === "failed"
                ? isDark
                  ? "bg-rose-500/15 text-rose-200"
                  : "bg-rose-50 text-rose-700"
                : isDark
                  ? "bg-white/[0.08] text-white"
                  : "bg-[rgb(245,245,245)] text-[rgb(35,36,37)]",
          )}
        >
          {sessionState.state === "ready"
            ? texts.ready
            : sessionState.state === "failed"
              ? texts.failed
              : sessionState.state === "closed"
                ? texts.closed
                : texts.starting}
        </span>
        <div
          role="group"
          aria-label={texts.viewerLabel}
          className={classNames(
            "inline-flex shrink-0 items-center rounded-full p-0.5",
            isDark ? "bg-white/[0.06]" : "bg-black/[0.045]",
          )}
        >
          {(["page", "browser"] as const).map((mode) => {
            const selected = effectiveViewerMode === mode;
            const browserMode = mode === "browser";
            return (
              <button
                key={mode}
                type="button"
                aria-pressed={selected}
                disabled={browserMode && !browserViewAvailable}
                onClick={() => handleViewerModeChange(mode)}
                title={browserMode ? browserModeTooltip : pageModeTooltip}
                className={classNames(
                  "rounded-full px-2.5 py-1 font-medium transition-colors",
                  selected
                    ? isDark
                      ? "bg-white/[0.11] text-slate-100 shadow-sm"
                      : "bg-white text-gray-800 shadow-sm ring-1 ring-black/[0.05]"
                    : isDark
                      ? "text-slate-400 hover:bg-white/[0.05] hover:text-slate-200"
                      : "text-gray-500 hover:bg-white/70 hover:text-gray-700",
                  browserMode && !browserViewAvailable
                    ? "cursor-not-allowed opacity-45 hover:bg-transparent"
                    : "",
                )}
              >
                {browserMode ? texts.viewerBrowser : texts.viewerPage}
              </button>
            );
          })}
        </div>
        <span className="min-w-0 flex-1 truncate">{sessionState.url || fallbackUrl || ""}</span>
        {refreshFeedbackLabel ? (
          <span
            className={classNames(
              "rounded-full px-2.5 py-1 font-medium",
              refreshFeedback === "failed"
                ? isDark
                  ? "bg-rose-500/15 text-rose-200"
                  : "bg-rose-50 text-rose-700"
                : isDark
                  ? "bg-cyan-500/15 text-cyan-200"
                  : "bg-cyan-50 text-cyan-700",
            )}
            role="status"
          >
            {refreshFeedbackLabel}
          </span>
        ) : null}
        {chromeMode === "standalone" ? (
          <button
            type="button"
            onClick={() => setIsExpanded((value) => !value)}
            className={classNames(
              "inline-flex h-9 w-9 items-center justify-center rounded-full transition-colors",
              isDark
                ? "bg-slate-800 text-slate-200 hover:bg-slate-700"
                : "bg-gray-100 text-gray-800 hover:bg-gray-200",
            )}
            aria-label={fullScreenLabel}
            title={fullScreenLabel}
          >
            <ProjectedBrowserExpandIcon expanded={isExpanded} />
          </button>
        ) : null}
        <button
          type="button"
          onClick={handleBack}
          className={classNames(
            "rounded-full px-3 py-1 font-medium transition-colors",
            isDark
              ? "bg-slate-800 text-slate-200 hover:bg-slate-700"
              : "bg-gray-100 text-gray-800 hover:bg-gray-200",
          )}
        >
          {texts.back}
        </button>
        {showReconnect ? (
          <button
            type="button"
            onClick={handleReconnect}
            className={classNames(
              "rounded-full px-3 py-1 font-medium transition-colors",
              isDark
                ? "bg-slate-800 text-slate-200 hover:bg-slate-700"
                : "bg-gray-100 text-gray-800 hover:bg-gray-200",
            )}
          >
            {texts.reconnect}
          </button>
        ) : null}
      </div>

      <div
        className={classNames(
          "relative flex min-h-0 flex-1 items-center justify-center overflow-hidden",
          chromeMode === "embedded" ? "bg-white" : "p-4",
        )}
      >
        {vncAvailable ? (
          <div className="relative h-full w-full">
            <div
              ref={vncTargetRef}
              className={classNames(
                "h-full w-full overflow-hidden",
                chromeMode === "embedded"
                  ? "bg-white [&>*]:!overflow-hidden [&_*]:!border-0 [&_canvas]:!block [&_canvas]:!outline-none"
                  : "",
              )}
            />
            {!vncConnected ? (
              <div
                className={classNames(
                  "pointer-events-none absolute inset-0 flex items-center justify-center text-sm",
                  isDark ? "text-slate-400" : "text-gray-500",
                )}
              >
                {sessionState.message || texts.waiting}
              </div>
            ) : null}
          </div>
        ) : renderedFrame ? (
          <img
            ref={imageRef}
            src={renderedFrame.dataUrl}
            alt={texts.frameAlt}
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={(event) => finishTouchGesture(event, false)}
            onPointerCancel={(event) => finishTouchGesture(event, true)}
            onContextMenu={(event) => event.preventDefault()}
            className={classNames(
              "max-h-full max-w-full touch-none select-none object-contain",
              chromeMode === "embedded"
                ? "h-full w-full"
                : "rounded-2xl border border-[var(--glass-border-subtle)] shadow-2xl",
            )}
            draggable={false}
          />
        ) : (
          <div
            className={classNames(
              "flex h-full min-h-[320px] w-full items-center justify-center rounded-2xl border border-dashed text-sm",
              isDark ? "border-white/10 text-slate-400" : "border-black/10 text-gray-500",
            )}
          >
            {sessionState.message ||
              (sessionState.state === "starting" ? texts.starting : texts.waiting)}
          </div>
        )}

        {panelError ? (
          <div
            className={classNames(
              "pointer-events-none absolute bottom-4 left-4 right-4 rounded-2xl border px-4 py-3 text-sm shadow-xl",
              isDark
                ? "border-rose-500/20 bg-rose-500/10 text-rose-100"
                : "border-rose-200 bg-white/90 text-rose-700",
            )}
          >
            {panelError}
          </div>
        ) : null}
      </div>
    </div>
  );

  if (isExpanded && typeof document !== "undefined") {
    return createPortal(
      <div className="fixed inset-0 z-[1000] animate-fade-in">
        <div
          className="absolute inset-0 glass-overlay"
          onPointerDown={(event) => {
            if (event.target === event.currentTarget) {
              setIsExpanded(false);
            }
          }}
        />
        <div className="absolute inset-0 flex items-stretch justify-center p-0 sm:items-center sm:p-6">
          {panelBody}
        </div>
      </div>,
      document.body,
    );
  }

  return panelBody;
}
