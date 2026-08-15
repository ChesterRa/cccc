import { useTranslation } from "react-i18next";
import type {
  LedgerEvent,
  PresentationMessageRef,
  Task,
  TaskMessageRef,
  VoiceDocumentMessageRef,
} from "../../types";
import { classNames } from "../../utils/classNames";
import { getPresentationRefChipLabel } from "../../utils/presentationRefs";
import {
  getTaskRefChipLabel,
  getTaskRefStateKey,
  type TaskRefStateKey,
} from "../../utils/taskRefs";
import { getVoiceDocumentRefLabel } from "../../utils/voiceDocumentRefs";
import { FileIcon } from "../Icons";

const TASK_REF_STATE_TONE_CLASS: Record<TaskRefStateKey, string> = {
  planned:
    "border-slate-300/70 bg-slate-100/90 text-slate-800 dark:border-slate-300/30 dark:bg-slate-950/80 dark:text-slate-100",
  active:
    "border-emerald-300/70 bg-emerald-100/90 text-emerald-800 dark:border-emerald-300/35 dark:bg-emerald-950/80 dark:text-emerald-100",
  handoff:
    "border-sky-300/70 bg-sky-100/90 text-sky-800 dark:border-sky-300/35 dark:bg-sky-950/80 dark:text-sky-100",
  waiting_user:
    "border-amber-300/70 bg-amber-100/90 text-amber-800 dark:border-amber-300/35 dark:bg-amber-950/80 dark:text-amber-100",
  blocked:
    "border-rose-300/70 bg-rose-100/90 text-rose-800 dark:border-rose-300/35 dark:bg-rose-950/80 dark:text-rose-100",
  done: "border-emerald-300/60 bg-emerald-50/95 text-emerald-800 dark:border-emerald-300/30 dark:bg-emerald-950/75 dark:text-emerald-100",
  archived:
    "border-slate-300/70 bg-slate-100/90 text-slate-700 dark:border-slate-300/25 dark:bg-slate-950/75 dark:text-slate-200",
  linked:
    "border-slate-300/70 bg-slate-100/90 text-slate-800 dark:border-slate-300/30 dark:bg-slate-950/80 dark:text-slate-100",
};

const TASK_REF_STATE_DOT_CLASS: Record<TaskRefStateKey, string> = {
  planned: "bg-slate-400/90 dark:bg-slate-400",
  active: "bg-emerald-500 dark:bg-emerald-400",
  handoff: "bg-sky-500 dark:bg-sky-400",
  waiting_user: "bg-amber-500 dark:bg-amber-400",
  blocked: "bg-rose-500 dark:bg-rose-400",
  done: "bg-emerald-500 dark:bg-emerald-400",
  archived: "bg-slate-400/90 dark:bg-slate-500",
  linked: "bg-slate-400/90 dark:bg-slate-400",
};

export function MessageReferenceSections({
  event,
  presentationRefs,
  voiceDocumentRefs,
  taskRefs,
  taskById,
  sectionClassName,
  onOpenPresentationRef,
  onOpenTaskRef,
}: {
  event: LedgerEvent;
  presentationRefs: PresentationMessageRef[];
  voiceDocumentRefs: VoiceDocumentMessageRef[];
  taskRefs: TaskMessageRef[];
  taskById: Map<string, Task>;
  sectionClassName: string;
  onOpenPresentationRef?: (ref: PresentationMessageRef, event: LedgerEvent) => void;
  onOpenTaskRef?: (ref: TaskMessageRef, event: LedgerEvent) => void;
}) {
  const { t } = useTranslation("chat");
  const taskStateLabels: Record<TaskRefStateKey, string> = {
    planned: t("taskRefStatePlanned", { defaultValue: "Planned" }),
    active: t("taskRefStateActive", { defaultValue: "Active" }),
    handoff: t("taskRefStateHandoff", { defaultValue: "Handoff" }),
    waiting_user: t("taskRefStateWaitingUser", { defaultValue: "Waiting user" }),
    blocked: t("taskRefStateBlocked", { defaultValue: "Blocked" }),
    done: t("taskRefStateDone", { defaultValue: "Done" }),
    archived: t("taskRefStateArchived", { defaultValue: "Archived" }),
    linked: t("taskRefStateLinked", { defaultValue: "Linked" }),
  };

  return (
    <>
      {presentationRefs.length > 0 ? (
        <div className={sectionClassName}>
          <div className="mb-2 text-[10px] font-semibold uppercase tracking-[0.14em] opacity-50">
            {t("presentation")}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {presentationRefs.map((ref, index) => (
              <button
                key={`${String(event.id || "message")}:presentation-ref:${index}:${String(ref.slot_id || "")}`}
                type="button"
                onClick={() => onOpenPresentationRef?.(ref, event)}
                className={classNames(
                  "inline-flex max-w-full items-center rounded-full border px-2.5 py-1 text-[11px] font-medium transition-colors",
                  "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]",
                )}
                title={getPresentationRefChipLabel(ref)}
              >
                <span className="truncate">{getPresentationRefChipLabel(ref)}</span>
              </button>
            ))}
          </div>
        </div>
      ) : null}

      {voiceDocumentRefs.length > 0 ? (
        <div className={sectionClassName}>
          <div className="mb-2 text-[10px] font-semibold uppercase tracking-[0.14em] opacity-50">
            {t("voiceSecretaryDocumentReferenceSection", { defaultValue: "Document" })}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {voiceDocumentRefs.map((ref, index) => (
              <div
                key={`${String(event.id || "message")}:voice-document-ref:${index}:${ref.document_path}`}
                className="inline-flex max-w-full items-center gap-1.5 rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-2.5 py-1 text-[11px] font-medium text-[var(--color-text-secondary)]"
                title={ref.document_path}
              >
                <FileIcon size={12} aria-hidden="true" />
                <span className="truncate">{getVoiceDocumentRefLabel(ref)}</span>
              </div>
            ))}
          </div>
        </div>
      ) : null}

      {taskRefs.length > 0 ? (
        <div className={sectionClassName}>
          <div className="mb-2 text-[10px] font-semibold uppercase tracking-[0.14em] opacity-50">
            {t("task", { defaultValue: "Task" })}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {taskRefs.map((ref, index) => {
              const taskId = String(ref.task_id || "").trim();
              const liveTask = taskId ? taskById.get(taskId) || null : null;
              const stateKey = getTaskRefStateKey(ref, liveTask);
              const stateLabel = taskStateLabels[stateKey];
              const chipLabel = getTaskRefChipLabel(ref, liveTask);
              return (
                <button
                  key={`${String(event.id || "message")}:task-ref:${index}:${taskId}`}
                  type="button"
                  onClick={() => onOpenTaskRef?.(ref, event)}
                  className={classNames(
                    "inline-flex max-w-full items-center gap-1.5 rounded-full border px-2.5 py-1 text-[11px] font-medium transition-colors",
                    "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]",
                  )}
                  title={`${chipLabel} · ${stateLabel}`}
                >
                  <span
                    className={classNames(
                      "h-1.5 w-1.5 rounded-full",
                      TASK_REF_STATE_DOT_CLASS[stateKey],
                    )}
                    aria-hidden="true"
                  />
                  <span className="truncate">{chipLabel}</span>
                  <span
                    className={classNames(
                      "shrink-0 rounded-full border px-1.5 py-0.5 text-[10px] font-semibold leading-none",
                      TASK_REF_STATE_TONE_CLASS[stateKey],
                    )}
                  >
                    {stateLabel}
                  </span>
                </button>
              );
            })}
          </div>
        </div>
      ) : null}
    </>
  );
}
