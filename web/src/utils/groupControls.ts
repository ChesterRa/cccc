import type { GroupStatusKey } from "./groupStatus";

export type GroupControl = "launch" | "activate" | "pause" | "stop";

export type GroupMenuControl = {
  control: GroupControl;
  labelKey: "launchAllAgents" | "resumeDelivery" | "pauseDelivery" | "stopAllAgents";
};

/**
 * Lifecycle actions shared by the Group header and sidebar menus. Only a
 * stopped Group hides Stop: an Offline Group is still active or idle, so it
 * offers Start for its Actors and Stop for its recorded lifecycle.
 */
export function getGroupMenuControls(
  status: GroupStatusKey | null | undefined,
): GroupMenuControl[] {
  const toggle: GroupMenuControl =
    status === "run"
      ? { control: "pause", labelKey: "pauseDelivery" }
      : status === "paused" || status === "idle"
        ? { control: "activate", labelKey: "resumeDelivery" }
        : { control: "launch", labelKey: "launchAllAgents" };
  return status === "stop"
    ? [toggle]
    : [
        toggle,
        ...(status === "idle" ? [{ control: "pause", labelKey: "pauseDelivery" } as const] : []),
        { control: "stop", labelKey: "stopAllAgents" },
      ];
}
