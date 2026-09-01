import { fetchTerminalTail } from "../../services/api";
import { getTerminalSignalFromChunk } from "../../utils/terminalWorkingState";
import type { TerminalSignal } from "../../stores/useTerminalSignalsStore";

export function resolveTerminalConnectionUrl(
  standardUrl: string,
  customBuilder?: (query: string) => string,
): string {
  return customBuilder ? customBuilder(standardUrl.split("?", 2)[1] || "") : standardUrl;
}

export function bootstrapActorTerminalSignal(args: {
  groupId: string;
  actorId: string;
  actorRuntime: string | undefined;
  isDisposed(): boolean;
  setBuffer(value: string): void;
  setSignal(signal: TerminalSignal): void;
  clearSignal(): void;
}) {
  const { groupId, actorId, actorRuntime, isDisposed, setBuffer, setSignal, clearSignal } = args;
  void fetchTerminalTail(groupId, actorId, 4000, true, true)
    .then((response) => {
      if (isDisposed() || !response.ok) return;
      const signal = getTerminalSignalFromChunk(
        "",
        String(response.result?.text || ""),
        actorRuntime,
      );
      setBuffer(signal.nextBuffer);
      if (signal.signalKind) {
        setSignal({ kind: signal.signalKind, updatedAt: Date.now() });
      } else {
        clearSignal();
      }
    })
    .catch(() => undefined);
}
