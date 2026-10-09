import { useEffect, useRef } from "react";
import type { TFunction } from "i18next";
import type {
  AssistantVoiceAskFeedback,
  AssistantVoiceDocument,
  SecretaryTaskSummary,
} from "../../../types";
import { MessageSquareQuoteIcon } from "../../../components/Icons";
import { LazyMarkdownRenderer } from "../../../components/LazyMarkdownRenderer";
import { classNames } from "../../../utils/classNames";
import { SecretaryTaskRow } from "./SecretaryTaskRow";
import { secretaryTaskNeedsAttention, secretaryTaskRunning } from "./secretaryTaskLineModel";
import type { SecretaryTasksController } from "./useSecretaryTasks";

type AskItemView = { item: AssistantVoiceAskFeedback; sortAt: number; status: string };

export function VoiceAskThread({
  items,
  isDark,
  t,
  documents,
  documentKey,
  tasks,
  statusLabel,
  statusClassName,
  formatTime,
  formatFullTime,
  onOpenDocument,
  onFollowUp,
  onCopyReply,
  onShowExecution,
  onClear,
  clearing,
}: {
  items: AskItemView[];
  isDark: boolean;
  t: TFunction;
  documents: AssistantVoiceDocument[];
  documentKey: (document: AssistantVoiceDocument) => string;
  tasks: SecretaryTasksController;
  statusLabel: (status: string) => string;
  statusClassName: (status: string) => string;
  formatTime: (value: number) => string;
  formatFullTime: (value: number) => string;
  onOpenDocument: (document: AssistantVoiceDocument) => void;
  onFollowUp: (item: AssistantVoiceAskFeedback) => void;
  onCopyReply: (item: AssistantVoiceAskFeedback) => void;
  onShowExecution: (task: SecretaryTaskSummary) => void;
  onClear: () => void;
  clearing: boolean;
}) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const lastId = items[items.length - 1]?.item.request_id || "";
  useEffect(() => {
    const node = scrollRef.current;
    if (node && lastId) node.scrollTop = node.scrollHeight;
  }, [lastId]);
  const taskFor = (requestId: string) =>
    tasks.tasks.find((task) => task.target.request_id === requestId && !task.superseded_by);
  const linkClassName = (enabled: boolean) =>
    classNames(
      "max-w-full truncate rounded-full border px-2 py-0.5 text-xs transition-colors disabled:cursor-default disabled:opacity-70",
      enabled
        ? isDark
          ? "border-cyan-300/20 bg-cyan-300/10 text-cyan-100 hover:bg-cyan-300/16"
          : "border-cyan-200 bg-cyan-50 text-cyan-800 hover:bg-cyan-100"
        : "border-[var(--glass-border-subtle)] text-[var(--color-text-muted)]",
    );
  const smallButton =
    "min-h-9 rounded-lg px-2.5 text-xs font-medium text-[var(--color-text-secondary)] hover:bg-[var(--color-bg-tertiary)] disabled:opacity-50 focus-visible:outline-2 focus-visible:outline-[var(--color-border-focus)]";

  return (
    <section
      data-voice-ask-thread
      className="flex min-h-0 flex-1 flex-col"
      aria-label={t("voiceSecretaryPanelViewAsk")}
    >
      <h3 className="sr-only">{t("voiceSecretaryPanelViewAsk")}</h3>
      <div
        className={classNames(
          "shrink-0 items-center justify-end gap-2 px-1 pb-2",
          items.length ? "flex" : "hidden",
        )}
      >
        {items.length ? (
          <button
            type="button"
            className={smallButton}
            onClick={onClear}
            disabled={clearing}
            title={t("voiceSecretaryClearRequestsTitle")}
          >
            {clearing ? t("voiceSecretaryClearingRequests") : t("voiceSecretaryClearRequests")}
          </button>
        ) : null}
      </div>
      {tasks.error ? (
        <p role="alert" className="px-1 pb-2 text-xs text-rose-600 dark:text-rose-300">
          {tasks.error}
        </p>
      ) : null}
      <div
        ref={scrollRef}
        data-voice-ask-scroll
        className="min-h-0 flex-1 space-y-3 overflow-y-auto scrollbar-subtle pr-1 [scrollbar-gutter:stable]"
      >
        {!items.length ? (
          <div className="flex h-full min-h-40 flex-col items-center justify-center px-6 text-center">
            <MessageSquareQuoteIcon
              size={28}
              aria-hidden="true"
              className="text-[var(--color-text-tertiary)]"
            />
            <p className="mt-3 max-w-md text-sm leading-6 text-[var(--color-text-muted)]">
              {t("voiceSecretaryAskEmpty")}
            </p>
          </div>
        ) : null}
        {items.map(({ item, sortAt, status }) => {
          const question = String(item.request_text || item.request_preview || "").trim();
          const reply = String(item.reply_text || "").trim();
          const sourceSummary = String(item.source_summary || "").trim();
          const checkedAtMs = item.checked_at ? Date.parse(item.checked_at) : NaN;
          const sourceUrls = (item.source_urls || [])
            .map((url) => String(url || "").trim())
            .filter((url, index, urls) => url && urls.indexOf(url) === index);
          const artifactPaths = [item.document_path, ...(item.artifact_paths || [])]
            .map((path) => String(path || "").trim())
            .filter((path, index, paths) => path && paths.indexOf(path) === index);
          const task = taskFor(item.request_id);
          const timeLabel = formatTime(sortAt);
          return (
            <article
              key={item.request_id}
              data-voice-ask-item={item.request_id}
              className="rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-3 py-2.5"
            >
              <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
                <span
                  className={classNames(
                    "rounded-full px-2 py-0.5 font-semibold",
                    status
                      ? statusClassName(status)
                      : "bg-[var(--color-bg-tertiary)] text-[var(--color-text-secondary)]",
                  )}
                >
                  {status ? statusLabel(status) : t("voiceSecretaryReplyReadyShort")}
                </span>
                <span className="flex min-w-0 items-center gap-1.5 text-[var(--color-text-muted)]">
                  {item.handoff_target ? (
                    <span className="min-w-0 truncate">{item.handoff_target}</span>
                  ) : null}
                  {timeLabel ? (
                    <time
                      className="shrink-0 tabular-nums"
                      dateTime={new Date(sortAt).toISOString()}
                      title={formatFullTime(sortAt)}
                    >
                      {timeLabel}
                    </time>
                  ) : null}
                </span>
              </div>
              {question ? (
                <p className="mt-2 whitespace-pre-wrap break-words text-sm font-medium leading-6">
                  {question}
                </p>
              ) : null}
              {reply ? (
                <div className="mt-2 border-l-2 border-[var(--glass-border-subtle)] pl-3 text-sm leading-6">
                  <LazyMarkdownRenderer
                    content={reply}
                    isDark={isDark}
                    className="max-w-full break-words [overflow-wrap:anywhere] [&_p]:my-1 [&_li]:my-0.5 [&_pre]:my-2"
                    fallback={<div className="whitespace-pre-wrap break-words">{reply}</div>}
                  />
                </div>
              ) : null}
              {artifactPaths.length ? (
                <div className="mt-2 flex min-w-0 flex-wrap gap-1">
                  {artifactPaths.map((path) => {
                    const linked =
                      documents.find(
                        (document) =>
                          documentKey(document) === path ||
                          String(document.document_path || document.workspace_path || "").trim() ===
                            path,
                      ) || null;
                    return (
                      <button
                        key={path}
                        type="button"
                        data-voice-document-link={path}
                        disabled={!linked}
                        onClick={() => linked && onOpenDocument(linked)}
                        className={linkClassName(!!linked)}
                        title={linked ? t("voiceSecretaryOpenLinkedDocument") : path}
                      >
                        {path}
                      </button>
                    );
                  })}
                </div>
              ) : null}
              {sourceSummary || Number.isFinite(checkedAtMs) || sourceUrls.length ? (
                <div className="mt-2 rounded-lg bg-[var(--color-bg-secondary)] px-2.5 py-2 text-xs leading-5 text-[var(--color-text-secondary)]">
                  <div className="font-semibold text-[var(--color-text-muted)]">
                    {t("voiceSecretarySourcesLabel")}
                  </div>
                  {sourceSummary ? (
                    <div className="whitespace-pre-wrap break-words">{sourceSummary}</div>
                  ) : null}
                  {Number.isFinite(checkedAtMs) ? (
                    <div
                      className="text-[var(--color-text-muted)]"
                      title={formatFullTime(checkedAtMs)}
                    >
                      {t("voiceSecretaryCheckedAtLabel")}: {formatTime(checkedAtMs)}
                    </div>
                  ) : null}
                  {sourceUrls.length ? (
                    <div className="mt-1 flex min-w-0 flex-wrap gap-1">
                      {sourceUrls.map((url) => (
                        <a
                          key={url}
                          href={url}
                          target="_blank"
                          rel="noreferrer"
                          className="max-w-full truncate rounded-full border border-[var(--glass-border-subtle)] px-1.5 py-0.5 text-cyan-700 hover:bg-[var(--color-bg-tertiary)] dark:text-cyan-200"
                          title={url}
                        >
                          {url.replace(/^https?:\/\//i, "")}
                        </a>
                      ))}
                    </div>
                  ) : null}
                </div>
              ) : null}
              {task && (secretaryTaskNeedsAttention(task) || task.receipt?.output.handoff_text) ? (
                <SecretaryTaskRow
                  task={task}
                  controller={tasks}
                  isDark={isDark}
                  open
                  onShowExecution={onShowExecution}
                />
              ) : null}
              <div className="mt-1 flex flex-wrap items-center gap-1">
                {reply ? (
                  <button type="button" className={smallButton} onClick={() => onCopyReply(item)}>
                    {t("copy")}
                  </button>
                ) : null}
                {reply || status === "needs_user" ? (
                  <button type="button" className={smallButton} onClick={() => onFollowUp(item)}>
                    {t("voiceSecretaryFollowUp")}
                  </button>
                ) : null}
                {task && secretaryTaskRunning(task) ? (
                  <button
                    type="button"
                    className={smallButton}
                    disabled={!!tasks.busy}
                    onClick={() => void tasks.act(task, "cancel")}
                  >
                    {t("settings:voiceSettings.cancelTask")}
                  </button>
                ) : null}
                {task &&
                !secretaryTaskNeedsAttention(task) &&
                !task.receipt?.output.handoff_text ? (
                  <button
                    type="button"
                    className={smallButton}
                    onClick={() => onShowExecution(task)}
                  >
                    {t("settings:voiceSettings.viewExecution")}
                  </button>
                ) : null}
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}
