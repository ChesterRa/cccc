// Shared actor utilities — extracted from ActorCharacter.tsx for react-refresh compatibility

import type { AgentState } from "../types";

export type AgentAnimState = "offline" | "blocked" | "working" | "thinking" | "idle";

/**
 * Freshness threshold: context updates happen at key transitions in multi-agent
 * systems, not every minute. 30 minutes balances accuracy with realistic update cadence.
 */
const FRESHNESS_MS = 30 * 60_000; // 30 minutes

/** Derive animation state from agent state data. Priority: offline > blocked > working > thinking > idle.
 *  When lastActivityAt (epoch ms from WS heartbeat) is provided, it takes priority
 *  over agent-state recency heuristics for recent activity windows. */
export function deriveAnimState(
  agent: AgentState,
  isRunning?: boolean,
  lastActivityAt?: number,
  taskStatus?: string,
): AgentAnimState {
  if (isRunning === false) return "offline";

  const hasActiveTask = !!(
    agent.hot?.active_task_id &&
    taskStatus !== "done" &&
    taskStatus !== "archived"
  );
  const hasIntent = !!(agent.hot?.focus || agent.hot?.next_action);
  const hasActiveBlocker = !!(hasActiveTask && Array.isArray(agent.hot?.blockers) && agent.hot.blockers.length > 0);

  if (hasActiveBlocker) return "blocked";

  // WS-priority: use lastActivityAt when available
  if (lastActivityAt != null) {
    const gap = Date.now() - lastActivityAt;
    if (hasActiveTask && gap < 10_000) return "working";   // < 10s + active task → actively working
    if ((hasActiveTask || hasIntent) && gap < 60_000) return "thinking"; // 仅有活动但无任务，不再升级为 working
    if (gap >= 300_000) return "idle";           // ≥ 5min → idle
    // 60s–300s: fall through to agent-state recency logic below
  }

  const age = agent.updated_at
    ? Date.now() - new Date(agent.updated_at).getTime()
    : Infinity;
  const fresh = age <= FRESHNESS_MS;

  // Has active task but task is already done/archived → treat as idle/thinking
  if (agent.hot?.active_task_id && (taskStatus === "done" || taskStatus === "archived")) {
    return (agent.hot?.focus && fresh) ? "thinking" : "idle";
  }
  // Has active task → working (if fresh + focus) or thinking (minimum)
  if (hasActiveTask) {
    return (agent.hot?.focus && fresh) ? "working" : "thinking";
  }
  // Has focus/next_action but no task → thinking (if fresh) or idle (if stale)
  if (agent.hot?.focus || agent.hot?.next_action) {
    return fresh ? "thinking" : "idle";
  }
  return "idle";
}

/** Map animation state to a short Chinese status label for the 3D scene HUD */
export function deriveStatusLabel(
  animState: AgentAnimState,
  hasActiveTask: boolean,
  isForeman = false,
): { text: string; color: string } {
  switch (animState) {
    case "working":
      return isForeman
        ? { text: "指挥中", color: "#4ade80" }
        : { text: "执行中", color: "#4ade80" };
    case "thinking":
      return { text: "思考中", color: "#facc15" };
    case "blocked":
      return { text: "受阻", color: "#f87171" };
    case "idle":
      return { text: "待命", color: "#94a3b8" };
    case "offline":
      return { text: "离线", color: "#64748b" };
  }
}

// Body part name → child index mapping (for unified useFrame animation)
export const PART_INDEX = {
  torso: 0,
  head: 1,
  leftArm: 2,
  rightArm: 3,
  leftLeg: 4,
  rightLeg: 5,
} as const;

export function hashCode(s: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < s.length; i++) {
    h ^= s.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}
