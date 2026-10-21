import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { apiJson } from "../../services/api/base";
import { localizedAccountUrl } from "../../components/modals/settings/reachMembershipModel";
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

export function useAccountGroupConnections(groupId: string, invitation: string) {
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
  return {
    value,
    error,
    busy,
    prepared,
    links,
    state,
    confirming,
    headline,
    accountLink,
    select,
    refresh: () => setRefresh((n) => n + 1),
  };
}
