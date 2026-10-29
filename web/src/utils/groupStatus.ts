import { getGroupPresenceDotClass } from "./statusIndicators";
import type { GroupDoc, GroupMeta, GroupRuntimeStatus } from "../types";

export type GroupStatusKey = "run" | "paused" | "idle" | "offline" | "stop";

export const GROUP_STATUS_DOT_BASE_CLASS = "size-[8px] shrink-0 rounded-full";

/** Layout-namespace label of each status, shared by every surface that names it. */
export const GROUP_STATUS_LABEL_KEYS: Record<GroupStatusKey, string> = {
  run: "statusRunning",
  paused: "statusPaused",
  idle: "statusIdle",
  offline: "statusOffline",
  stop: "statusStopped",
};

export type GroupStatus = { key: GroupStatusKey; dotClass: string };

type GroupStatusSource =
  | Pick<GroupMeta, "running" | "state" | "runtime_status">
  | Pick<GroupDoc, "running" | "state" | "runtime_status">;

/**
 * The terminal interface uses the same words. Paused and Stopped name the
 * recorded lifecycle whether or not processes are alive, so the controls take
 * the matching resume or launch path. A Group that was not stopped but has no
 * running Actor is Offline, which keeps a deliberate stop distinguishable from
 * a Group whose Actors have not started or have exited.
 */
export function getGroupStatus(running: boolean, state?: string): GroupStatus {
  if (state === "paused") return { key: "paused", dotClass: getGroupPresenceDotClass("paused") };
  if (state === "stopped") return { key: "stop", dotClass: getGroupPresenceDotClass("stop") };
  // Nothing runs in either case, so Offline shares the stopped dot and differs in its word.
  if (!running) return { key: "offline", dotClass: getGroupPresenceDotClass("stop") };
  if (state === "idle") return { key: "idle", dotClass: getGroupPresenceDotClass("idle") };
  return { key: "run", dotClass: getGroupPresenceDotClass("run") };
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
  return getGroupStatus(runtime.runtime_running, runtime.lifecycle_state);
}
