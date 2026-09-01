import type { RefObject } from "react";
import type { Terminal } from "@xterm/xterm";
import type { TerminalSignal } from "../../stores/useTerminalSignalsStore";

export type AgentTerminalConnectionArgs = {
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
  buildCustomWebSocketUrl?: (query: string) => string;
  inspectActorTail?: boolean;
};
