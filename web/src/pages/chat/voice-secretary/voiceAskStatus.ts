import type { AssistantVoiceAskFeedback, SecretaryTaskSummary } from "../../../types";
import { hasFinalAskReply } from "./voiceComposerUtils";
import { secretaryTaskRunning } from "./secretaryTaskLineModel";

export type VoiceAskNotice = { requestId: string; hidden: boolean };
const noticeKey = (groupId: string) => `cccc_voice_ask_notice:${encodeURIComponent(groupId)}`;

/** A tab remembers only its current request identity and whether its hint was hidden. */
export function readVoiceAskNotice(groupId: string): VoiceAskNotice {
  try {
    const value = JSON.parse(window.sessionStorage.getItem(noticeKey(groupId)) || "null");
    if (typeof value?.requestId === "string")
      return { requestId: value.requestId.trim(), hidden: value.hidden === true };
  } catch {
    // Storage can be unavailable; the mounted view still keeps its current notice.
  }
  return { requestId: "", hidden: false };
}

export function saveVoiceAskNotice(groupId: string, notice: VoiceAskNotice): void {
  try {
    window.sessionStorage.setItem(noticeKey(groupId), JSON.stringify(notice));
  } catch {
    // Hiding a hint must not fail or affect accepted work when storage is unavailable.
  }
}

export function currentVoiceAskTask(
  tasks: SecretaryTaskSummary[] | undefined,
  groupId: string,
  requestId: string,
  previous: SecretaryTaskSummary | null,
): SecretaryTaskSummary | null {
  const matches = (task: SecretaryTaskSummary) =>
    task.target.group_id === groupId &&
    task.target.request_id === requestId &&
    task.target.kind === "ask" &&
    !task.superseded_by;
  const candidate = tasks?.find(matches);
  const retained = previous && matches(previous) ? previous : null;
  // An accepted retry names its predecessor; a read begun before admission
  // cannot restore that predecessor while React is retiring the old observer.
  if (candidate && retained?.previous_task_id === candidate.task_id) return retained;
  return candidate || retained;
}

/** A retry preserves the old reply until the successor's result is actually projected. */
export function voiceAskReplyIsCurrent(
  task: SecretaryTaskSummary | null,
  feedback: AssistantVoiceAskFeedback | null,
): boolean {
  if (!hasFinalAskReply(feedback)) return false;
  if (!task) return true;
  if (feedback?.secretary_task_id) return feedback.secretary_task_id === task.task_id;
  if (secretaryTaskRunning(task)) return false;
  return !!task.projected_at;
}
