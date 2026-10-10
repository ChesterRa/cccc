import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { AssistantVoiceDocument, SecretaryTaskSummary } from "../../../types";
import { ChevronDownIcon } from "../../../components/Icons";
import { secretaryTaskSubject, secretaryTaskTime } from "./secretaryTaskPresentation";
import { SecretaryRuntimePanel } from "./SecretaryRuntimePanel";
import { SecretaryTaskStage } from "./SecretaryTaskStage";
import { SecretaryTaskRow } from "./SecretaryTaskRow";
import { useSecretaryTasks } from "./useSecretaryTasks";
import { secretaryTaskNeedsAttention, summarizeSecretaryTasks } from "./secretaryTaskLineModel";
import { useUIStore } from "../../../stores/useUIStore";

export function SecretaryTasks({
  groupId,
  groupTitle,
  active,
  isDark,
  workspace = false,
  kinds,
  initialTaskId,
  hideReadinessError = false,
  onShowExecution,
  onOpenTarget,
  canOpenTarget,
  documents,
  onRetryAccepted,
}: {
  documents?: AssistantVoiceDocument[];
  groupId: string;
  groupTitle?: string;
  active: boolean;
  isDark: boolean;
  workspace?: boolean;
  /** Restricts the compact status line to these task kinds; the workspace shows all. */
  kinds?: SecretaryTaskSummary["target"]["kind"][];
  initialTaskId?: string;
  hideReadinessError?: boolean;
  onOpenTarget?: (task: SecretaryTaskSummary) => void;
  canOpenTarget?: (task: SecretaryTaskSummary) => boolean;
  onShowExecution?: (task: SecretaryTaskSummary) => void;
  onRetryAccepted?: (task: SecretaryTaskSummary) => void;
}) {
  const { t, i18n } = useTranslation("settings");
  const runtimeAccessible = useUIStore((state) => state.canAccessGlobalSettings) !== false;
  const controller = useSecretaryTasks(groupId, active, onRetryAccepted);
  const { tasks, coverage, error, loadError } = controller;
  const [selectedTask, setSelectedTask] = useState<{ groupId: string; taskId: string } | null>(() =>
    initialTaskId ? { groupId, taskId: initialTaskId } : null,
  );
  const historyTrigger = useRef<HTMLButtonElement>(null);
  const [historyOpen, setHistoryOpen] = useState(false);
  // History follows its displayed creation time; reminders keep the API's unresolved-first order.
  const groupTasks = tasks
    .filter((task) => task.target.group_id === groupId)
    .sort(
      (a, b) =>
        Date.parse(b.created_at) - Date.parse(a.created_at) || b.task_id.localeCompare(a.task_id),
    );
  const documentTitle = (task: SecretaryTaskSummary) =>
    documents?.find(
      (doc) =>
        doc.document_id === task.target.document_id &&
        doc.document_path === task.target.document_path,
    )?.title;
  const lineTasks = kinds ? tasks.filter((task) => kinds.includes(task.target.kind)) : tasks;
  const selectedId = selectedTask?.groupId === groupId ? selectedTask.taskId : "";
  const stageTask = groupTasks.find((task) => task.task_id === selectedId);
  const readinessErrorDuplicated =
    hideReadinessError || (workspace && !stageTask && runtimeAccessible);
  const visibleError =
    error || loadError || (readinessErrorDuplicated ? "" : coverage.readiness_error);
  useEffect(() => {
    setSelectedTask(initialTaskId ? { groupId, taskId: initialTaskId } : null);
  }, [groupId, initialTaskId]);
  const coverageNotes = (
    <>
      {coverage.deferred > 0 && (
        <p role="status" className="mt-2 text-xs leading-5 text-[var(--color-text-secondary)]">
          {t(
            coverage.ready
              ? "voiceSettings.sourcesDeferred"
              : coverage.readiness_code === "not_configured"
                ? "voiceSettings.sourcesUnconfigured"
                : "voiceSettings.sourcesUnavailable",
            { count: coverage.deferred },
          )}
        </p>
      )}
      {coverage.unprocessed > 0 && (
        <p className="mt-2 text-xs leading-5 text-[var(--color-text-secondary)]">
          {t("voiceSettings.sourcesUnprocessed", { count: coverage.unprocessed })}
        </p>
      )}
      {coverage.held > 0 && (
        <p role="status" className="mt-2 text-xs leading-5 text-[var(--color-text-secondary)]">
          {t("voiceSettings.sourcesHeld", { count: coverage.held })}
        </p>
      )}
      {coverage.invalid > 0 && (
        <p role="status" className="mt-2 text-xs leading-5 text-[var(--color-text-secondary)]">
          {t("voiceSettings.sourcesInvalid", { count: coverage.invalid })}
        </p>
      )}
      {visibleError && (
        <p role="alert" className="mt-2 text-xs text-rose-600 dark:text-rose-300">
          {visibleError}
        </p>
      )}
    </>
  );

  if (!workspace) {
    if (
      !lineTasks.length &&
      !visibleError &&
      !coverage.deferred &&
      !coverage.held &&
      !coverage.invalid
    )
      return null;
    const summary = summarizeSecretaryTasks(lineTasks, coverage);
    const focusTask = lineTasks.find(secretaryTaskNeedsAttention);
    const summaryText =
      summary.kind === "running" || summary.kind === "queued"
        ? t(`voiceSettings.taskLine.${summary.kind}`, { count: summary.count }) +
          (summary.pending
            ? ` · ${t("voiceSettings.taskLine.pending", { count: summary.pending })}`
            : "")
        : summary.kind === "attention"
          ? t("voiceSettings.taskLine.attention", { count: summary.count })
          : summary.kind === "recent"
            ? t("voiceSettings.taskLine.recent", { count: summary.count })
            : "";
    return (
      <section
        className="min-w-0 border-b border-[var(--glass-border-subtle)] px-1 py-2"
        aria-label={t("voiceSettings.tasks")}
        data-secretary-tasks
      >
        {summaryText ? (
          <button
            type="button"
            aria-expanded={historyOpen}
            onClick={() => setHistoryOpen((open) => !open)}
            className="flex min-h-9 w-full items-center gap-2 rounded-lg px-1 text-left text-xs font-medium text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-tertiary)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
          >
            <span
              aria-hidden="true"
              className={
                summary.kind === "attention"
                  ? "h-2 w-2 shrink-0 rounded-full bg-amber-500"
                  : summary.kind === "running"
                    ? "h-2 w-2 shrink-0 animate-pulse rounded-full bg-[var(--color-accent-primary)]"
                    : "h-2 w-2 shrink-0 rounded-full bg-[var(--color-text-tertiary)]"
              }
            />
            <span className="min-w-0 flex-1 truncate">{summaryText}</span>
            <ChevronDownIcon
              size={14}
              aria-hidden="true"
              className={historyOpen ? "shrink-0 rotate-180" : "shrink-0"}
            />
          </button>
        ) : null}
        {coverageNotes}
        {focusTask && !historyOpen ? (
          <SecretaryTaskRow
            key={focusTask.task_id}
            task={focusTask}
            controller={controller}
            isDark={isDark}
            open
            onShowExecution={onShowExecution}
          />
        ) : null}
        <div hidden={!historyOpen} className="divide-y divide-[var(--glass-border-subtle)]">
          {(historyOpen ? lineTasks : lineTasks.filter((task) => task !== focusTask)).map(
            (task) => (
              <SecretaryTaskRow
                key={task.task_id}
                task={task}
                controller={controller}
                isDark={isDark}
                onShowExecution={onShowExecution}
              />
            ),
          )}
        </div>
      </section>
    );
  }

  return (
    <section
      className="flex min-h-0 min-w-0 flex-1 flex-col gap-4"
      aria-label={stageTask ? t("voiceSettings.executionTitle") : t("voiceSettings.resident.title")}
      data-secretary-tasks
    >
      <div className="flex shrink-0 flex-wrap items-center justify-between gap-2">
        <h3 className="text-sm font-medium text-[var(--color-text-secondary)]">
          {stageTask ? t("voiceSettings.executionTitle") : t("voiceSettings.resident.title")}
        </h3>
        {groupTasks.length > 0 && (
          <button
            ref={historyTrigger}
            type="button"
            aria-expanded={historyOpen}
            onClick={() => setHistoryOpen((value) => !value)}
            className="inline-flex min-h-10 items-center gap-2 rounded-lg px-2 text-xs font-medium text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-tertiary)] focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]"
          >
            {groupTitle ? `${groupTitle} · ` : ""}
            {t("voiceSettings.recentTasks", { count: groupTasks.length })}
            <ChevronDownIcon
              size={14}
              aria-hidden="true"
              className={historyOpen ? "rotate-180" : ""}
            />
          </button>
        )}
      </div>
      {historyOpen && (
        <ul
          aria-label={t("voiceSettings.executionChooseTask")}
          className="max-h-[35dvh] shrink-0 overflow-y-auto rounded-lg border border-[var(--glass-border-subtle)] scrollbar-subtle"
        >
          {groupTasks.map((task) => (
            <li key={task.task_id}>
              <button
                type="button"
                aria-current={stageTask?.task_id === task.task_id ? "true" : undefined}
                onClick={() => {
                  setSelectedTask({ groupId, taskId: task.task_id });
                  setHistoryOpen(false);
                  historyTrigger.current?.focus();
                }}
                className="flex w-full flex-col gap-1 px-3 py-2.5 text-left hover:bg-[var(--color-bg-tertiary)] aria-[current=true]:bg-[var(--color-bg-tertiary)] focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-[var(--color-border-focus)]"
              >
                <span className="line-clamp-2 break-words text-sm">
                  {t(`voiceSettings.kinds.${task.target.kind}`)}
                  {secretaryTaskSubject(task, documentTitle(task))
                    ? ` · ${secretaryTaskSubject(task, documentTitle(task))}`
                    : ""}
                </span>
                <span className="flex w-full flex-wrap justify-between gap-x-3 text-xs text-[var(--color-text-muted)]">
                  <span>
                    {t(
                      task.cancellation_reason === "shutdown"
                        ? "voiceSettings.interrupted"
                        : `voiceSettings.phases.${task.phase}`,
                    )}
                    {task.projection_error ? ` · ${t("voiceSettings.outcome.syncFailed")}` : ""}
                  </span>
                  <time dateTime={task.created_at}>{secretaryTaskTime(task, i18n?.language)}</time>
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <div className="shrink-0 empty:hidden">{coverageNotes}</div>
      {stageTask && (
        <button
          type="button"
          onClick={() => setSelectedTask(null)}
          className="w-fit shrink-0 rounded px-2 py-2 text-sm text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-tertiary)] focus-visible:outline-2"
        >
          {t("voiceSettings.resident.backToSecretary")}
        </button>
      )}
      {!stageTask ? (
        <SecretaryRuntimePanel active={active} isDark={isDark} />
      ) : (
        <SecretaryTaskStage
          key={`${groupId}:${stageTask?.task_id || "empty"}`}
          task={stageTask}
          documentTitle={stageTask ? documentTitle(stageTask) : undefined}
          active={active}
          isDark={isDark}
          controller={controller}
          onOpenTarget={
            stageTask && (!canOpenTarget || canOpenTarget(stageTask)) ? onOpenTarget : undefined
          }
        />
      )}
    </section>
  );
}
