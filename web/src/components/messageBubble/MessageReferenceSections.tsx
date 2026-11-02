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

// The state is a word in a frame. The waiting colour marks a task waiting on the user and
// the danger colour a blocked one; finished tasks step back.
const TASK_REF_STATE_TONE: Record<TaskRefStateKey, "neutral" | "warning" | "error" | "muted"> = {
  planned: "neutral",
  active: "neutral",
  handoff: "neutral",
  waiting_user: "warning",
  blocked: "error",
  done: "muted",
  archived: "muted",
  linked: "neutral",
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
          <div className="mb-2 text-xs font-semibold uppercase tracking-[0.14em] text-[var(--color-text-tertiary)]">
            {t("presentation")}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {presentationRefs.map((ref, index) => (
              <button
                key={`${String(event.id || "message")}:presentation-ref:${index}:${String(ref.slot_id || "")}`}
                type="button"
                onClick={() => onOpenPresentationRef?.(ref, event)}
                className={classNames(
                  "inline-flex max-w-full items-center rounded-full border px-2.5 py-1 text-xs font-medium transition-colors",
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
          <div className="mb-2 text-xs font-semibold uppercase tracking-[0.14em] text-[var(--color-text-tertiary)]">
            {t("voiceSecretaryDocumentReferenceSection", { defaultValue: "Document" })}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {voiceDocumentRefs.map((ref, index) => (
              <div
                key={`${String(event.id || "message")}:voice-document-ref:${index}:${ref.document_path}`}
                className="inline-flex max-w-full items-center gap-1.5 rounded-full border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-2.5 py-1 text-xs font-medium text-[var(--color-text-secondary)]"
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
          <div className="mb-2 text-xs font-semibold uppercase tracking-[0.14em] text-[var(--color-text-tertiary)]">
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
                    "inline-flex max-w-full items-center gap-1.5 rounded-full border px-2.5 py-1 text-xs font-medium transition-colors",
                    "border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)]",
                  )}
                  title={`${chipLabel} · ${stateLabel}`}
                >
                  <span className="truncate">{chipLabel}</span>
                  <span
                    className="shrink-0 border px-1.5 py-0.5 text-xs font-semibold leading-none"
                    data-status-tone={TASK_REF_STATE_TONE[stateKey]}
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
