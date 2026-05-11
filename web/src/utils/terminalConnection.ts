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
