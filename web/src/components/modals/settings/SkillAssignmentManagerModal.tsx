import { Actor, CapabilityOverviewItem, CapabilityUsageActorEntry, CapabilityUsageSummary } from "../../../types";
import { useModalA11y } from "../../../hooks/useModalA11y";
import { BodyPortal } from "../../ui/BodyPortal";
import {
  inputClass,
  primaryButtonClass,
  secondaryButtonClass,
  settingsWorkspacePanelClass,
  settingsWorkspaceSoftPanelClass,
} from "./types";

const noticeClass =
  "rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--glass-tab-bg)] px-3 py-2 text-xs text-[var(--color-text-secondary)]";
const neutralActionButtonClass =
  "bg-[var(--glass-panel-bg)] text-[var(--color-text-primary)] border border-[var(--glass-border-subtle)] hover:bg-[var(--glass-bg-hover)] active:bg-[var(--glass-bg-active)]";

export type SkillManagerProvenanceRow = {
  label: string;
  value: string;
};

interface SkillAssignmentManagerModalLabels {
  title: string;
  subtitle: string;
  close: string;
  statusBlocked: string;
  noGroupHint: string;
  duplicateTitle: string;
  duplicateHint: string;
  provenanceTitle: string;
  provenanceHint: string;
  name: string;
  description: string;
  capsule: string;
  capsuleLimit: string;
  save: string;
  saving: string;
  blockedBanner: string;
  runtimeTitle: string;
  autoloadHint: string;
  currentUseTitle: string;
  currentUseHint: string;
  usageLoading: string;
  usageSummary: string;
  usageGroup: string;
  usageSession: (row: CapabilityUsageActorEntry) => string;
  usageActor: (row: CapabilityUsageActorEntry) => string;
  usageActorAutoload: (row: CapabilityUsageActorEntry) => string;
  usageProfileAutoload: (row: CapabilityUsageActorEntry) => string;
  usageActorHidden: (row: CapabilityUsageActorEntry) => string;
  usageBlocked: string;
  noCurrentUse: string;
  actorAssignmentsTitle: string;
  actorAssignmentsHint: string;
  profileBadge: string;
  temporaryBadge: string;
  actorScopeBadge: string;
  hiddenBadge: string;
  noActors: string;
  hideInMenus: string;
  saveActorAssignments: string;
  otherActionsTitle: string;
  otherActionsHint: string;
  unblockSkill: string;
  blockSkill: string;
  remove: string;
}

interface SkillAssignmentManagerModalProps {
  isDark: boolean;
  candidate: CapabilityOverviewItem;
  editable: boolean;
  capabilityId: string;
  groupId: string;
  name: string;
  description: string;
  capsuleText: string;
  capsuleTextMax: number;
  qualificationStatus: "qualified" | "blocked";
  error: string;
  notice: string;
  duplicateCandidates: CapabilityOverviewItem[];
  provenanceRows: SkillManagerProvenanceRow[];
  usage: CapabilityUsageSummary | null;
  usageLoading: boolean;
  actors: Actor[];
  assignedActorIds: Set<string>;
  hiddenActorIds: Set<string>;
  profileActorIds: Set<string>;
  sessionActorIds: Set<string>;
  actorScopeIds: Set<string>;
  busyKey: string;
  labels: SkillAssignmentManagerModalLabels;
  onClose: () => void;
  onNameChange: (value: string) => void;
  onDescriptionChange: (value: string) => void;
  onCapsuleTextChange: (value: string) => void;
  onSaveRecord: () => void;
  onToggleRecordBlock: () => void;
  onRemoveRecord: () => void;
  onToggleActor: (actorId: string) => void;
  onToggleActorVisibility: (actorId: string) => void;
  onSaveActorAssignments: () => void;
}

