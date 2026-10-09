import type { SecretaryTaskSummary } from "../../../types";

/** Keep task labels independent of transport prompts and opaque scope IDs. */
export function secretaryTaskSubject(task: SecretaryTaskSummary, documentTitle?: string): string {
  const text =
    task.target.kind === "document"
      ? documentTitle || task.target.document_path.split("/").pop() || task.preview
      : task.preview;
  return text.replace(/\s+/g, " ").trim();
}

export function secretaryTaskTime(task: SecretaryTaskSummary, language?: string): string {
  const date = new Date(task.created_at);
  return Number.isNaN(date.getTime())
    ? ""
    : date.toLocaleString(language, {
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
      });
}

/** Completion and delivery/application are separate, authoritative outcomes. */
export function secretaryTaskOutcome(task: SecretaryTaskSummary): string | null {
  if (task.phase !== "done") return null;
  if (task.projection_error) return "syncFailed";
  if (task.target.kind === "prompt") {
    if (task.receipt?.output.no_op || task.prompt_draft_status === "no_change") return "noChange";
    switch (task.prompt_draft_status) {
      case "applied":
        return "applied";
      case "stale":
        return "stale";
      case "dismissed":
        return "dismissed";
      default:
        return "draftReady";
    }
  }
  if (task.target.kind === "document" && task.receipt?.document_version) return "documentWritten";
  if (task.target.kind === "ask" && task.projected_at) return "answerReady";
  return "completed";
}
