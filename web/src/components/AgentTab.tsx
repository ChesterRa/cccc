/* eslint-disable no-control-regex */
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { Terminal, ITheme } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { useTranslation } from "react-i18next";
import { Actor, PresenceAgent, getRuntimeColor, RUNTIME_INFO } from "../types";
import { getTerminalTheme } from "../hooks/useTheme";
import { classNames } from "../utils/classNames";
import { formatFullTime, formatTime } from "../utils/time";
import { useObservabilityStore } from "../stores";
import { withAuthToken } from "../services/api";
import { StopIcon, RefreshIcon, InboxIcon, TrashIcon, PlayIcon, EditIcon, RocketIcon, TerminalIcon } from "./Icons";
import { ScrollFade } from "./ScrollFade";

type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'reconnecting';

// Delay before showing terminal after connection (allows backlog replay to complete without visible scrolling)
const TERMINAL_SHOW_DELAY_MS = 150;

// WebSocket reconnect configuration (moved outside component to avoid recreation)
const RECONNECT_BASE_DELAY_MS = 1000;
const RECONNECT_MAX_DELAY_MS = 30000;
const MAX_RECONNECT_ATTEMPTS = 10;

interface AgentTabProps {
  actor: Actor;
  groupId: string;
  presenceAgent: PresenceAgent | null;
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
  presenceAgent,
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
  const isRunning = actor.running ?? actor.enabled ?? false;
  const isHeadless = actor.runner === "headless";
  const canControl = !readOnly;
  const terminalScrollbackLines = useObservabilityStore((s) => s.terminalScrollbackLines);

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

  // Best-effort terminal query responder state (per mounted actor tab).
  // Some runtimes (notably opencode) emit terminal *queries* that xterm.js doesn't answer (e.g. OSC 4 palette).
  // Without a reply, they can get stuck or render nothing on attach.
  const terminalReplyStateRef = useRef<{ osc4Idx: Set<string>; osc10Sent: boolean; osc11Sent: boolean }>({
    osc4Idx: new Set<string>(),
    osc10Sent: false,
    osc11Sent: false,
  });
  const pasteStateRef = useRef<{ inFlight: boolean; lastAt: number }>({ inFlight: false, lastAt: 0 });

  // WebSocket reconnect state
  const reconnectAttemptRef = useRef(0);
  const reconnectTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // Stop reconnecting if server reports actor is not running
  const actorNotRunningRef = useRef(false);

  // Ref to avoid stale closure in WebSocket callbacks
  const isRunningRef = useRef(isRunning);

  // Keep ref in sync with prop
  useEffect(() => {
    isRunningRef.current = isRunning;
    // Reset the "actor not running" flag when actor starts running again
    if (isRunning) {
      actorNotRunningRef.current = false;
    }
  }, [isRunning]);

  // Activate the terminal only after the user has visited this actor tab at least once.
  // Once activated, keep the PTY session connected even when the tab is hidden to avoid backlog replay and scroll jumps.
  useEffect(() => {
    if (isVisible) setActivated(true);
  }, [isVisible]);

