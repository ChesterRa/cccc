// Pure, UI-framework-free logic for the Federation registration Settings section.
// Kept separate from the React component so it can be unit-tested directly
// (the repo has no DOM-render test harness).

import type { GroupMeta } from "../../../types";

export interface RegisterableGroupOption {
  id: string;
  title: string;
}

export function buildRegisterableOptions(groups: GroupMeta[] | undefined | null): RegisterableGroupOption[] {
  return (groups || [])
    .map((g) => ({
      id: String(g?.group_id || "").trim(),
      title: String(g?.title || g?.group_id || "").trim(),
    }))
    .filter((o) => o.id.length > 0);
}

// Registration always starts with an empty selection — admin is NOT pre-selected.
export function initialRegisterSelection(): Set<string> {
  return new Set<string>();
}

export function toggleGroupSelection(current: Set<string>, id: string): Set<string> {
  const next = new Set(current);
  if (next.has(id)) {
    next.delete(id);
  } else {
    next.add(id);
  }
  return next;
}

// Group checkboxes are only shown after a successful verify.
export function registerableOptionsForDisplay(
  verified: boolean,
  options: RegisterableGroupOption[],
): RegisterableGroupOption[] {
  return verified ? options : [];
}

// The peer_cccc_http transport posts to /api/v1/groups/<remoteGroupId>/send, so
// an empty remote group id produces a broken target. Require it before
// verify/register.
export function isPeerHttpRemoteGroupMissing(transport: string, remoteGroupId: string): boolean {
  const t = String(transport || "peer_cccc_http").trim() || "peer_cccc_http";
  return t === "peer_cccc_http" && String(remoteGroupId || "").trim().length === 0;
}

export function canVerify(opts: { url: string; transport: string; remoteGroupId: string; busy: boolean }): boolean {
  return (
    String(opts.url || "").trim().length > 0 &&
    !opts.busy &&
    !isPeerHttpRemoteGroupMissing(opts.transport, opts.remoteGroupId)
  );
}

export function canSubmitRegister(opts: {
  verified: boolean;
  url: string;
  selectedCount: number;
  busy: boolean;
  transport?: string;
  remoteGroupId?: string;
}): boolean {
  return (
    opts.verified &&
    String(opts.url || "").trim().length > 0 &&
    opts.selectedCount > 0 &&
    !opts.busy &&
    !isPeerHttpRemoteGroupMissing(opts.transport ?? "peer_cccc_http", opts.remoteGroupId ?? "")
  );
}

// Redact the user's raw credential and any access-token-shaped substring from
// any error text so the UI never echoes a secret back to the screen.
export function safeFederationErrorText(message: string, secret?: string): string {
  let text = String(message || "").trim() || "Request failed";
  const raw = String(secret || "").trim();
  if (raw.length >= 4) {
    text = text.split(raw).join("***");
  }
  return text.replace(/acc_[0-9A-Za-z_-]{6,}/g, "acc_***");
}
