import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import type { Terminal } from "@xterm/xterm";

import { fetchTerminalTail, withAuthToken } from "../../services/api";
import type { TerminalSignal } from "../../stores/useTerminalSignalsStore";
import { getTerminalSignalFromChunk } from "../../utils/terminalWorkingState";
import {
  createTerminalOutputController,
  type TerminalOutputCursorState,
} from "./terminalOutputController";
import { createTerminalOutputStreamWriter } from "./terminalOutputStreamWriter";
import { createTerminalReplayWriteGuard } from "./terminalReplayWriteGuard";
import {
  buildTerminalWebSocketUrl,
  buildTerminalConnectionKey,
  encodeTerminalInputFrame,
  encodeTerminalResizeFrame,
  filterTerminalInputForRuntime,
  isTerminalAttachNonRetryableErrorCode,
  isTerminalAttachStartupRaceErrorCode,
  shouldSuppressTerminalAttachErrorOutput,
  shouldRetryTerminalClose,
} from "../../utils/terminalConnection";

export type AgentTerminalConnectionStatus =
  | "disconnected"
  | "connecting"
  | "connected"
  | "reconnecting";

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

  const [connectionStatus, setConnectionStatus] =
    useState<AgentTerminalConnectionStatus>("disconnected");
  const [connectionFailed, setConnectionFailed] = useState(false);
  const [terminalReady, setTerminalReady] = useState(false);
  const [terminalWritable, setTerminalWritable] = useState(false);
  const wsRef = useRef<WebSocket | null>(null);
  const reconnectAttemptRef = useRef(0);
  const reconnectTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const terminalReadyTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
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
  }, [
    actorRuntime,
    canControl,
    clearTerminalSignal,
    isHeadless,
    isRunning,
    onStatusChange,
    setTerminalSignal,
  ]);

  useEffect(() => {
    if (isRunning && !isHeadless) return;
    terminalSignalBufferRef.current = "";
    clearTerminalSignalRef.current(groupId, actorId);
  }, [actorId, groupId, isHeadless, isRunning]);

  const requestReconnect = useCallback(() => {
    reconnectAttemptRef.current = 0;
    terminalAttachNoRetryRef.current = false;
    setConnectionFailed(false);
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
    const replayWriteGuard = createTerminalReplayWriteGuard(terminalRef.current);

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
    // Absolute raw PTY offsets. A snapshot is committed only after xterm has
    // parsed it; its encoded byte length never advances these cursors.
    const cursors: TerminalOutputCursorState = {
      deliveredCursor: null,
      receivedCursor: null,
      replayEndCursor: null,
    };
    let connectionGeneration = 0;

    const connect = async () => {
      if (disposed) return;
      const existingWs = wsRef.current;
      if (
        existingWs &&
        (existingWs.readyState === WebSocket.OPEN || existingWs.readyState === WebSocket.CONNECTING)
      ) {
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
        const generation = ++connectionGeneration;
        const fittedTerm = terminalRef.current;
        const wsUrl = buildTerminalWebSocketUrl({
          protocol: window.location.protocol,
          host: window.location.host,
          groupId,
          actorId,
          since: isFirstAttach ? null : cursors.deliveredCursor,
          mode: canControlRef.current ? "control" : "viewer",
          takeover: canControlRef.current,
          outputFlowControl: "ack_v1",
          bootstrap: "snapshot_v1",
          cols: canControlRef.current ? fittedTerm?.cols : undefined,
          rows: canControlRef.current ? fittedTerm?.rows : undefined,
        });

        const ws = new WebSocket(withAuthToken(wsUrl));
        let serverOwnsTerminalResponses = false;
        ws.binaryType = "arraybuffer";
        wsRef.current = ws;

        ws.onopen = () => {
          if (disposed) {
            ws.close(1000, "Component unmounted during connection");
            return;
          }
          setConnectionStatus("connected");
          setConnectionFailed(false);
          setTerminalWritable(false);
          reconnectAttemptRef.current = 0;
          terminalSignalBufferRef.current = "";
          // The first attach is rebuilt from either a negotiated snapshot or a
          // legacy raw replay. Tail-only reconnects keep the current viewport.
          if (isFirstAttach) {
            try {
              terminalRef.current?.reset();
            } catch {
              // ignore
            }
          }

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

        const observeTerminalText = (data: string) => {
          if (!data) return;
          const signal = getTerminalSignalFromChunk(
            terminalSignalBufferRef.current,
            data,
            runtimeRef.current,
          );
          terminalSignalBufferRef.current = signal.nextBuffer;
          if (signal.signalKind) {
            setTerminalSignalRef.current(groupId, actorId, {
              kind: signal.signalKind,
              updatedAt: Date.now(),
            });
          }
        };

        const handleDecoded = (data: string, replaying = false, onParsed?: () => void) => {
          if (disposed) return;
          const term = terminalRef.current;
          if (!term) {
            onParsed?.();
            return;
          }
          observeTerminalText(data);
          try {
            if (data) replayWriteGuard.write(data, replaying, onParsed);
            else onParsed?.();
          } catch (err) {
            console.error("terminal write failed", err);
            ws.close(1011, "Terminal renderer failed");
          }
        };

        const outputWriter = createTerminalOutputStreamWriter({
          write: (data, replaying, onParsed) => {
            replayWriteGuard.write(data, replaying, onParsed);
          },
          onText: observeTerminalText,
        });

        const scheduleTerminalReady = () => {
          if (terminalReadyTimeoutRef.current) return;
          terminalReadyTimeoutRef.current = setTimeout(() => {
            terminalReadyTimeoutRef.current = null;
            if (!disposed && generation === connectionGeneration) setTerminalReady(true);
          }, TERMINAL_SHOW_DELAY_MS);
        };

        const outputController = createTerminalOutputController({
          ws,
          cursors,
          outputWriter,
          getTerminal: () => terminalRef.current,
          isCurrentGeneration: () => generation === connectionGeneration,
          canControl: () => canControlRef.current,
          onDecoded: handleDecoded,
          setWritable: setTerminalWritable,
          setServerResponseOwnership: (owned) => {
            serverOwnsTerminalResponses = owned;
          },
          resetReady: () => {
            if (terminalReadyTimeoutRef.current) {
              clearTimeout(terminalReadyTimeoutRef.current);
              terminalReadyTimeoutRef.current = null;
            }
            setTerminalReady(false);
          },
          scheduleReady: scheduleTerminalReady,
          fitAfterSnapshot: fitBeforeAttach,
        });

        ws.onmessage = (event) => {
          if (disposed) return;

          if (event.data instanceof ArrayBuffer) {
            outputController.handleBinaryFrame(event.data);
          } else if (event.data instanceof Blob) {
            void event.data.arrayBuffer().then((buf) => {
              if (!disposed) outputController.handleBinaryFrame(buf);
            });
          } else if (typeof event.data === "string") {
            try {
              const msg = JSON.parse(event.data);
              if (msg.type === "terminal.attach" && msg.ok === true) {
                const result = msg.result && typeof msg.result === "object" ? msg.result : {};
                outputController.handleAttachResult(result);
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
              outputController.handleOutputPayload(new TextEncoder().encode(event.data));
            }
          }
        };

        ws.onclose = (event) => {
          if (disposed) return;
          try {
            outputWriter.flush();
          } catch (err) {
            console.error("terminal output flush failed", err);
          }
          wsRef.current = null;
          const shouldRetry = shouldRetryTerminalClose({
            actorRunning: isRunningRef.current,
            isHeadless,
            attachNonRetryable: terminalAttachNoRetryRef.current,
            closeCode: event.code,
          });

          if (shouldRetry) {
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
              void connect();
            }, delay);
          } else {
            setConnectionStatus("disconnected");
          }
        };

        ws.onerror = () => {
          setConnectionFailed(true);
          // onclose owns reconnect policy.
        };

        const term = terminalRef.current;
        if (term && canControlRef.current) {
          disposable = term.onData((data) => {
            if (ws.readyState !== WebSocket.OPEN) return;
            const runtime = runtimeRef.current;
            const input = filterTerminalInputForRuntime(data, runtime, {
              replaying: replayWriteGuard.isReplaying(),
              serverResponses: serverOwnsTerminalResponses,
            });
            if (!input) return;
            if (input.includes("\r") || input.includes("\n") || input.includes("\x03")) {
              setTerminalSignalRef.current(groupId, actorId, {
                kind: "working_output",
                updatedAt: Date.now(),
              });
            }
            ws.send(encodeTerminalInputFrame(input));
          });

          resizeDisposable = term.onResize(({ cols, rows }) => {
            if (ws.readyState === WebSocket.OPEN && cols >= 10 && rows >= 2) {
              ws.send(encodeTerminalResizeFrame(cols, rows));
            }
          });
        }
      };

      // Fit once so the initial resize frame (sent on open) matches the visible
      // size and the resize SIGWINCH prompts the runtime to repaint correctly.
      fitBeforeAttach?.();
      openWebSocket(cursors.deliveredCursor === null);
    };

    setConnectionFailed(false);
    void connect();

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
    connectionFailed,
    terminalReady,
    terminalWritable,
    requestReconnect,
    sendInterrupt,
  };
}
