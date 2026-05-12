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

export function isTerminalAttachNonRetryableErrorCode(code: unknown): boolean {
  const normalized = String(code || "").trim();
  return [
    "actor_not_found",
    "actor_not_running",
    "auth_required",
    "group_not_found",
    "not_pty_actor",
    "permission_denied",
    "read_only_terminal",
  ].includes(normalized);
}
