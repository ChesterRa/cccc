import type { MembershipState } from "../../../types";

export type { MembershipState };

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

export function membershipPublicAddress(membership: MembershipState | null | undefined): string {
  if (!membership?.logged_in || !membership.online) return "";
  const hostname = String(membership.hostname || "").trim();
  return hostname && hostnameLooksTokenless(hostname) ? hostname : "";
}

export function membershipAdminWebUrl(membership: MembershipState | null | undefined): string {
  if (!membership?.logged_in) return "";
  const value = String(membership.web_url || "").trim();
  try {
    const url = new URL(value);
    return /^https?:$/.test(url.protocol) &&
      !url.username &&
      !url.password &&
      hostnameLooksTokenless(value)
      ? value
      : "";
  } catch {
    return "";
  }
}

export function membershipPanelKind(
  membership: MembershipState | null | undefined,
): "logged_out" | "pending" | "cut" | "offline" | "online" {
  if (!membership?.logged_in) return membership?.pending ? "pending" : "logged_out";
  if (membership.cut || membership.disabled) return "cut";
  if (membership.online) return "online";
  return "offline";
}

function trustedAccountUrl(value: unknown): URL | null {
  try {
    const parsed = new URL(String(value || "").trim());
    if (!/^https?:$/.test(parsed.protocol) || parsed.username || parsed.password) return null;
    return parsed;
  } catch {
    return null;
  }
}

function accountLanguage(value: unknown): "zh" | "en" | "ja" | "" {
  const language = String(value || "")
    .trim()
    .toLowerCase()
    .split("-")[0];
  return language === "zh" || language === "en" || language === "ja" ? language : "";
}

function localizedAccountUrl(url: URL, language: unknown): string {
  const normalized = accountLanguage(language);
  if (normalized) url.searchParams.set("lang", normalized);
  return url.toString();
}

export function membershipApprovalUrl(
  membership: MembershipState | null | undefined,
  language?: unknown,
): string {
  const issuer = trustedAccountUrl(membership?.account_origin);
  const pending = membership?.pending;
  const approval = trustedAccountUrl(
    pending?.verification_uri_complete || pending?.verification_uri,
  );
  if (!issuer || !approval || issuer.origin !== approval.origin) return "";
  return localizedAccountUrl(approval, language);
}

export function membershipManagementUrl(
  membership: MembershipState | null | undefined,
  language?: unknown,
): string {
  const issuer = trustedAccountUrl(membership?.account_origin);
  return issuer ? localizedAccountUrl(issuer, language) : "";
}
