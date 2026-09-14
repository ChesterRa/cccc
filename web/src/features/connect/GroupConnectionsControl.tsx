import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Link2 } from "lucide-react";
import { apiJson } from "../../services/api/base";
import { Button } from "../../components/ui/button";
import { IconButton } from "../../components/ui/icon-button";
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
  const { t } = useTranslation("layout");
  const [invitation, setInvitation] = useState(() => {
    const id = new URLSearchParams(window.location.search).get("connect_invite") || "";
    return /^[a-f0-9-]{36}$/.test(id) ? id : "";
  });
  const [open, setOpen] = useState(Boolean(invitation));
  const [chosen, setChosen] = useState("");
  const [status, setStatus] = useState<{ group: string; value: Status } | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [prepared, setPrepared] = useState("");
  const [refresh, setRefresh] = useState(0);
  const selection = useRef<AbortController | null>(null);
  const current = chosen || groupId || groups[0]?.group_id || "";
  const value = status?.group === current ? status.value : null;
  useEffect(() => () => selection.current?.abort(), [enabled]);
  useEffect(() => {
    if (!enabled || !open || !value?.expires_at) return;
    const delay = Date.parse(value.expires_at) - Date.now();
    if (delay <= 0) return;
    const timer = setTimeout(() => setRefresh((n) => n + 1), delay + 1);
    return () => clearTimeout(timer);
  }, [enabled, open, value]);

  useEffect(() => {
    if (!enabled || !open || !current) return;
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
  }, [current, enabled, open, refresh]);

  const close = (next: boolean) => {
    selection.current?.abort();
    setBusy(false);
    setChosen(next ? groupId : "");
    setOpen(next);
    setPrepared("");
    if (!next && invitation) {
      setInvitation("");
      const url = new URL(window.location.href);
      url.searchParams.delete("connect_invite");
      window.history.replaceState(window.history.state, "", url);
    }
  };
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
      setPrepared(result.result.url);
      if (tab) tab.location.replace(result.result.url);
    } else {
      tab?.close();
      setError(result.ok ? t("groupConnections.unavailable") : result.error.message);
    }
  };
  if (!enabled) return null;
  const links = value?.expires_at && Date.parse(value.expires_at) > Date.now() ? value.links : [];
  const state =
    value?.status === "ready" && (!value.expires_at || Date.parse(value.expires_at) <= Date.now())
      ? "unavailable"
      : value?.status;
  return (
    <>
      <IconButton
        type="button"
        variant="ghost"
        size="rail"
        label={t("groupConnections.title")}
        disabled={!groupId}
        onClick={() => close(true)}
      >
        <Link2 size={17} />
      </IconButton>
      <Dialog open={open} onOpenChange={close}>
        <DialogContent className="gap-3 p-5">
          <DialogTitle className="pr-8 text-lg font-semibold">
            {t("groupConnections.title")}
          </DialogTitle>
          <DialogDescription className="text-sm text-[var(--color-text-secondary)]">
            {t("groupConnections.description")}
          </DialogDescription>
          <div className="min-h-0 space-y-4 overflow-y-auto">
            <label className="block space-y-1 text-sm">
              {t("groupConnections.localGroup")}
              <select
                className="glass-input w-full rounded-lg p-2"
                value={current}
                disabled={busy}
                onChange={(event) => {
                  setChosen(event.target.value);
                  setPrepared("");
                }}
              >
                {groups.map((group) => (
                  <option key={group.group_id} value={group.group_id}>
                    {group.title || group.group_id}
                  </option>
                ))}
              </select>
            </label>
            {invitation && <p className="text-sm">{t("groupConnections.invitation")}</p>}
            {error && (
              <p role="alert" className="text-sm text-[var(--color-danger)]">
                {error}
              </p>
            )}
            {state === "not_linked" ? (
              <Button
                onClick={() => {
                  close(false);
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
                      href={`${value.account_origin}/connect`}
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
            <p className="text-sm text-[var(--color-text-secondary)]">
              {t("groupConnections.sync")}
            </p>
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
                {value?.error_message && (
                  <p className="break-words text-xs">{value.error_message}</p>
                )}
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
              <p className="text-sm text-[var(--color-text-secondary)]">
                {t("groupConnections.empty")}
              </p>
            )}
            <ul className="divide-y divide-[var(--glass-border-subtle)]">
              {links.map((link) => {
                const peer =
                  link.source.account_id === value?.account_id ? link.target : link.source;
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
                        href={`${value.account_origin}/connect/${link.id}/disconnect`}
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
        </DialogContent>
      </Dialog>
    </>
  );
}
