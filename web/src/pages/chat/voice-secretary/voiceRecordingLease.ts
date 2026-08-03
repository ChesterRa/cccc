import type { ApiResponse } from "../../../services/api/base";
import type { AssistantVoiceRecordingLeaseResult } from "../../../types";

export function voiceRecordingLeaseIsDefinitelyLost(
  response: ApiResponse<AssistantVoiceRecordingLeaseResult>,
): boolean {
  if (response.ok) return Boolean(response.result.lost);
  return (
    response.error.code === "assistant_voice_recording_busy" ||
    response.error.code === "assistant_voice_recording_lease_lost"
  );
}
