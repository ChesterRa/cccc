import { useMemo, useState, type FormEvent } from "react";

import { classNames } from "../../../utils/classNames";
import type {
  GroupLearningSnapshot,
  LearningObservingSkill,
  LearningPendingPatch,
  LearningProceduralSkill,
  LearningRecentItem,
} from "../../../types";
import { deleteProceduralSkill, updateProceduralSkill, type ProceduralSkillPayload } from "../../../services/api/context";
import { noteTimestamp, type ContextTranslator } from "../model";
import type { ContextModalUi } from "../ui";

interface LearningPanelProps {
  groupId: string;
  tr: ContextTranslator;
  ui: ContextModalUi;
  data: GroupLearningSnapshot | null;
  loading: boolean;
  error: string;
  onRefresh: () => void;
}

type SkillDraft = {
  skillId: string;
  title: string;
  goal: string;
  steps: string;
  constraints: string;
  failureSignals: string;
  stability: string;
  reviewMode: string;
  status: string;
};

function statusTone(status: string): string {
  const normalized = String(status || "").trim().toLowerCase();
  if (normalized === "disabled") return "bg-slate-500/15 text-slate-600 dark:text-slate-300";
  if (normalized === "regressed") return "bg-rose-500/15 text-rose-600 dark:text-rose-400";
  if (normalized === "needs_followup") return "bg-amber-500/15 text-amber-600 dark:text-amber-400";
  if (normalized === "observing") return "bg-cyan-500/15 text-cyan-600 dark:text-cyan-400";
  if (normalized === "validated" || normalized === "active") return "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400";
  return "glass-panel text-[var(--color-text-secondary)]";
}

function policyTone(mode: string): string {
  const normalized = String(mode || "").trim().toLowerCase();
  if (normalized === "manual_review_required") return "bg-amber-500/15 text-amber-700 dark:text-amber-300";
  if (normalized === "auto_merge_eligible") return "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300";
  return "glass-panel text-[var(--color-text-secondary)]";
}

function stabilityTone(stability: string): string {
  const normalized = String(stability || "").trim().toLowerCase();
  if (normalized === "unstable") return "bg-rose-500/15 text-rose-600 dark:text-rose-400";
  if (normalized === "probation") return "bg-amber-500/15 text-amber-700 dark:text-amber-300";
  if (normalized === "stable") return "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300";
  return "glass-panel text-[var(--color-text-secondary)]";
}

function renderPatchSummary(item: LearningPendingPatch | LearningRecentItem): string {
  const kind = String(item.patch_kind || "").trim();
  const source = "step" in (item as Record<string, unknown>) ? String((item as Record<string, unknown>).step || "").trim() : "";
  if (source) return `${kind}: ${source}`;
  return kind || "patch";
}

function metaLine(parts: Array<string | null | undefined>): string {
  return parts.map((item) => String(item || "").trim()).filter(Boolean).join(" · ");
}

function badge(tr: ContextTranslator, kind: "status" | "stability" | "policy", value?: string | null) {
  const text = String(value || "").trim();
  if (!text) return null;
  const tone = kind === "status" ? statusTone(text) : kind === "stability" ? stabilityTone(text) : policyTone(text);
  const prefix =
    kind === "status"
      ? tr("context.learningBadgeStatus", "status")
      : kind === "stability"
        ? tr("context.learningBadgeStability", "stability")
        : tr("context.learningBadgeReview", "review");
  return (
    <span className={classNames("rounded-full px-2 py-0.5 text-[11px]", tone)}>
      {prefix}: {text}
    </span>
  );
}

function listToLines(values?: string[] | null): string {
  return Array.isArray(values) ? values.map((item) => String(item || "").trim()).filter(Boolean).join("\n") : "";
}

function linesToList(value: string): string[] {
  return String(value || "").split("\n").map((item) => item.trim()).filter(Boolean);
}

