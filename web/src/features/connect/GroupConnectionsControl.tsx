import { localizedAccountUrl } from "../../components/modals/settings/reachMembershipModel";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { useModalStore } from "../../stores/useModalStore";
import { apiJson } from "../../services/api/base";
import { Button } from "../../components/ui/button";
import { SelectMenu } from "../../components/ui/select-menu";
import { Dialog, DialogContent, DialogTitle, DialogDescription } from "../../components/ui/dialog";
import type { GroupMeta } from "../../types";
import type { ConnectInstance } from "./protocol";

type Endpoint = { account_id: string; instance: ConnectInstance; group_id: string; title: string };
type GroupLink = { id: string; source: Endpoint; target: Endpoint };
type Status = {
  status: "not_linked" | "syncing" | "ready" | "unavailable";
  error_code: string | null;
  error_message: string | null;
  checked_at: string | null;
  links: GroupLink[];
  expires_at: string | null;
  account_origin: string | null;
  account_id: string | null;
};

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
        className="gap-3 p-5"
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          if (document.activeElement === document.body && returnFocus.current?.isConnected)
            returnFocus.current.focus();
        }}
      >
        <DialogTitle className="break-words pr-8 text-lg font-semibold">
          {t("groupConnections.title")} ·{" "}
          {groups.find((group) => group.group_id === current)?.title || current}
        </DialogTitle>
        <DialogDescription>{t("groupConnections.description")}</DialogDescription>
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
        <GroupConnectionsPanel
          key={current}
          groupId={current}
          invitation={invitation}
          onOpenAccount={() => {
            onClose();
            onOpenAccount();
          }}
        />
      </DialogContent>
    </Dialog>
  );
}