  const copyToClipboard = async (text: string): Promise<boolean> => {
    const t = (text || "").toString();
    if (!t) return false;
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(t);
        return true;
      }
    } catch {
      // ignore
    }
    try {
      window.prompt("Copy to clipboard:", t);
      return true;
    } catch {
      return false;
    }
  };

  const color = getRuntimeColor(actor.runtime, isDark);
  const rtInfo = (actor.runtime && RUNTIME_INFO[actor.runtime]) ? RUNTIME_INFO[actor.runtime] : RUNTIME_INFO.codex;
  const unreadCount = actor.unread_count ?? 0;
  const statusClamp2Style: CSSProperties = {
    display: "-webkit-box",
    WebkitBoxOrient: "vertical",
    WebkitLineClamp: 2,
    overflow: "hidden",
  };

  const _hexToOscRgb = (hex: string): string => {
    const h = (hex || "").trim().toLowerCase();
    const m = /^#([0-9a-f]{6})$/.exec(h);
    if (!m) return "0000/0000/0000";
    const raw = m[1] || "000000";
    const r8 = parseInt(raw.slice(0, 2), 16);
    const g8 = parseInt(raw.slice(2, 4), 16);
    const b8 = parseInt(raw.slice(4, 6), 16);
    const to16 = (v8: number) => (v8 * 257).toString(16).padStart(4, "0");
    return `${to16(r8)}/${to16(g8)}/${to16(b8)}`;
  };

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
      if (isPaste && canControl) {
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

    // Initial fit
    setTimeout(() => fitAddon.fit(), 0);

    return () => {
      term.element?.removeEventListener("contextmenu", onContextMenu);
      term.element?.removeEventListener("mousedown", onPointerDown);
      term.element?.removeEventListener("touchstart", onPointerDown);
      term.dispose();
      terminalRef.current = null;
      fitAddonRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps -- Theme changes are handled in a dedicated effect; avoid re-creating the terminal.
  }, [isHeadless, isRunning, activated, canControl]);

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
      if (wsRef.current) {
        wsRef.current.close();
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
        // Reset responder state on each successful (re)connect.
        terminalReplyStateRef.current = { osc4Idx: new Set<string>(), osc10Sent: false, osc11Sent: false };
        // Reset output filter state on each successful (re)connect.
        outputFilterTailRef.current = "";

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

        // Send initial resize (ops mode only). Exhibit should be view-only and not affect PTY size.
        if (canControl) {
          const term = terminalRef.current;
          if (term) {
            ws.send(JSON.stringify({ t: "r", c: term.cols, r: term.rows }));
          }
        }
      };

      const _maybeReplyOpencodeQueries = (data: string) => {
        if (!canControl) return;
        if (actor.runtime !== "opencode" || ws.readyState !== WebSocket.OPEN || !terminalRef.current) return;

        // Prefer the terminal's current theme (keeps replies consistent even after theme toggles).
        const fallback = getTerminalTheme(isDark);
        const theme: ITheme = terminalRef.current.options.theme || fallback;

        const bg = (typeof theme.background === "string" ? theme.background : fallback.background) as string;
        const fg = (typeof theme.foreground === "string" ? theme.foreground : fallback.foreground) as string;
        const palette: string[] = [
          (typeof theme.black === "string" ? theme.black : fallback.black) as string,
          (typeof theme.red === "string" ? theme.red : fallback.red) as string,
          (typeof theme.green === "string" ? theme.green : fallback.green) as string,
          (typeof theme.yellow === "string" ? theme.yellow : fallback.yellow) as string,
          (typeof theme.blue === "string" ? theme.blue : fallback.blue) as string,
          (typeof theme.magenta === "string" ? theme.magenta : fallback.magenta) as string,
          (typeof theme.cyan === "string" ? theme.cyan : fallback.cyan) as string,
          (typeof theme.white === "string" ? theme.white : fallback.white) as string,
          (typeof theme.brightBlack === "string" ? theme.brightBlack : fallback.brightBlack) as string,
          (typeof theme.brightRed === "string" ? theme.brightRed : fallback.brightRed) as string,
          (typeof theme.brightGreen === "string" ? theme.brightGreen : fallback.brightGreen) as string,
          (typeof theme.brightYellow === "string" ? theme.brightYellow : fallback.brightYellow) as string,
          (typeof theme.brightBlue === "string" ? theme.brightBlue : fallback.brightBlue) as string,
          (typeof theme.brightMagenta === "string" ? theme.brightMagenta : fallback.brightMagenta) as string,
          (typeof theme.brightCyan === "string" ? theme.brightCyan : fallback.brightCyan) as string,
          (typeof theme.brightWhite === "string" ? theme.brightWhite : fallback.brightWhite) as string,
        ];

        const state = terminalReplyStateRef.current;

        // OSC 10/11 queries: ask for terminal default fg/bg.
        // Reply format: ESC ] 10/11 ; rgb:RRRR/GGGG/BBBB BEL
        if (!state.osc10Sent && /\x1b\]10;\?(?:\x07|\x1b\\)/.test(data)) {
          state.osc10Sent = true;
          ws.send(JSON.stringify({ t: "i", d: `\x1b]10;rgb:${_hexToOscRgb(fg)}\x07` }));
        }
        if (!state.osc11Sent && /\x1b\]11;\?(?:\x07|\x1b\\)/.test(data)) {
          state.osc11Sent = true;
          ws.send(JSON.stringify({ t: "i", d: `\x1b]11;rgb:${_hexToOscRgb(bg)}\x07` }));
        }

        // OSC 4 palette query: ESC ] 4 ; <idx> ; ? (BEL/ST)
        // xterm.js doesn't answer this query by default; some TUIs can stall waiting for it.
        const osc4 = /\x1b\]4;(\d+);[?](?:\x07|\x1b\\)/g;
        let m: RegExpExecArray | null = null;
        while ((m = osc4.exec(data)) !== null) {
          const idx = String(m[1] || "0");
          if (state.osc4Idx.has(idx)) continue;
          state.osc4Idx.add(idx);
          const n = Number.parseInt(idx, 10);
          if (Number.isFinite(n) && n >= 0 && n < palette.length) {
            ws.send(JSON.stringify({ t: "i", d: `\x1b]4;${idx};rgb:${_hexToOscRgb(palette[n])}\x07` }));
          } else {
            ws.send(JSON.stringify({ t: "i", d: `\x1b]4;${idx};rgb:0000/0000/0000\x07` }));
          }
        }
      };

      const _handleDecoded = (data: string) => {
        if (disposed) return;
        _maybeReplyOpencodeQueries(data);
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

        // Only auto-reconnect if not a clean close (code 1000)
        // Use ref to get latest prop value (avoid stale closure)
        // Also skip reconnect if server reported actor is not running
        if (event.code !== 1000 && isRunningRef.current && !isHeadless && !actorNotRunningRef.current) {
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
      if (term && canControl) {
        disposable = term.onData((data) => {
          if (ws.readyState === WebSocket.OPEN) {
            // xterm.js can emit terminal replies (not user keystrokes), e.g. device attributes / color queries.
            // Some runtimes can echo these back as literal text (seen as "1;2c" or "]11;rgb:..."), so filter for those.
            // Keep the filter runtime-scoped to avoid interfering with full-screen TUIs that may rely on terminal queries.
            if (actor.runtime === "droid" || actor.runtime === "gemini" || actor.runtime === "kilocode" || actor.runtime === "copilot" || actor.runtime === "neovate") {
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
            ws.send(JSON.stringify({ t: "i", d: data }));
          }
        });

        // Handle terminal resize - send as JSON with type "r" (resize)
        resizeDisposable = term.onResize(({ cols, rows }) => {
          if (ws.readyState === WebSocket.OPEN) {
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
  }, [activated, isRunning, isHeadless, groupId, actor.id, actor.runtime, canControl]);

  // Fit terminal on visibility change and resize (with debounce to reduce jitter)
  useEffect(() => {
    if (!isVisible || !fitAddonRef.current) return;

    let resizeTimeout: ReturnType<typeof setTimeout> | null = null;

    const fit = () => {
      if (fitAddonRef.current) {
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
    return () => {
      window.removeEventListener("resize", debouncedFit);
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

  return (
    <div className="flex flex-col h-full">
      {/* Agent Header */}
      <div className={classNames(
        "flex items-center gap-4 px-4 py-3 border-b",
        color.border, color.bg
      )}>
        <span
          className={classNames(
            "w-3 h-3 rounded-full flex-shrink-0",
            isRunning ? "bg-emerald-500" : isDark ? "bg-slate-600" : "bg-gray-400"
          )}
        />
        <div className="flex items-start gap-3 min-w-0">
          <div className="min-w-0">
            <div className="flex items-center gap-2 min-w-0">
              <span className={classNames("font-semibold truncate min-w-0", color.text)}>{actor.title || actor.id}</span>
              {actor.role === "foreman" && (
                <span className="text-[10px] px-1.5 py-0.5 rounded bg-amber-900/50 text-amber-300 font-medium">
                  {t('foreman')}
                </span>
              )}
            </div>
            <div className={classNames("mt-0.5 text-xs truncate", isDark ? "text-slate-400" : "text-gray-500")}>
              {rtInfo?.label || t('custom')} • {isRunning ? t('running') : t('stopped')}
              {isHeadless && ` • ${t('headless')}`}
            </div>
            {/* Mobile-only: condensed single-line presence */}
            <div
              className={classNames(
                "sm:hidden mt-1 text-[11px] truncate leading-tight",
                presenceAgent?.status
                  ? isDark ? "text-slate-300" : "text-gray-600"
                  : isDark ? "text-slate-500 italic" : "text-gray-400 italic"
              )}
              title={presenceAgent?.status || t('noPresenceYet')}
            >
              {presenceAgent?.status || t('noPresenceYet')}
            </div>
          </div>

          <div
            className={classNames(
              "hidden sm:flex flex-shrink-0 px-3 py-2 rounded-xl border shadow-sm backdrop-blur-sm max-w-[min(420px,36vw)]",
              isDark ? "bg-slate-950/30 border-white/10" : "bg-white/70 border-black/10"
            )}
            aria-label={t('agentPresence')}
          >
            <div
              className={classNames(
                "text-xs font-medium leading-snug min-w-0",
                presenceAgent?.status
                  ? isDark
                    ? "text-slate-200"
                    : "text-gray-800"
                  : isDark
                    ? "text-slate-500 italic"
                    : "text-gray-500 italic"
              )}
              style={statusClamp2Style}
              title={
                presenceAgent?.updated_at
                  ? `${presenceAgent?.status || t('noPresenceYet')}\nUpdated: ${formatFullTime(presenceAgent.updated_at)}`
                  : (presenceAgent?.status || t('noPresenceYet'))
              }
            >
              <span>{presenceAgent?.status ? presenceAgent.status : t('noPresenceYet')}</span>
              {presenceAgent?.updated_at ? (
                <span className={classNames("ml-2 text-[11px] tabular-nums font-normal", isDark ? "text-slate-400" : "text-gray-600")}>
                  · {formatTime(presenceAgent.updated_at)}
                </span>
              ) : null}
            </div>
          </div>
        </div>
      </div>

      {/* Terminal or Status Area */}
      {/* contain: layout prevents terminal content changes from triggering parent layout recalculation */}
      <div className={classNames("flex-1 min-h-0", isDark ? "bg-slate-950" : "bg-gray-50")} style={{ contain: 'layout', overflow: 'hidden' }}>
        {isHeadless ? (
          // Headless agent - show status
          <div className={classNames("flex flex-col items-center justify-center h-full p-8", isDark ? "text-slate-400" : "text-slate-500")}>
            <div className="mb-4"><RocketIcon size={48} /></div>
            <div className="text-lg font-medium mb-2">{t('headlessAgent')}</div>
            <div className="text-sm text-center max-w-md">
              {t('headlessDescription')}
            </div>
            {isRunning && (
              <div className={classNames("mt-4 px-3 py-1.5 rounded text-sm", isDark ? "bg-emerald-900/30 text-emerald-300" : "bg-emerald-50 text-emerald-600")}>
                {t('statusRunning')}
              </div>
            )}
          </div>
        ) : isRunning ? (
          // PTY agent - show terminal
          // contain: layout paint isolates layout/paint calculations to prevent jitter when terminal content updates
          // opacity transition hides initial backlog replay scrolling
          <div
            ref={termRef}
            className="h-full w-full transition-opacity duration-100"
            style={{
              contain: 'layout paint',
              overflow: 'hidden',
              opacity: terminalReady ? 1 : 0,
            }}
          />
        ) : (
          // Stopped agent
          <div className={classNames("flex flex-col items-center justify-center h-full p-8", isDark ? "text-slate-400" : "text-slate-500")}>
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
        )}
      </div>

      {/* Action Buttons - Scrollable on mobile with fade edges */}
      {canControl ? (
        <ScrollFade
          className={classNames(
            "border-t select-none",
            isDark ? "bg-slate-900/50 border-slate-800" : "bg-gray-100 border-gray-200"
          )}
          innerClassName="flex items-center gap-2 px-4 py-3"
          fadeWidth={20}
        >
          {isRunning ? (
            <>
              <button
                onClick={onQuit}
                disabled={isBusy}
                className={classNames(
                  "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm disabled:opacity-50 min-h-[44px] transition-colors flex-shrink-0 whitespace-nowrap",
                  isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-300"
                )}
                aria-label={t('quitAgent')}
              >
                <StopIcon size={16} />
                {!isSmallScreen && t('quit')}
              </button>
              <button
                onClick={sendInterrupt}
                disabled={connectionStatus !== 'connected'}
                className={classNames(
                  "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm disabled:opacity-50 min-h-[44px] transition-colors flex-shrink-0 whitespace-nowrap",
                  isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-300"
                )}
                title={t('sendInterruptTitle')}
                aria-label={t('sendInterruptLabel')}
              >
                ⌃C
              </button>
              <button
                onClick={onRelaunch}
                disabled={isBusy}
                className={classNames(
                  "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm disabled:opacity-50 min-h-[44px] transition-colors flex-shrink-0 whitespace-nowrap",
                  isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-300"
                )}
                aria-label={t('relaunchAgent')}
              >
                <RefreshIcon size={16} />
                {!isSmallScreen && t('relaunch')}
              </button>
              <button
                onClick={onEdit}
                disabled={isBusy}
                className={classNames(
                  "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm disabled:opacity-50 min-h-[44px] transition-colors flex-shrink-0 whitespace-nowrap",
                  isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-300"
                )}
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
                className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-emerald-700 hover:bg-emerald-600 text-white text-sm disabled:opacity-50 min-h-[44px] transition-colors flex-shrink-0 whitespace-nowrap"
                aria-label={t('launchAgentLabel')}
              >
                <PlayIcon size={16} />
                {isBusy ? t('launching') : t('launch')}
              </button>
              <button
                onClick={onEdit}
                disabled={isBusy}
                className={classNames(
                  "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm disabled:opacity-50 min-h-[44px] transition-colors",
                  isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-300"
                )}
                aria-label={t('editAgentConfig')}
              >
                <EditIcon size={16} /> {t('common:edit')}
              </button>
            </>
          )}
          <button
            onClick={onInbox}
            className={classNames(
              "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm min-h-[44px] transition-colors flex-shrink-0 whitespace-nowrap",
              unreadCount > 0
                ? isDark
                  ? "bg-indigo-500/10 text-indigo-200 border border-indigo-500/20 hover:bg-indigo-500/15"
                  : "bg-indigo-50 text-indigo-700 border border-indigo-200 hover:bg-indigo-100"
                : isDark
                  ? "bg-slate-800 hover:bg-slate-700 text-slate-200"
                  : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-300"
            )}
            aria-label={`${t('openInbox')}${unreadCount > 0 ? t('unreadMessages', { count: unreadCount }) : ""}`}
          >
            <InboxIcon size={16} />
            {!isSmallScreen && t('inbox')}
            {unreadCount > 0 && (
              <span
                className={classNames(
                  "text-white text-[10px] px-1.5 py-0.5 rounded-full font-semibold tracking-tight shadow-sm",
                  isDark ? "bg-indigo-500" : "bg-indigo-600"
                )}
                aria-hidden="true"
              >
                {unreadCount > 99 ? "99+" : unreadCount}
              </span>
            )}
          </button>
          <div className="flex-1" />
          <button
            onClick={onRemove}
            disabled={isBusy || isRunning}
            className={classNames(
              "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm disabled:opacity-50 min-h-[44px] transition-colors flex-shrink-0 whitespace-nowrap",
              isDark ? "hover:bg-rose-900/30 text-rose-400" : "hover:bg-rose-50 text-rose-600"
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
