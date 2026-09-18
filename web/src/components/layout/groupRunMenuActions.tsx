import type { TFunction } from "i18next";
import { getGroupMenuControls, type GroupControl } from "../../utils/groupControls";
import type { GroupStatusKey } from "../../utils/groupStatus";
import { PauseIcon, PlayIcon, StopIcon } from "../Icons";
import type { GroupMenuActionItem } from "./useGroupMenu";

/**
 * Run actions for one Group: a launch/resume/pause toggle plus stop unless
 * already stopped. Shared by the sidebar row menu and the header status button
 * so both surfaces offer exactly the same choices.
 */
export function groupRunMenuActions(
  statusKey: GroupStatusKey | null | undefined,
  t: TFunction<"layout">,
  onControl: (control: GroupControl) => void,
  disabled = false,
): GroupMenuActionItem[] {
  return getGroupMenuControls(statusKey).map(({ control, labelKey }) => ({
    label: t(labelKey),
    disabled,
    icon:
      control === "pause" ? (
        <PauseIcon size={15} />
      ) : control === "stop" ? (
        <StopIcon size={15} />
      ) : (
        <PlayIcon size={15} />
      ),
    tone: control === "stop" ? "danger" : undefined,
    section: "run",
    onClick: () => onControl(control),
  }));
}
