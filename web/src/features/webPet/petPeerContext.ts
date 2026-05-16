import { useEffect, useState } from "react";
import type { PetPeerContextResponse } from "../../services/api";
import { fetchPetPeerContext } from "../../services/api";
import type { PetCompanionProfile, PetReminder } from "./types";

export type PetPeerContextStatus = "idle" | "loading" | "loaded" | "error";

const PET_CONTEXT_INITIAL_FETCH_DELAY_MS = 800;
const petPeerContextCache = new Map<string, Partial<PetPeerContextResponse> | null>();

export type PetTaskEvidence = {
  kind: string;
  priority: number;
  hypothesis: string;
  actor: {
    id: string;
    activeTaskId?: string;
    focus?: string;
    nextAction?: string;
    blockers: string[];
  };
  task: {
    id: string;
    title: string;
    status: string;
    assignee?: string;
    waitingOn?: string;
    blockedBy: string[];
    handoffTo?: string;
    updatedAt?: string;
  };
  currentActiveTask?: {
    id: string;
    title: string;
    status: string;
    assignee?: string;
    waitingOn?: string;
    blockedBy: string[];
    handoffTo?: string;
    updatedAt?: string;
  };
  signals: {
    taskStaleMinutes: number;
    sameWorkstreamHint: boolean;
    blockerCount: number;
  };
};

export type PetPeerContext = {
  companion: PetCompanionProfile;
  decisions: PetReminder[];
  taskEvidence: PetTaskEvidence[];
  signals: {
    replyPressure: {
      severity: string;
      pendingCount: number;
      overdueCount: number;
      oldestPendingSeconds: number;
      baselineMedianReplySeconds: number;
    };
    coordinationRhythm: {
      severity: string;
      foremanId?: string;
      silenceSeconds: number;
      baselineMedianGapSeconds: number;
    };
    taskPressure: {
      severity: string;
      score: number;
      trendScore: number;
      blockedCount: number;
      waitingUserCount: number;
      handoffCount: number;
      plannedBacklogCount: number;
      recentBlockedUpdates: number;
      recentWaitingUserUpdates: number;
      recentHandoffUpdates: number;
      recentTaskCreateOps: number;
      recentTaskUpdateOps: number;
      recentTaskMoveOps: number;
      recentTaskRestoreOps: number;
      recentTaskDeleteOps: number;
      recentTaskChangeCount: number;
      recentTaskContextSyncEvents: number;
      ledgerTrendScore: number;
    };
    proposalReady: {
      ready: boolean;
      focus: string;
      severity: string;
      summary: string;
      pendingReplyCount: number;
      overdueReplyCount: number;
      waitingUserCount: number;
      blockedCount: number;
      handoffCount: number;
      recentTaskChangeCount: number;
      foremanSilenceSeconds: number;
    };
  };
  persona: string;
  help: string;
  prompt: string;
  snapshot: string;
  source: "help" | "default";
  status: PetPeerContextStatus;
};

const DEFAULT_COMPANION: PetCompanionProfile = {
  name: "Momo",
  species: "cat",
  identity: "Momo is a small desk-side companion who watches team flow quietly.",
  temperament: "steady",
  speechStyle: "short, plain sentences",
  careStyle: "prefers the smallest next step that unblocks progress",
};

function mapTaskProposalOperation(value: unknown): "create" | "update" | "move" | "handoff" | "archive" {
  const normalized = String(value || "").trim().toLowerCase();
  if (normalized === "create") return "create";
  if (normalized === "move") return "move";
  if (normalized === "handoff") return "handoff";
  if (normalized === "archive") return "archive";
  return "update";
}

