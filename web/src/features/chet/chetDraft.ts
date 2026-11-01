import type { ChetGroupContext, ChetTextRequest } from "../../services/api/chet";

/**
 * A sent request this browser has not yet seen in history. `unknown`: the registration may
 * or may not have happened, so only a retry with the same ID is safe. `registered`: the host
 * accepted it and history will show it. Each request has its own key, so tabs sending at
 * the same time cannot overwrite each other's IDs.
 */
export type ChetPendingRequest = {
  id: string;
  request: ChetTextRequest;
  state: "registered" | "unknown";
  createdAt: string;
};

/** The unsent text, kept only in this browser. Its tabs share one draft. */
export const CHET_DRAFT_KEY = "cccc.chet.draft.v1";
export const CHET_PENDING_PREFIX = "cccc.chet.pending.";

function isContext(value: unknown): value is ChetGroupContext {
  if (!value || typeof value !== "object") return false;
  const context = value as Record<string, unknown>;
  return (
    typeof context.group_id === "string" &&
    typeof context.group_title === "string" &&
    (context.scope_key === null || typeof context.scope_key === "string")
  );
}

export function parseChetPending(id: string, raw: string | null): ChetPendingRequest | null {
  if (!raw) return null;
  try {
    const value = JSON.parse(raw) as Record<string, unknown>;
    const request = value.request as Record<string, unknown> | null | undefined;
    if (
      !request ||
      typeof request.text !== "string" ||
      !(request.context === null || isContext(request.context))
    ) {
      return null;
    }
    return {
      id,
      request: { text: request.text, context: request.context },
      state: value.state === "registered" ? "registered" : "unknown",
      createdAt: typeof value.createdAt === "string" ? value.createdAt : "",
    };
  } catch {
    return null;
  }
}

/** The stored draft, or null when this browser gives no storage access. */
export function loadChetDraft(): string | null {
  try {
    return window.localStorage.getItem(CHET_DRAFT_KEY) ?? "";
  } catch {
    return null;
  }
}

/** Returns false when this browser keeps no storage; the draft then lives only in the page. */
export function saveChetDraft(text: string): boolean {
  try {
    if (!text) window.localStorage.removeItem(CHET_DRAFT_KEY);
    else window.localStorage.setItem(CHET_DRAFT_KEY, text);
    return true;
  } catch {
    return false;
  }
}

export function loadChetPending(): ChetPendingRequest[] {
  try {
    const pending: ChetPendingRequest[] = [];
    for (let index = 0; index < window.localStorage.length; index += 1) {
      const key = window.localStorage.key(index);
      if (!key?.startsWith(CHET_PENDING_PREFIX)) continue;
      const entry = parseChetPending(
        key.slice(CHET_PENDING_PREFIX.length),
        window.localStorage.getItem(key),
      );
      if (entry) pending.push(entry);
    }
    return pending.sort((left, right) => left.createdAt.localeCompare(right.createdAt));
  } catch {
    return [];
  }
}

/** Returns false when the ID cannot outlive this page. */
export function saveChetPending(entry: ChetPendingRequest): boolean {
  try {
    const { request, state, createdAt } = entry;
    window.localStorage.setItem(
      CHET_PENDING_PREFIX + entry.id,
      JSON.stringify({ request, state, createdAt }),
    );
    return true;
  } catch {
    return false;
  }
}

export function removeChetPending(id: string): void {
  try {
    window.localStorage.removeItem(CHET_PENDING_PREFIX + id);
  } catch {
    // Nothing was stored.
  }
}
