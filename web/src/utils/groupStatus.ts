import { getGroupPresenceDotClass } from "./statusIndicators";
import type { GroupDoc, GroupMeta, GroupRuntimeStatus } from "../types";

export type GroupStatusKey = "run" | "paused" | "idle" | "stop";

export const GROUP_STATUS_DOT_BASE_CLASS = "size-[8px] shrink-0 rounded-full";

export type GroupStatus = {
  key: GroupStatusKey;
  label: string;
  pillClass: string;
  dotClass: string;
};

type GroupStatusSource =
  | Pick<GroupMeta, "running" | "state" | "runtime_status">
  | Pick<GroupDoc, "running" | "state" | "runtime_status">;

function buildStatus(key: GroupStatusKey, label: string, dotClass: string): GroupStatus {
  return { key, label, pillClass: `glass-status-pill glass-status-pill-${key}`, dotClass };
}

export function getGroupStatus(running: boolean, state?: string): GroupStatus {
  if (state === "paused") {
    return buildStatus("paused", "PAUSED", getGroupPresenceDotClass("paused"));
  }
  if (!running) {
    return buildStatus("stop", "STOP", getGroupPresenceDotClass("stop"));
  }
  if (state === "idle") {
    return buildStatus("idle", "IDLE", getGroupPresenceDotClass("idle"));
  }
  return buildStatus("run", "RUN", getGroupPresenceDotClass("run"));
}

export function getGroupStatusLight(running: boolean, state?: string): GroupStatus {
  if (state === "paused") {
    return buildStatus("paused", "PAUSED", getGroupPresenceDotClass("paused"));
  }
  if (!running) {
    return buildStatus("stop", "STOP", getGroupPresenceDotClass("stop"));
  }
  if (state === "idle") {
    return buildStatus("idle", "IDLE", getGroupPresenceDotClass("idle"));
  }
  return buildStatus("run", "RUN", getGroupPresenceDotClass("run"));
}

/** Unified group status using dark: prefix - no isDark dependency needed */
export function getGroupStatusUnified(running: boolean, state?: string): GroupStatus {
  // Paused is a lifecycle state independent of whether processes are alive.
  // Show it truthfully so the launch button takes the correct resume path.
  if (state === "paused") {
    return buildStatus("paused", "PAUSED", getGroupPresenceDotClass("paused"));
  }
  if (!running) {
    return buildStatus("stop", "STOP", getGroupPresenceDotClass("stop"));
  }
  if (state === "idle") {
    return buildStatus("idle", "IDLE", getGroupPresenceDotClass("idle"));
  }
  return buildStatus("run", "RUN", getGroupPresenceDotClass("run"));
}

export function getGroupRuntimeStatus(source?: GroupStatusSource | null): GroupRuntimeStatus {
  const runtime = source?.runtime_status;
  return {
    lifecycle_state: String(runtime?.lifecycle_state || source?.state || "active"),
    runtime_running: Boolean(runtime?.runtime_running ?? source?.running ?? false),
    running_actor_count: Number.isFinite(Number(runtime?.running_actor_count))
      ? Number(runtime?.running_actor_count)
      : 0,
    has_running_foreman: Boolean(runtime?.has_running_foreman ?? false),
  };
}

export function getGroupStatusFromSource(source?: GroupStatusSource | null): GroupStatus {
  const runtime = getGroupRuntimeStatus(source);
  return getGroupStatusUnified(runtime.runtime_running, runtime.lifecycle_state);
}
