import { DirectConnectionsPanel } from "./DirectConnectionsPanel";
import { localizedAccountUrl } from "../../components/modals/settings/reachMembershipModel";
import { useEffect, useRef, useState } from "react";
import { classNames } from "../../utils/classNames";
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
  direct_routes?: string[];
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
        <div className="min-h-0 overflow-y-auto">
          <GroupConnectionsPanel
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
  onOpenAccount: () => void;
}) {
  const { t } = useTranslation("layout");
  const [mode, setMode] = useState<"account" | "direct">("account");
  const active = props.invitation ? "account" : mode;
  return (
    <div className="space-y-4">
      {!props.invitation && (
        <div
          className="inline-flex items-center rounded-lg bg-[var(--glass-tab-bg)] p-0.5"
          role="group"
          aria-label={t("groupConnections.title")}
        >
          {(["account", "direct"] as const).map((option) => (
            <button
              key={option}
              type="button"
              aria-pressed={active === option}
              className={classNames(
                "inline-flex h-8 items-center justify-center rounded-md px-3 text-sm transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-text-secondary)]",
                active === option
                  ? "bg-[var(--color-bg-primary)] font-semibold text-[var(--color-text-primary)] shadow-sm"
                  : "text-[var(--color-text-tertiary)] hover:text-[var(--color-text-secondary)]",
              )}
              onClick={() => setMode(option)}
            >
              {t(option === "account" ? "direct.account" : "direct.title")}
            </button>
          ))}
        </div>
      )}
      {active === "direct" ? (
        <DirectConnectionsPanel
          key={props.groupId}
          groupId={props.groupId}
          groupTitle={props.groupTitle}
        />
      ) : (
        <AccountGroupConnectionsPanel {...props} />
      )}
    </div>
  );
}

/** Shared Group-scoped content; only the invitation shell offers Group selection. */
function AccountGroupConnectionsPanel({
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
  const language = i18n.resolvedLanguage || i18n.language;
  const accountLink = (path: string) =>
    value?.account_origin
      ? localizedAccountUrl(new URL(`${value.account_origin}${path}`), language)
      : "";
  // One headline carries the state; everything else is secondary to it.
  const confirming = (!value && !error) || state === "syncing";
  const headline = confirming
    ? t("groupConnections.syncing")
    : state === "unavailable"
      ? t(
          value?.error_code === "connect_groups_unsupported"
            ? "groupConnections.unsupported"
            : "groupConnections.syncFailed",
        )
      : state === "ready"
        ? links.length > 0
          ? t("groupConnections.count", { count: links.length })
          : t("groupConnections.empty")
        : "";
  return (
    <div className="min-h-0 space-y-4 overflow-y-auto">
      {invitation && <p className="text-sm">{t("groupConnections.invitation")}</p>}
      {error && (
        <p role="alert" className="text-sm text-[var(--color-danger)]">
          {error}
        </p>
      )}

      {state !== "not_linked" && (
        <section className="rounded-xl border border-[var(--glass-border-subtle)]">
          <div className="flex items-start justify-between gap-3 px-4 py-3">
            <div
              className="min-w-0 space-y-1"
              role={state === "unavailable" ? "alert" : confirming ? "status" : undefined}
            >
              {headline && (
                <p
                  className={classNames(
                    "text-sm font-medium",
                    state === "unavailable"
                      ? "text-[var(--color-text-secondary)]"
                      : "text-[var(--color-text-primary)]",
                  )}
                >
                  {headline}
                </p>
              )}
              {state === "unavailable" && value?.error_message && (
                <p className="break-words text-xs text-[var(--color-text-tertiary)]">
                  {value.error_message}
                </p>
              )}
              {value?.checked_at && (
                <p className="text-xs text-[var(--color-text-tertiary)]">
                  {t("groupConnections.checkedAt", {
                    time: new Date(value.checked_at).toLocaleTimeString(),
                  })}
                </p>
              )}
            </div>
            <Button
              variant="ghost"
              size="sm"
              className="shrink-0"
              disabled={busy}
              onClick={() => setRefresh((n) => n + 1)}
            >
              {t("groupConnections.refresh")}
            </Button>
          </div>
          {links.length > 0 && (
            <ul className="divide-y divide-[var(--glass-border-subtle)] border-t border-[var(--glass-border-subtle)] px-4">
              {links.map((link) => {
                const peer =
                  link.source.account_id === value?.account_id ? link.target : link.source;
                return (
                  <li
                    key={link.id}
                    className="flex flex-wrap items-start justify-between gap-x-4 gap-y-1 py-3"
                  >
                    <div className="min-w-0 space-y-0.5">
                      <p className="break-words text-sm font-medium">
                        {peer.instance.display_name} · {peer.title}
                      </p>
                      <p className="text-xs text-[var(--color-text-secondary)]">
                        {t(
                          value?.direct_routes?.includes(link.id)
                            ? "direct.routeSelected"
                            : "groupConnections.connected",
                        )}
                      </p>
                      <code className="block break-all text-xs text-[var(--color-text-tertiary)]">
                        {peer.group_id}
                      </code>
                    </div>
                    {value?.account_origin && (
                      <a
                        className="shrink-0 text-xs text-[var(--color-text-secondary)] underline-offset-2 hover:text-[var(--color-danger)] hover:underline"
                        href={accountLink(`/connect/${link.id}/disconnect`)}
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
          )}
        </section>
      )}

      <div className="space-y-2">
        <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
          {state === "not_linked" ? (
            <Button onClick={onOpenAccount}>{t("groupConnections.linkAccount")}</Button>
          ) : (
            <Button
              disabled={busy || state !== "ready" || !value?.account_id || !current}
              onClick={() => void select()}
            >
              {t(invitation ? "groupConnections.accept" : "groupConnections.invite")}
            </Button>
          )}
          {state !== "not_linked" && value?.account_origin && (
            <a
              className="text-sm text-[var(--color-text-secondary)] underline-offset-2 hover:text-[var(--color-text-primary)] hover:underline"
              href={accountLink("/connect")}
              target="_blank"
              rel="noopener noreferrer"
            >
              {t("groupConnections.manage")}
            </a>
          )}
        </div>
        {prepared && (
          <p className="text-sm">
            <a className="underline" href={prepared} target="_blank" rel="noopener noreferrer">
              {t("groupConnections.continue")}
            </a>
          </p>
        )}
        <p className="text-xs text-[var(--color-text-tertiary)]">{t("groupConnections.sync")}</p>
      </div>
    </div>
  );
}
