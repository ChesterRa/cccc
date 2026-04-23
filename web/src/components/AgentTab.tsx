/* eslint-disable no-control-regex */
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useTranslation } from "react-i18next";
import { Actor, AgentState, HeadlessPreviewSession, HeadlessStreamEvent, StreamingActivity, RUNTIME_INFO } from "../types";
import { useActorDisplayState } from "../hooks/useActorDisplayState";
import { getTerminalTheme } from "../hooks/useTheme";
import { classNames } from "../utils/classNames";
import { formatFullTime, formatTime } from "../utils/time";
import { useGroupStore, useObservabilityStore, useTerminalSignalsStore } from "../stores";
import { withAuthToken, fetchTerminalTail } from "../services/api";
import { HeadlessLiveTrace } from "./headless/HeadlessLiveTrace";
import { HeadlessRawTrace } from "./headless/HeadlessRawTrace";
import { StopIcon, RefreshIcon, InboxIcon, TrashIcon, PlayIcon, EditIcon, TerminalIcon } from "./Icons";
import { ScrollFade } from "./ScrollFade";
import { getTerminalSignalFromChunk } from "../utils/terminalWorkingState";
import { getRuntimeIndicatorState } from "../utils/statusIndicators";
import { getEffectiveActorRunner } from "../utils/headlessRuntimeSupport";
import { copyTextToClipboard } from "../utils/copy";

type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'reconnecting';
const EMPTY_STREAMING_ACTIVITIES: StreamingActivity[] = [];
const EMPTY_HEADLESS_PREVIEW_SESSIONS: HeadlessPreviewSession[] = [];
const EMPTY_HEADLESS_RAW_EVENTS: HeadlessStreamEvent[] = [];

// Delay before showing terminal after connection (allows backlog replay to complete without visible scrolling)
const TERMINAL_SHOW_DELAY_MS = 150;

// WebSocket reconnect configuration (moved outside component to avoid recreation)
const RECONNECT_BASE_DELAY_MS = 1000;
const RECONNECT_MAX_DELAY_MS = 30000;
const MAX_RECONNECT_ATTEMPTS = 10;
const copyToClipboard = copyTextToClipboard;

interface AgentTabProps {
  actor: Actor;
  groupId: string;
  termEpoch?: number;
  agentState: AgentState | null;
  isVisible: boolean;
  readOnly?: boolean;
  onQuit: () => void;
  onLaunch: () => void;
  onRelaunch: () => void;
  onEdit: () => void;
  onRemove: () => void;
  onInbox: () => void;
  busy: string;
  isDark: boolean;
  isSmallScreen: boolean;
  /** Called when the component detects actor status may have changed (e.g., process exited) */
  onStatusChange?: () => void;
}