function emptyDraft(): SkillDraft {
  return {
    skillId: "",
    title: "",
    goal: "",
    steps: "",
    constraints: "",
    failureSignals: "",
    stability: "stable",
    reviewMode: "auto_merge_eligible",
    status: "active",
  };
}

function draftFromSkill(skill: LearningProceduralSkill): SkillDraft {
  return {
    skillId: String(skill.skill_id || "").trim(),
    title: String(skill.title || "").trim(),
    goal: String(skill.goal || "").trim(),
    steps: listToLines(skill.steps),
    constraints: listToLines(skill.constraints),
    failureSignals: listToLines(skill.failure_signals),
    stability: String(skill.stability || "stable").trim() || "stable",
    reviewMode: String(skill.review_mode || "auto_merge_eligible").trim() || "auto_merge_eligible",
    status: String(skill.status || "active").trim() || "active",
  };
}

function draftToPayload(draft: SkillDraft): ProceduralSkillPayload {
  return {
    skill_id: String(draft.skillId || "").trim(),
    title: String(draft.title || "").trim(),
    goal: String(draft.goal || "").trim(),
    steps: linesToList(draft.steps),
    constraints: linesToList(draft.constraints),
    failure_signals: linesToList(draft.failureSignals),
    stability: String(draft.stability || "stable").trim() || "stable",
    review_mode: String(draft.reviewMode || "auto_merge_eligible").trim() || "auto_merge_eligible",
    status: String(draft.status || "active").trim() || "active",
  };
}

