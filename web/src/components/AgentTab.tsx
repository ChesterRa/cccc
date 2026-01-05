import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { Actor, getRuntimeColor, RUNTIME_INFO } from "../types";
import { getTerminalTheme } from "../hooks/useTheme";
import { classNames } from "../utils/classNames";
import { StopIcon, RefreshIcon, InboxIcon, TrashIcon, PlayIcon, EditIcon, RocketIcon, TerminalIcon } from "./Icons";

type ConnectionStatus = 'disconnected' | 'connecting' | 'connected' | 'reconnecting';

// WebSocket reconnect configuration (moved outside component to avoid recreation)
const RECONNECT_BASE_DELAY_MS = 1000;
const RECONNECT_MAX_DELAY_MS = 30000;
const MAX_RECONNECT_ATTEMPTS = 10;

interface AgentTabProps {
  actor: Actor;
  groupId: string;
  isVisible: boolean;
  onQuit: () => void;
  onLaunch: () => void;
  onRelaunch: () => void;
  onEdit: () => void;
  onRemove: () => void;
  onInbox: () => void;
  busy: string;
  isDark: boolean;
  /** Called when the component detects actor status may have changed (e.g., process exited) */
  onStatusChange?: () => void;
}

export function AgentTab({
  actor,
  groupId,
  isVisible,
  onQuit,
  onLaunch,
  onRelaunch,
  onEdit,
  onRemove,
  onInbox,
  busy,
  isDark,
  onStatusChange,
}: AgentTabProps) {
  // Derived state (must be defined before refs that use them)
  const isRunning = actor.running ?? actor.enabled ?? false;
  const isHeadless = actor.runner === "headless";

  const termRef = useRef<HTMLDivElement>(null);
  const terminalRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const [connectionStatus, setConnectionStatus] = useState<ConnectionStatus>('disconnected');

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

  // Initialize terminal
  useEffect(() => {
    if (!termRef.current || isHeadless || !isRunning) return;

    const term = new Terminal({
      cursorBlink: true,
      // Avoid an extra blinking "outline" cursor when the terminal isn't focused.
      // Some runtimes render their own cursor; xterm's inactive cursor can look like a second cursor.
      cursorInactiveStyle: "none",
      fontSize: 13,
      fontFamily: '"JetBrains Mono", "Fira Code", "SF Mono", Menlo, Monaco, monospace',
      theme: getTerminalTheme(isDark),
      // Bigger scrollback improves history browsing without going "infinite" and hurting perf.
      // 8k lines is a good default for multi-agent (memory scales with #opened terminals).
      scrollback: 8000,
      allowProposedApi: true,
    });

    const fitAddon = new FitAddon();
    
    term.loadAddon(fitAddon);
    term.open(termRef.current);
    // Ensure focus works consistently across browsers (and prevents the inactive cursor style).
    const onPointerDown = () => term.focus();
    term.element?.addEventListener("mousedown", onPointerDown);
    term.element?.addEventListener("touchstart", onPointerDown);

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
      if (isPaste) {
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
  }, [isHeadless, isRunning]);

  // Connect WebSocket when visible and running (with auto-reconnect).
  useEffect(() => {
    if (!isVisible || !isRunning || isHeadless || !terminalRef.current) return;

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

      const ws = new WebSocket(wsUrl);
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

        // Send initial resize
        const term = terminalRef.current;
        if (term) {
          ws.send(JSON.stringify({ t: "r", c: term.cols, r: term.rows }));
        }
      };

      const _maybeReplyOpencodeQueries = (data: string) => {
        if (actor.runtime !== "opencode" || ws.readyState !== WebSocket.OPEN || !terminalRef.current) return;

        // Prefer the terminal's current theme (keeps replies consistent even after theme toggles).
        const fallback = getTerminalTheme(isDark);
        const theme = (terminalRef.current.options.theme || fallback) as any;

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
        try {
          term.write(data);
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

      ws.onerror = (error) => {
        // onclose will be called after onerror, reconnect logic is handled there
      };

      // Handle terminal input - send as JSON with type "i" (input)
      const term = terminalRef.current;
      if (term) {
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
    };
  }, [isVisible, isRunning, isHeadless, groupId, actor.id, actor.runtime]);

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

  const isBusy = busy.includes(actor.id);

  return (
    <div className="flex flex-col h-full">
      {/* Agent Header */}
      <div className={classNames(
        "flex items-center justify-between px-4 py-3 border-b",
        color.border, color.bg
      )}>
        <div className="flex items-center gap-3">
          <span className={classNames("w-3 h-3 rounded-full", isRunning ? "bg-emerald-500" : isDark ? "bg-slate-600" : "bg-gray-400")} />
          <div>
            <div className="flex items-center gap-2">
              <span className={classNames("font-semibold", color.text)}>{actor.id}</span>
              {actor.role === "foreman" && (
                <span className="text-[10px] px-1.5 py-0.5 rounded bg-amber-900/50 text-amber-300 font-medium">
                  foreman
                </span>
              )}
            </div>
            <div className="text-xs text-slate-400">
              {rtInfo?.label || "Custom"} • {isRunning ? "Running" : "Stopped"}
              {isHeadless && " • Headless"}
            </div>
          </div>
        </div>
        <div className="flex items-center gap-2 text-xs text-slate-400">
          {connectionStatus === 'connected' && <span className="text-emerald-400">● Connected</span>}
          {connectionStatus === 'connecting' && <span className="text-yellow-400">○ Connecting...</span>}
          {connectionStatus === 'reconnecting' && <span className="text-yellow-400">↻ Reconnecting...</span>}
        </div>
      </div>

      {/* Terminal or Status Area */}
      {/* contain: layout prevents terminal content changes from triggering parent layout recalculation */}
      <div className={classNames("flex-1 min-h-0", isDark ? "bg-slate-950" : "bg-gray-50")} style={{ contain: 'layout', overflow: 'hidden' }}>
        {isHeadless ? (
          // Headless agent - show status
          <div className="flex flex-col items-center justify-center h-full text-slate-400 dark:text-slate-400 p-8">
            <div className="mb-4"><RocketIcon size={48} /></div>
            <div className="text-lg font-medium mb-2">Headless Agent</div>
            <div className="text-sm text-center max-w-md">
              This agent runs without a terminal. It communicates via MCP tools and the inbox system.
            </div>
            {isRunning && (
              <div className="mt-4 px-3 py-1.5 rounded bg-emerald-900/30 dark:bg-emerald-900/30 text-emerald-600 dark:text-emerald-300 text-sm">
                Status: Running
              </div>
            )}
          </div>
        ) : isRunning ? (
          // PTY agent - show terminal
          // contain: layout paint isolates layout/paint calculations to prevent jitter when terminal content updates
          <div ref={termRef} className="h-full w-full" style={{ contain: 'layout paint', overflow: 'hidden' }} />
        ) : (
          // Stopped agent
          <div className="flex flex-col items-center justify-center h-full text-slate-500 dark:text-slate-400 p-8">
            <div className="mb-4"><TerminalIcon size={48} /></div>
            <div className="text-lg font-medium mb-2">Agent Not Running</div>
            <div className="text-sm text-center max-w-md mb-4">
              Click Launch to start this agent's terminal session.
            </div>
            <button
              onClick={onLaunch}
              disabled={isBusy}
              className="flex items-center gap-2 px-4 py-2 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white font-medium disabled:opacity-50 min-h-[44px] transition-colors"
              aria-label="Launch agent"
            >
              <PlayIcon size={16} />
              {isBusy ? "Launching..." : "Launch Agent"}
            </button>
          </div>
        )}
      </div>

      {/* Action Buttons */}
      <div className={classNames(
        "flex items-center gap-2 px-4 py-3 border-t",
        isDark ? "bg-slate-900/50 border-slate-800" : "bg-gray-100 border-gray-200"
      )}>
        {isRunning ? (
          <>
            <button
              onClick={onQuit}
              disabled={isBusy}
              className={classNames(
                "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm disabled:opacity-50 min-h-[44px] transition-colors",
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-300"
              )}
              aria-label="Quit agent"
            >
              <StopIcon size={16} /> Quit
            </button>
            <button
              onClick={sendInterrupt}
              disabled={connectionStatus !== 'connected'}
              className={classNames(
                "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm disabled:opacity-50 min-h-[44px] transition-colors",
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-300"
              )}
              title="Send Ctrl+C to interrupt"
              aria-label="Send interrupt signal"
            >
              ⌃C
            </button>
            <button
              onClick={onRelaunch}
              disabled={isBusy}
              className={classNames(
                "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm disabled:opacity-50 min-h-[44px] transition-colors",
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-300"
              )}
              aria-label="Relaunch agent"
            >
              <RefreshIcon size={16} /> Relaunch
            </button>
          </>
        ) : (
          <>
            <button
              onClick={onLaunch}
              disabled={isBusy}
              className="flex items-center gap-1.5 px-3 py-2 rounded-lg bg-emerald-700 hover:bg-emerald-600 text-white text-sm disabled:opacity-50 min-h-[44px] transition-colors"
              aria-label="Launch agent"
            >
              <PlayIcon size={16} /> Launch
            </button>
            <button
              onClick={onEdit}
              disabled={isBusy}
              className={classNames(
                "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm disabled:opacity-50 min-h-[44px] transition-colors",
                isDark ? "bg-slate-800 hover:bg-slate-700 text-slate-200" : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-300"
              )}
              aria-label="Edit agent configuration"
            >
              <EditIcon size={16} /> Edit
            </button>
          </>
        )}
        <button
          onClick={onInbox}
          className={classNames(
            "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm min-h-[44px] transition-colors",
            unreadCount > 0
              ? isDark
                ? "bg-indigo-500/10 text-indigo-200 border border-indigo-500/20 hover:bg-indigo-500/15"
                : "bg-indigo-50 text-indigo-700 border border-indigo-200 hover:bg-indigo-100"
              : isDark 
                ? "bg-slate-800 hover:bg-slate-700 text-slate-200" 
                : "bg-white hover:bg-gray-50 text-gray-700 border border-gray-300"
          )}
          aria-label={`Open inbox${unreadCount > 0 ? `, ${unreadCount} unread messages` : ""}`}
        >
          <InboxIcon size={16} /> Inbox
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
            "flex items-center gap-1.5 px-3 py-2 rounded-lg text-sm disabled:opacity-50 min-h-[44px] transition-colors",
            isDark ? "hover:bg-rose-900/30 text-rose-400" : "hover:bg-rose-50 text-rose-600"
          )}
          title={isRunning ? "Stop the agent before removing" : "Remove agent"}
          aria-label="Remove agent"
        >
          <TrashIcon size={16} /> Remove
        </button>
      </div>
    </div>
  );
}
