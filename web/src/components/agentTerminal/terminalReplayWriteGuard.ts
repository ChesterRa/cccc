import type { Terminal } from "@xterm/xterm";

export function createTerminalReplayWriteGuard(term: Pick<Terminal, "write">) {
  let pendingReplayWrites = 0;

  return {
    write(data: string | Uint8Array, replaying: boolean, onParsed?: () => void): void {
      if (replaying) pendingReplayWrites += 1;
      try {
        term.write(data, () => {
          if (replaying) pendingReplayWrites = Math.max(0, pendingReplayWrites - 1);
          onParsed?.();
        });
      } catch (error) {
        if (replaying) pendingReplayWrites = Math.max(0, pendingReplayWrites - 1);
        throw error;
      }
    },
    isReplaying(): boolean {
      return pendingReplayWrites > 0;
    },
  };
}
