import { GroupConnectionsPanel } from "./GroupConnectionsPanel";
export { GroupConnectionsPanel } from "./GroupConnectionsPanel";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useModalStore } from "../../stores/useModalStore";
import { useChatObscuringView } from "../../hooks/useChatObscuringView";
import { SelectMenu } from "../../components/ui/select-menu";
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "../../components/ui/dialog";
import type { GroupMeta } from "../../types";

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
  useChatObscuringView(true);
  const returnFocus = useRef(document.activeElement as HTMLElement | null);
  const [chosen, setChosen] = useState("");
  const current = chosen || groupId || (invitation ? groups[0]?.group_id : "") || "";
  const currentTitle = groups.find((group) => group.group_id === current)?.title || current;
  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DialogContent
        className="w-[min(calc(100vw-2rem),60rem)] max-h-[calc(100dvh-2rem)] gap-4 p-4 max-sm:max-h-[calc(100dvh-1rem)] max-sm:w-[calc(100vw-1rem)] sm:p-6"
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          if (document.activeElement === document.body && returnFocus.current?.isConnected)
            returnFocus.current.focus();
        }}
      >
        <DialogTitle
          title={`${t("groupConnections.title")} · ${currentTitle}`}
          className="line-clamp-2 shrink-0 break-words pr-8 text-lg font-semibold [overflow-wrap:anywhere]"
        >
          {t("groupConnections.title")} · {currentTitle}
        </DialogTitle>
        <DialogDescription className="shrink-0 text-sm leading-relaxed">
          {t("groupConnections.description")}
        </DialogDescription>
        {/* One scrolling body keeps actions reachable even with long names on short screens. */}
        <div
          role="region"
          aria-label={t("groupConnections.title")}
          tabIndex={0}
          className="min-h-0 flex-1 overflow-y-auto"
        >
          <div className="flex flex-col gap-3">
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
            <div className="min-h-64 flex-1">
              <GroupConnectionsPanel
                key={current}
                groupId={current}
                groupTitle={currentTitle}
                invitation={invitation}
                onOpenAccount={() => {
                  onClose();
                  onOpenAccount();
                }}
              />
            </div>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
