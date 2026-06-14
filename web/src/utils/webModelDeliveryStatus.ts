import type { LedgerEvent, WebModelDeliveryStatusPayload } from "../types";

export type WebModelDeliveryState = "submitting" | "submitted" | "bound" | "pending" | "ambiguous" | "failed";

export type WebModelDeliveryStatus = {
  state: WebModelDeliveryState;
  actorId: string;
  deliveryId: string;
  updatedAt: string;
  detail: string;
};

export const WEB_MODEL_DELIVERY_APP_PERMISSION_HINT_MAX_AGE_MS = 30 * 60 * 1000;

export function webModelDeliveryNeedsAppPermissionHint(
  status?: WebModelDeliveryStatus | null,
  dismissed = false,
): boolean {
  if (dismissed) return false;
  return (
    status?.state === "submitting" ||
    status?.state === "submitted" ||
    status?.state === "pending" ||
    status?.state === "bound" ||
    status?.state === "ambiguous"
  );
}

export function webModelDeliveryStatusIsFreshForAppPermissionHint(
  status?: WebModelDeliveryStatus | null,
  nowMs = Date.now(),
  maxAgeMs = WEB_MODEL_DELIVERY_APP_PERMISSION_HINT_MAX_AGE_MS,
): boolean {
  if (!status) return false;
  const updatedMs = Date.parse(String(status.updatedAt || ""));
  if (!Number.isFinite(updatedMs)) return false;
  const ageMs = Number(nowMs) - updatedMs;
  return ageMs >= 0 && ageMs <= Math.max(0, Number(maxAgeMs));
}

export function latestWebModelDeliveryStatusNeedingAppPermissionHint(
  statuses: Record<string, WebModelDeliveryStatus> | undefined,
  dismissed = false,
  options?: { nowMs?: number; maxAgeMs?: number },
): WebModelDeliveryStatus | null {
  if (dismissed || !statuses) return null;
  const nowMs = options?.nowMs ?? Date.now();
  const maxAgeMs = options?.maxAgeMs ?? WEB_MODEL_DELIVERY_APP_PERMISSION_HINT_MAX_AGE_MS;
  let latest: WebModelDeliveryStatus | null = null;
  for (const status of Object.values(statuses)) {
    if (!webModelDeliveryNeedsAppPermissionHint(status)) continue;
    if (!webModelDeliveryStatusIsFreshForAppPermissionHint(status, nowMs, maxAgeMs)) continue;
    if (!latest || String(status.updatedAt || "") >= String(latest.updatedAt || "")) {
      latest = status;
    }
  }
  return latest;
}

const DELIVERY_KIND_TO_STATE: Record<string, WebModelDeliveryState> = {
  "web_model.browser_delivery.submitting": "submitting",
  "web_model.browser_delivery.submitted": "submitted",
  "web_model.browser_delivery.bound": "bound",
  "web_model.browser_delivery.pending": "pending",
  "web_model.browser_delivery.ambiguous": "ambiguous",
  "web_model.browser_delivery.failed": "failed",
};

const DELIVERY_STATES: WebModelDeliveryState[] = ["submitting", "submitted", "bound", "pending", "ambiguous", "failed"];

function eventData(event: LedgerEvent): Record<string, unknown> {
  return event.data && typeof event.data === "object" ? event.data as Record<string, unknown> : {};
}

function eventIds(data: Record<string, unknown>): string[] {
  const ids = Array.isArray(data.event_ids)
    ? data.event_ids.map((id) => String(id || "").trim()).filter((id) => id)
    : [];
  if (ids.length > 0) return ids;
  const triggerId = String(data.trigger_event_id || "").trim();
  return triggerId ? [triggerId] : [];
}

function browserDetail(data: Record<string, unknown>): string {
  const browser = data.browser && typeof data.browser === "object" ? data.browser as Record<string, unknown> : {};
  const evidence = String(browser.submission_evidence || data.submission_evidence || "").trim();
  if (evidence) return evidence;
  const error = String(data.error || data.commit_error || "").trim();
  return error;
}

function statusFromMessageEvent(event: LedgerEvent): WebModelDeliveryStatus | null {
  const eventId = String(event.id || "").trim();
  if (!eventId) return null;
  const payload = event._web_model_delivery_status as WebModelDeliveryStatusPayload | undefined;
  if (!payload || typeof payload !== "object") return null;
  const state = String(payload.state || "").trim() as WebModelDeliveryState;
  if (!state || !DELIVERY_STATES.includes(state)) return null;
  return {
    state,
    actorId: String(payload.actor_id || "").trim(),
    deliveryId: String(payload.delivery_id || "").trim(),
    updatedAt: String(payload.updated_at || "").trim(),
    detail: String(payload.detail || "").trim(),
  };
}

export function buildWebModelDeliveryStatusByEventId(events: LedgerEvent[] | undefined): Record<string, WebModelDeliveryStatus> {
  const statuses: Record<string, WebModelDeliveryStatus> = {};
  for (const event of Array.isArray(events) ? events : []) {
    const messageStatus = statusFromMessageEvent(event);
    if (messageStatus && event.id) {
      statuses[String(event.id)] = messageStatus;
      continue;
    }
    const kind = String(event.kind || "").trim();
    const state = DELIVERY_KIND_TO_STATE[kind];
    if (!state) continue;
    const data = eventData(event);
    const ids = eventIds(data);
    if (ids.length === 0) continue;
    const status: WebModelDeliveryStatus = {
      state,
      actorId: String(data.actor_id || "").trim(),
      deliveryId: String(data.delivery_id || "").trim(),
      updatedAt: String(event.ts || "").trim(),
      detail: browserDetail(data),
    };
    for (const id of ids) {
      statuses[id] = status;
    }
  }
  return statuses;
}
