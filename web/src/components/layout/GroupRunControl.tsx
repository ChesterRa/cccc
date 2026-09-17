import { LoaderCircle } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { GroupMeta } from "../../types";
import { groupRunActions, type GroupRunControls } from "../../utils/groupControls";
import { getGroupStatusFromSource } from "../../utils/groupStatus";
import { PlayIcon, PauseIcon, StopIcon } from "../Icons";
import { GroupStatusIndicator } from "./GroupStatusIndicator";
import { useGroupMenu } from "./useGroupMenu";
import "./GroupRunControl.css";

const actionIcons = { start: PlayIcon, resume: PlayIcon, pause: PauseIcon, stop: StopIcon };

/** A stable status button: open explicit actions instead of cycling lifecycle states. */
export function GroupRunControl({
  group,
  controls,
  compact = false,
  actorCount,
}: {
  group: GroupMeta;
  controls?: GroupRunControls;
  compact?: boolean;
  actorCount?: number;
}) {
  const { t } = useTranslation("layout");
  const status = getGroupStatusFromSource(group);
  const groupId = group.group_id;
  const title = group.title || groupId;
  const pending = controls?.pending?.groupId === groupId ? controls.pending.action : null;
  const stateLabel = t(
    { run: "statusRunning", paused: "statusPaused", idle: "statusIdle", stop: "statusStopped" }[
      status.key
    ],
  );
  const label = t("groupRun.control", {
    group: title,
    state: pending ? t(`groupRun.${pending}Pending`) : stateLabel,
  });
  const menu = useGroupMenu(
    label,
    controls
      ? groupRunActions(status.key).map((action) => {
          const Icon = actionIcons[action];
          return {
            label: t(`groupRun.${action}`),
            icon: <Icon size={16} />,
            description: t(`groupRun.${action}Hint`),
            destructive: action === "stop",
            disabled: !!controls.pending || (actorCount === 0 && action !== "stop"),
            onClick: () => {
              void controls.run(groupId, action);
            },
          };
        })
      : [],
    <div className="border-b border-[var(--glass-border-subtle)] px-3 py-2">
      <div className="truncate text-sm font-semibold" title={title}>
        {title}
      </div>
      <div className="mt-0.5 text-xs text-[var(--color-text-secondary)]">
        {actorCount === 0 ? t("groupRun.noActors") : stateLabel}
      </div>
    </div>,
  );
  if (!controls)
    return <GroupStatusIndicator status={status} variant={compact ? "dot" : "badge"} />;
  return (
    <>
      <button
        type="button"
        title={label}
        aria-label={label}
        aria-haspopup="menu"
        aria-expanded={menu.open}
        aria-busy={!!pending}
        data-group-run-control={groupId}
        className={`inline-flex shrink-0 items-center justify-center gap-1.5 text-xs font-medium text-[var(--color-text-secondary)] ${compact ? "group-run-control-compact h-8 w-8 pointer-coarse:h-10 pointer-coarse:w-10" : "min-h-8 min-w-8 rounded-lg border border-[var(--glass-border-subtle)] bg-[var(--color-bg-secondary)] px-2 hover:bg-[var(--glass-tab-bg-hover)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 pointer-coarse:min-h-10 pointer-coarse:min-w-10"}`}
        onPointerDown={(event) => event.stopPropagation()}
        onMouseDown={(event) => event.stopPropagation()}
        onTouchStart={(event) => event.stopPropagation()}
        onClick={(event) => {
          event.stopPropagation();
          menu.toggle(event.currentTarget);
        }}
        onKeyDown={(event) => {
          // Keep Enter/Space and arrows out of the sortable row's keyboard handler.
          event.stopPropagation();
          if ((event.key === "ArrowDown" || event.key === "ArrowUp") && !menu.open) {
            event.preventDefault();
            menu.toggle(event.currentTarget);
          }
        }}
      >
        {pending ? (
          <LoaderCircle className="h-3.5 w-3.5 animate-spin" aria-hidden="true" />
        ) : (
          <GroupStatusIndicator status={status} />
        )}
        {!compact && (
          <span className="@max-[479px]/group-header:hidden">
            {pending ? t(`groupRun.${pending}Pending`) : stateLabel}
          </span>
        )}
      </button>
      {menu.menu}
    </>
  );
}
