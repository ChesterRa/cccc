import type { ApiResponse } from "../../../services/api/base";
import type { AssistantVoiceRecordingLeaseResult } from "../../../types";

export type VoiceRecordingLeaseConflict = { ownerId: string; groupId: string; groupTitle?: string };

export function voiceRecordingLeaseConflictFromDetails(
  details: unknown,
): VoiceRecordingLeaseConflict | null {
  const record = details && typeof details === "object" ? (details as Record<string, unknown>) : {};
  const active =
    record.active_lease && typeof record.active_lease === "object"
      ? (record.active_lease as Record<string, unknown>)
      : {};
  const ownerId = String(active.owner_id || "").trim();
  const groupId = String(active.group_id || "").trim();
  if (!ownerId || !groupId) return null;
  return { ownerId, groupId, groupTitle: String(active.group_title || "").trim() || undefined };
}

export function voiceRecordingLeaseIsDefinitelyLost(
  response: ApiResponse<AssistantVoiceRecordingLeaseResult>,
): boolean {
  if (response.ok) return Boolean(response.result.lost);
  return (
    response.error.code === "assistant_voice_recording_busy" ||
    response.error.code === "assistant_voice_recording_lease_lost"
  );
}
