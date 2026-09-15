import { GroupConnectionBadge } from "./GroupConnectionBadge";
import { useTranslation } from "react-i18next";
import { Monitor, LockKeyhole, ChevronRight } from "lucide-react";
import { instanceListing } from "./useConnectWorkbench";
import type { ConnectWorkbench } from "./useConnectWorkbench";
import { GroupItemMenuTrigger } from "../../components/layout/GroupItemMenuTrigger";
import { useGroupMenu } from "../../components/layout/useGroupMenu";
import type { RemoteGroup } from "./protocol";
import { instanceName } from "./instanceName";

export function ConnectSidebar({
  workbench,
  collapsed,
  onSelected,
}: {
  workbench: ConnectWorkbench;
  collapsed: boolean;
  onSelected: () => void;
}) {
  const { t } = useTranslation("layout");
  if (!workbench.instances.length) return null;
  return (
    <div className="mt-4 border-t border-[var(--glass-border-subtle)] pt-3">
      {!collapsed ? (
        <div className="px-2 pb-2 text-[11px] font-semibold text-[var(--color-text-tertiary)]">
          CCCC Connect
        </div>
      ) : null}
      {!collapsed && (workbench.failed || workbench.available === false) ? (
        <p role="status" className="px-2 pb-2 text-xs leading-5 text-[var(--color-text-muted)]">
          {t(
            workbench.available === false ? "connect.confirmationExpired" : "connect.refreshFailed",
          )}
        </p>
      ) : null}
      {workbench.instances.map((instance) => {
        const label = instanceName(
          instance,
          workbench.ownInstance
            ? [...workbench.instances, workbench.ownInstance]
            : workbench.instances,
        );
        const listing = instanceListing(workbench, instance);
        const active = workbench.selected?.instanceId === instance.instance_id;
        const expanded = !workbench.collapsedInstances.includes(instance.instance_id);
        const status = !instance.public_origin
          ? t("connect.noRemoteAccess")
          : listing
            ? t(active ? "connect.adminView" : "connect.savedGroups")
            : t("connect.signIn");
        const choose = (groupId = "") => {
          workbench.select(instance.instance_id, groupId);
          onSelected();
        };
        return (
          <section key={instance.instance_id} className="mb-2">
            <div className="flex items-center">
              <button
                type="button"
                disabled={workbench.available === false}
                onClick={() => choose()}
                title={[instance.display_name, status, instance.public_origin]
                  .filter(Boolean)
                  .join(" · ")}
                aria-label={`${label} · ${status}`}
                aria-current={active && !listing ? "page" : undefined}
                className={`flex min-h-10 min-w-0 flex-1 items-center gap-2 rounded-lg px-2 text-left text-sm hover:bg-[var(--glass-panel-bg)] ${active ? "text-[var(--color-text-primary)]" : "text-[var(--color-text-secondary)]"}`}
              >
                <Monitor size={16} className="shrink-0" />
                {!collapsed ? (
                  <>
                    <span className="min-w-0 flex-1 truncate">{label}</span>
                    {!listing ? <LockKeyhole size={13} className="shrink-0" /> : null}
                  </>
                ) : null}
              </button>
              {!collapsed && listing ? (
                <button
                  type="button"
                  aria-label={t(expanded ? "connect.collapseGroups" : "connect.expandGroups", {
                    name: label,
                  })}
                  aria-expanded={expanded}
                  onClick={() => workbench.toggleExpanded(instance.instance_id)}
                  className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg text-[var(--color-text-secondary)] hover:bg-[var(--glass-panel-bg)]"
                >
                  <ChevronRight size={15} className={expanded ? "rotate-90" : ""} />
                </button>
              ) : null}
            </div>
            {!collapsed ? (
              <>
                {!instance.public_origin ? (
                  <div className="px-8 pb-1 text-xs text-[var(--color-text-tertiary)]">
                    {status}
                  </div>
                ) : null}
                {expanded &&
                  listing?.groups.map((group) => (
                    <ConnectGroupItem
                      key={group.group_id}
                      group={group}
                      disabled={workbench.available === false}
                      live={active}
                      selected={active && workbench.selected?.groupId === group.group_id}
                      onSelect={() => choose(group.group_id)}
                      onOpenConnections={() => {
                        workbench.select(instance.instance_id, group.group_id, "connections");
                        onSelected();
                      }}
                    />
                  ))}
                {expanded && listing && !listing.groups.length ? (
                  <p className="px-3 text-xs text-[var(--color-text-tertiary)]">
                    {t("noGroupsYet")}
                  </p>
                ) : null}
              </>
            ) : null}
          </section>
        );
      })}
    </div>
  );
}

function ConnectGroupItem({
  group,
  disabled,
  live,
  selected,
  onSelect,
  onOpenConnections,
}: {
  group: RemoteGroup;
  disabled?: boolean;
  live: boolean;
  selected: boolean;
  onSelect: () => void;
  onOpenConnections: () => void;
}) {
  const { t } = useTranslation("layout");
  const label = `${t("groupActions")} · ${group.title || group.group_id}`;
  const menu = useGroupMenu(
    label,
    disabled ? [] : [{ label: t("groupConnections.title"), onClick: onOpenConnections }],
  );
  return (
    <div className="group/item relative flex items-center pr-1">
      <button
        type="button"
        disabled={disabled}
        onClick={onSelect}
        onContextMenu={menu.onContextMenu}
        onKeyDown={menu.onKeyDown}
        aria-current={selected ? "page" : undefined}
        className={`flex min-h-9 min-w-0 flex-1 items-center gap-2 rounded-lg pl-8 pr-2 text-left text-sm hover:bg-[var(--glass-panel-bg)] ${selected ? "bg-[var(--glass-panel-bg)] font-medium" : "text-[var(--color-text-secondary)]"}`}
      >
        <span
          className={`h-1.5 w-1.5 shrink-0 rounded-full ${live && group.running ? "bg-emerald-400" : "bg-[var(--color-text-tertiary)]"}`}
        />
        <span className="truncate">{group.title || group.group_id}</span>
      </button>
      <GroupConnectionBadge
        connection={group.connection}
        onClick={onOpenConnections}
        disabled={disabled}
      />
      {menu.available && (
        <GroupItemMenuTrigger
          isActive={selected}
          label={label}
          open={menu.open}
          onToggle={menu.toggle}
        />
      )}
      {menu.menu}
    </div>
  );
}