export function SkillAssignmentManagerModal({
  isDark,
  candidate,
  editable,
  capabilityId,
  groupId,
  name,
  description,
  capsuleText,
  capsuleTextMax,
  qualificationStatus,
  error,
  notice,
  duplicateCandidates,
  provenanceRows,
  usage,
  usageLoading,
  actors,
  assignedActorIds,
  hiddenActorIds,
  profileActorIds,
  sessionActorIds,
  actorScopeIds,
  busyKey,
  labels,
  onClose,
  onNameChange,
  onDescriptionChange,
  onCapsuleTextChange,
  onSaveRecord,
  onToggleRecordBlock,
  onRemoveRecord,
  onToggleActor,
  onToggleActorVisibility,
  onSaveActorAssignments,
}: SkillAssignmentManagerModalProps) {
  const { modalRef } = useModalA11y(true, onClose);
  const sourceId = String(candidate.source_id || "");

  return (
    <BodyPortal>
      <div
        key={capabilityId}
        ref={modalRef}
        className="fixed inset-0 z-[1000] flex items-end justify-center bg-black/35 px-3 py-4 sm:items-center"
        role="dialog"
        aria-modal="true"
        aria-labelledby="skill-assignment-manager-title"
        onPointerDown={onClose}
      >
        <div
          className="w-full max-w-4xl max-h-[88vh] overflow-hidden rounded-2xl border border-[var(--glass-border-subtle)] bg-[var(--color-bg-elevated)] shadow-2xl"
          onPointerDown={(e) => e.stopPropagation()}
        >
          <div className="flex items-start justify-between gap-3 border-b border-[var(--glass-border-subtle)] px-4 py-3">
            <div className="min-w-0">
              <div id="skill-assignment-manager-title" className="text-sm font-semibold text-[var(--color-text-primary)]">
                {labels.title}
              </div>
              <div className="mt-1 text-xs text-[var(--color-text-muted)]">{labels.subtitle}</div>
            </div>
            <button type="button" className={secondaryButtonClass("sm")} onClick={onClose}>
              {labels.close}
            </button>
          </div>

          <div className="max-h-[calc(88vh-62px)] overflow-auto px-4 py-4">
            <div className="flex flex-wrap gap-1.5">
              <span className="rounded bg-[var(--glass-tab-bg)] px-1.5 py-0.5 font-mono text-[10px] text-[var(--color-text-secondary)]">
                {capabilityId}
              </span>
              <span className="rounded bg-[var(--glass-tab-bg)] px-1.5 py-0.5 text-[10px] text-[var(--color-text-secondary)]">
                {sourceId}
              </span>
              {qualificationStatus === "blocked" ? (
                <span className="rounded bg-rose-500/10 px-1.5 py-0.5 text-[10px] text-rose-600 dark:text-rose-300">
                  {labels.statusBlocked}
                </span>
              ) : null}
            </div>

            {!String(groupId || "").trim() ? (
              <div className="mt-3 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-700 dark:text-amber-300">
                {labels.noGroupHint}
              </div>
            ) : null}

            {error ? (
              <div className="mt-3 rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs text-rose-600 dark:text-rose-400" role="alert">
                {error}
              </div>
            ) : null}
            {notice ? (
              <div className={`mt-3 ${noticeClass}`} role="status">
                {notice}
              </div>
            ) : null}

            {editable && duplicateCandidates.length ? (
              <div className="mt-3 rounded-lg border border-amber-500/30 bg-amber-500/10 px-3 py-2">
                <div className="text-xs font-medium text-amber-800 dark:text-amber-200">{labels.duplicateTitle}</div>
                <div className="mt-1 text-[11px] text-amber-700 dark:text-amber-300">{labels.duplicateHint}</div>
                <div className="mt-2 space-y-1">
                  {duplicateCandidates.map((row) => (
                    <div key={String(row.capability_id || "")} className="truncate font-mono text-[11px] text-amber-800 dark:text-amber-200">
                      {String(row.capability_id || "")}
                    </div>
                  ))}
                </div>
              </div>
            ) : null}

            <div className={`mt-4 ${settingsWorkspacePanelClass(isDark)}`}>
              <div className="text-xs font-medium text-[var(--color-text-primary)]">{labels.provenanceTitle}</div>
              <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">{labels.provenanceHint}</div>
              <dl className="mt-3 grid gap-2 sm:grid-cols-2">
                {provenanceRows.map((row) => (
                  <div key={row.label} className={settingsWorkspaceSoftPanelClass(isDark)}>
                    <dt className="text-[10px] uppercase tracking-[0.08em] text-[var(--color-text-muted)]">{row.label}</dt>
                    <dd className="mt-1 break-words font-mono text-[11px] text-[var(--color-text-secondary)]">{row.value}</dd>
                  </div>
                ))}
              </dl>
            </div>

            {editable ? (
              <>
                <div className="mt-4 grid gap-3 md:grid-cols-2">
                  <label className="block">
                    <span className="text-xs font-medium text-[var(--color-text-secondary)]">{labels.name}</span>
                    <input value={name} onChange={(e) => onNameChange(e.target.value)} className={`mt-1 ${inputClass()}`} />
                  </label>
                  <label className="block">
                    <span className="text-xs font-medium text-[var(--color-text-secondary)]">{labels.description}</span>
                    <input value={description} onChange={(e) => onDescriptionChange(e.target.value)} className={`mt-1 ${inputClass()}`} />
                  </label>
                </div>

                <div className="mt-3">
                  <label className="block">
                    <span className="text-xs font-medium text-[var(--color-text-secondary)]">{labels.capsule}</span>
                    <textarea
                      value={capsuleText}
                      onChange={(e) => onCapsuleTextChange(e.target.value)}
                      maxLength={capsuleTextMax}
                      rows={16}
                      className={`mt-1 ${inputClass()} resize-y font-mono text-xs leading-5`}
                    />
                    <span className="mt-1 block text-[10px] text-[var(--color-text-muted)]">{labels.capsuleLimit}</span>
                  </label>
                </div>

                <div className="mt-4 flex flex-wrap items-center gap-2">
                  <button type="button" className={primaryButtonClass(false)} disabled={busyKey === `manage:${capabilityId}`} onClick={onSaveRecord}>
                    {busyKey === `manage:${capabilityId}` ? labels.saving : labels.save}
                  </button>
                </div>
              </>
            ) : null}

            {qualificationStatus === "blocked" ? (
              <div className="mt-4 rounded-lg border border-rose-500/20 bg-rose-500/10 px-3 py-2 text-xs text-rose-700 dark:text-rose-300">
                {labels.blockedBanner}
              </div>
            ) : null}

            <div className="mt-4">
              <div className={settingsWorkspacePanelClass(isDark)}>
                <div className="text-sm font-medium text-[var(--color-text-primary)]">{labels.runtimeTitle}</div>
                <div className="mt-1 text-xs text-[var(--color-text-muted)]">{labels.autoloadHint}</div>
                <div className={`mt-3 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
                  <div className="text-xs font-medium text-[var(--color-text-primary)]">{labels.currentUseTitle}</div>
                  <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">{labels.currentUseHint}</div>
                  {usageLoading ? (
                    <div className="mt-2 text-[11px] text-[var(--color-text-tertiary)]">{labels.usageLoading}</div>
                  ) : usage?.used ? (
                    <div className="mt-2 space-y-1.5">
                      <div className={settingsWorkspaceSoftPanelClass(isDark)}>{labels.usageSummary}</div>
                      {usage.group_enabled ? <div className={settingsWorkspaceSoftPanelClass(isDark)}>{labels.usageGroup}</div> : null}
                      {(usage.session_enabled || []).map((row) => (
                        <div key={`session:${row.actor_id}:${row.expires_at || ""}`} className={settingsWorkspaceSoftPanelClass(isDark)}>
                          {labels.usageSession(row)}
                        </div>
                      ))}
                      {(usage.actor_enabled || []).map((row) => (
                        <div key={`actor:${row.actor_id}`} className={settingsWorkspaceSoftPanelClass(isDark)}>
                          {labels.usageActor(row)}
                        </div>
                      ))}
                      {(usage.actor_autoload || []).map((row) => (
                        <div key={`autoload:${row.actor_id}`} className={settingsWorkspaceSoftPanelClass(isDark)}>
                          {labels.usageActorAutoload(row)}
                        </div>
                      ))}
                      {(usage.profile_autoload || []).map((row) => (
                        <div key={`profile:${row.actor_id}:${row.profile_id || ""}`} className={settingsWorkspaceSoftPanelClass(isDark)}>
                          {labels.usageProfileAutoload(row)}
                        </div>
                      ))}
                      {(usage.actor_hidden || []).map((row) => (
                        <div key={`hidden:${row.actor_id}`} className={settingsWorkspaceSoftPanelClass(isDark)}>
                          {labels.usageActorHidden(row)}
                        </div>
                      ))}
                      {usage.blocked ? (
                        <div className="rounded-md bg-rose-500/10 px-2 py-1.5 text-[11px] text-rose-600 dark:text-rose-300">
                          {labels.usageBlocked}
                        </div>
                      ) : null}
                    </div>
                  ) : (
                    <div className={`mt-2 ${settingsWorkspaceSoftPanelClass(isDark)} text-[11px] text-[var(--color-text-tertiary)]`}>
                      {labels.noCurrentUse}
                    </div>
                  )}
                </div>
                <div className="mt-4 border-t border-[var(--glass-border-subtle)] pt-3">
                  <div className="text-xs font-medium text-[var(--color-text-primary)]">{labels.actorAssignmentsTitle}</div>
                  <div className="mt-1 text-[11px] text-[var(--color-text-muted)]">{labels.actorAssignmentsHint}</div>
                </div>
                <div className="mt-3 space-y-2">
                  {actors
                    .filter((actor) => String(actor.id || "").trim())
                    .map((actor) => {
                      const actorId = String(actor.id || "").trim();
                      const runtimeLabel = [actor.runtime, actor.runner_effective || actor.runner].filter(Boolean).join(" / ");
                      return (
                        <div key={actorId} className={`flex items-start gap-2 ${settingsWorkspaceSoftPanelClass(isDark)}`}>
                          <label className="flex items-start gap-2">
                            <input
                              type="checkbox"
                              className="mt-0.5"
                              checked={assignedActorIds.has(actorId)}
                              onChange={() => onToggleActor(actorId)}
                            />
                            <span className="sr-only">{labels.actorAssignmentsTitle}</span>
                          </label>
                          <span className="min-w-0 flex-1">
                            <span className="block truncate text-xs font-medium text-[var(--color-text-primary)]">
                              {actor.title ? `${actor.title} (${actorId})` : actorId}
                            </span>
                            {runtimeLabel ? <span className="mt-0.5 block text-[11px] text-[var(--color-text-tertiary)]">{runtimeLabel}</span> : null}
                            <span className="mt-1 flex flex-wrap gap-1">
                              {profileActorIds.has(actorId) ? (
                                <span className="rounded bg-amber-500/10 px-1.5 py-0.5 text-[10px] text-amber-700 dark:text-amber-300">
                                  {labels.profileBadge}
                                </span>
                              ) : null}
                              {sessionActorIds.has(actorId) ? (
                                <span className="rounded border border-black/10 bg-[rgb(245,245,245)] px-1.5 py-0.5 text-[10px] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white">
                                  {labels.temporaryBadge}
                                </span>
                              ) : null}
                              {actorScopeIds.has(actorId) ? (
                                <span className="rounded border border-black/10 bg-[rgb(245,245,245)] px-1.5 py-0.5 text-[10px] text-[rgb(35,36,37)] dark:border-white/12 dark:bg-white/[0.08] dark:text-white">
                                  {labels.actorScopeBadge}
                                </span>
                              ) : null}
                              {hiddenActorIds.has(actorId) ? (
                                <span className="rounded bg-slate-500/10 px-1.5 py-0.5 text-[10px] text-slate-600 dark:text-slate-300">
                                  {labels.hiddenBadge}
                                </span>
                              ) : null}
                            </span>
                          </span>
                          <label className="shrink-0 text-[11px] text-[var(--color-text-secondary)]">
                            <input
                              type="checkbox"
                              className="mr-1 align-[-2px]"
                              checked={hiddenActorIds.has(actorId)}
                              onChange={() => onToggleActorVisibility(actorId)}
                            />
                            {labels.hideInMenus}
                          </label>
                        </div>
                      );
                    })}
                  {actors.length === 0 ? (
                    <div className={`${settingsWorkspaceSoftPanelClass(isDark)} text-[11px] text-[var(--color-text-tertiary)]`}>
                      {labels.noActors}
                    </div>
                  ) : null}
                </div>
                <div className="mt-3 flex justify-end">
                  <button type="button" className={secondaryButtonClass()} disabled={busyKey === `manage-use:${capabilityId}`} onClick={onSaveActorAssignments}>
                    {labels.saveActorAssignments}
                  </button>
                </div>
              </div>
            </div>

            {editable ? (
              <div className={`mt-4 ${settingsWorkspacePanelClass(isDark)}`}>
                <div className="text-sm font-medium text-[var(--color-text-primary)]">{labels.otherActionsTitle}</div>
                <div className="mt-1 text-xs text-[var(--color-text-muted)]">{labels.otherActionsHint}</div>
                <div className="mt-3 flex flex-wrap items-center justify-between gap-2">
                  <button
                    type="button"
                    className={`rounded-lg border px-3 py-2 text-xs min-h-[38px] disabled:opacity-60 ${
                      qualificationStatus === "blocked"
                        ? neutralActionButtonClass
                        : "border-rose-500/30 bg-rose-500/10 text-rose-600 dark:text-rose-300"
                    }`}
                    disabled={busyKey === `manage:${capabilityId}`}
                    onClick={onToggleRecordBlock}
                  >
                    {qualificationStatus === "blocked" ? labels.unblockSkill : labels.blockSkill}
                  </button>
                  <button
                    type="button"
                    className="rounded-lg border border-rose-500/30 bg-rose-500/10 px-3 py-2 text-xs min-h-[38px] text-rose-600 dark:text-rose-300 disabled:opacity-60"
                    disabled={busyKey === `manage-remove:${capabilityId}`}
                    onClick={onRemoveRecord}
                  >
                    {labels.remove}
                  </button>
                </div>
              </div>
            ) : null}
          </div>
        </div>
      </div>
    </BodyPortal>
  );
}
