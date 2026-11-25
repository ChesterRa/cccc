import type { RefObject } from "react";
import type { Terminal } from "@xterm/xterm";
import type { TerminalSignal } from "../../stores/useTerminalSignalsStore";
import type { TerminalSize } from "./terminalViewportSize";

export type AgentTerminalConnectionArgs = {
  activated: boolean;
  isRunning: boolean;
  isHeadless: boolean;
  groupId: string;
  actorId: string;
  actorRuntime: string | undefined;
  canControl: boolean;
  /** Hidden retained views keep streaming, but cannot send input or resize the PTY. */
  isVisible?: boolean;
  termEpoch: number;
  reconnectTrigger: number;
  terminalRef: RefObject<Terminal | null>;
  fitBeforeAttach?: () => void;
  onStatusChange?: () => void;
  setTerminalSignal: (groupId: string, actorId: string, signal: TerminalSignal) => void;
  clearTerminalSignal: (groupId: string, actorId: string) => void;
  setReconnectTrigger: (updater: (value: number) => number) => void;
  buildCustomWebSocketUrl?: (query: string) => string;
  inspectActorTail?: boolean;
  takeoverOnAttach?: boolean;
  /** An explicitly supported task viewer may resize without terminal input authority. */
  resizeReadOnly?: boolean;
  /**
   * Set while this window does not write: the PTY size it must show. The view's fit
   * reads it (see fitTerminalViewport) instead of measuring its container.
   */
  followSizeRef?: { current: TerminalSize | null };
};
