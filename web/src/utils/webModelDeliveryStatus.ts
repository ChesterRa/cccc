import type { LedgerEvent } from "../types";

export type WebModelDeliveryState = "submitting" | "submitted" | "pending" | "failed";

export type WebModelDeliveryStatus = {
  state: WebModelDeliveryState;
  actorId: string;
  deliveryId: string;
  updatedAt: string;
  detail: string;
};

const DELIVERY_KIND_TO_STATE: Record<string, WebModelDeliveryState> = {
  "web_model.browser_delivery.submitting": "submitting",
  "web_model.browser_delivery.submitted": "submitted",
  "web_model.browser_delivery.pending": "pending",
  "web_model.browser_delivery.failed": "failed",
};

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

export function buildWebModelDeliveryStatusByEventId(events: LedgerEvent[] | undefined): Record<string, WebModelDeliveryStatus> {
  const statuses: Record<string, WebModelDeliveryStatus> = {};
  for (const event of Array.isArray(events) ? events : []) {
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
