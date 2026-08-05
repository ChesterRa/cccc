const GLOBAL_REFRESH_EVENT_KINDS = new Set([
  "group.created",
  "group.updated",
  "group.deleted",
  "group.state_changed",
  "group.create",
  "group.update",
  "group.start",
  "group.stop",
  "group.set_state",
  "actor.remove",
  "actor.start",
  "actor.stop",
  "actor.restart",
]);

const ACTOR_REFRESH_EVENT_KINDS = new Set([
  // actor.activity is projected directly by the selected group's ledger stream.
  // Refetching the full actor list here duplicates that owner and causes visible churn.
  "actor.remove",
  "actor.start",
  "actor.stop",
  "actor.restart",
  "group.state_changed",
  "group.start",
  "group.stop",
  "group.set_state",
]);

const CAPABILITY_REFRESH_EVENT_KINDS = new Set(["capability.changed"]);

const GROUP_BRIDGE_PAIRING_EVENT_KINDS = new Set([
  "group_bridge.pairing.invite_created",
  "group_bridge.pairing.request_created",
  "group_bridge.pairing.request_approved",
  "group_bridge.pairing.request_rejected",
  "group_bridge.pairing.trust_access_updated",
  "group_bridge.pairing.trust_revoked",
  "group_bridge.pairing.outbound_changed",
  "group_bridge.pairing.outbound_approved",
]);

export function shouldRefreshGroupsAfterGlobalEvent(ev: unknown): boolean {
  return GLOBAL_REFRESH_EVENT_KINDS.has(eventKind(ev));
}

export function shouldRefreshGroupsAfterGlobalEventsOpen(_hasConnectedOnce: boolean): boolean {
  return true;
}

export function shouldRefreshGroupBridgePairingAfterGlobalEventsOpen(
  _hasConnectedOnce: boolean,
): boolean {
  return true;
}

export function shouldKeepGlobalEventsConnected(documentHidden: boolean): boolean {
  return !documentHidden;
}

export function getGlobalEventGroupId(ev: unknown): string {
  if (!ev || typeof ev !== "object") return "";
  const directGroupId = String((ev as { group_id?: unknown }).group_id || "").trim();
  if (directGroupId) return directGroupId;
  const data = (ev as { data?: unknown }).data;
  if (!data || typeof data !== "object") return "";
  return String((data as { group_id?: unknown }).group_id || "").trim();
}

export function shouldRefreshActorsAfterGlobalEvent(ev: unknown, selectedGroupId: string): boolean {
  return matchesSelectedGroup(ev, selectedGroupId, ACTOR_REFRESH_EVENT_KINDS);
}

export function shouldRefreshCapabilitiesAfterGlobalEvent(
  ev: unknown,
  selectedGroupId: string,
): boolean {
  return matchesSelectedGroup(ev, selectedGroupId, CAPABILITY_REFRESH_EVENT_KINDS);
}

export function shouldRefreshGroupBridgePairingAfterGlobalEvent(
  ev: unknown,
  selectedGroupId: string,
): boolean {
  return matchesSelectedGroup(ev, selectedGroupId, GROUP_BRIDGE_PAIRING_EVENT_KINDS);
}

function eventKind(ev: unknown): string {
  if (!ev || typeof ev !== "object") return "";
  return String((ev as { kind?: unknown }).kind || "").trim();
}

function matchesSelectedGroup(
  ev: unknown,
  selectedGroupId: string,
  kinds: ReadonlySet<string>,
): boolean {
  if (!kinds.has(eventKind(ev))) return false;
  const selected = String(selectedGroupId || "").trim();
  return Boolean(selected) && getGlobalEventGroupId(ev) === selected;
}
