import type { MembershipState } from "../../../types";

export type { MembershipState };

export type MembershipCopyRowId = "hostname" | "web" | "connector";

export type MembershipCopyRow = { id: MembershipCopyRowId; value: string; available: boolean };

export function membershipCopyRows(
  membership: MembershipState | null | undefined,
): MembershipCopyRow[] {
  if (!membership?.logged_in) return [];
  const hostname = String(membership.hostname || "").trim();
  const web = String(membership.web_url || "").trim();
  const connector = String(membership.connector_url || "").trim();
  return [
    { id: "hostname", value: hostname, available: Boolean(hostname) },
    { id: "web", value: web, available: Boolean(web) },
    { id: "connector", value: connector, available: Boolean(connector) },
  ];
}

export function hostnameLooksTokenless(hostname: string): boolean {
  const value = String(hostname || "").trim();
  if (!value) return true;
  try {
    const url = new URL(value);
    return !url.search && !url.hash && !/\/token\//i.test(url.pathname);
  } catch {
    return !/[?&]token=|\/token\//i.test(value);
  }
}

export function membershipPanelKind(
  membership: MembershipState | null | undefined,
): "logged_out" | "cut" | "offline" | "online" {
  if (!membership?.logged_in) return "logged_out";
  if (membership.cut || membership.disabled) return "cut";
  if (membership.online) return "online";
  return "offline";
}
