import { lazy, Suspense, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../../components/ui/button";
import { LazyMarkdownRenderer } from "../../../components/LazyMarkdownRenderer";
import { getSecretaryTerminalWebSocketUrl } from "../../../services/api";
import { RUNTIME_INFO, type SecretaryTaskSummary } from "../../../types";
import { SecretaryTaskRow } from "./SecretaryTaskRow";
import { secretaryTaskRunning } from "./secretaryTaskLineModel";
import {
  secretaryTaskOutcome,
  secretaryTaskSubject,
  secretaryTaskTime,
} from "./secretaryTaskPresentation";
import type { SecretaryTasksController } from "./useSecretaryTasks";

const NativeSessionTerminal = lazy(() =>
  import("../../../features/voice/NativeSessionTerminal").then((module) => ({
    default: module.NativeSessionTerminal,
  })),
);

export function SecretaryTaskStage({
  task,
  active,
  isDark,
  controller,
  onOpenTarget,
  documentTitle,
}: {
  documentTitle?: string;
  task: SecretaryTaskSummary | undefined;
  active: boolean;
  isDark: boolean;
  controller: SecretaryTasksController;
  onOpenTarget?: (task: SecretaryTaskSummary) => void;
}) {
  const { t, i18n } = useTranslation("settings");
  const execution = task?.execution;
  const generation = execution?.generation || "";
  const buildWebSocketUrl = useCallback(
    (query: string) => getSecretaryTerminalWebSocketUrl(generation, query),
    [generation],
  );
  if (!task)
    return (
      <p role="status" className="p-4 text-sm text-[var(--color-text-muted)]">
        {t("voiceSettings.executionEmpty")}
      </p>
    );

  const running = secretaryTaskRunning(task);
  const runtime = execution?.runtime;
  const runtimeLabel = runtime ? RUNTIME_INFO[runtime]?.label || runtime : "";
  const result = task.receipt?.output.reply_text || task.receipt?.output.draft_text;
  const outcome = secretaryTaskOutcome(task);
  const subject = secretaryTaskSubject(task, documentTitle);
  const nativeTerminal = execution?.native_terminal && running;
  return (
    <section
      data-secretary-stage
      className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto scrollbar-subtle"
    >
      <header className="flex shrink-0 flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <p role="status" className="mb-1 text-xs leading-5 text-[var(--color-text-secondary)]">
            {t(`voiceSettings.kinds.${task.target.kind}`)} ·{" "}
            {t(
              task.cancellation_reason === "shutdown"
                ? "voiceSettings.interrupted"
                : `voiceSettings.phases.${task.phase}`,
            )}
          </p>
          <h4 className="line-clamp-2 break-words text-base font-semibold" title={subject}>
            {subject || t(`voiceSettings.kinds.${task.target.kind}`)}
          </h4>
          <p className="mt-1 flex flex-wrap gap-x-3 text-xs leading-5 text-[var(--color-text-muted)]">
            <span>{t(`voiceSettings.destination.${task.target.kind}`)}</span>
            <time dateTime={task.created_at}>{secretaryTaskTime(task, i18n?.language)}</time>
            {runtimeLabel && <span>{runtimeLabel}</span>}
          </p>
          {task.target.document_path && (
            <p className="mt-1 break-words text-xs text-[var(--color-text-muted)]">
              {task.target.document_path}
            </p>
          )}
        </div>
        {running && (
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={!!controller.busy}
            onClick={() => void controller.act(task, "cancel")}
          >
            {t("voiceSettings.cancelTask")}
          </Button>
        )}
      </header>
      {execution?.activity && running && (
        <p
          role="status"
          className="shrink-0 break-words text-xs text-[var(--color-text-secondary)]"
        >
          {execution.activity}
        </p>
      )}
      {nativeTerminal ? (
        <div
          className="min-h-[12rem] flex-1 overflow-hidden rounded-lg border border-[var(--glass-border-subtle)]"
          data-secretary-native-terminal
        >
          <Suspense
            fallback={
              <p role="status" className="p-4 text-sm">
                {t("common:loading")}
              </p>
            }
          >
            <NativeSessionTerminal
              generation={generation}
              scopeId="voice-secretary"
              runtime={runtime}
              isVisible={active}
              buildWebSocketUrl={buildWebSocketUrl}
              lifecycleHint={t("voiceSettings.resident.terminalHint")}
              rejectedHint={t("voiceSettings.terminalRejected")}
              reconnectHint={t("voiceSettings.terminalReconnectHint")}
            />
          </Suspense>
        </div>
      ) : (
        <div
          data-secretary-progress
          className="shrink-0 space-y-3 border-t border-[var(--glass-border-subtle)] pt-4 text-sm leading-6"
        >
          {running ? (
            <>
              <p className="text-xs text-[var(--color-text-muted)]">
                {t(execution ? "voiceSettings.acpProgressHint" : "voiceSettings.executionWaiting")}
              </p>
              {execution?.progress && (
                <div className="whitespace-pre-wrap break-words">{execution.progress}</div>
              )}
            </>
          ) : (
            <>
              {outcome && (
                <p role="status" className="text-sm text-[var(--color-text-secondary)]">
                  {t(`voiceSettings.outcome.${outcome}`)}
                </p>
              )}
              {task.phase === "done" && result && (
                <LazyMarkdownRenderer content={result} isDark={isDark} />
              )}
              <SecretaryTaskRow task={task} controller={controller} isDark={isDark} embedded />
              <div className="flex flex-wrap gap-2">
                {onOpenTarget && (
                  <Button
                    type="button"
                    size="sm"
                    variant="secondary"
                    onClick={() => onOpenTarget(task)}
                  >
                    {t(`voiceSettings.openTarget.${task.target.kind}`)}
                  </Button>
                )}
                {result && task.phase === "done" && (
                  <Button
                    type="button"
                    size="sm"
                    variant="ghost"
                    onClick={() => controller.copyCandidate(result)}
                  >
                    {t("common:copy")}
                  </Button>
                )}
              </div>
            </>
          )}
        </div>
      )}
      <details className="shrink-0 text-xs leading-5 text-[var(--color-text-muted)]">
        <summary className="w-fit cursor-pointer rounded py-2 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]">
          {t("voiceSettings.technicalDetails")}
        </summary>
        <dl className="space-y-1 break-all pb-3">
          <div>
            <dt className="inline">{t("voiceSettings.taskId")}: </dt>
            <dd className="inline">{task.task_id}</dd>
          </div>
          {task.target.scope_key && (
            <div>
              <dt className="inline">{t("voiceSettings.workspaceId")}: </dt>
              <dd className="inline">{task.target.scope_key}</dd>
            </div>
          )}
          {task.target.request_id && (
            <div>
              <dt className="inline">{t("voiceSettings.requestId")}: </dt>
              <dd className="inline">{task.target.request_id}</dd>
            </div>
          )}
        </dl>
      </details>
    </section>
  );
}
