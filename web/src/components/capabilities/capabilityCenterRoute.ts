export const CAPABILITY_CENTER_PATH = "/ui/capabilities";

export function isCapabilityCenterPath(pathname: string): boolean {
  const normalized = String(pathname || "").replace(/\/+$/, "");
  return normalized === CAPABILITY_CENTER_PATH;
}

export function capabilityCenterGroupIdFromSearch(search: string): string {
  return new URLSearchParams(String(search || "")).get("group_id")?.trim() || "";
}

export function buildCapabilityCenterUrl(groupId?: string, origin?: string): string {
  const baseOrigin = String(origin || (typeof window !== "undefined" ? window.location.origin : "http://localhost")).trim();
  const url = new URL(CAPABILITY_CENTER_PATH, baseOrigin);
  const gid = String(groupId || "").trim();
  if (gid) url.searchParams.set("group_id", gid);
  return `${url.pathname}${url.search}`;
}
