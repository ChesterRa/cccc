/* eslint-disable no-control-regex */
import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import type { Terminal } from "@xterm/xterm";

import { fetchTerminalTail, withAuthToken } from "../../services/api";
import type { TerminalSignal } from "../../stores/useTerminalSignalsStore";
import { getTerminalSignalFromChunk } from "../../utils/terminalWorkingState";
import {
  buildTerminalConnectionKey,
  isTerminalAttachNonRetryableErrorCode,
  isTerminalAttachStartupRaceErrorCode,
  shouldSuppressTerminalAttachErrorOutput,
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
    onStatusChange,
    setTerminalSignal,
    clearTerminalSignal,
    setReconnectTrigger,
  } = args;

  const [connectionStatus, setConnectionStatus] = useState<AgentTerminalConnectionStatus>("disconnected");
  const [terminalReady, setTerminalReady] = useState(false);
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
  }, [actorRuntime, canControl, clearTerminalSignal, isRunning, onStatusChange, setTerminalSignal]);

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
    ws.send(JSON.stringify({ t: "i", d: "\x03" }));
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

      const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
      const wsUrl = `${protocol}//${window.location.host}/api/v1/groups/${encodeURIComponent(groupId)}/actors/${encodeURIComponent(actorId)}/term`;

      const ws = new WebSocket(withAuthToken(wsUrl));
      ws.binaryType = "arraybuffer";
      wsRef.current = ws;

      ws.onopen = () => {
        if (disposed) {
          ws.close(1000, "Component unmounted during connection");
          return;
        }
        setConnectionStatus("connected");
        reconnectAttemptRef.current = 0;
        outputFilterTailRef.current = "";
        terminalSignalBufferRef.current = "";

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
            ws.send(JSON.stringify({ t: "r", c: term.cols, r: term.rows }));
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
          term.write(safe);
        } catch (err) {
          console.error("terminal write failed", err);
        }
      };

      ws.onmessage = (event) => {
        if (disposed) return;

        if (event.data instanceof ArrayBuffer) {
          handleDecoded(new TextDecoder().decode(event.data));
        } else if (event.data instanceof Blob) {
          void event.data.arrayBuffer().then((buf) => handleDecoded(new TextDecoder().decode(buf)));
        } else if (typeof event.data === "string") {
          try {
            const msg = JSON.parse(event.data);
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
          if (runtime === "droid" || runtime === "gemini" || runtime === "neovate") {
            const isDeviceAttributesReply = /^\x1b\[(?:\?|>)(?:\d+)(?:;\d+)*c$/.test(data);
            if (isDeviceAttributesReply) return;
            const isOscColorReply = /^\x1b\](?:10|11);rgb:[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}\/[0-9a-fA-F]{1,4}(?:\x07|\x1b\\)$/.test(data);
            if (isOscColorReply) return;
            const isFocusEvent = /^\x1b\[[IO]$/.test(data);
            if (isFocusEvent) return;
          }
          if (data.includes("\r") || data.includes("\n") || data.includes("\x03")) {
            setTerminalSignalRef.current(groupId, actorId, {
              kind: "working_output",
              updatedAt: Date.now(),
            });
          }
          ws.send(JSON.stringify({ t: "i", d: data }));
        });

        resizeDisposable = term.onResize(({ cols, rows }) => {
          if (ws.readyState === WebSocket.OPEN && cols >= 10 && rows >= 2) {
            ws.send(JSON.stringify({ t: "r", c: cols, r: rows }));
          }
        });
      }
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
    };
  }, [
    activated,
    actorId,
    canControl,
    groupId,
    isHeadless,
    isRunning,
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
    requestReconnect,
    sendInterrupt,
  };
}
