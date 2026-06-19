import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import type { Terminal } from "@xterm/xterm";

import { fetchTerminalTail, withAuthToken } from "../../services/api";
import type { TerminalSignal } from "../../stores/useTerminalSignalsStore";
import { getTerminalSignalFromChunk } from "../../utils/terminalWorkingState";
import {
  buildTerminalWebSocketUrl,
  buildTerminalConnectionKey,
  decodeTerminalJsonFrame,
  encodeTerminalInputFrame,
  encodeTerminalResizeFrame,
  isTerminalAttachNonRetryableErrorCode,
  isTerminalAttachStartupRaceErrorCode,
  parseTerminalBinaryFrame,
  shouldSuppressTerminalAttachErrorOutput,
  shouldSuppressTerminalGeneratedInput,
} from "../../utils/terminalConnection";

export type AgentTerminalConnectionStatus = "disconnected" | "connecting" | "connected" | "reconnecting";

const TERMINAL_SHOW_DELAY_MS = 150;
const RECONNECT_BASE_DELAY_MS = 1000;
const RECONNECT_MAX_DELAY_MS = 30000;
const MAX_RECONNECT_ATTEMPTS = 10;
const STARTUP_RACE_RECONNECT_DELAY_MS = 750;

export function useAgentTerminalConnection(args: {
  activated: boolean;
  isRunning: boolean;
  isHeadless: boolean;
  groupId: string;
  actorId: string;
  actorRuntime: string | undefined;
  canControl: boolean;
  termEpoch: number;
  reconnectTrigger: number;
  terminalRef: RefObject<Terminal | null>;
  fitBeforeAttach?: () => void;
  onStatusChange?: () => void;
  setTerminalSignal: (groupId: string, actorId: string, signal: TerminalSignal) => void;
  clearTerminalSignal: (groupId: string, actorId: string) => void;
  setReconnectTrigger: (updater: (value: number) => number) => void;
}) {
  const {
    activated,
    isRunning,
    isHeadless,
    groupId,
    actorId,
    actorRuntime,
    canControl,
    termEpoch,
    reconnectTrigger,
    terminalRef,
    fitBeforeAttach,
    onStatusChange,
    setTerminalSignal,
    clearTerminalSignal,
    setReconnectTrigger,
  } = args;

  const [connectionStatus, setConnectionStatus] = useState<AgentTerminalConnectionStatus>("disconnected");
  const [terminalReady, setTerminalReady] = useState(false);
  const [terminalWritable, setTerminalWritable] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectAttemptRef = useRef(0);
  const reconnectTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const terminalReadyTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const outputFilterTailRef = useRef("");
  const terminalSignalBufferRef = useRef("");
  const terminalAttachNoRetryRef = useRef(false);
  const terminalAttachStartupRaceRef = useRef(false);
  const lastTermEpochRef = useRef(termEpoch);

  const isRunningRef = useRef(isRunning);
  const runtimeRef = useRef(actorRuntime);
  const canControlRef = useRef(canControl);
  const onStatusChangeRef = useRef(onStatusChange);
  const setTerminalSignalRef = useRef(setTerminalSignal);
  const clearTerminalSignalRef = useRef(clearTerminalSignal);

  useEffect(() => {
    isRunningRef.current = isRunning;
    runtimeRef.current = actorRuntime;
    canControlRef.current = canControl;
    onStatusChangeRef.current = onStatusChange;
    setTerminalSignalRef.current = setTerminalSignal;
    clearTerminalSignalRef.current = clearTerminalSignal;
    if (isRunning) {
      terminalAttachNoRetryRef.current = false;
      terminalAttachStartupRaceRef.current = false;
    }
    if (!isRunning || isHeadless || !canControl) {
      const timer = window.setTimeout(() => setTerminalWritable(false), 0);
      return () => window.clearTimeout(timer);
    }
  }, [actorRuntime, canControl, clearTerminalSignal, isHeadless, isRunning, onStatusChange, setTerminalSignal]);

  useEffect(() => {
    if (isRunning && !isHeadless) return;
    terminalSignalBufferRef.current = "";
    clearTerminalSignalRef.current(groupId, actorId);
  }, [actorId, groupId, isHeadless, isRunning]);

  const requestReconnect = useCallback(() => {
    reconnectAttemptRef.current = 0;
    terminalAttachNoRetryRef.current = false;
    setReconnectTrigger((n) => n + 1);
  }, [setReconnectTrigger]);

  const sendInterrupt = useCallback(() => {
    if (!canControlRef.current) return;
    const ws = wsRef.current;
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(encodeTerminalInputFrame("\x03"));
  }, []);

  const terminalConnectionKey = buildTerminalConnectionKey({
    activated,
    isRunning,
    isHeadless,
    groupId,
    actorId,
    reconnectTrigger,
    canControl,
  });

  useEffect(() => {
    if (!activated || !isRunning || isHeadless || !terminalRef.current) return;

    if (reconnectTimeoutRef.current) {
      clearTimeout(reconnectTimeoutRef.current);
      reconnectTimeoutRef.current = null;
    }
    reconnectAttemptRef.current = 0;
    terminalAttachNoRetryRef.current = false;
    terminalAttachStartupRaceRef.current = false;

    let disposed = false;
    let disposable: { dispose: () => void } | null = null;
    let resizeDisposable: { dispose: () => void } | null = null;
    // Absolute offset (in raw PTY bytes) of everything delivered so far this
    // mount. null until the first attach. The first attach replays the full
    // backlog (current screen); transient reconnects resume from this exact
    // cursor so output produced while disconnected is delivered, not skipped,
    // and the whole ring isn't re-streamed/re-cleared on every blip.
    let deliveredCursor: number | null = null;

    // Seed the delivered-byte cursor from the daemon's attach frame. If the daemon
    // replayed from earlier than we asked (our cursor fell out of the ring buffer),
    // reset so the replay rebuilds the screen cleanly instead of appending onto
    // stale content.
    const seedCursorFromAttach = (
      result: Record<string, unknown>,
      current: number | null,
    ): number | null => {
      const rc = Number(result?.replay_cursor);
      if (!Number.isFinite(rc)) return current;
      if (current !== null && rc > current) {
        try {
          terminalRef.current?.reset();
        } catch {
          // ignore
        }
      }
      return rc;
    };

    const connect = () => {
      if (disposed) return;
      const existingWs = wsRef.current;
      if (existingWs && (existingWs.readyState === WebSocket.OPEN || existingWs.readyState === WebSocket.CONNECTING)) {
        return;
      }

      if (disposable) {
        disposable.dispose();
        disposable = null;
      }
      if (resizeDisposable) {
        resizeDisposable.dispose();
        resizeDisposable = null;
      }

      if (existingWs) {
        existingWs.close();
        wsRef.current = null;
      }

      setConnectionStatus("connecting");

      const openWebSocket = (isFirstAttach: boolean) => {
        if (disposed) return;
        // First attach (since=null): full backlog replay so xterm — a real
        // terminal emulator — reconstructs the current screen (wide chars, SGR,
        // alt-screen). We let xterm own all emulation instead of pre-rendering a
        // lossy snapshot. Reconnect (since=deliveredCursor): the daemon sends
        // exactly the bytes produced since we left off, so nothing is skipped and
        // the whole ring isn't re-streamed.
        const wsUrl = buildTerminalWebSocketUrl({
          protocol: window.location.protocol,
          host: window.location.host,
          groupId,
          actorId,
          since: isFirstAttach ? null : deliveredCursor,
          mode: canControlRef.current ? "control" : "viewer",
          takeover: canControlRef.current,
        });

        const ws = new WebSocket(withAuthToken(wsUrl));
        ws.binaryType = "arraybuffer";
        wsRef.current = ws;

        ws.onopen = () => {
          if (disposed) {
            ws.close(1000, "Component unmounted during connection");
            return;
          }
          setConnectionStatus("connected");
          setTerminalWritable(false);
          reconnectAttemptRef.current = 0;
          outputFilterTailRef.current = "";
          terminalSignalBufferRef.current = "";
          // Reset only on the first (full-backlog) attach so the replay rebuilds
          // the exact current screen with no duplicated scrollback. Reconnects are
          // tail-only and must NOT reset — that would clear the user's screen and
          // scroll position on every transient websocket blip.
          if (isFirstAttach) {
            try {
              terminalRef.current?.reset();
            } catch {
              // ignore
            }
          }

          if (terminalReadyTimeoutRef.current) {
            clearTimeout(terminalReadyTimeoutRef.current);
          }
          setTerminalReady(false);
          terminalReadyTimeoutRef.current = setTimeout(() => {
            if (!disposed) setTerminalReady(true);
          }, TERMINAL_SHOW_DELAY_MS);

          void fetchTerminalTail(groupId, actorId, 4000, true, true)
            .then((resp) => {
              if (disposed || !resp.ok) return;
              const tailText = String(resp.result?.text || "");
              const signal = getTerminalSignalFromChunk("", tailText, runtimeRef.current);
              terminalSignalBufferRef.current = signal.nextBuffer;
              if (signal.signalKind) {
                setTerminalSignalRef.current(groupId, actorId, {
                  kind: signal.signalKind,
                  updatedAt: Date.now(),
                });
                return;
              }
              clearTerminalSignalRef.current(groupId, actorId);
            })
            .catch(() => {
              if (disposed) return;
            });

          if (canControlRef.current) {
            const term = terminalRef.current;
            if (term && term.cols >= 10 && term.rows >= 2) {
              ws.send(encodeTerminalResizeFrame(term.cols, term.rows));
            }
          }
        };

        const handleDecoded = (data: string) => {
          if (disposed) return;
          const term = terminalRef.current;
          if (!term) return;
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
          const signal = getTerminalSignalFromChunk(terminalSignalBufferRef.current, safe, runtimeRef.current);
          terminalSignalBufferRef.current = signal.nextBuffer;
          if (signal.signalKind) {
            setTerminalSignalRef.current(groupId, actorId, {
              kind: signal.signalKind,
              updatedAt: Date.now(),
            });
          }
          try {
            if (safe) term.write(safe);
          } catch (err) {
            console.error("terminal write failed", err);
          }
        };

        ws.onmessage = (event) => {
          if (disposed) return;

          if (event.data instanceof ArrayBuffer) {
            const frame = parseTerminalBinaryFrame(event.data);
            if (!frame) {
              if (deliveredCursor !== null) deliveredCursor += event.data.byteLength;
              handleDecoded(new TextDecoder().decode(event.data));
              return;
            }
            if (frame.type === "output") {
              // Advance the delivered-byte cursor by the raw PTY bytes received
              // (matches the daemon's offset accounting) so reconnects can resume
              // from exactly here.
              if (deliveredCursor !== null) deliveredCursor += frame.payload.byteLength;
              handleDecoded(new TextDecoder().decode(frame.payload));
              return;
            }
            if (frame.type === "attach") {
              const result = decodeTerminalJsonFrame<Record<string, unknown>>(frame.payload) || {};
              deliveredCursor = seedCursorFromAttach(result, deliveredCursor);
              const writable = Boolean(result.terminal_writable);
              setTerminalWritable(writable);
              if (canControlRef.current && !writable) {
                handleDecoded("\r\n[terminal] read-only connection; reconnect to take control.\r\n");
              }
              return;
            }
            if (frame.type === "input_ack") {
              const msg = decodeTerminalJsonFrame<{ ok?: boolean; error?: { message?: string } }>(frame.payload);
              if (msg?.ok === false) {
                const message = String(msg.error?.message || "Terminal input was rejected.");
                handleDecoded(`\r\n[terminal] ${message}\r\n`);
              }
              return;
            }
          } else if (event.data instanceof Blob) {
            void event.data.arrayBuffer().then((buf) => {
              if (deliveredCursor !== null) deliveredCursor += buf.byteLength;
              handleDecoded(new TextDecoder().decode(buf));
            });
          } else if (typeof event.data === "string") {
            try {
              const msg = JSON.parse(event.data);
              if (msg.type === "terminal.attach" && msg.ok === true) {
                const result = msg.result && typeof msg.result === "object" ? msg.result : {};
                deliveredCursor = seedCursorFromAttach(result, deliveredCursor);
                const writable = Boolean(result.terminal_writable);
                setTerminalWritable(writable);
                if (canControlRef.current && !writable) {
                  handleDecoded("\r\n[terminal] read-only connection; reconnect to take control.\r\n");
                }
                return;
              }
              if (msg.type === "terminal.input_ack" && msg.ok === false) {
                const message = String(msg.error?.message || "Terminal input was rejected.");
                handleDecoded(`\r\n[terminal] ${message}\r\n`);
                return;
              }
              if (msg.ok === false && msg.error) {
                const code = String(msg.error.code || "").trim();
                if (!shouldSuppressTerminalAttachErrorOutput(code)) {
                  handleDecoded(`\r\n[error] ${msg.error.message || "Unknown error"}\r\n`);
                }
                if (isTerminalAttachNonRetryableErrorCode(code)) {
                  terminalAttachNoRetryRef.current = true;
                }
                if (isTerminalAttachStartupRaceErrorCode(code)) {
                  terminalAttachStartupRaceRef.current = true;
                }
                onStatusChangeRef.current?.();
              }
            } catch {
              if (deliveredCursor !== null) deliveredCursor += new TextEncoder().encode(event.data).length;
              handleDecoded(event.data);
            }
          }
        };

        ws.onclose = (event) => {
          if (disposed) return;
          wsRef.current = null;
          const noRetry = event.code === 1000 || event.code === 4401 || terminalAttachNoRetryRef.current;

          if (!noRetry && isRunningRef.current && !isHeadless) {
            const startupRace = terminalAttachStartupRaceRef.current;
            const attempt = startupRace ? 0 : reconnectAttemptRef.current;
            if (!startupRace && attempt >= MAX_RECONNECT_ATTEMPTS) {
              setConnectionStatus("disconnected");
              return;
            }

            const delay = startupRace
              ? STARTUP_RACE_RECONNECT_DELAY_MS
              : Math.min(RECONNECT_BASE_DELAY_MS * Math.pow(2, attempt), RECONNECT_MAX_DELAY_MS);
            setConnectionStatus("reconnecting");

            reconnectTimeoutRef.current = setTimeout(() => {
              if (startupRace) {
                terminalAttachStartupRaceRef.current = false;
              } else {
                reconnectAttemptRef.current++;
              }
              connect();
            }, delay);
          } else {
            setConnectionStatus("disconnected");
          }
        };

        ws.onerror = () => {
          // onclose owns reconnect policy.
        };

        const term = terminalRef.current;
        if (term && canControlRef.current) {
          disposable = term.onData((data) => {
            if (ws.readyState !== WebSocket.OPEN) return;
            const runtime = runtimeRef.current;
            if (shouldSuppressTerminalGeneratedInput(data, runtime)) return;
            if (data.includes("\r") || data.includes("\n") || data.includes("\x03")) {
              setTerminalSignalRef.current(groupId, actorId, {
                kind: "working_output",
                updatedAt: Date.now(),
              });
            }
            ws.send(encodeTerminalInputFrame(data));
          });

          resizeDisposable = term.onResize(({ cols, rows }) => {
            if (ws.readyState === WebSocket.OPEN && cols >= 10 && rows >= 2) {
              ws.send(encodeTerminalResizeFrame(cols, rows));
            }
          });
        }
      };

      // Fit once so the initial resize frame (sent on open) matches the visible
      // size; xterm renders the replayed backlog correctly regardless, and the
      // resize SIGWINCH prompts the runtime to repaint at the right dimensions.
      fitBeforeAttach?.();
      // First attach (no delivered cursor yet) replays the full backlog; later
      // reconnects resume from the cursor we've tracked.
      openWebSocket(deliveredCursor === null);
    };

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
        if (wsRef.current.readyState === WebSocket.OPEN) {
          wsRef.current.close(1000, "Component cleanup");
        }
        wsRef.current = null;
      }
      setConnectionStatus("disconnected");
      setTerminalReady(false);
      setTerminalWritable(false);
    };
  }, [
    activated,
    actorId,
    canControl,
    groupId,
    isHeadless,
    isRunning,
    fitBeforeAttach,
    terminalConnectionKey,
    terminalRef,
  ]);

  useEffect(() => {
    if (!activated || isHeadless || !isRunning || !terminalRef.current) return;
    if (lastTermEpochRef.current === termEpoch) return;
    lastTermEpochRef.current = termEpoch;
    requestReconnect();
  }, [activated, isHeadless, isRunning, requestReconnect, termEpoch, terminalRef]);

  return {
    connectionStatus,
    terminalReady,
    terminalWritable,
    requestReconnect,
    sendInterrupt,
  };
}
