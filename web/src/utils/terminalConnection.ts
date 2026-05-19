export function buildTerminalConnectionKey(args: {
  activated: boolean;
  isRunning: boolean;
  isHeadless: boolean;
  groupId: string;
  actorId: string;
  reconnectTrigger: number;
  canControl: boolean;
}): string {
  return [
    args.activated ? "active" : "inactive",
    args.isRunning ? "running" : "stopped",
    args.isHeadless ? "headless" : "pty",
    String(args.groupId || "").trim(),
    String(args.actorId || "").trim(),
    String(args.reconnectTrigger || 0),
    args.canControl ? "control" : "readonly",
  ].join(":");
}

export function buildTerminalWebSocketUrl(args: {
  protocol: string;
  host: string;
  groupId: string;
  actorId: string;
  since?: number | string | null;
}): string {
  const protocol = args.protocol === "https:" ? "wss:" : "ws:";
  const url = `${protocol}//${args.host}/api/v1/groups/${encodeURIComponent(args.groupId)}/actors/${encodeURIComponent(args.actorId)}/term`;
  const since = args.since;
  if (since === null || since === undefined || !String(since).trim()) return url;
  return `${url}?since=${encodeURIComponent(String(since))}`;
}

export function createTerminalAttachCursorResolver(readCursor: () => Promise<number | null>): {
  resolve: () => Promise<number | null>;
} {
  let cursorPromise: Promise<number | null> | null = null;

  return {
    resolve: () => {
      if (!cursorPromise) {
        cursorPromise = readCursor().finally(() => {
          cursorPromise = null;
        });
      }
      return cursorPromise;
    },
  };
}

export function isTerminalAttachNonRetryableErrorCode(code: unknown): boolean {
  const normalized = String(code || "").trim();
  return [
    "actor_not_found",
    "auth_required",
    "group_not_found",
    "not_pty_actor",
    "permission_denied",
    "read_only_terminal",
  ].includes(normalized);
}

export function isTerminalAttachStartupRaceErrorCode(code: unknown): boolean {
  const normalized = String(code || "").trim();
  return normalized === "actor_not_running";
}

export function shouldSuppressTerminalAttachErrorOutput(code: unknown): boolean {
  const normalized = String(code || "").trim();
  return normalized === "actor_not_running" || normalized === "not_pty_actor";
}