function mapSignals(raw?: PetPeerContextResponse["signals"] | null): PetPeerContext["signals"] {
  const reply = raw?.reply_pressure || {};
  const rhythm = raw?.coordination_rhythm || {};
  const task = raw?.task_pressure || {};
  const proposal = raw?.proposal_ready || {};
  return {
    replyPressure: {
      severity: String(reply.severity || "low").trim() || "low",
      pendingCount: Number(reply.pending_count || 0),
      overdueCount: Number(reply.overdue_count || 0),
      oldestPendingSeconds: Number(reply.oldest_pending_seconds || 0),
      baselineMedianReplySeconds: Number(reply.baseline_median_reply_seconds || 0),
    },
    coordinationRhythm: {
      severity: String(rhythm.severity || "low").trim() || "low",
      foremanId: String(rhythm.foreman_id || "").trim() || undefined,
      silenceSeconds: Number(rhythm.silence_seconds || 0),
      baselineMedianGapSeconds: Number(rhythm.baseline_median_gap_seconds || 0),
    },
    taskPressure: {
      severity: String(task.severity || "low").trim() || "low",
      score: Number(task.score || 0),
      trendScore: Number(task.trend_score || 0),
      blockedCount: Number(task.blocked_count || 0),
      waitingUserCount: Number(task.waiting_user_count || 0),
      handoffCount: Number(task.handoff_count || 0),
      plannedBacklogCount: Number(task.planned_backlog_count || 0),
      recentBlockedUpdates: Number(task.recent_blocked_updates || 0),
      recentWaitingUserUpdates: Number(task.recent_waiting_user_updates || 0),
      recentHandoffUpdates: Number(task.recent_handoff_updates || 0),
      recentTaskCreateOps: Number(task.recent_task_create_ops || 0),
      recentTaskUpdateOps: Number(task.recent_task_update_ops || 0),
      recentTaskMoveOps: Number(task.recent_task_move_ops || 0),
      recentTaskRestoreOps: Number(task.recent_task_restore_ops || 0),
      recentTaskDeleteOps: Number(task.recent_task_delete_ops || 0),
      recentTaskChangeCount: Number(task.recent_task_change_count || 0),
      recentTaskContextSyncEvents: Number(task.recent_task_context_sync_events || 0),
      ledgerTrendScore: Number(task.ledger_trend_score || 0),
    },
    proposalReady: {
      ready: !!proposal.ready,
      focus: String(proposal.focus || "none").trim() || "none",
      severity: String(proposal.severity || "low").trim() || "low",
      summary: String(proposal.summary || "").trim(),
      pendingReplyCount: Number(proposal.pending_reply_count || 0),
      overdueReplyCount: Number(proposal.overdue_reply_count || 0),
      waitingUserCount: Number(proposal.waiting_user_count || 0),
      blockedCount: Number(proposal.blocked_count || 0),
      handoffCount: Number(proposal.handoff_count || 0),
      recentTaskChangeCount: Number(proposal.recent_task_change_count || 0),
      foremanSilenceSeconds: Number(proposal.foreman_silence_seconds || 0),
    },
  };
}

function mapDecision(raw: NonNullable<PetPeerContextResponse["decisions"]>[number]): PetReminder | null {
  const id = String(raw?.id || "").trim();
  const kind = String(raw?.kind || "").trim();
  const fingerprint = String(raw?.fingerprint || "").trim();
  const actionType = String(raw?.action?.type || "").trim();
  if (!id || !kind || !fingerprint || !actionType) return null;

  const action =
    actionType === "restart_actor"
      ? {
          type: "restart_actor" as const,
          groupId: String(raw?.action?.group_id || "").trim(),
          actorId: String(raw?.action?.actor_id || "").trim(),
        }
      : actionType === "task_proposal"
        ? {
            type: "task_proposal" as const,
            groupId: String(raw?.action?.group_id || "").trim(),
            operation: mapTaskProposalOperation(raw?.action?.operation),
            taskId: String(raw?.action?.task_id || "").trim() || undefined,
            title: String(raw?.action?.title || "").trim() || undefined,
            status: String(raw?.action?.status || "").trim() || undefined,
            assignee: String(raw?.action?.assignee || "").trim() || undefined,
            text: String(raw?.action?.text || "").trim() || undefined,
          }
      : actionType === "draft_message"
        ? {
          type: "draft_message" as const,
          groupId: String(raw?.action?.group_id || "").trim(),
          text: String(raw?.action?.text || "").trim(),
          to: Array.isArray(raw?.action?.to)
            ? raw.action.to.map((entry) => String(entry || "").trim()).filter(Boolean)
            : undefined,
          replyTo: String(raw?.action?.reply_to || "").trim() || undefined,
        }
        : null;

  if (!action) return null;

  if (action.type === "restart_actor" && (!action.groupId || !action.actorId)) return null;
  if (action.type === "draft_message" && !action.text) return null;
  if (action.type === "task_proposal" && !action.groupId) return null;

  return {
    id,
    kind: kind === "actor_down" ? "actor_down" : "suggestion",
    priority: Number(raw?.priority || 0),
    summary: String(raw?.summary || "").trim(),
    confidence:
      String(raw?.confidence || "").trim() === "low"
        ? "low"
        : String(raw?.confidence || "").trim() === "high"
          ? "high"
          : String(raw?.confidence || "").trim() === "medium"
            ? "medium"
            : undefined,
    reasoningBrief: String(raw?.reasoning_brief || "").trim() || undefined,
    agent: String(raw?.agent || "").trim(),
    ephemeral: !!raw?.ephemeral,
    source: {
      eventId: String(raw?.source?.event_id || "").trim() || undefined,
      taskId: String(raw?.source?.task_id || "").trim() || undefined,
      actorId: String(raw?.source?.actor_id || "").trim() || undefined,
      actorRole: String(raw?.source?.actor_role || "").trim() || undefined,
      errorReason: String(raw?.source?.error_reason || "").trim() || undefined,
      suggestionKind:
        String(raw?.source?.suggestion_kind || "").trim() === "reply_required"
          ? "reply_required"
          : String(raw?.source?.suggestion_kind || "").trim() === "mention"
            ? "mention"
            : undefined,
    },
    fingerprint,
    action,
  };
}

