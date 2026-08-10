import type { Terminal } from "@xterm/xterm";

export function createTerminalReplayWriteGuard(term: Pick<Terminal, "write">) {
  let pendingReplayWrites = 0;

  return {
    write(data: string, replaying: boolean): void {
      if (!replaying) {
        term.write(data);
        return;
      }

      pendingReplayWrites += 1;
      try {
        term.write(data, () => {
          pendingReplayWrites = Math.max(0, pendingReplayWrites - 1);
        });
      } catch (error) {
        pendingReplayWrites = Math.max(0, pendingReplayWrites - 1);
        throw error;
      }
    },
    isReplaying(): boolean {
      return pendingReplayWrites > 0;
    },
  };
}