/** Shared Group-scoped content; only the invitation shell offers Group selection. */
export function GroupConnectionsPanel({
  groupId,
  invitation = "",
  onOpenAccount,
}: {
  groupId: string;
  invitation?: string;
  onOpenAccount: () => void;
}) {
  const { t, i18n } = useTranslation("layout");
  const [status, setStatus] = useState<{ group: string; value: Status } | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [prepared, setPrepared] = useState("");
  const [refresh, setRefresh] = useState(0);
  const selection = useRef<AbortController | null>(null);
  const current = groupId;
  const value = status?.group === current ? status.value : null;
  useEffect(() => () => selection.current?.abort(), []);
  useEffect(() => {
    if (!value?.expires_at) return;
    const delay = Date.parse(value.expires_at) - Date.now();
    if (delay <= 0) return;
    const timer = setTimeout(() => setRefresh((n) => n + 1), delay + 1);
    return () => clearTimeout(timer);
  }, [value]);

  useEffect(() => {
    if (!current) return;
    const controller = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const poll = async () => {
      const result = await apiJson<Status>(
        `/api/v1/connect/groups?group_id=${encodeURIComponent(current)}`,
        { signal: controller.signal },
      );
      if (controller.signal.aborted) return;
      if (result.ok) {
        setStatus({ group: current, value: result.result });
        setError("");
      } else {
        setStatus(null);
        setError(result.error.message);
      }
      timer = setTimeout(() => void poll(), 15000);
    };
    void poll();
    return () => {
      controller.abort();
      clearTimeout(timer);
    };
  }, [current, refresh]);

  const select = async () => {
    selection.current?.abort();
    const controller = new AbortController();
    selection.current = controller;
    setBusy(true);
    setError("");
    setPrepared("");
    const tab = window.open("about:blank", "_blank");
    if (tab) tab.opener = null;
    const result = await apiJson<{ url: string }>("/api/v1/connect/groups", {
      method: "POST",
      body: JSON.stringify({ group_id: current, invitation }),
      signal: controller.signal,
    });
    if (controller.signal.aborted) {
      tab?.close();
      return;
    }
    setBusy(false);
    if (result.ok && new URL(result.result.url).origin === value?.account_origin) {
      const url = localizedAccountUrl(
        new URL(result.result.url),
        i18n.resolvedLanguage || i18n.language,
      );
      setPrepared(url);
      if (tab) tab.location.replace(url);
    } else {
      tab?.close();
      setError(result.ok ? t("groupConnections.unavailable") : result.error.message);
    }
  };
  const links = value?.expires_at && Date.parse(value.expires_at) > Date.now() ? value.links : [];
  const state =
    value?.status === "ready" && (!value.expires_at || Date.parse(value.expires_at) <= Date.now())
      ? "unavailable"
      : value?.status;
  return (
    <div className="min-h-0 space-y-4 overflow-y-auto">
      {invitation && <p className="text-sm">{t("groupConnections.invitation")}</p>}
      {error && (
        <p role="alert" className="text-sm text-[var(--color-danger)]">
          {error}
        </p>
      )}
      {state === "not_linked" ? (
        <Button
          onClick={() => {
            onOpenAccount();
          }}
        >
          {t("groupConnections.linkAccount")}
        </Button>
      ) : (
        <div className="flex flex-wrap gap-2">
          <Button
            disabled={busy || state !== "ready" || !value?.account_id || !current}
            onClick={() => void select()}
          >
            {t(invitation ? "groupConnections.accept" : "groupConnections.invite")}
          </Button>
          {value?.account_origin && (
            <Button variant="secondary" asChild>
              <a
                href={localizedAccountUrl(
                  new URL(`${value.account_origin}/connect`),
                  i18n.resolvedLanguage || i18n.language,
                )}
                target="_blank"
                rel="noopener noreferrer"
              >
                {t("groupConnections.manage")}
              </a>
            </Button>
          )}
          <Button variant="ghost" disabled={busy} onClick={() => setRefresh((n) => n + 1)}>
            {t("groupConnections.refresh")}
          </Button>
        </div>
      )}
      {prepared && (
        <p className="text-sm">
          <a className="underline" href={prepared} target="_blank" rel="noopener noreferrer">
            {t("groupConnections.continue")}
          </a>
        </p>
      )}
      <p className="text-sm text-[var(--color-text-secondary)]">{t("groupConnections.sync")}</p>
      {((!value && !error) || state === "syncing") && (
        <p role="status" className="text-sm text-[var(--color-text-secondary)]">
          {t("groupConnections.syncing")}
        </p>
      )}
      {state === "unavailable" && (
        <div role="alert" className="space-y-1 text-sm text-[var(--color-text-secondary)]">
          <p>
            {t(
              value?.error_code === "connect_groups_unsupported"
                ? "groupConnections.unsupported"
                : "groupConnections.syncFailed",
            )}
          </p>
          {value?.error_message && <p className="break-words text-xs">{value.error_message}</p>}
        </div>
      )}
      {value?.checked_at && (
        <p className="text-xs text-[var(--color-text-tertiary)]">
          {t("groupConnections.checkedAt", {
            time: new Date(value.checked_at).toLocaleTimeString(),
          })}
        </p>
      )}
      {state === "ready" && links.length === 0 && (
        <p className="text-sm text-[var(--color-text-secondary)]">{t("groupConnections.empty")}</p>
      )}
      <ul className="divide-y divide-[var(--glass-border-subtle)]">
        {links.map((link) => {
          const peer = link.source.account_id === value?.account_id ? link.target : link.source;
          return (
            <li key={link.id} className="space-y-1 py-3">
              <p className="font-medium">
                {peer.instance.display_name} · {peer.title}
              </p>
              <p className="text-xs text-[var(--color-text-secondary)]">
                {t("groupConnections.connected")}
              </p>
              <code className="block break-all text-xs text-[var(--color-text-tertiary)]">
                {peer.group_id}
              </code>
              {value?.account_origin && (
                <a
                  className="text-sm underline"
                  href={localizedAccountUrl(
                    new URL(`${value.account_origin}/connect/${link.id}/disconnect`),
                    i18n.resolvedLanguage || i18n.language,
                  )}
                  target="_blank"
                  rel="noopener noreferrer"
                >
                  {t("groupConnections.disconnect")}
                </a>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}
