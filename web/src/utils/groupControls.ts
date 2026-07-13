import { classNames } from "./classNames";
import type { GroupStatusKey } from "./groupStatus";

export type GroupControlKey = "launch" | "pause" | "stop";

const CONTROL_ACTIVE_STATUS: Record<GroupControlKey, GroupStatusKey> = {
  launch: "run",
  pause: "paused",
  stop: "stop",
};

const CONTROL_BUSY_MAP: Record<string, GroupControlKey> = {
  "group-start": "launch",
  "group-activate": "launch",
  "group-pause": "pause",
  "group-stop": "stop",
};

const CONTROL_ACTIVE_CLASS: Record<GroupControlKey, string> = {
  launch:
    "bg-emerald-700 text-white shadow-[0_10px_24px_rgba(4,120,87,0.28)] ring-1 ring-black/5 dark:ring-white/10",
  pause:
    "bg-amber-700 text-white shadow-[0_10px_24px_rgba(180,83,9,0.28)] ring-1 ring-black/5 dark:ring-white/10",
  stop: "bg-rose-700 text-white shadow-[0_10px_24px_rgba(190,24,93,0.28)] ring-1 ring-black/5 dark:ring-white/10",
};

const CONTROL_PENDING_CLASS: Record<GroupControlKey, string> = {
  launch: "ring-2 ring-emerald-300/70 dark:ring-emerald-300/35 animate-pulse",
  pause: "ring-2 ring-amber-300/75 dark:ring-amber-300/35 animate-pulse",
  stop: "ring-2 ring-rose-300/75 dark:ring-rose-300/35 animate-pulse",
};

export function getLaunchControlMode(
  status: GroupStatusKey | null | undefined,
): "start" | "activate" {
  return status === "paused" || status === "idle" ? "activate" : "start";
}

export function getGroupControlVisual(
  status: GroupStatusKey | null | undefined,
  control: GroupControlKey,
  busy: string,
): { active: boolean; pending: boolean; className: string } {
  const active = Boolean(
    status &&
    (CONTROL_ACTIVE_STATUS[control] === status || (control === "launch" && status === "idle")),
  );
  const pending = CONTROL_BUSY_MAP[busy] === control;

  if (active || pending) {
    return {
      active,
      pending,
      className: classNames(
        "border border-transparent hover:-translate-y-px active:translate-y-0",
        CONTROL_ACTIVE_CLASS[control],
        active && "cursor-default",
        pending && CONTROL_PENDING_CLASS[control],
      ),
    };
  }

  return {
    active: false,
    pending: false,
    className:
      "border border-transparent bg-transparent text-[var(--color-text-secondary)] hover:bg-black/5 hover:text-[var(--color-text-primary)] dark:hover:bg-white/6 active:translate-y-0",
  };
}

export function resolveGroupControls(args: {
  selectedGroupId: string;
  actorCount: number;
  statusKey: GroupStatusKey | null | undefined;
  busy: string;
}): {
  isGroupBusy: boolean;
  launchHardUnavailable: boolean;
  pauseHardUnavailable: boolean;
  stopHardUnavailable: boolean;
  launchDisabled: boolean;
  pauseDisabled: boolean;
  stopDisabled: boolean;
} {
  const selectedGroupId = String(args.selectedGroupId || "").trim();
  const actorCount = Number(args.actorCount || 0);
  const statusKey = args.statusKey ?? null;
  const busy = String(args.busy || "");

  const isGroupBusy = busy.startsWith("group-");
  const launchHardUnavailable = !selectedGroupId || actorCount === 0;
  const pauseHardUnavailable = !selectedGroupId || actorCount === 0 || statusKey === "stop";
  const stopHardUnavailable = !selectedGroupId;

  return {
    isGroupBusy,
    launchHardUnavailable,
    pauseHardUnavailable,
    stopHardUnavailable,
    launchDisabled: launchHardUnavailable || isGroupBusy,
    pauseDisabled: pauseHardUnavailable || isGroupBusy,
    stopDisabled: stopHardUnavailable || isGroupBusy,
  };
}
