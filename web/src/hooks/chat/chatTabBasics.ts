import type { ChatMessageData, GroupMeta, LedgerEvent } from "../../types";
import { isDelegationSourceOutboundEvent } from "../../components/messageBubbleDelegation";
import { CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION } from "../../stores/useUIStore";

export const CHAT_SCROLL_SNAPSHOT_MAX_AGE_MS = 30 * 60 * 1000;

export function shouldRestoreDetachedScrollSnapshot(
  snapshot:
    | { coordinateVersion?: unknown; mode?: unknown; anchorId?: unknown; updatedAt?: unknown }
    | null
    | undefined,
  now = Date.now(),
): boolean {
  if (!snapshot || snapshot.mode !== "detached") return false;
  if (snapshot.coordinateVersion !== CHAT_SCROLL_SNAPSHOT_COORDINATE_VERSION) return false;
  const anchorId = typeof snapshot.anchorId === "string" ? snapshot.anchorId.trim() : "";
  if (!anchorId) return false;
  const updatedAt = Number(snapshot.updatedAt);
  if (!Number.isFinite(updatedAt) || updatedAt <= 0) return false;
  return now - updatedAt <= CHAT_SCROLL_SNAPSHOT_MAX_AGE_MS;
}

export function buildComposerTrustFetchGroupId(_selectedGroupId: string): string | undefined {
  const gid = String(_selectedGroupId || "").trim();
  return gid || undefined;
}

export function canOpenSourceMessageLocally(groups: GroupMeta[], srcGroupId: string): boolean {
  const gid = String(srcGroupId || "").trim();
  if (!gid) return false;
  return (groups || []).some((group) => {
    if (String(group?.group_id || "").trim() !== gid) return false;
    return !group.group_bridge_remote;
  });
}

export function shouldShowInConversation(event: LedgerEvent): boolean {
  const data =
    event.data && typeof event.data === "object"
      ? (event.data as { hidden?: unknown; refs?: unknown })
      : {};
  if (data.hidden === true && !getSlashSkillDispatchRef(event)) return false;
  return !isDelegationSourceOutboundEvent(event.data);
}

function isSlashSkillDispatchRef(
  ref: unknown,
): ref is {
  hidden?: unknown;
  control_kind?: unknown;
  title?: unknown;
  command?: unknown;
  task_text?: unknown;
} {
  if (!ref || typeof ref !== "object") return false;
  const record = ref as { hidden?: unknown; control_kind?: unknown; title?: unknown };
  return (
    record.hidden === true &&
    (String(record.control_kind || "").trim() === "slash_skill_dispatch" ||
      String(record.title || "").trim() === "slash_skill_dispatch")
  );
}

function getSlashSkillDispatchRef(event: LedgerEvent) {
  const data =
    event.data && typeof event.data === "object" ? (event.data as { refs?: unknown }) : {};
  const refs = Array.isArray(data.refs) ? data.refs : [];
  return refs.find(isSlashSkillDispatchRef) || null;
}

export function toVisibleConversationEvent(event: LedgerEvent): LedgerEvent {
  const ref = getSlashSkillDispatchRef(event);
  if (!ref) return event;
  const command = String(ref.command || "").trim();
  const taskText = String(ref.task_text || "").trim();
  const visibleText = [command, taskText].filter(Boolean).join(" ").trim();
  if (!visibleText) return event;
  const data = event.data && typeof event.data === "object" ? (event.data as ChatMessageData) : {};
  return { ...event, data: { ...data, text: visibleText, refs: [], attachments: [] } };
}

export function mergeStreamingCandidates(
  primary: LedgerEvent,
  secondary: LedgerEvent,
): LedgerEvent {
  const primaryData =
    primary.data && typeof primary.data === "object"
      ? (primary.data as ChatMessageData & {
          pending_placeholder?: unknown;
          pending_event_id?: unknown;
          stream_id?: unknown;
        })
      : {};
  const secondaryData =
    secondary.data && typeof secondary.data === "object"
      ? (secondary.data as ChatMessageData & {
          pending_placeholder?: unknown;
          pending_event_id?: unknown;
          stream_id?: unknown;
        })
      : {};
  const primaryText = typeof primaryData.text === "string" ? primaryData.text.trim() : "";
  const secondaryText = typeof secondaryData.text === "string" ? secondaryData.text.trim() : "";
  const primaryTs = String(primary.ts || "");
  const secondaryTs = String(secondary.ts || "");
  const primaryHasText = primaryText.length > 0;
  const secondaryHasText = secondaryText.length > 0;
  const primaryIsPlaceholder = Boolean(primaryData.pending_placeholder);
  const secondaryIsPlaceholder = Boolean(secondaryData.pending_placeholder);

  let display = primary;
  let support = secondary;
  if (secondaryHasText && !primaryHasText) {
    display = secondary;
    support = primary;
  } else if (secondaryHasText === primaryHasText) {
    if (primaryIsPlaceholder && !secondaryIsPlaceholder) {
      display = secondary;
      support = primary;
    } else if (primaryIsPlaceholder === secondaryIsPlaceholder && secondaryTs > primaryTs) {
      display = secondary;
      support = primary;
    }
  }

  const displayData =
    display.data && typeof display.data === "object"
      ? (display.data as ChatMessageData & {
          pending_placeholder?: unknown;
          pending_event_id?: unknown;
          stream_id?: unknown;
        })
      : {};
  const supportData =
    support.data && typeof support.data === "object"
      ? (support.data as ChatMessageData & {
          pending_placeholder?: unknown;
          pending_event_id?: unknown;
          stream_id?: unknown;
        })
      : {};
  const displayActivities = Array.isArray(displayData.activities) ? displayData.activities : [];
  const supportActivities = Array.isArray(supportData.activities) ? supportData.activities : [];
  return {
    ...support,
    ...display,
    ts: String(primary.ts || "") >= String(secondary.ts || "") ? primary.ts : secondary.ts,
    data: {
      ...supportData,
      ...displayData,
      text:
        typeof displayData.text === "string"
          ? displayData.text
          : typeof supportData.text === "string"
            ? supportData.text
            : "",
      activities: displayActivities.length > 0 ? displayActivities : supportActivities,
      pending_event_id:
        String(displayData.pending_event_id || "").trim() ||
        String(supportData.pending_event_id || "").trim() ||
        undefined,
      stream_id:
        String(displayData.stream_id || "").trim() ||
        String(supportData.stream_id || "").trim() ||
        undefined,
      pending_placeholder: Boolean(displayData.pending_placeholder),
    },
  };
}
