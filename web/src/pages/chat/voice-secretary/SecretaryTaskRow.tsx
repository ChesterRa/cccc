import { useTranslation } from "react-i18next";
import type { SecretaryTaskSummary } from "../../../types";
import { Button } from "../../../components/ui/button";
import { LazyMarkdownRenderer } from "../../../components/LazyMarkdownRenderer";
import type { SecretaryTasksController } from "./useSecretaryTasks";
import { secretaryTaskSubject, secretaryTaskTime } from "./secretaryTaskPresentation";
import { secretaryTaskRunning } from "./secretaryTaskLineModel";

export function SecretaryTaskRow({
  task,
  controller,
  isDark,
  open,
  onShowExecution,
  embedded = false,
}: {
  task: SecretaryTaskSummary;
  controller: SecretaryTasksController;
  isDark: boolean;
  open?: boolean;
  embedded?: boolean;
  onShowExecution?: (task: SecretaryTaskSummary) => void;
}) {
  const { t, i18n } = useTranslation("settings");
  const { busy, followups, setFollowup, candidate, copyCandidate, act } = controller;
  const running = secretaryTaskRunning(task);
  const terminal = !running && task.cleanup_confirmed && !task.superseded_by;
  const body = (
    <div className="mt-2 space-y-2 text-xs leading-5">
      {task.receipt?.status === "needs_user" && (
        <p className="whitespace-pre-wrap break-words">{task.receipt.output.reply_text}</p>
      )}
      {task.receipt?.output.handoff_text && (
        <p className="whitespace-pre-wrap break-words">
          {t("voiceSettings.handoffProposal", { target: task.receipt.output.handoff_target })}
          <br />
          {task.receipt.output.handoff_text}
        </p>
      )}
      {task.diagnostic && (
        <p className="whitespace-pre-wrap break-words text-[var(--color-text-secondary)]">
          {task.diagnostic}
        </p>
      )}
      {task.projection_error && (
        <p role="alert" className="break-words text-rose-600 dark:text-rose-300">
          {task.projection_error}
        </p>
      )}
      {!task.cleanup_confirmed && !running && <p>{t("voiceSettings.cleanupPending")}</p>}
      {terminal && task.phase === "needs_user" && (
        <textarea
          value={followups[task.task_id] || ""}
          onChange={(e) => setFollowup(task.task_id, e.target.value)}
          aria-label={t("voiceSettings.followup")}
          placeholder={t("voiceSettings.followup")}
          maxLength={8000}
          className="glass-input min-h-20 w-full rounded-lg p-3"
        />
      )}
      <div className="flex flex-wrap gap-2">
        {running && (
          <Button
            size="sm"
            variant="secondary"
            disabled={!!busy}
            onClick={() => void act(task, "cancel")}
          >
            {t("voiceSettings.cancelTask")}
          </Button>
        )}
        {terminal && (task.phase !== "done" || !!task.projection_error) && (
          <Button
            size="sm"
            variant="secondary"
            disabled={!!busy || (task.phase === "needs_user" && !followups[task.task_id]?.trim())}
            onClick={() => void act(task, "retry")}
          >
            {t(
              task.phase === "needs_user"
                ? "voiceSettings.continueTask"
                : task.phase === "done"
                  ? "voiceSettings.syncResult"
                  : "voiceSettings.retryTask",
            )}
          </Button>
        )}
        {!running &&
          task.cleanup_confirmed &&
          task.candidate_available &&
          task.phase !== "done" && (
            <Button
              size="sm"
              variant="secondary"
              disabled={!!busy}
              onClick={() => void act(task, "candidate")}
            >
              {t("voiceSettings.viewCandidate")}
            </Button>
          )}
        {terminal && task.receipt?.output.handoff_text && (
          <Button
            size="sm"
            variant="secondary"
            disabled={!!busy || !!task.forwarded_event_id}
            onClick={() => void act(task, "handoff")}
          >
            {t(
              task.forwarded_event_id ? "voiceSettings.forwarded" : "voiceSettings.forwardProposal",
            )}
          </Button>
        )}
        {onShowExecution && (
          <Button size="sm" variant="ghost" onClick={() => onShowExecution(task)}>
            {t("voiceSettings.viewExecution")}
          </Button>
        )}
      </div>
      {candidate?.taskId === task.task_id && (
        <div className="min-w-0 rounded-lg border border-[var(--glass-border-subtle)] p-3">
          <p className="mb-2 text-[var(--color-text-muted)]">{t("voiceSettings.candidateHint")}</p>
          <Button size="sm" variant="secondary" onClick={() => copyCandidate(candidate.content)}>
            {t("common:copy")}
          </Button>
          <div className="mt-3 max-h-80 overflow-y-auto">
            <LazyMarkdownRenderer content={candidate.content} isDark={isDark} />
          </div>
        </div>
      )}
    </div>
  );
  if (embedded) return body;
  return (
    <details open={open} className="py-2">
      <summary className="cursor-pointer text-xs leading-5 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]">
        <span className="break-words">
          {t(`voiceSettings.kinds.${task.target.kind}`)} · {secretaryTaskSubject(task)}
        </span>
        <span className="ml-2 text-[var(--color-text-muted)]">
          {task.cancellation_reason === "shutdown"
            ? t("voiceSettings.interrupted")
            : t(`voiceSettings.phases.${task.phase}`)}
        </span>
        <time dateTime={task.created_at} className="ml-2 text-[var(--color-text-muted)]">
          {secretaryTaskTime(task, i18n?.language)}
        </time>
      </summary>
      {body}
    </details>
  );
}