export function AgentTab({
  actor,
  groupId,
  termEpoch = 0,
  agentState,
  isVisible,
  readOnly,
  onQuit,
  onLaunch,
  onRelaunch,
  onEdit,
  onRemove,
  onInbox,
  busy,
  isDark,
  isSmallScreen,
  onStatusChange,
}: AgentTabProps) {
  const { t } = useTranslation('actors');
  // Derived state (must be defined before refs that use them)
  const { isRunning, workingState } = useActorDisplayState({ groupId, actor });
  const effectiveRunner = getEffectiveActorRunner(actor);
  const isHeadless = effectiveRunner === "headless";
  const canControl = !readOnly;
  const latestHeadlessText = useGroupStore((state) => {
    const bucket = state.chatByGroup[String(groupId || "").trim()];
    if (!bucket) return "";
    const actorId = String(actor.id || "").trim();
    if (!actorId) return "";
    return String(bucket.latestActorTextByActorId?.[actorId] || "");
  });
  const headlessPreviewSessions = useGroupStore((state) => {
    const bucket = state.chatByGroup[String(groupId || "").trim()];
    if (!bucket) return EMPTY_HEADLESS_PREVIEW_SESSIONS;
    const actorId = String(actor.id || "").trim();
    if (!actorId) return EMPTY_HEADLESS_PREVIEW_SESSIONS;
    const sessions = bucket.previewSessionsByActorId?.[actorId];
    return Array.isArray(sessions) ? sessions : EMPTY_HEADLESS_PREVIEW_SESSIONS;
  });
  const latestHeadlessActivities = useGroupStore((state) => {
    const bucket = state.chatByGroup[String(groupId || "").trim()];
    if (!bucket) return EMPTY_STREAMING_ACTIVITIES;
    const actorId = String(actor.id || "").trim();
    if (!actorId) return EMPTY_STREAMING_ACTIVITIES;
    const activities = bucket.latestActorActivitiesByActorId?.[actorId];
    return Array.isArray(activities) ? activities : EMPTY_STREAMING_ACTIVITIES;
  });
  const rawHeadlessEvents = useGroupStore((state) => {
    const bucket = state.chatByGroup[String(groupId || "").trim()];
    if (!bucket) return EMPTY_HEADLESS_RAW_EVENTS;
    const actorId = String(actor.id || "").trim();
    if (!actorId) return EMPTY_HEADLESS_RAW_EVENTS;
    const events = bucket.rawHeadlessEventsByActorId?.[actorId];
    return Array.isArray(events) ? events : EMPTY_HEADLESS_RAW_EVENTS;
  });
  const observabilityLoaded = useObservabilityStore((s) => s.loaded);
  const loadObservability = useObservabilityStore((s) => s.load);
  const terminalScrollbackLines = useObservabilityStore((s) => s.terminalScrollbackLines);
  const setTerminalSignal = useTerminalSignalsStore((s) => s.setSignal);
  const clearTerminalSignal = useTerminalSignalsStore((s) => s.clearSignal);

  const termRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const [connectionStatus, setConnectionStatus] = useState<ConnectionStatus>('disconnected');
  // Hide terminal during initial backlog replay to avoid visible scrolling
  const [terminalReady, setTerminalReady] = useState(false);
  const terminalReadyTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const outputFilterTailRef = useRef<string>("");
  const [activated, setActivated] = useState(false);
  // Bumped to trigger a fresh WebSocket connection from the reconnect button
  const [reconnectTrigger, setReconnectTrigger] = useState(0);
  const terminalSignalBufferRef = useRef("");

  const pasteStateRef = useRef<{ inFlight: boolean; lastAt: number }>({ inFlight: false, lastAt: 0 });

  // WebSocket reconnect state
  const reconnectAttemptRef = useRef(0);
  const reconnectTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Stop reconnecting if server reports actor is not running
  const actorNotRunningRef = useRef(false);
  const lastTermEpochRef = useRef(termEpoch);

  // Ref to avoid stale closure in WebSocket callbacks
  const isRunningRef = useRef(isRunning);
  const runtimeRef = useRef(actor.runtime);
  const canControlRef = useRef(canControl);

  // Keep ref in sync with prop
  useEffect(() => {
    isRunningRef.current = isRunning;
    runtimeRef.current = actor.runtime;
    canControlRef.current = canControl;
    // Reset the "actor not running" flag when actor starts running again
    if (isRunning) {
      actorNotRunningRef.current = false;
    }
  }, [actor.runtime, canControl, isRunning]);

  useEffect(() => {
    if (isRunning && !isHeadless) return;
    terminalSignalBufferRef.current = "";
    clearTerminalSignal(groupId, actor.id);
  }, [actor.id, clearTerminalSignal, groupId, isHeadless, isRunning]);

  // Activate the terminal only after the user has visited this actor tab at least once.
  // Once activated, keep the PTY session connected even when the tab is hidden to avoid backlog replay and scroll jumps.
  useEffect(() => {
    if (isVisible) setActivated(true);
  }, [isVisible]);

  useEffect(() => {
    if (!activated || observabilityLoaded) return;
    void loadObservability();
  }, [activated, loadObservability, observabilityLoaded]);

  const rtInfo = (actor.runtime && RUNTIME_INFO[actor.runtime]) ? RUNTIME_INFO[actor.runtime] : RUNTIME_INFO.codex;
  const unreadCount = actor.unread_count ?? 0;
  const statusClamp2Style: CSSProperties = {
    display: "-webkit-box",
    WebkitBoxOrient: "vertical",
    WebkitLineClamp: 2,
    overflow: "hidden",
  };

  const runtimeIndicator = getRuntimeIndicatorState({ isRunning: Boolean(isRunning), workingState });
  const statusTone = (() => {
    switch (runtimeIndicator.tone) {
      case "stop":
        return {
          dotClass: runtimeIndicator.dotClass,
          pulse: runtimeIndicator.pulse,
          strongPulse: runtimeIndicator.strongPulse,
          badgeClass: "bg-slate-500/10 text-slate-500 dark:text-slate-300",
        };
      case "working":
        return {
          dotClass: runtimeIndicator.dotClass,
          pulse: runtimeIndicator.pulse,
          strongPulse: runtimeIndicator.strongPulse,
          badgeClass: "bg-emerald-500/15 text-emerald-600 dark:text-emerald-300",
        };
      case "run":
      default:
        return {
          dotClass: runtimeIndicator.dotClass,
          pulse: runtimeIndicator.pulse,
          strongPulse: runtimeIndicator.strongPulse,
          badgeClass: "bg-emerald-500/15 text-emerald-600 dark:text-emerald-300",
        };
    }
  })();

  const runtimeStatusText = (() => {
    if (!isRunning) return t("stopped");
    if (workingState === "working") return t("working");
    return t("running");
  })();
  const latestHeadlessPreview = headlessPreviewSessions.length > 0
    ? headlessPreviewSessions[headlessPreviewSessions.length - 1]
    : null;
  const primaryActionButtonClass =
    "inline-flex items-center gap-1.5 rounded-xl border border-[rgb(35,36,37)] bg-[rgb(35,36,37)] px-3.5 py-2.5 text-sm font-medium text-white transition-colors hover:bg-black disabled:opacity-50 disabled:cursor-not-allowed dark:border-white dark:bg-white dark:text-[rgb(35,36,37)] dark:hover:bg-white/92";
  const secondaryActionButtonClass =
    "inline-flex items-center gap-1.5 rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-3.5 py-2.5 text-sm font-medium text-[var(--color-text-secondary)] transition-colors hover:bg-[var(--glass-tab-bg-hover)] disabled:opacity-50 disabled:cursor-not-allowed";
  const ghostActionButtonClass =
    "inline-flex items-center gap-1.5 rounded-xl border border-transparent px-3 py-2.5 text-sm font-medium text-[var(--color-text-tertiary)] transition-colors hover:border-[var(--glass-border-subtle)] hover:bg-[var(--glass-tab-bg-hover)] hover:text-[var(--color-text-primary)] disabled:opacity-50 disabled:cursor-not-allowed";

  // Send interrupt (Ctrl+C)
  const sendInterrupt = () => {
    if (readOnly) return;
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(JSON.stringify({ t: "i", d: "\x03" }));
  };

  // Update terminal theme when isDark changes
  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.options.theme = getTerminalTheme(isDark);
    }
  }, [isDark]);

  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.options.disableStdin = !canControl;
      terminalRef.current.options.cursorBlink = canControl;
    }
  }, [canControl]);

  // Update terminal scrollback when global settings change.
  useEffect(() => {
    if (terminalRef.current) {
      terminalRef.current.options.scrollback = terminalScrollbackLines;
    }
  }, [terminalScrollbackLines]);

  // Initialize terminal
  useEffect(() => {
    if (!termRef.current || isHeadless || !isRunning || !activated) return;

    const term = new Terminal({
      cursorBlink: canControl,
      // Avoid an extra blinking "outline" cursor when the terminal isn't focused.
      // Some runtimes render their own cursor; xterm's inactive cursor can look like a second cursor.
      cursorInactiveStyle: "none",
      fontSize: 13,
      fontFamily: '"JetBrains Mono", "Fira Code", "SF Mono", Menlo, Monaco, monospace',
      theme: getTerminalTheme(isDark),
      disableStdin: !canControl,
      // Bigger scrollback improves history browsing without going "infinite" and hurting perf.
      // Default is 8k lines; the user can override it in Global → Developer settings.
      scrollback: terminalScrollbackLines || 8000,
      allowProposedApi: true,
    });

    const fitAddon = new FitAddon();

    term.loadAddon(fitAddon);
    term.open(termRef.current);
    // Ensure focus works consistently across browsers (and prevents the inactive cursor style).
    const onPointerDown = () => term.focus();
    term.element?.addEventListener("mousedown", onPointerDown);
    term.element?.addEventListener("touchstart", onPointerDown, { passive: true });

    const copySelection = async (): Promise<boolean> => {
      try {
        const sel = term.getSelection ? term.getSelection() : "";
        if (!sel) return false;
        return await copyToClipboard(sel);
      } catch {
        return false;
      }
    };

    // High-ROI copy UX:
    // - If text is selected, Ctrl/Cmd+C copies (instead of sending SIGINT to the runtime)
    // - Right-click copies selection (common web terminal behavior)
    term.attachCustomKeyEventHandler((ev) => {
      const key = (ev.key || "").toLowerCase();
      const isCopy = (ev.ctrlKey || ev.metaKey) && !ev.shiftKey && key === "c";
      const isCopyShift = (ev.ctrlKey || ev.metaKey) && ev.shiftKey && key === "c";
      const isPaste = (ev.ctrlKey || ev.metaKey) && !ev.altKey && key === "v";
      if (isCopy || isCopyShift) {
        if (term.hasSelection?.()) {
          void copySelection();
          return false; // prevent ^C from reaching the runtime
        }
      }
      if (isPaste && canControlRef.current) {
        // xterm.js intentionally doesn't map Ctrl+V to paste by default (to preserve terminal semantics),
        // but for CCCC agents the high-ROI expectation is "Ctrl/Cmd+V pastes text into the PTY".
        const readText = navigator.clipboard?.readText;
        if (typeof readText === "function") {
          // Prevent the browser's default paste behavior (xterm's textarea may also handle paste),
          // otherwise we can end up pasting the same payload multiple times.
          ev.preventDefault();
          ev.stopPropagation();

          const now = Date.now();
          if (pasteStateRef.current.inFlight) return false;
          if (now - pasteStateRef.current.lastAt < 250) return false;
          pasteStateRef.current.inFlight = true;
          pasteStateRef.current.lastAt = now;

          void readText.call(navigator.clipboard).then((text: string) => {
            const t = (text || "").toString();
            if (!t) return;
            try {
              term.paste(t);
            } catch {
              // ignore
            }
          }).catch(() => {
            // If clipboard read is blocked, fall back to default behavior.
          }).finally(() => {
            pasteStateRef.current.inFlight = false;
          });
          return false;
        }
      }
      return true;
    });

    const onContextMenu = (ev: MouseEvent) => {
      if (!term.hasSelection?.()) return;
      ev.preventDefault();
      void copySelection();
    };
    term.element?.addEventListener("contextmenu", onContextMenu);

    terminalRef.current = term;
    fitAddonRef.current = fitAddon;

    // Initial fit — use requestAnimationFrame to wait for layout completion
    requestAnimationFrame(() => {
      if (termRef.current && termRef.current.clientWidth > 50) {
        fitAddon.fit();
      }
    });

    return () => {
      term.element?.removeEventListener("contextmenu", onContextMenu);
      term.element?.removeEventListener("mousedown", onPointerDown);
      term.element?.removeEventListener("touchstart", onPointerDown);
      term.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Theme changes are handled in a dedicated effect; avoid re-creating the terminal.
  }, [isHeadless, isRunning, activated]);

  // Connect WebSocket when visible and running (with auto-reconnect).
  useEffect(() => {
    if (!activated || !isRunning || isHeadless || !terminalRef.current) return;

    // Clear any pending reconnect
    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current);
      reconnectTimeoutRef.current = null;
    }

    let disposed = false;
    let disposable: { dispose: () => void } | null = null;
    let resizeDisposable: { dispose: () => void } | null = null;

    const connect = () => {
      if (disposed) return;
      const existingWs = wsRef.current;
      if (existingWs && (existingWs.readyState === WebSocket.OPEN || existingWs.readyState === WebSocket.CONNECTING)) {
        return;
      }

      // Clean up old disposables to avoid race conditions on rapid reconnect
      if (disposable) {
        disposable.dispose();
        disposable = null;
      }
      if (resizeDisposable) {
        resizeDisposable.dispose();
        resizeDisposable = null;
      }

      // Close existing connection if any
      if (existingWs) {
        existingWs.close();
        wsRef.current = null;
      }

      setConnectionStatus('connecting');

      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const wsUrl = `${protocol}//${window.location.host}/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actor.id)}/term`;

      const ws = new WebSocket(withAuthToken(wsUrl));
      ws.binaryType = "arraybuffer";
      wsRef.current = ws;

      ws.onopen = () => {
        if (disposed) {
          // Component unmounted during connection (e.g., React Strict Mode)
          ws.close(1000, 'Component unmounted during connection');
          return;
        }
        setConnectionStatus('connected');
        reconnectAttemptRef.current = 0; // Reset reconnect counter
        // Reset output filter state on each successful (re)connect.
        outputFilterTailRef.current = "";
        terminalSignalBufferRef.current = "";

        // Delay showing terminal to let backlog replay complete (avoids visible scrolling)
        if (terminalReadyTimeoutRef.current) {
          clearTimeout(terminalReadyTimeoutRef.current);
        }
        setTerminalReady(false);
        terminalReadyTimeoutRef.current = setTimeout(() => {
          if (!disposed) {
            setTerminalReady(true);
          }
        }, TERMINAL_SHOW_DELAY_MS);

        // Rebuild terminal signal from the current visible tail so the working badge
        // does not depend on catching a later incremental chunk.
        void fetchTerminalTail(groupId, actor.id, 4000, true, true)
          .then((resp) => {
            if (disposed || !resp.ok) return;
            const tailText = String(resp.result?.text || "");
            const signal = getTerminalSignalFromChunk("", tailText, actor.runtime);
            terminalSignalBufferRef.current = signal.nextBuffer;
            if (signal.signalKind) {
              setTerminalSignal(groupId, actor.id, {
                kind: signal.signalKind,
                updatedAt: Date.now(),
              });
              return;
            }
            clearTerminalSignal(groupId, actor.id);
          })
          .catch(() => {
            if (disposed) return;
          });

        // Send initial resize (ops mode only). Exhibit should be view-only and not affect PTY size.
        // Guard against sending tiny cols (layout not yet complete) which would break line wrapping.
        if (canControlRef.current) {
          const term = terminalRef.current;
          if (term && term.cols >= 10 && term.rows >= 2) {
            ws.send(JSON.stringify({ t: "r", c: term.cols, r: term.rows }));
          }
        }
      };

      const _handleDecoded = (data: string) => {
        if (disposed) return;
        const term = terminalRef.current;
        if (!term) return;
        // Preserve scrollback: many TUIs emit CSI 3 J (clear scrollback) which makes the terminal
        // history appear "very short" regardless of scrollback buffer size. Convert it to CSI 2 J
        // (clear screen only) so users can still scroll back. This also makes the scrollback_lines
        // setting meaningful across runtimes.
        //
        // Note: we keep a tiny tail buffer so we can catch the escape sequence even if it spans
        // WebSocket frame boundaries.
        const seq = "\x1b[3J";
        const repl = "\x1b[2J";
        const combined = `${outputFilterTailRef.current}${data || ""}`;
        const replaced = combined.split(seq).join(repl);
        let tail = "";
        for (let n = seq.length - 1; n > 0; n--) {
          const suffix = replaced.slice(-n);
          if (seq.startsWith(suffix)) {
            tail = suffix;
            break;
          }
        }
        outputFilterTailRef.current = tail;
        const safe = tail ? replaced.slice(0, -tail.length) : replaced;
        const signal = getTerminalSignalFromChunk(terminalSignalBufferRef.current, safe, actor.runtime);
        terminalSignalBufferRef.current = signal.nextBuffer;
        if (signal.signalKind) {
          setTerminalSignal(groupId, actor.id, {
            kind: signal.signalKind,
            updatedAt: Date.now(),
          });
        }
        try {
          term.write(safe);
        } catch (err) {
          console.error("terminal write failed", err);
        }
      };

      ws.onmessage = (event) => {
        if (disposed) return;

        if (event.data instanceof ArrayBuffer) {
          _handleDecoded(new TextDecoder().decode(event.data));
        } else if (event.data instanceof Blob) {
          // Safari/iOS can deliver binary WebSocket frames as Blob even with binaryType="arraybuffer".
          void event.data.arrayBuffer().then((buf) => _handleDecoded(new TextDecoder().decode(buf)));
        } else if (typeof event.data === "string") {
          // Server might send JSON messages for status
          try {
            const msg = JSON.parse(event.data);
            if (msg.ok === false && msg.error) {
              _handleDecoded(`\r\n[error] ${msg.error.message || "Unknown error"}\r\n`);
              // If actor is not running, stop reconnecting and notify parent to refresh state
              if (msg.error.code === "actor_not_running") {
                actorNotRunningRef.current = true;
                // Notify parent to refresh actor state - this will update actor.running
                // and trigger proper UI update through normal React data flow
                onStatusChange?.();
              }
            }
          } catch {
            // Not JSON, write as text
            _handleDecoded(event.data);
          }
        }
      };

      ws.onclose = (event) => {
        if (disposed) return;
        wsRef.current = null;

        // Don't retry on clean close (1000), auth error (4401), or actor not running
        const noRetry = event.code === 1000 || event.code === 4401 || actorNotRunningRef.current;

        if (!noRetry && isRunningRef.current && !isHeadless) {
          const attempt = reconnectAttemptRef.current;

          // Check max reconnect attempts
          if (attempt >= MAX_RECONNECT_ATTEMPTS) {
            setConnectionStatus('disconnected');
            return;
          }

          const delay = Math.min(RECONNECT_BASE_DELAY_MS * Math.pow(2, attempt), RECONNECT_MAX_DELAY_MS);
          setConnectionStatus('reconnecting');

          reconnectTimeoutRef.current = setTimeout(() => {
            reconnectAttemptRef.current++;
            connect();
          }, delay);
        } else {
          setConnectionStatus('disconnected');
        }
      };

      ws.onerror = (_error) => {
        // onclose will be called after onerror, reconnect logic is handled there
      };

      // Handle terminal input - send as JSON with type "i" (input)
      const term = terminalRef.current;
      if (term && canControlRef.current) {
        disposable = term.onData((data) => {
          if (ws.readyState === WebSocket.OPEN) {
            // xterm.js can emit terminal replies (not user keystrokes), e.g. device attributes / color queries.
            // Some runtimes can echo these back as literal text (seen as "1;2c" or "]11;rgb:..."), so filter for those.
            // Keep the filter runtime-scoped to avoid interfering with full-screen TUIs that may rely on terminal queries.
            const runtime = runtimeRef.current;
            if (runtime === "droid" || runtime === "gemini" || runtime === "neovate") {
              const isDeviceAttributesReply = /^\x1b\[(?:\?|>)(?:\d+)(?:;\d+)*c$/.test(data);
              if (isDeviceAttributesReply) return;

              // OSC color query replies (e.g. background/foreground): ESC ] 10/11 ; rgb:.... BEL or ST
              const isOscColorReply = /^\x1b\](?:10|11);rgb:[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}(?:\x07|\x1b\\)$/.test(data);
              if (isOscColorReply) return;

              // Focus in/out events (CSI I / CSI O) can be emitted by xterm when apps enable focus tracking (DEC 1004).
              // Some runtimes echo them as literal text on tab focus changes (seen as "[O").
              const isFocusEvent = /^\x1b\[[IO]$/.test(data);
              if (isFocusEvent) return;
            }
            if (data.includes("\r") || data.includes("\n") || data.includes("\x03")) {
              setTerminalSignal(groupId, actor.id, {
                kind: "working_output",
                updatedAt: Date.now(),
              });
            }
            ws.send(JSON.stringify({ t: "i", d: data }));
          }
        });

        // Handle terminal resize - send as JSON with type "r" (resize)
        resizeDisposable = term.onResize(({ cols, rows }) => {
          if (ws.readyState === WebSocket.OPEN && cols >= 10 && rows >= 2) {
            ws.send(JSON.stringify({ t: "r", c: cols, r: rows }));
          }
        });
      }
    };

    // Start initial connection
    connect();

    return () => {
      disposed = true;
      if (reconnectTimeoutRef.current) {
        clearTimeout(reconnectTimeoutRef.current);
        reconnectTimeoutRef.current = null;
      }
      if (terminalReadyTimeoutRef.current) {
        clearTimeout(terminalReadyTimeoutRef.current);
        terminalReadyTimeoutRef.current = null;
      }
      if (disposable) disposable.dispose();
      if (resizeDisposable) resizeDisposable.dispose();
      if (wsRef.current) {
        // Only close if already connected; if still CONNECTING, just nullify ref
        // to avoid "WebSocket is closed before the connection is established" warning
        // (common in React Strict Mode double-invoke during development)
        if (wsRef.current.readyState === WebSocket.OPEN) {
          wsRef.current.close(1000, 'Component cleanup');
        }
        wsRef.current = null;
      }
      setConnectionStatus('disconnected');
      setTerminalReady(false);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Theme changes are handled separately; onStatusChange should not trigger reconnects.
  }, [activated, isRunning, isHeadless, groupId, actor.id, reconnectTrigger]);

  useEffect(() => {
    if (!activated || isHeadless || !isRunning || !terminalRef.current) return;
    if (lastTermEpochRef.current === termEpoch) return;
    lastTermEpochRef.current = termEpoch;
    reconnectAttemptRef.current = 0;
    actorNotRunningRef.current = false;
    setReconnectTrigger((n) => n + 1);
  }, [termEpoch, activated, isHeadless, isRunning]);

  // Fit terminal on visibility change and resize (with debounce to reduce jitter)
  useEffect(() => {
    if (!isVisible || !fitAddonRef.current) return;

    let resizeTimeout: ReturnType<typeof setTimeout> | null = null;

    const fit = () => {
      if (fitAddonRef.current && termRef.current && termRef.current.clientWidth > 0) {
        fitAddonRef.current.fit();
      }
    };

    // Debounced fit to prevent jitter during rapid resize events
    const debouncedFit = () => {
      if (resizeTimeout) clearTimeout(resizeTimeout);
      resizeTimeout = setTimeout(fit, 100);
    };

    // Fit when becoming visible
    setTimeout(fit, 50);

    // Fit on window resize (debounced)
    window.addEventListener("resize", debouncedFit);

    // Observe container resize to catch layout changes (e.g. sidebar toggle, split pane)
    const container = termRef.current;
    let ro: ResizeObserver | null = null;
    if (container) {
      ro = new ResizeObserver(() => debouncedFit());
      ro.observe(container);
    }

    return () => {
      window.removeEventListener("resize", debouncedFit);
      if (ro) ro.disconnect();
      if (resizeTimeout) clearTimeout(resizeTimeout);
    };
  }, [isVisible]);

  // UX: when the user switches to an agent tab (ops mode), focus the terminal automatically.
  // This avoids "typing into nowhere" if the chat composer was previously focused.
  useEffect(() => {
    if (!canControl) return;
    if (!isVisible) return;
    if (!terminalReady) return;
    if (isSmallScreen) return;
    const term = terminalRef.current;
    if (!term) return;
    const t = setTimeout(() => {
      try {
        term.focus();
      } catch {
        // ignore
      }
    }, 0);
    return () => clearTimeout(t);
  }, [canControl, isVisible, isSmallScreen, terminalReady]);

  const isBusy = busy.includes(actor.id);
  const stateHeadline = String(agentState?.hot?.focus || agentState?.hot?.next_action || "").trim() || t('noAgentStateYet');
  const stateTask = String(agentState?.hot?.active_task_id || "").trim();
  const blockerCount = Array.isArray(agentState?.hot?.blockers) ? agentState.hot.blockers.length : 0;
  const stateNext = String(agentState?.hot?.next_action || "").trim();

  return (
    <div className="flex flex-col h-full">
      {/* Agent Header */}
      <div className={classNames(
        "border-b px-4 py-3 sm:px-5",
        isDark
          ? "border-white/8 bg-[linear-gradient(180deg,rgba(255,255,255,0.025),rgba(255,255,255,0.01))]"
          : "border-black/6 bg-[linear-gradient(180deg,rgba(255,255,255,0.96),rgba(248,250,252,0.88))]"
      )}>
        <div className="flex items-start justify-between gap-4">
          <div className="flex min-w-0 items-start gap-4">
            <span
              className={classNames(
                "relative mt-[0.42rem] inline-flex h-2.5 w-2.5 flex-shrink-0 rounded-full transition-all",
                statusTone.dotClass
              )}
            >
              {statusTone.pulse && (
                <span
                  className={classNames(
                    "absolute inset-[-3px] rounded-full motion-reduce:animate-none",
                    statusTone.strongPulse
                      ? "animate-ping bg-emerald-300/35"
                      : "animate-pulse bg-current/20"
                  )}
                />
              )}
              {statusTone.strongPulse && (
                <span className="absolute inset-[-7px] rounded-full border border-emerald-300/35 animate-ping motion-reduce:animate-none [animation-duration:1.6s]" />
              )}
            </span>

            <div className="flex min-w-0 items-start gap-4">
              <div className="min-w-0 shrink-0">
                <div className="flex items-center gap-2 min-w-0">
                  <span className="min-w-0 truncate font-semibold text-[var(--color-text-primary)]">{actor.title || actor.id}</span>
                  {actor.role === "foreman" && (
                    <span className="rounded-md border border-amber-500/25 bg-amber-500/12 px-1.5 py-0.5 text-[10px] font-medium text-amber-700 dark:text-amber-300">
                      {t('foreman')}
                    </span>
                  )}
                </div>
                <div className={classNames("mt-0.5 text-xs truncate", "text-[var(--color-text-tertiary)]")}>
                  {rtInfo?.label || t('custom')} • {runtimeStatusText}
                  {isHeadless && ` • ${t('headless')}`}
                </div>
                {/* Mobile-only: condensed single-line agent state */}
                <div
                  className={classNames(
                    "sm:hidden mt-1 text-[11px] truncate leading-tight",
                    stateHeadline !== t('noAgentStateYet')
                      ? "text-[var(--color-text-secondary)]"
                      : "text-[var(--color-text-muted)] italic"
                  )}
                  title={stateHeadline}
                >
                  {stateHeadline}
                </div>
              </div>

              <div
                className={classNames(
                  "hidden sm:flex min-w-0 flex-1 items-start gap-3 rounded-2xl border px-4 py-3 shadow-sm backdrop-blur-sm",
                  isDark
                    ? "max-w-[min(760px,58vw)] border-white/10 bg-white/[0.04]"
                    : "max-w-[min(760px,58vw)] border-black/8 bg-white/90"
                )}
                aria-label={t('agentState')}
              >
                <div className="min-w-0 flex-1">
                  <div
                    className={classNames(
                      "min-w-0 text-sm font-medium leading-6",
                      stateHeadline !== t('noAgentStateYet')
                        ? "text-[var(--color-text-primary)]"
                        : isDark
                          ? "text-slate-500 italic"
                          : "text-gray-500 italic"
                    )}
                    style={statusClamp2Style}
                    title={
                      agentState?.updated_at
                        ? `${stateHeadline}\nUpdated: ${formatFullTime(agentState.updated_at)}`
                        : stateHeadline
                    }
                  >
                    <span>{stateHeadline}</span>
                  </div>
                  {(stateTask || blockerCount > 0 || stateNext) ? (
                    <div className="mt-2 flex flex-wrap items-center gap-1.5">
                      {stateTask ? (
                        <span className={classNames("rounded-full bg-[var(--glass-tab-bg)] px-2.5 py-1 text-[11px] text-[var(--color-text-secondary)]")}>
                          {t("taskShort", { id: stateTask })}
                        </span>
                      ) : null}
                      {blockerCount > 0 ? (
                        <span className={classNames("rounded-full bg-rose-500/15 px-2.5 py-1 text-[11px] text-rose-600 dark:text-rose-300")}>
                          {t("blockersShort", { count: blockerCount })}
                        </span>
                      ) : null}
                      {stateNext ? (
                        <span
                          className={classNames("truncate text-[11px]", "text-[var(--color-text-tertiary)]")}
                          title={stateNext}
                        >
                          {t("nextShort", { value: stateNext })}
                        </span>
                      ) : null}
                    </div>
                  ) : null}
                </div>
                {agentState?.updated_at ? (
                  <div className="shrink-0 rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] px-2.5 py-1 text-[10px] font-medium text-[var(--color-text-tertiary)]">
                    {formatTime(agentState.updated_at)}
                  </div>
                ) : null}
              </div>
            </div>
          </div>

        </div>
      </div>

      {/* Terminal or Status Area */}
      {/* contain: layout prevents terminal content changes from triggering parent layout recalculation */}
      <div className={classNames("flex-1 min-h-0 relative", "bg-[var(--color-bg-secondary)]")} style={{ contain: 'layout', overflow: 'hidden' }}>
        {isHeadless ? (
          <div className="flex h-full min-h-0 flex-col px-5 pb-5 pt-3 sm:px-7 sm:pb-6 sm:pt-3">
            <div className="mx-auto flex w-full max-w-6xl flex-col gap-4 min-h-0 flex-1">
              <div className="min-h-0 flex-1">
              {rawHeadlessEvents.length > 0 ? (
                <HeadlessRawTrace
                  events={rawHeadlessEvents}
                  emptyLabel={t('noStreamingOutputYet', { defaultValue: '当前还没有可显示的流式输出。' })}
                  isDark={isDark}
                  className={classNames(
                    "h-full min-h-[420px] text-left text-[var(--color-text-secondary)]"
                  )}
                />
              ) : (
                <HeadlessLiveTrace
                  previewSessions={headlessPreviewSessions}
                  fallbackText={latestHeadlessText}
                  fallbackActivities={latestHeadlessActivities}
                  fallbackUpdatedAt={String(latestHeadlessPreview?.updatedAt || "").trim()}
                  fallbackPendingEventId={String(latestHeadlessPreview?.pendingEventId || `preview:${actor.id}`).trim()}
                  fallbackStreamId={String(latestHeadlessPreview?.currentStreamId || "").trim()}
                  fallbackStreamPhase={String(latestHeadlessPreview?.streamPhase || "").trim().toLowerCase()}
                  fallbackPhase={String(latestHeadlessPreview?.phase || "").trim().toLowerCase()}
                  emptyLabel={t('noStreamingOutputYet', { defaultValue: '当前还没有可显示的流式输出。' })}
                  recentLabel="Recent"
                  isDark={isDark}
                  density="expanded"
                  className={classNames(
                    "h-full min-h-[420px] overflow-y-auto text-left text-[var(--color-text-secondary)]"
                  )}
                />
              )}
              </div>
            </div>
          </div>
        ) : isRunning ? (
          // PTY agent - show terminal
          // contain: layout paint isolates layout/paint calculations to prevent jitter when terminal content updates
          // opacity transition hides initial backlog replay scrolling
          <>
            <div
              ref={termRef}
              className="h-full w-full transition-opacity duration-100"
              style={{
                contain: 'layout paint',
                overflow: 'hidden',
                opacity: terminalReady ? 1 : 0,
              }}
            />
            {/* Connection error overlay — shown when all reconnect attempts failed and terminal never became ready */}
            {connectionStatus === 'disconnected' && !terminalReady && (
              <div className={classNames(
                "absolute inset-0 flex flex-col items-center justify-center p-8",
                "text-[var(--color-text-tertiary)] bg-[var(--glass-panel-bg)]"
              )}>
                <div className="mb-4"><TerminalIcon size={48} /></div>
                <div className="text-lg font-medium mb-2">{t('connectionLost')}</div>
                <div className="text-sm text-center max-w-md mb-4">
                  {t('connectionLostDescription')}
                </div>
                {canControl && (
                  <button
                    onClick={() => {
                      reconnectAttemptRef.current = 0;
                      actorNotRunningRef.current = false;
                      setReconnectTrigger((n) => n + 1);
                    }}
                    className="flex items-center gap-2 px-4 py-2 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white font-medium min-h-[44px] transition-colors"
                  >
                    <RefreshIcon size={16} />
                    {t('reconnect')}
                  </button>
                )}
              </div>
            )}
          </>
        ) : (
          // Stopped agent
          <div className={classNames("flex flex-col items-center h-full p-8 overflow-y-auto", "text-[var(--color-text-tertiary)]")}>
            <div className="flex flex-col items-center flex-shrink-0">
              <div className="mb-4"><TerminalIcon size={48} /></div>
              <div className="text-lg font-medium mb-2">{t('agentNotRunning')}</div>
              <div className="text-sm text-center max-w-md mb-4">
                {t('agentStoppedDescription')}
              </div>
              {canControl ? (
                <button
                  onClick={onLaunch}
                  disabled={isBusy}
                  className="flex items-center gap-2 px-4 py-2 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white font-medium disabled:opacity-50 min-h-[44px] transition-colors"
                  aria-label={t('launchAgentLabel')}
                >
                  <PlayIcon size={16} />
                  {isBusy ? t('launching') : t('launchAgent')}
                </button>
              ) : null}
            </div>
            <div className="mt-6 w-full max-w-xl flex-shrink-0 rounded-lg border border-dashed border-[var(--glass-border-subtle)] px-4 py-3 text-sm text-[var(--color-text-secondary)]">
              {t('noRecentTerminalOutput')}
            </div>
          </div>
        )}
      </div>

      {/* Action Buttons - Scrollable on mobile with fade edges */}
      {canControl ? (
        <ScrollFade
          className={classNames(
            "border-t select-none",
            "glass-header"
          )}
          innerClassName="flex items-center gap-2 px-4 py-3 sm:px-5"
          fadeWidth={20}
        >
          {isRunning ? (
            <>
              <button
                onClick={onQuit}
                disabled={isBusy}
                className={`${secondaryActionButtonClass} flex-shrink-0 whitespace-nowrap`}
                aria-label={t('quitAgent')}
              >
                <StopIcon size={16} />
                {!isSmallScreen && t('quit')}
              </button>
              <button
                onClick={sendInterrupt}
                disabled={connectionStatus !== 'connected'}
                className={`${ghostActionButtonClass} flex-shrink-0 whitespace-nowrap`}
                title={t('sendInterruptTitle')}
                aria-label={t('sendInterruptLabel')}
              >
                ⌃C
              </button>
              <button
                onClick={onRelaunch}
                disabled={isBusy}
                className={`${secondaryActionButtonClass} flex-shrink-0 whitespace-nowrap`}
                aria-label={t('relaunchAgent')}
              >
                <RefreshIcon size={16} />
                {!isSmallScreen && t('relaunch')}
              </button>
              <button
                onClick={onEdit}
                disabled={isBusy}
                className={`${ghostActionButtonClass} flex-shrink-0 whitespace-nowrap`}
                aria-label={t('editAgentConfig')}
              >
                <EditIcon size={16} />
                {!isSmallScreen && t('common:edit')}
              </button>
            </>
          ) : (
            <>
              <button
                onClick={onLaunch}
                disabled={isBusy}
                className={`${primaryActionButtonClass} flex-shrink-0 whitespace-nowrap`}
                aria-label={t('launchAgentLabel')}
              >
                <PlayIcon size={16} />
                {isBusy ? t('launching') : t('launch')}
              </button>
              <button
                onClick={onEdit}
                disabled={isBusy}
                className={ghostActionButtonClass}
                aria-label={t('editAgentConfig')}
              >
                <EditIcon size={16} /> {t('common:edit')}
              </button>
            </>
          )}
          <button
            onClick={onInbox}
            className={classNames(
              "ml-auto flex items-center gap-1.5 rounded-xl px-3.5 py-2.5 text-sm min-h-[44px] transition-colors flex-shrink-0 whitespace-nowrap border",
              unreadCount > 0
                ? isDark
                  ? "border-white/12 bg-white/[0.08] text-white hover:bg-white/[0.12]"
                  : "border-black/10 bg-[rgb(245,245,245)] text-[rgb(35,36,37)] hover:bg-white"
                : isDark
                  ? "border-white/10 bg-white/[0.06] hover:bg-white/[0.1] text-white"
                  : "border-black/10 bg-white hover:bg-[rgb(245,245,245)] text-[rgb(35,36,37)]"
            )}
            aria-label={`${t('openInbox')}${unreadCount > 0 ? t('unreadMessages', { count: unreadCount }) : ""}`}
          >
            <InboxIcon size={16} />
            {!isSmallScreen && t('inbox')}
            {unreadCount > 0 && (
              <span
                className={classNames(
                  "text-[10px] px-1.5 py-0.5 rounded-full font-semibold tracking-tight shadow-sm",
                  isDark ? "bg-white text-[rgb(20,20,22)]" : "bg-[rgb(35,36,37)] text-white"
                )}
                aria-hidden="true"
              >
                {unreadCount > 99 ? "99+" : unreadCount}
              </span>
            )}
          </button>
          <button
            onClick={onRemove}
            disabled={isBusy || isRunning}
            className={classNames(
              "flex items-center gap-1.5 rounded-xl px-3 py-2.5 text-sm disabled:opacity-50 min-h-[44px] transition-colors flex-shrink-0 whitespace-nowrap",
              "text-rose-600 hover:bg-rose-500/10 dark:text-rose-400"
            )}
            title={isRunning ? t('stopBeforeRemoving') : t('removeAgent')}
            aria-label={t('removeAgent')}
          >
            <TrashIcon size={16} />
            {!isSmallScreen && t('common:remove')}
          </button>
        </ScrollFade>
      ) : null}
    </div>
  );
}
