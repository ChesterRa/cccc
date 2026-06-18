import type { FederationRouteMessageRef } from "../types";

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function trimString(value: unknown): string {
  return typeof value === "string" ? value.trim() : value == null ? "" : String(value).trim();
}

export function isFederationRouteMessageRef(value: unknown): value is FederationRouteMessageRef {
  const record = asRecord(value);
  if (!record) return false;
  return trimString(record.kind) === "federation_route" && !!trimString(record.remote_group_id);
}

export function getFederationRouteMessageRefs(value: unknown): FederationRouteMessageRef[] {
  if (!Array.isArray(value)) return [];
  return value.filter(isFederationRouteMessageRef);
}
