export type AccessGoal = "local" | "lan" | "public";

export type ReachabilityAction = "save" | "apply" | "idle";

export type ReachDraft = {
  savedProvider: string;
  draftProvider: string;
  goal: AccessGoal;
  savedMode: string;
  draftMode: string;
  savedHost: string;
  draftHost: string;
  savedPort: string;
  draftPort: string;
  savedPublicUrl: string;
  draftPublicUrl: string;
};

export function keepsActiveReach(draft: ReachDraft): boolean {
  return (
    draft.savedProvider === "reach" &&
    draft.draftProvider === "reach" &&
    draft.goal === "public" &&
    draft.savedMode.trim() === draft.draftMode.trim() &&
    draft.savedHost.trim() === draft.draftHost.trim() &&
    draft.savedPort.trim() === draft.draftPort.trim() &&
    draft.savedPublicUrl.trim() === draft.draftPublicUrl.trim()
  );
}

export function isLoopbackHost(host: string): boolean {
  const normalized = String(host || "")
    .trim()
    .toLowerCase();
  return (
    normalized === "" ||
    normalized === "127.0.0.1" ||
    normalized === "localhost" ||
    normalized === "::1" ||
    normalized === "[::1]"
  );
}

export function isWildcardHost(host: string): boolean {
  const normalized = String(host || "")
    .trim()
    .toLowerCase();
  return normalized === "0.0.0.0" || normalized === "::" || normalized === "[::]";
}

export function httpUrl(host: string, port: string | number): string {
  const rawHost = String(host || "").trim() || "127.0.0.1";
  const normalizedHost =
    rawHost.includes(":") && !rawHost.startsWith("[") && !rawHost.endsWith("]")
      ? `[${rawHost}]`
      : rawHost;
  return `http://${normalizedHost}:${String(port || "").trim() || "8848"}/ui/`;
}

export function inferAccessGoal(provider: string, host: string, publicUrl: string): AccessGoal {
  if (String(provider || "").trim() === "reach") return "public";
  if (String(publicUrl || "").trim()) return "public";
  if (String(provider || "").trim() === "tailscale") return "lan";
  if (String(provider || "").trim() === "off" || isLoopbackHost(host)) return "local";
  return "lan";
}

export function isRemoteAccessBlockedByMissingAdminToken(
  goal: AccessGoal,
  hasAdminToken: boolean,
): boolean {
  return goal !== "local" && !hasAdminToken;
}

export function isReachabilityActionBlockedByMissingAdminToken(
  action: ReachabilityAction,
  draftGoal: AccessGoal,
  savedGoal: AccessGoal,
  hasAdminToken: boolean,
): boolean {
  if (action === "idle") return false;
  return isRemoteAccessBlockedByMissingAdminToken(
    action === "save" ? draftGoal : savedGoal,
    hasAdminToken,
  );
}
