import { DirectConnectionsPanel } from "./DirectConnectionsPanel";
import { useId, useRef, useState } from "react";
import { classNames } from "../../utils/classNames";
import { useTranslation } from "react-i18next";
import { useModalStore } from "../../stores/useModalStore";
import { Button } from "../../components/ui/button";
import { SelectMenu } from "../../components/ui/select-menu";
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "../../components/ui/dialog";
import type { GroupMeta } from "../../types";
import { AccountGroupConnectionsPanel } from "./AccountGroupConnectionsPanel";
import { Cable, Globe2 } from "lucide-react";

export function GroupConnectionsControl({
  enabled,
  groupId,
  groups,
  onOpenAccount,
}: {
  enabled: boolean;
  groupId: string;
  groups: GroupMeta[];
  onOpenAccount: () => void;
}) {
  const requestedGroup = useModalStore((state) => state.groupConnectionsId);
  const setGroupConnections = useModalStore((state) => state.setGroupConnections);
  const [invitation, setInvitation] = useState(() => {
    const id = new URLSearchParams(window.location.search).get("connect_invite") || "";
    return /^[a-f0-9-]{36}$/.test(id) ? id : "";
  });
  const close = () => {
    setGroupConnections(null);
    setInvitation("");
    if (invitation) {
      const url = new URL(window.location.href);
      url.searchParams.delete("connect_invite");
      window.history.replaceState(window.history.state, "", url);
    }
  };
  if (!enabled || (!requestedGroup && !invitation)) return null;
  return (
    <GroupConnectionsDialog
      key={requestedGroup || "invitation"}
      groupId={requestedGroup || groupId}
      groups={groups}
      invitation={invitation}
      onClose={close}
      onOpenAccount={onOpenAccount}
    />
  );
}

function GroupConnectionsDialog({
  groupId,
  groups,
  invitation,
  onClose,
  onOpenAccount,
}: {
  groupId: string;
  groups: GroupMeta[];
  invitation: string;
  onClose: () => void;
  onOpenAccount: () => void;
}) {
  const { t } = useTranslation("layout");
  const returnFocus = useRef(document.activeElement as HTMLElement | null);
  const [chosen, setChosen] = useState("");
  const current = chosen || groupId || (invitation ? groups[0]?.group_id : "") || "";
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent
        className="h-[min(36rem,calc(100dvh-2rem))] gap-3 p-5"
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          if (document.activeElement === document.body && returnFocus.current?.isConnected)
            returnFocus.current.focus();
        }}
      >
        <DialogTitle className="shrink-0 break-words pr-8 text-lg font-semibold">
          {t("groupConnections.title")} ·{" "}
          {groups.find((group) => group.group_id === current)?.title || current}
        </DialogTitle>
        <DialogDescription className="shrink-0 text-xs leading-relaxed">
          {t("groupConnections.description")}
        </DialogDescription>
        {invitation && (
          <SelectMenu
            value={current}
            options={groups.map((group) => ({
              value: group.group_id,
              label: group.title || group.group_id,
            }))}
            onChange={setChosen}
            ariaLabel={t("groupConnections.localGroup")}
            align="start"
            className="w-full"
            contentClassName="z-[1002] w-[var(--radix-popover-trigger-width)]"
            triggerProps={{ "data-connect-group-select": "true" }}
          />
        )}
        <div className="min-h-0 flex-1 overflow-hidden">
          <GroupConnectionsPanel
            fill
            key={current}
            groupId={current}
            groupTitle={groups.find((group) => group.group_id === current)?.title || current}
            invitation={invitation}
            onOpenAccount={() => {
              onClose();
              onOpenAccount();
            }}
          />
        </div>
      </DialogContent>
    </Dialog>
  );
}

export function GroupConnectionsPanel(props: {
  groupId: string;
  groupTitle?: string;
  invitation?: string;
  fill?: boolean;
  onOpenAccount: () => void;
}) {
  const { t } = useTranslation("layout");
  const modeId = useId();
  const [mode, setMode] = useState<"account" | "direct">("account");
  const active = props.invitation ? "account" : mode;
  return (
    <div className={props.fill ? "flex h-full min-h-0 flex-col gap-3" : "space-y-4"}>
      {!props.invitation && (
        <div
          className="grid shrink-0 grid-cols-2 gap-1 rounded-xl bg-[var(--glass-tab-bg)] p-1"
          role="group"
          aria-label={t("groupConnections.title")}
        >
          {(["account", "direct"] as const).map((option) => (
            <Button
              variant="ghost"
              key={option}
              type="button"
              aria-pressed={active === option}
              aria-label={t(option === "account" ? "direct.account" : "direct.title")}
              aria-describedby={`${modeId}-${option}`}
              className={classNames(
                "h-auto min-h-11 min-w-0 justify-start gap-2 rounded-lg px-3 py-2.5 text-left text-sm whitespace-normal duration-200 motion-reduce:!transition-none",
                active === option
                  ? "bg-[var(--color-bg-primary)] font-semibold text-[var(--color-text-primary)] shadow-sm"
                  : "text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]",
              )}
              onClick={() => setMode(option)}
            >
              {option === "account" ? (
                <Globe2 aria-hidden="true" className="h-4 w-4 shrink-0" />
              ) : (
                <Cable aria-hidden="true" className="h-4 w-4 shrink-0" />
              )}
              <span className="min-w-0 space-y-1">
                <span className="block">
                  {t(option === "account" ? "direct.account" : "direct.title")}
                </span>
                <span
                  id={`${modeId}-${option}`}
                  className="block text-xs font-normal leading-4 text-[var(--color-text-secondary)]"
                >
                  {t(
                    option === "account"
                      ? "groupConnections.accountRequirement"
                      : "groupConnections.directRequirement",
                  )}
                </span>
              </span>
            </Button>
          ))}
        </div>
      )}
      <div
        key={active}
        role="region"
        aria-label={t(active === "account" ? "direct.account" : "direct.title")}
        className={classNames(
          "motion-safe:animate-fade-in motion-reduce:animate-none",
          props.fill ? "scrollbar-hide min-h-0 flex-1 overflow-y-auto" : "",
        )}
      >
        {active === "direct" ? (
          <DirectConnectionsPanel
            fill={props.fill}
            key={props.groupId}
            groupId={props.groupId}
            groupTitle={props.groupTitle}
          />
        ) : (
          <AccountGroupConnectionsPanel {...props} />
        )}
      </div>
    </div>
  );
}