export function LearningPanel({ groupId, tr, ui, data, loading, error, onRefresh }: LearningPanelProps) {
  const overview = data?.overview;
  const funnel = data?.funnel;
  const pending = Array.isArray(data?.pending_patches) ? data?.pending_patches : [];
  const recent = Array.isArray(data?.recent_learning) ? data?.recent_learning : [];
  const observing = Array.isArray(data?.observing_skills) ? data?.observing_skills : [];
  const skills = Array.isArray(data?.skills) ? data?.skills : [];

  const [editingSkillId, setEditingSkillId] = useState("");
  const [editDraft, setEditDraft] = useState<SkillDraft>(() => emptyDraft());
  const [actionBusy, setActionBusy] = useState("");
  const [actionError, setActionError] = useState("");

  const cardClass = classNames("rounded-xl border p-4", "glass-card");
  const statClass = classNames("rounded-xl border p-3", "glass-card");
  const skillCountLabel = useMemo(() => tr("context.learningSkillCount", "Skills: {{count}}", { count: skills.length }), [skills.length, tr]);
  const activeSkillCount = Number(overview?.active_skill_count || 0);
  const skillsDataMismatch = activeSkillCount > 0 && skills.length === 0;

  function startEdit(skill: LearningProceduralSkill) {
    setEditingSkillId(skill.skill_id);
    setEditDraft(draftFromSkill(skill));
    setActionError("");
  }

  async function handleSaveEdit() {
    if (!groupId || !editingSkillId) return;
    setActionBusy(`save:${editingSkillId}`);
    setActionError("");
    const resp = await updateProceduralSkill(groupId, editingSkillId, draftToPayload(editDraft));
    if (!resp.ok) {
      setActionError(resp.error?.message || tr("context.learningSkillSaveFailed", "Failed to save procedural skill"));
      setActionBusy("");
      return;
    }
    setEditingSkillId("");
    setActionBusy("");
    onRefresh();
  }

  async function handleToggleStatus(skill: LearningProceduralSkill) {
    if (!groupId) return;
    const nextStatus = String(skill.status || "").trim().toLowerCase() === "active" ? "disabled" : "active";
    setActionBusy(`toggle:${skill.skill_id}`);
    setActionError("");
    const resp = await updateProceduralSkill(groupId, skill.skill_id, {
      title: String(skill.title || "").trim(),
      goal: String(skill.goal || "").trim(),
      steps: Array.isArray(skill.steps) ? skill.steps : [],
      constraints: Array.isArray(skill.constraints) ? skill.constraints : [],
      failure_signals: Array.isArray(skill.failure_signals) ? skill.failure_signals : [],
      stability: String(skill.stability || "stable").trim() || "stable",
      review_mode: String(skill.review_mode || "auto_merge_eligible").trim() || "auto_merge_eligible",
      status: nextStatus,
    });
    if (!resp.ok) {
      setActionError(resp.error?.message || tr("context.learningSkillToggleFailed", "Failed to update skill status"));
      setActionBusy("");
      return;
    }
    setActionBusy("");
    onRefresh();
  }

  async function handleDelete(skillId: string) {
    if (!groupId) return;
    setActionBusy(`delete:${skillId}`);
    setActionError("");
    const resp = await deleteProceduralSkill(groupId, skillId);
    if (!resp.ok) {
      setActionError(resp.error?.message || tr("context.learningSkillDeleteFailed", "Failed to delete procedural skill"));
      setActionBusy("");
      return;
    }
    if (editingSkillId === skillId) {
      setEditingSkillId("");
    }
    setActionBusy("");
    onRefresh();
  }

  return (
    <section className="space-y-4">
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>
            {tr("context.learningTitle", "Learning loop")}
          </div>
          <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>
            {tr("context.learningHint", "Show evidence, patch review pressure, and the learning that already changed runtime behavior.")}
          </div>
        </div>
        <button type="button" onClick={onRefresh} disabled={loading || !!actionBusy} className={ui.buttonSecondaryClass}>
          {loading ? tr("context.loading", "Loading…") : tr("context.refresh", "Refresh")}
        </button>
      </div>

      {error ? (
        <div className={classNames("rounded-xl border px-3 py-2 text-sm", "border-rose-500/30 bg-rose-500/15 text-rose-600 dark:text-rose-400")}>
          {error}
        </div>
      ) : null}
      {actionError ? (
        <div className={classNames("rounded-xl border px-3 py-2 text-sm", "border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300")}>
          {actionError}
        </div>
      ) : null}
      {skillsDataMismatch ? (
        <div className={classNames("rounded-xl border px-3 py-2 text-sm", "border-cyan-500/30 bg-cyan-500/10 text-cyan-700 dark:text-cyan-300")}>
          {tr(
            "context.learningSkillDataMismatch",
            "This group has {{count}} active procedural skill entries, but the current panel snapshot did not include their details. Refreshing should recover it.",
            { count: activeSkillCount },
          )}
        </div>
      ) : null}

      <div className="grid gap-3 lg:grid-cols-4">
        <div className={statClass}>
          <div className={classNames("text-[11px] font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.learningEvidence24h", "Evidence 24h")}</div>
          <div className={classNames("mt-2 text-2xl font-semibold", "text-[var(--color-text-primary)]")}>{Number(overview?.usage_events_24h || 0)}</div>
          <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{tr("context.learningEvidence7d", "7d total: {{count}}", { count: Number(overview?.usage_events_7d || 0) })}</div>
        </div>
        <div className={statClass}>
          <div className={classNames("text-[11px] font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.learningPending", "Pending patches")}</div>
          <div className={classNames("mt-2 text-2xl font-semibold", "text-[var(--color-text-primary)]")}>{Number(overview?.pending_patch_count || 0)}</div>
          <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{tr("context.learningBelowThreshold", "Below threshold: {{count}}", { count: Number(funnel?.below_threshold_count || 0) })}</div>
        </div>
        <div className={statClass}>
          <div className={classNames("text-[11px] font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.learningMerged", "Merged 7d")}</div>
          <div className={classNames("mt-2 text-2xl font-semibold", "text-[var(--color-text-primary)]")}>{Number(overview?.merged_patch_count_7d || 0)}</div>
          <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{tr("context.learningRejected", "Rejected 7d: {{count}}", { count: Number(overview?.rejected_patch_count_7d || 0) })}</div>
        </div>
        <div className={statClass}>
          <div className={classNames("text-[11px] font-medium uppercase tracking-wide", ui.mutedTextClass)}>{tr("context.learningRuntime", "Runtime-active")}</div>
          <div className={classNames("mt-2 text-2xl font-semibold", "text-[var(--color-text-primary)]")}>{Number(overview?.runtime_consumed_recent_count || 0)}</div>
          <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{tr("context.learningObserving", "Observing: {{count}}", { count: Number(overview?.observing_skill_count || 0) })}</div>
        </div>
      </div>

      <section className={cardClass}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>{tr("context.learningSkillAdmin", "Procedural skills")}</div>
            <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{tr("context.learningSkillAdminHint", "Review and trim the runtime-consumed procedural skill assets directly from this panel.")}</div>
          </div>
          <div className={classNames("text-xs", ui.mutedTextClass)}>{skillCountLabel}</div>
        </div>
        {activeSkillCount > 0 ? (
          <div className={classNames("mt-3 rounded-xl border px-3 py-2 text-sm", "border-emerald-500/25 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300")}>
            {tr("context.learningSkillActiveSummary", "Runtime is currently consuming {{count}} active procedural skill(s).", { count: activeSkillCount })}
          </div>
        ) : null}

        <div className="mt-4 space-y-3">
          {skills.length > 0 ? skills.map((item) => {
            const isEditing = editingSkillId === item.skill_id;
            return (
              <div key={item.skill_id} className={classNames("rounded-lg border p-3", "glass-panel")}>
                <div className="flex items-start justify-between gap-3">
                  <div>
                    <div className={classNames("text-sm font-medium", "text-[var(--color-text-primary)]")}>{item.title || item.skill_id}</div>
                    <div className={classNames("mt-1 text-sm", ui.subtleTextClass)}>{item.goal || tr("context.learningNoGoal", "No goal recorded")}</div>
                  </div>
                  <div className="flex flex-wrap justify-end gap-2">
                    {badge(tr, "status", item.status)}
                    {badge(tr, "stability", item.stability)}
                    {badge(tr, "policy", item.review_mode)}
                  </div>
                </div>
                <div className={classNames("mt-2 text-xs", ui.mutedTextClass)}>
                  {metaLine([
                    item.skill_id,
                    item.updated_at ? noteTimestamp({ at: item.updated_at }) : "",
                    item.source_experience_candidate_id || "",
                  ])}
                </div>
                <div className={classNames("mt-2 text-xs", ui.subtleTextClass)}>
                  {tr("context.learningSkillSteps", "Steps")}: {item.steps.length > 0 ? item.steps.join(" · ") : tr("context.learningSkillEmpty", "none")}
                </div>
                {item.constraints.length > 0 ? (
                  <div className={classNames("mt-1 text-xs", ui.subtleTextClass)}>
                    {tr("context.learningSkillConstraints", "Constraints")}: {item.constraints.join(" · ")}
                  </div>
                ) : null}
                {item.failure_signals.length > 0 ? (
                  <div className={classNames("mt-1 text-xs", ui.subtleTextClass)}>
                    {tr("context.learningSkillFailures", "Failure signals")}: {item.failure_signals.join(" · ")}
                  </div>
                ) : null}
                <div className="mt-3 flex flex-wrap gap-2">
                  <button type="button" onClick={() => startEdit(item)} disabled={!!actionBusy} className={ui.buttonSecondaryClass}>
                    {tr("context.edit", "Edit")}
                  </button>
                  <button type="button" onClick={() => void handleToggleStatus(item)} disabled={!!actionBusy} className={ui.buttonSecondaryClass}>
                    {actionBusy === `toggle:${item.skill_id}`
                      ? tr("context.saving", "Saving…")
                      : String(item.status || "").trim().toLowerCase() === "active"
                        ? tr("context.learningSkillDisable", "Disable")
                        : tr("context.learningSkillEnable", "Enable")}
                  </button>
                  <button type="button" onClick={() => void handleDelete(item.skill_id)} disabled={!!actionBusy} className={ui.buttonDangerClass}>
                    {actionBusy === `delete:${item.skill_id}` ? tr("context.deleting", "Deleting…") : tr("context.delete", "Delete")}
                  </button>
                </div>
                {isEditing ? (
                  <form
                    className="mt-3 space-y-3 border-t border-[var(--glass-border-subtle)] pt-3"
                    onSubmit={(event: FormEvent<HTMLFormElement>) => {
                      event.preventDefault();
                      void handleSaveEdit();
                    }}
                  >
                    <input value={editDraft.title} onChange={(event) => setEditDraft((prev) => ({ ...prev, title: event.target.value }))} className={ui.inputClass} />
                    <textarea value={editDraft.goal} onChange={(event) => setEditDraft((prev) => ({ ...prev, goal: event.target.value }))} className={classNames(ui.textareaClass, "min-h-[80px]")} />
                    <textarea value={editDraft.steps} onChange={(event) => setEditDraft((prev) => ({ ...prev, steps: event.target.value }))} className={classNames(ui.textareaClass, "min-h-[96px]")} placeholder={tr("context.learningSkillStepsPlaceholder", "One step per line")} />
                    <div className="grid gap-3 lg:grid-cols-2">
                      <textarea value={editDraft.constraints} onChange={(event) => setEditDraft((prev) => ({ ...prev, constraints: event.target.value }))} className={classNames(ui.textareaClass, "min-h-[72px]")} placeholder={tr("context.learningSkillConstraintsPlaceholder", "Constraints, one per line")} />
                      <textarea value={editDraft.failureSignals} onChange={(event) => setEditDraft((prev) => ({ ...prev, failureSignals: event.target.value }))} className={classNames(ui.textareaClass, "min-h-[72px]")} placeholder={tr("context.learningSkillFailuresPlaceholder", "Failure signals, one per line")} />
                    </div>
                    <div className="grid gap-3 sm:grid-cols-3">
                      <select value={editDraft.status} onChange={(event) => setEditDraft((prev) => ({ ...prev, status: event.target.value }))} className={ui.inputClass}>
                        <option value="active">{tr("context.learningSkillStatusActive", "active")}</option>
                        <option value="disabled">{tr("context.learningSkillStatusDisabled", "disabled")}</option>
                      </select>
                      <select value={editDraft.stability} onChange={(event) => setEditDraft((prev) => ({ ...prev, stability: event.target.value }))} className={ui.inputClass}>
                        <option value="stable">{tr("context.learningSkillStabilityStable", "stable")}</option>
                        <option value="probation">{tr("context.learningSkillStabilityProbation", "probation")}</option>
                        <option value="unstable">{tr("context.learningSkillStabilityUnstable", "unstable")}</option>
                      </select>
                      <select value={editDraft.reviewMode} onChange={(event) => setEditDraft((prev) => ({ ...prev, reviewMode: event.target.value }))} className={ui.inputClass}>
                        <option value="auto_merge_eligible">{tr("context.learningSkillReviewAuto", "auto_merge_eligible")}</option>
                        <option value="manual_review_required">{tr("context.learningSkillReviewManual", "manual_review_required")}</option>
                      </select>
                    </div>
                    <div className="flex justify-end gap-2">
                      <button type="button" onClick={() => setEditingSkillId("")} disabled={!!actionBusy} className={ui.buttonSecondaryClass}>
                        {tr("context.cancel", "Cancel")}
                      </button>
                      <button type="submit" disabled={!!actionBusy} className={ui.buttonPrimaryClass}>
                        {actionBusy === `save:${item.skill_id}` ? tr("context.saving", "Saving…") : tr("context.save", "Save")}
                      </button>
                    </div>
                  </form>
                ) : null}
              </div>
            );
          }) : <div className={classNames("text-sm", ui.mutedTextClass)}>{tr("context.learningSkillEmptyList", "No procedural skills yet.")}</div>}
        </div>
      </section>

      <section className={cardClass}>
        <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>{tr("context.learningFunnel", "Learning funnel")}</div>
        <div className="mt-3 grid gap-2 md:grid-cols-7">
          {[
            { label: tr("context.learningFunnelEvidence", "Evidence"), value: funnel?.evidence_count || 0 },
            { label: tr("context.learningFunnelCreated", "Candidates"), value: funnel?.candidate_created_count || 0 },
            { label: tr("context.learningFunnelCandidate", "Patch ready"), value: funnel?.candidate_ready_count || 0 },
            { label: tr("context.learningFunnelPending", "Manual review"), value: funnel?.pending_review_count || 0 },
            { label: tr("context.learningFunnelMerged", "Merged"), value: funnel?.merged_count || 0 },
            { label: tr("context.learningFunnelConsumed", "Consumed"), value: funnel?.runtime_consumed_count || 0 },
            { label: tr("context.learningFunnelThreshold", "Threshold"), value: Number(funnel?.threshold || 0).toFixed(2) },
          ].map((item) => (
            <div key={item.label} className={classNames("rounded-lg border px-3 py-2", "glass-panel")}>
              <div className={classNames("text-[11px] uppercase tracking-wide", ui.mutedTextClass)}>{item.label}</div>
              <div className={classNames("mt-1 text-lg font-semibold", "text-[var(--color-text-primary)]")}>{item.value}</div>
            </div>
          ))}
        </div>
      </section>

      <div className="grid gap-4 xl:grid-cols-2">
        <section className={cardClass}>
          <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>{tr("context.learningNeedsReview", "Needs review")}</div>
          <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{tr("context.learningNeedsReviewHint", "High-signal patch candidates waiting for governance.")}</div>
          <div className="mt-3 space-y-2">
            {pending.length > 0 ? pending.map((item) => (
              <div key={item.candidate_id} className={classNames("rounded-lg border p-3", "glass-panel")}>
                <div className="flex items-start justify-between gap-3">
                  <div className={classNames("text-sm font-medium", "text-[var(--color-text-primary)]")}>{renderPatchSummary(item)}</div>
                  <span className={classNames("rounded-full px-2 py-0.5 text-[11px]", "bg-amber-500/15 text-amber-600 dark:text-amber-400")}>
                    {Number(item.score || 0).toFixed(2)}
                  </span>
                </div>
                <div className={classNames("mt-1 text-sm", ui.subtleTextClass)}>{item.reason || tr("context.learningNoReason", "No reason recorded")}</div>
                <div className="mt-2 flex flex-wrap gap-2">
                  {badge(tr, "policy", item.review_mode)}
                  {item.regressed_from_candidate_id ? (
                    <span className={classNames("rounded-full px-2 py-0.5 text-[11px]", "bg-rose-500/15 text-rose-600 dark:text-rose-400")}>
                      {tr("context.learningBadgeRegressedFrom", "from")}: {item.regressed_from_candidate_id}
                    </span>
                  ) : null}
                </div>
                <div className={classNames("mt-2 text-xs", ui.mutedTextClass)}>
                  {metaLine([
                    item.skill_id,
                    item.sample_evidence_type,
                    item.sample_outcome,
                    item.last_evidence_at ? noteTimestamp({ at: item.last_evidence_at }) : "",
                    `${tr("context.learningEvidenceCount", "evidence")} ${item.evidence_count}`,
                  ])}
                </div>
              </div>
            )) : <div className={classNames("text-sm", ui.mutedTextClass)}>{tr("context.learningNothingPending", "No pending learning patches.")}</div>}
          </div>
        </section>

        <section className={cardClass}>
          <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>{tr("context.learningRecent", "Recently effective learning")}</div>
          <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{tr("context.learningRecentHint", "Merged skill updates and whether they have already been consumed again at runtime.")}</div>
          <div className="mt-3 space-y-2">
            {recent.length > 0 ? recent.map((item) => (
              <div key={`${item.skill_id}:${item.candidate_id}`} className={classNames("rounded-lg border p-3", "glass-panel")}>
                <div className="flex items-start justify-between gap-3">
                  <div className={classNames("text-sm font-medium", "text-[var(--color-text-primary)]")}>{item.title || item.skill_id}</div>
                  <div className="flex flex-wrap justify-end gap-2">
                    {badge(tr, "status", item.post_merge_status)}
                    {badge(tr, "stability", item.stability)}
                    {badge(tr, "policy", item.patch_review_mode)}
                  </div>
                </div>
                <div className={classNames("mt-1 text-sm", ui.subtleTextClass)}>{item.reason || renderPatchSummary(item)}</div>
                {item.followup_candidate_id ? (
                  <div className={classNames("mt-2 text-xs", ui.subtleTextClass)}>
                    {tr("context.learningFollowupLink", "Follow-up")}: {item.followup_candidate_id}
                    {item.regressed_from_candidate_id ? ` (${tr("context.learningFollowupFrom", "from")} ${item.regressed_from_candidate_id})` : ""}
                  </div>
                ) : null}
                <div className={classNames("mt-2 text-xs", ui.mutedTextClass)}>
                  {metaLine([
                    item.patch_kind,
                    item.merged_by,
                    item.merged_at ? noteTimestamp({ at: item.merged_at }) : "",
                    `${tr("context.learningConsumed", "consumed")} ${item.runtime_consumed_count}`,
                  ])}
                </div>
              </div>
            )) : <div className={classNames("text-sm", ui.mutedTextClass)}>{tr("context.learningNothingMerged", "No merged learning yet.")}</div>}
          </div>
        </section>
      </div>

      <section className={cardClass}>
        <div className={classNames("text-sm font-semibold", "text-[var(--color-text-primary)]")}>{tr("context.learningObservationQueue", "Observation queue")}</div>
        <div className={classNames("mt-1 text-xs", ui.mutedTextClass)}>{tr("context.learningObservationHint", "Merged patches stay here until post-merge evidence validates or regresses them.")}</div>
        <div className="mt-3 space-y-2">
          {observing.length > 0 ? observing.map((item: LearningObservingSkill) => (
            <div key={`${item.skill_id}:${item.candidate_id || item.status}`} className={classNames("rounded-lg border p-3", "glass-panel")}>
              <div className="flex items-start justify-between gap-3">
                <div className={classNames("text-sm font-medium", "text-[var(--color-text-primary)]")}>{item.title || item.skill_id}</div>
                <div className="flex flex-wrap justify-end gap-2">
                  {badge(tr, "status", item.status)}
                  {badge(tr, "stability", item.stability)}
                  {badge(tr, "policy", item.patch_review_mode)}
                </div>
              </div>
              <div className={classNames("mt-1 text-sm", ui.subtleTextClass)}>{item.goal || tr("context.learningNoGoal", "No goal recorded")}</div>
              <div className={classNames("mt-2 text-xs", ui.mutedTextClass)}>
                {metaLine([
                  item.candidate_id ? `${tr("context.learningCandidateLabel", "candidate")} ${item.candidate_id}` : "",
                  item.opened_at ? noteTimestamp({ at: item.opened_at }) : "",
                  item.observe_until ? `${tr("context.learningObserveUntil", "until")} ${noteTimestamp({ at: item.observe_until })}` : "",
                ])}
              </div>
              {item.followup_candidate_id ? (
                <div className={classNames("mt-2 text-xs", ui.subtleTextClass)}>
                  {tr("context.learningFollowupLink", "Follow-up")}: {item.followup_candidate_id}
                  {item.followup_review_mode ? ` · ${tr("context.learningBadgeReview", "review")}: ${item.followup_review_mode}` : ""}
                  {item.regressed_from_candidate_id ? ` · ${tr("context.learningFollowupFrom", "from")} ${item.regressed_from_candidate_id}` : ""}
                </div>
              ) : null}
            </div>
          )) : <div className={classNames("text-sm", ui.mutedTextClass)}>{tr("context.learningObservationEmpty", "No learning items are waiting on observation.")}</div>}
        </div>
      </section>
    </section>
  );
}
