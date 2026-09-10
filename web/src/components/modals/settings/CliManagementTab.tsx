import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import type {
  CliJob,
  CliManagementState,
  CliManagementStatus,
  CliSchedule,
} from "../../../services/api";
import { CliCronSummary, CliScheduleEditor } from "./CliScheduleEditor";
import { localTimeZone, nowId } from "./automationUtils";
import {
  secondaryButtonClass,
  settingsWorkspaceBodyClass,
  settingsWorkspaceHeaderClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceShellClass,
  preClass,
} from "./types";

function errorMessage(error: { code: string; message: string }, fallback: string): string {
  return ["NETWORK_ERROR", "EMPTY_RESPONSE", "PARSE_ERROR", "HTTP_ERROR"].includes(error.code)
    ? fallback
    : error.message;
}

export function CliManagementTab({ isDark }: { isDark: boolean }) {
  const { t } = useTranslation("settings");
  const [data, setData] = useState<CliManagementStatus | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [logJobId, setLogJobId] = useState<string | null>(null);
  const [editing, setEditing] = useState<{
    initial: CliSchedule;
    sourceId: string | null;
    state: CliManagementState;
  } | null>(null);
  const operationPending = useRef(false);
  const reloadPending = useRef(false);
  const readGeneration = useRef(0);
  const mounted = useRef(false);
  // 响应丢失时复用请求 ID；同一按钮重试不会生成第二次安装。
  const pendingRequests = useRef(new Map<string, string>());

  const reload = useCallback(async () => {
    if (reloadPending.current || operationPending.current) return;
    const generation = readGeneration.current;
    reloadPending.current = true;
    try {
      const response = await api.fetchCliManagement();
      if (!mounted.current || generation !== readGeneration.current) return;
      if (!response.ok) {
        setError(errorMessage(response.error, t("cliManagement.loadFailed")));
        return;
      }
      setData(response.result);
      setError("");
    } catch {
      if (mounted.current && generation === readGeneration.current)
        setError(t("cliManagement.loadFailed"));
    } finally {
      reloadPending.current = false;
      if (mounted.current) setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    mounted.current = true;
    void reload();
    const timer = window.setInterval(() => {
      void reload();
    }, 5000);
    return () => {
      mounted.current = false;
      readGeneration.current += 1;
      window.clearInterval(timer);
    };
  }, [reload]);

  const run = async (
    action: () => Promise<void>,
    uncertainMessage = t("cliManagement.requestUncertain"),
  ) => {
    if (operationPending.current) return;
    operationPending.current = true;
    readGeneration.current += 1;
    setBusy(true);
    setNotice("");
    try {
      await action();
    } catch {
      setNotice(uncertainMessage);
    } finally {
      operationPending.current = false;
      if (mounted.current) {
        setBusy(false);
        void reload();
      }
    }
  };

  const submit = (runtime: string, operation: api.CliOperation) =>
    run(async () => {
      const key = `${runtime}:${operation}`;
      const id = pendingRequests.current.get(key) || crypto.randomUUID();
      pendingRequests.current.set(key, id);
      const response = await api.submitCliJob(runtime, operation, id);
      if (!mounted.current) return;
      if (!response.ok) {
        setNotice(errorMessage(response.error, t("cliManagement.requestUncertain")));
        return;
      }
      pendingRequests.current.delete(key);
      setLogJobId(response.result.job.id);
      setData((previous) =>
        previous
          ? {
              ...previous,
              state: {
                ...previous.state,
                jobs: { ...previous.state.jobs, [response.result.job.id]: response.result.job },
              },
            }
          : previous,
      );
      setNotice(t("cliManagement.jobSubmitted"));
    });

  const saveRules = (state: CliManagementState, rules: CliSchedule[]) =>
    run(async () => {
      const response = await api.saveCliSchedules(
        state.revision,
        rules.map(({ id, enabled, trigger }) => ({ id, enabled, trigger })),
      );
      if (!mounted.current) return;
      if (!response.ok) {
        setNotice(
          response.error.code === "cli_schedule_revision_conflict"
            ? t("cliManagement.scheduleConflict")
            : errorMessage(response.error, t("cliManagement.saveUncertain")),
        );
        return;
      }
      setData((previous) => (previous ? { ...previous, state: response.result.state } : previous));
      setEditing(null);
      setNotice(t("cliManagement.scheduleSaved"));
    }, t("cliManagement.saveUncertain"));

  const jobs = Object.values(data?.state.jobs || {}).sort((a, b) =>
    b.created_at.localeCompare(a.created_at),
  );
  const [showAllJobs, setShowAllJobs] = useState(false);
  const logJob = jobs.find((job) => job.id === logJobId);

  return (
    <div className="space-y-5">
      <div className={settingsWorkspaceShellClass(isDark)}>
        <div className={settingsWorkspaceHeaderClass(isDark)}>
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-[var(--color-text-primary)]">
              {t("cliManagement.title")}
            </h3>
            <p className="mt-1 text-xs text-[var(--color-text-secondary)]">
              {t("cliManagement.description")}
            </p>
          </div>
          <button
            type="button"
            className={secondaryButtonClass("sm")}
            disabled={busy || loading}
            onClick={() => void reload()}
          >
            {t("cliManagement.refresh")}
          </button>
        </div>
        <div className={settingsWorkspaceBodyClass}>
          {error && (
            <p role="alert" className="text-xs text-rose-600 dark:text-rose-400">
              {error}
            </p>
          )}
          {notice && (
            <p
              role="status"
              className="text-sm whitespace-pre-wrap text-[var(--color-text-primary)]"
            >
              {notice}
            </p>
          )}
          <p className="text-xs text-[var(--color-text-secondary)]">
            {t("cliManagement.sourceHint")}
          </p>
          {loading && !data && (
            <p role="status" className="text-sm">
              {t("common:loading")}
            </p>
          )}
          {data && (
            <div className="divide-y divide-[var(--glass-border-subtle)]">
              {data.runtimes.map((runtime) => {
                if (runtime.source.kind === "not_applicable") return null;
                const job = jobs.find(
                  (item) =>
                    item.runtime === runtime.name &&
                    (item.status === "queued" || item.status === "running"),
                );
                return (
                  <div key={runtime.name} className="py-3 first:pt-0 space-y-2">
                    <div className="flex flex-wrap items-start justify-between gap-3">
                      <div className="min-w-0 flex-1">
                        <h4 className="text-sm font-semibold">{runtime.display_name}</h4>
                        <p className="text-xs text-[var(--color-text-secondary)]">
                          {runtime.installation
                            ? t("cliManagement.managedVersion", {
                                version: runtime.installation.version,
                              })
                            : runtime.external_available
                              ? t("cliManagement.externalInstallation")
                              : t("cliManagement.notInstalled")}
                        </p>
                        <p className="text-xs font-mono break-all text-[var(--color-text-secondary)]">
                          {runtime.installation?.executable ||
                            runtime.external_path ||
                            runtime.command}
                        </p>
                        {runtime.managed_error && (
                          <p role="alert" className="text-xs text-rose-600 dark:text-rose-400">
                            {t("cliManagement.managedUnavailable")}
                          </p>
                        )}
                      </div>
                      <div className="flex flex-wrap gap-2">
                        {!runtime.installation && (
                          <button
                            type="button"
                            className={secondaryButtonClass("sm")}
                            disabled={busy || Boolean(job)}
                            onClick={() => void submit(runtime.name, "install")}
                            aria-label={`${t(runtime.external_available ? "cliManagement.installManaged" : "cliManagement.install")} ${runtime.display_name}`}
                          >
                            {t(
                              runtime.external_available
                                ? "cliManagement.installManaged"
                                : "cliManagement.install",
                            )}
                          </button>
                        )}
                        <button
                          type="button"
                          className={secondaryButtonClass("sm")}
                          disabled={busy || Boolean(job) || !runtime.installation}
                          onClick={() => void submit(runtime.name, "update")}
                          aria-label={`${t("cliManagement.update")} ${runtime.display_name}`}
                        >
                          {t("cliManagement.update")}
                        </button>
                        <button
                          type="button"
                          className={secondaryButtonClass("sm")}
                          disabled={
                            busy ||
                            Boolean(job) ||
                            !runtime.installation ||
                            !runtime.uninstall_available
                          }
                          onClick={() => {
                            if (
                              window.confirm(
                                t("cliManagement.uninstallConfirm", { name: runtime.display_name }),
                              )
                            )
                              void submit(runtime.name, "uninstall");
                          }}
                          aria-label={`${t("cliManagement.uninstall")} ${runtime.display_name}`}
                        >
                          {t("cliManagement.uninstall")}
                        </button>
                      </div>
                    </div>
                    {job && (
                      <p role="status" className="text-xs">
                        {t(`cliManagement.status.${job.status}`)} ·{" "}
                        {t(`cliManagement.${job.operation}`)}
                      </p>
                    )}
                  </div>
                );
              })}
            </div>
          )}
        </div>
      </div>

      {data && (
        <section className={settingsWorkspaceShellClass(isDark)}>
          <div className={settingsWorkspaceHeaderClass(isDark)}>
            <div className="min-w-0">
              <h3 className="text-sm font-semibold">{t("cliManagement.schedules")}</h3>
              <p className="mt-1 text-xs text-[var(--color-text-secondary)]">
                {t("cliManagement.schedulesHint")}
              </p>
            </div>
            <button
              type="button"
              className={secondaryButtonClass("sm")}
              disabled={busy || Boolean(editing) || data.state.rules.length >= 64}
              onClick={() => {
                setNotice("");
                setEditing({
                  sourceId: null,
                  state: data.state,
                  initial: {
                    id: nowId("cli_update"),
                    enabled: false,
                    trigger: { kind: "cron", cron: "0 3 * * *", timezone: localTimeZone() },
                  },
                });
              }}
            >
              {t("cliManagement.addSchedule")}
            </button>
          </div>
          <div className={settingsWorkspaceBodyClass}>
            {data.state.rules.length === 0 && !editing && (
              <p className="text-sm text-[var(--color-text-secondary)]">
                {t("cliManagement.noSchedules")}
              </p>
            )}
            {data.state.rules.map((rule) => (
              <div
                key={rule.id}
                className="flex flex-wrap items-center justify-between gap-3 border-b border-[var(--glass-border-subtle)] pb-3"
              >
                <div className="min-w-0 flex-1">
                  <div className="font-mono text-sm break-all">{rule.id}</div>
                  <p className="text-xs text-[var(--color-text-secondary)]">
                    {rule.enabled ? t("cliManagement.enabled") : t("cliManagement.disabled")} ·{" "}
                    {rule.trigger.kind === "cron" ? (
                      <CliCronSummary
                        cron={rule.trigger.cron}
                        timezone={rule.trigger.timezone || "UTC"}
                      />
                    ) : rule.trigger.kind === "interval" ? (
                      t("cliManagement.everyMinutes", { minutes: rule.trigger.every_seconds / 60 })
                    ) : (
                      formatDate(rule.trigger.at)
                    )}
                  </p>
                  <p className="text-xs text-[var(--color-text-secondary)]">
                    {t("cliManagement.nextRun", {
                      time: rule.next_run_at
                        ? formatDate(rule.next_run_at)
                        : t("cliManagement.notScheduled"),
                    })}
                  </p>
                </div>
                <div className="flex flex-wrap gap-2">
                  <button
                    type="button"
                    className={secondaryButtonClass("sm")}
                    disabled={busy || Boolean(editing)}
                    onClick={() => {
                      setNotice("");
                      setEditing({
                        sourceId: rule.id,
                        initial: { id: rule.id, enabled: rule.enabled, trigger: rule.trigger },
                        state: data.state,
                      });
                    }}
                  >
                    {t("common:edit")}
                  </button>
                  <button
                    type="button"
                    className={secondaryButtonClass("sm")}
                    disabled={busy || Boolean(editing)}
                    onClick={() =>
                      void saveRules(
                        data.state,
                        data.state.rules.filter((item) => item.id !== rule.id),
                      )
                    }
                    aria-label={`${t("cliManagement.removeSchedule")} ${rule.id}`}
                  >
                    {t("cliManagement.removeSchedule")}
                  </button>
                </div>
              </div>
            ))}
            {editing && (
              <div className={settingsWorkspacePanelClass(isDark)}>
                {notice && (
                  <p role="status" className="mb-3 text-sm text-[var(--color-text-primary)]">
                    {notice}
                  </p>
                )}
                <CliScheduleEditor
                  key={editing.initial.id}
                  initial={editing.initial}
                  busy={busy}
                  onCancel={() => {
                    setEditing(null);
                    setNotice("");
                  }}
                  onSave={async (rule) => {
                    const rules = editing.state.rules.filter(
                      (item) => item.id !== editing.sourceId,
                    );
                    if (rules.some((item) => item.id === rule.id)) {
                      setNotice(t("cliManagement.duplicateSchedule"));
                      return;
                    }
                    await saveRules(editing.state, [...rules, rule]);
                  }}
                />
              </div>
            )}
          </div>
        </section>
      )}

      {data && (
        <section className={settingsWorkspaceShellClass(isDark)}>
          <div className={settingsWorkspaceHeaderClass(isDark)}>
            <h3 className="text-sm font-semibold">{t("cliManagement.jobs")}</h3>
          </div>
          <div className={settingsWorkspaceBodyClass}>
            {jobs.length === 0 && (
              <p className="text-sm text-[var(--color-text-secondary)]">
                {t("cliManagement.noJobs")}
              </p>
            )}
            {(showAllJobs ? jobs : jobs.slice(0, 20)).map((job) => (
              <div
                key={job.id}
                data-job-id={job.id}
                className="space-y-1 border-b border-[var(--glass-border-subtle)] pb-3"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="text-sm">
                    {job.runtime} · {t(`cliManagement.${job.operation}`)} ·{" "}
                    {t(`cliManagement.status.${job.status}`)}
                  </div>
                  <button
                    type="button"
                    className={secondaryButtonClass("sm")}
                    onClick={() => setLogJobId(job.id)}
                  >
                    {t("cliManagement.viewLog")}
                  </button>
                </div>
                <p className="text-xs text-[var(--color-text-secondary)]">
                  {t(
                    job.source_rule
                      ? "cliManagement.scheduledOperation"
                      : "cliManagement.manualOperation",
                  )}
                  {job.source_rule ? ` · ${job.source_rule}` : ""}
                </p>
                <p className="text-xs text-[var(--color-text-secondary)]">
                  {t("cliManagement.createdAt")}: {formatDate(job.created_at)}
                  {job.started_at
                    ? ` · ${t("cliManagement.startedAt")}: ${formatDate(job.started_at)}`
                    : ""}
                  {job.finished_at
                    ? ` · ${t("cliManagement.finishedAt")}: ${formatDate(job.finished_at)}`
                    : ""}
                </p>
                {job.error && (
                  <p className="text-xs break-words text-rose-600 dark:text-rose-400">
                    {job.error}
                  </p>
                )}
              </div>
            ))}
            {jobs.length > 20 && (
              <button
                type="button"
                className={secondaryButtonClass("sm")}
                onClick={() => setShowAllJobs(!showAllJobs)}
              >
                {showAllJobs ? t("cliManagement.recentJobs") : t("cliManagement.allJobs")}
              </button>
            )}
            {logJobId && (
              <CliJobLog
                key={logJobId}
                jobId={logJobId}
                job={logJob}
                onClose={() => setLogJobId(null)}
              />
            )}
          </div>
        </section>
      )}
    </div>
  );
}

function formatDate(value: string): string {
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? date.toLocaleString() : value;
}

function CliJobLog({ jobId, job, onClose }: { jobId: string; job?: CliJob; onClose: () => void }) {
  const { t } = useTranslation("settings");
  const [page, setPage] = useState<api.CliLogPage | null>(null);
  const [offset, setOffset] = useState(0);
  const [history, setHistory] = useState<number[]>([]);
  const [error, setError] = useState("");
  useEffect(() => {
    let active = true;
    let pending = false;
    setPage(null);
    const read = async () => {
      if (pending) return;
      pending = true;
      try {
        const response = await api.fetchCliJobLog(jobId, offset);
        if (!active) return;
        if (!response.ok) {
          setError(errorMessage(response.error, t("cliManagement.logFailed")));
          return;
        }
        setPage(response.result);
        setError("");
      } catch {
        if (active) setError(t("cliManagement.logFailed"));
      } finally {
        pending = false;
      }
    };
    void read();
    const timer = window.setInterval(() => {
      void read();
    }, 3000);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [jobId, offset, job?.status, t]);
  return (
    <div>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <h4 className="text-sm font-semibold break-all">
          {t("cliManagement.jobLog")} · {jobId}
        </h4>
        <button type="button" className={secondaryButtonClass("sm")} onClick={onClose}>
          {t("common:close")}
        </button>
      </div>
      {error && (
        <p role="alert" className="text-xs text-rose-600 dark:text-rose-400">
          {error}
        </p>
      )}
      <pre
        className={`${preClass()} max-h-80 overflow-y-auto`}
        tabIndex={0}
        aria-label={t("cliManagement.jobLog")}
      >
        {page
          ? page.entries
              .map((entry) => `${entry.ts} [${entry.stream}] ${entry.text.trimEnd()}`)
              .join("\n") || t("cliManagement.waitingLog")
          : t("common:loading")}
      </pre>
      <div className="mt-2 flex flex-wrap gap-2">
        <button
          type="button"
          className={secondaryButtonClass("sm")}
          disabled={history.length === 0}
          onClick={() => {
            setOffset(history[history.length - 1] || 0);
            setHistory(history.slice(0, -1));
          }}
        >
          {t("cliManagement.previousPage")}
        </button>
        <button
          type="button"
          className={secondaryButtonClass("sm")}
          disabled={!page?.has_more}
          onClick={() => {
            if (page) {
              setHistory([...history, offset]);
              setOffset(page.next_offset);
            }
          }}
        >
          {t("cliManagement.nextPage")}
        </button>
      </div>
    </div>
  );
}