function mapEvidenceTask(
  raw?: {
    id?: string | null;
    title?: string | null;
    status?: string | null;
    assignee?: string | null;
    waiting_on?: string | null;
    blocked_by?: string[] | null;
    handoff_to?: string | null;
    updated_at?: string | null;
  } | null,
): PetTaskEvidence["task"] | undefined {
  const id = String(raw?.id || "").trim();
  const title = String(raw?.title || "").trim();
  const status = String(raw?.status || "").trim();
  if (!id || !status) return undefined;
  return {
    id,
    title,
    status,
    assignee: String(raw?.assignee || "").trim() || undefined,
    waitingOn: String(raw?.waiting_on || "").trim() || undefined,
    blockedBy: Array.isArray(raw?.blocked_by)
      ? raw.blocked_by.map((item) => String(item || "").trim()).filter(Boolean)
      : [],
    handoffTo: String(raw?.handoff_to || "").trim() || undefined,
    updatedAt: String(raw?.updated_at || "").trim() || undefined,
  };
}

function mapTaskEvidence(raw?: NonNullable<PetPeerContextResponse["task_evidence"]>[number] | null): PetTaskEvidence | null {
  const kind = String(raw?.kind || "").trim();
  const actorId = String(raw?.actor?.id || "").trim();
  const task = mapEvidenceTask(raw?.task);
  if (!kind || !actorId || !task) return null;
  return {
    kind,
    priority: Number(raw?.priority || 0),
    hypothesis: String(raw?.hypothesis || "").trim(),
    actor: {
      id: actorId,
      activeTaskId: String(raw?.actor?.active_task_id || "").trim() || undefined,
      focus: String(raw?.actor?.focus || "").trim() || undefined,
      nextAction: String(raw?.actor?.next_action || "").trim() || undefined,
      blockers: Array.isArray(raw?.actor?.blockers)
        ? raw.actor.blockers.map((item) => String(item || "").trim()).filter(Boolean)
        : [],
    },
    task,
    currentActiveTask: mapEvidenceTask(raw?.current_active_task),
    signals: {
      taskStaleMinutes: Number(raw?.signals?.task_stale_minutes || 0),
      sameWorkstreamHint: !!raw?.signals?.same_workstream_hint,
      blockerCount: Number(raw?.signals?.blocker_count || 0),
    },
  };
}

function mapCompanion(raw?: PetPeerContextResponse["companion"] | null): PetCompanionProfile {
  const name = String(raw?.name || "").trim();
  const species = String(raw?.species || "").trim();
  const identity = String(raw?.identity || "").trim();
  const temperament = String(raw?.temperament || "").trim();
  const speechStyle = String(raw?.speech_style || "").trim();
  const careStyle = String(raw?.care_style || "").trim();
  return {
    name: name || DEFAULT_COMPANION.name,
    species: species || DEFAULT_COMPANION.species,
    identity: identity || DEFAULT_COMPANION.identity,
    temperament: temperament || DEFAULT_COMPANION.temperament,
    speechStyle: speechStyle || DEFAULT_COMPANION.speechStyle,
    careStyle: careStyle || DEFAULT_COMPANION.careStyle,
  };
}

export function buildPetPeerContext(
  raw?: Partial<PetPeerContextResponse> | null,
  opts?: { status?: PetPeerContextStatus },
): PetPeerContext {
  const persona = String(raw?.persona || "").trim();
  const help = String(raw?.help || "").trim();
  const prompt = String(raw?.prompt || "").trim();
  const snapshot = String(raw?.snapshot || "").trim();
  const decisions = Array.isArray(raw?.decisions)
    ? raw.decisions.map((item) => mapDecision(item)).filter((item): item is PetReminder => item !== null)
    : [];
  const taskEvidence = Array.isArray(raw?.task_evidence)
    ? raw.task_evidence.map((item) => mapTaskEvidence(item)).filter((item): item is PetTaskEvidence => item !== null)
    : [];

  return {
    companion: mapCompanion(raw?.companion),
    decisions,
    taskEvidence,
    signals: mapSignals(raw?.signals),
    persona,
    help,
    prompt,
    snapshot,
    source: raw?.source === "help" ? "help" : "default",
    status: opts?.status || "loaded",
  };
}

export function shouldFetchPetPeerContext(input: {
  groupId: string | null | undefined;
  enabled?: boolean;
}): boolean {
  const groupId = String(input.groupId || "").trim();
  return !!groupId && input.enabled !== false;
}

export function usePetPeerContext(input: {
  groupId: string | null | undefined;
  enabled?: boolean;
  refreshToken?: number;
}): PetPeerContext {
  const groupId = String(input.groupId || "").trim();
  const enabled = input.enabled !== false;
  const refreshToken = Number(input.refreshToken || 0);
  const cachedContext = groupId ? (petPeerContextCache.get(groupId) ?? null) : null;
  const [state, setState] = useState<{
    groupId: string;
    rawContext: Partial<PetPeerContextResponse> | null;
    status: PetPeerContextStatus;
  }>({
    groupId,
    rawContext: cachedContext,
    status: shouldFetchPetPeerContext({ groupId, enabled }) ? (cachedContext ? "loaded" : "loading") : "idle",
  });

  useEffect(() => {
    if (!shouldFetchPetPeerContext({ groupId, enabled })) {
      return;
    }

    let cancelled = false;
    const cached = petPeerContextCache.get(groupId) ?? null;
    const timeout = window.setTimeout(() => {
      void fetchPetPeerContext(groupId)
        .then((resp) => {
          if (cancelled) return;
          const nextRawContext = resp.ok ? resp.result || null : cached;
          if (resp.ok) {
            petPeerContextCache.set(groupId, nextRawContext);
          }
          setState({
            groupId,
            rawContext: nextRawContext,
            status: resp.ok ? "loaded" : cached ? "loaded" : "error",
          });
        })
        .catch((error) => {
          if (cancelled) return;
          console.warn("failed to load pet peer context", error);
          setState({
            groupId,
            rawContext: cached,
            status: cached ? "loaded" : "error",
          });
        });
    }, refreshToken > 0 ? 0 : PET_CONTEXT_INITIAL_FETCH_DELAY_MS);

    return () => {
      cancelled = true;
      window.clearTimeout(timeout);
    };
  }, [enabled, groupId, refreshToken]);

  const canFetch = shouldFetchPetPeerContext({ groupId, enabled });
  if (!canFetch || state.groupId !== groupId) {
    return buildPetPeerContext(cachedContext, {
      status: canFetch ? (cachedContext ? "loaded" : "loading") : "idle",
    });
  }
  if (state.status === "idle") {
    return buildPetPeerContext(cachedContext, {
      status: cachedContext ? "loaded" : "loading",
    });
  }

  return buildPetPeerContext(state.rawContext, { status: state.status });
}
