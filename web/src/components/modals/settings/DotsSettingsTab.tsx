import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
import { formatMessageTimestamp } from "../../../utils/time";
import { connectorAddress } from "./connectorAddress";
import { SettingsCard, SettingsNotice, SettingsPage } from "./SettingsKit";
import { secondaryButtonClass } from "./types";

interface Props {
  isActive: boolean;
  onOpenChatGPT: () => void;
  onOpenWebAccess?: () => void;
}

type Snapshot = {
  base?: string;
  connector: api.WebModelConnector | null;
  actor: api.DotsActorState | null;
  /** The bound Actor's status could not be read this time. */
  actorUnreadable: boolean;
};

function reasonHint(reason: string | null): string {
  switch (reason) {
    case "pairing_expired":
    case "pairing_invalid":
    case "pairing_failed":
    case "pairing_not_received":
      return "pairingExpired";
    case "pairing_changed":
    case "pairing_already_used":
    case "binding_conflict":
    case "dots_binding_conflict":
    case "dots_binding_changed":
    case "dots_binding_unavailable":
    case "dots_pairing_changed":
      return "bindingChanged";
    case "conversation_not_paired":
    case "conversation_pairing_changed":
    case "session_missing":
    case "session_metadata_invalid":
      return "conversationUnpaired";
    case "dots_token_invalid":
    case "dots_execution_token_required":
      return "executionToken";
    case "dots_actor_stopped":
      return "actorStopped";
    case "rpc_-32601":
    case "rpc_-32602":
      return "refreshTools";
    default:
      return "callFailed";
  }
}

/**
 * Dots on the shared ChatGPT connector: what it needs, which Actor it serves,
 * and recent shared event/tool outcomes. It never creates or rotates the
 * connector; that stays on the ChatGPT view so only one exists.
 */
export default function DotsSettingsTab({ isActive, onOpenChatGPT, onOpenWebAccess }: Props) {
  const { t, i18n } = useTranslation("settings");
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState("");

  useEffect(() => {
    if (!isActive) return;
    let cancelled = false;
    // One read at a time, so a slow read can never land after a newer one.
    let loading = false;
    const load = async () => {
      if (loading) return;
      loading = true;
      try {
        const listed = await api.fetchWebModelConnectors();
        if (cancelled) return;
        if (!listed.ok) {
          setError(listed.error.message);
          return;
        }
        const connector =
          listed.result.connectors.find(
            (item) => !item.revoked && (item.provider || "chatgpt_web") === "chatgpt_web",
          ) || null;
        const bound = connector?.dots_actor ?? null;
        let actor: api.DotsActorState | null = null;
        let actorUnreadable = false;
        if (bound) {
          try {
            const state = await api.fetchDotsActorState(bound.group_id, bound.actor_id);
            if (
              state.ok &&
              state.result.group_id === bound.group_id &&
              state.result.actor_id === bound.actor_id
            )
              actor = state.result;
            else actorUnreadable = true;
          } catch {
            actorUnreadable = true;
          }
        }
        if (cancelled) return;
        setSnapshot((previous) => {
          const sameActor =
            previous?.actor?.group_id === bound?.group_id &&
            previous?.actor?.actor_id === bound?.actor_id;
          return {
            base: listed.result.connector_base_url,
            connector,
            // An unreadable status is unknown, not "not subscribed": keep the last
            // trusted snapshot of the same Actor instead.
            actor: actorUnreadable && sameActor ? (previous?.actor ?? null) : actor,
            actorUnreadable,
          };
        });
        setError("");
      } catch (e) {
        if (!cancelled) setError(e instanceof Error ? e.message : String(e));
      } finally {
        loading = false;
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 5_000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [isActive]);

  const connector = snapshot?.connector ?? null;
  const address = connectorAddress(snapshot?.base ?? connector?.connector_url ?? "");
  const actor = snapshot?.actor ?? null;
  const bound = connector?.dots_actor ?? null;
  const calls = [...(connector?.events_log ?? [])].reverse();
  const subscription = !actor
    ? t("dotsActor.unavailable")
    : actor.subscription_expires_at
      ? t(
          actor.subscription_expires_at <= Date.now()
            ? "dotsActor.subscriptionExpired"
            : "dotsActor.subscriptionExpires",
          { time: new Date(actor.subscription_expires_at).toLocaleString(i18n.language) },
        )
      : t(actor.subscribed ? "dotsActor.status.permanent" : "dotsActor.status.notSubscribed");
  const manage = (
    <button type="button" className={secondaryButtonClass("sm")} onClick={onOpenChatGPT}>
      {t("dotsSettings.manageConnector")}
    </button>
  );

  return (
    <SettingsPage title={t("dotsSettings.title")} description={t("dotsSettings.description")}>
      {error ? <SettingsNotice tone="error">{error}</SettingsNotice> : null}
      <SettingsCard title={t("dotsSettings.prereq")} description={t("dotsSettings.prereqHint")}>
        <div className="min-w-0 space-y-3 px-5 py-3.5">
          {!snapshot ? (
            <p className="text-sm text-[var(--color-text-muted)]">{t("common:loading")}</p>
          ) : address.kind === "https" ? (
            <p className="text-sm" data-dots-prereq="address">
              {t("dotsSettings.addressReady")}
            </p>
          ) : (
            <SettingsNotice
              tone="warning"
              action={
                onOpenWebAccess ? (
                  <button
                    type="button"
                    className={secondaryButtonClass("sm")}
                    onClick={onOpenWebAccess}
                  >
                    {t("dotsSettings.configureWebAccess")}
                  </button>
                ) : undefined
              }
            >
              {address.kind === "missing"
                ? t("webModelShared.publicUrlMissing")
                : t("webModelShared.publicUrlInvalid", { url: address.origin })}
            </SettingsNotice>
          )}
          {snapshot ? (
            connector ? (
              <div className="flex flex-wrap items-center justify-between gap-3">
                <p className="text-sm" data-dots-prereq="connector">
                  {t("dotsSettings.connectorReady")}
                </p>
                {manage}
              </div>
            ) : (
              <SettingsNotice tone="warning" action={manage}>
                {t("dotsSettings.connectorMissing")}
              </SettingsNotice>
            )
          ) : null}
        </div>
      </SettingsCard>
      <SettingsCard title={t("dotsSettings.boundActor")} description={t("dotsSettings.boundHint")}>
        <div className="min-w-0 px-5 py-3.5 text-sm">
          {bound && snapshot?.actorUnreadable ? (
            <div className="mb-2">
              <SettingsNotice tone="warning">{t("dotsSettings.actorReadFailed")}</SettingsNotice>
            </div>
          ) : null}
          {bound ? (
            <div data-dots-actor={bound.actor_id}>
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-semibold">{bound.actor_id}</span>
                {actor ? (
                  <span className="rounded-full bg-[var(--glass-tab-bg)] px-2 py-0.5 text-xs">
                    {t(`dotsActor.phases.${actor.phase}`)}
                  </span>
                ) : null}
              </div>
              <p className="mt-1 text-xs text-[var(--color-text-muted)]">
                {t("dotsSettings.actorGroup", { group: bound.group_id })} · {subscription}
                {actor?.last_execution
                  ? ` · ${t(`dotsActor.lastExecution.${actor.last_execution.status}`)}`
                  : ""}
              </p>
            </div>
          ) : (
            <p className="text-[var(--color-text-muted)]">{t("dotsSettings.noActor")}</p>
          )}
        </div>
      </SettingsCard>
      <SettingsCard
        title={t("dotsSettings.diagnostics")}
        description={t("dotsSettings.diagnosticsHint")}
      >
        {calls.length ? (
          <ul className="divide-y divide-[var(--glass-border-subtle)]">
            {calls.map((call, index) => (
              <li
                key={`${call.at}-${index}`}
                data-dots-call={call.ok ? "ok" : "failed"}
                className="flex flex-wrap items-center justify-between gap-2 px-5 py-2.5 text-sm"
              >
                <span className="break-all font-mono text-xs">{call.tool_name || call.method}</span>
                <span
                  className={
                    call.ok
                      ? "text-xs text-emerald-700 dark:text-emerald-300"
                      : "break-words text-xs text-rose-700 dark:text-rose-300"
                  }
                >
                  {call.ok
                    ? t("dotsSettings.callOk")
                    : t("dotsSettings.callFailed", { reason: call.reason || "-" })}
                </span>
                <span className="w-full text-xs text-[var(--color-text-muted)] sm:w-auto">
                  {formatMessageTimestamp(call.at)}
                </span>
                {!call.ok && call.tool_name ? (
                  <p className="w-full text-xs text-[var(--color-text-secondary)]">
                    {t(`dotsSettings.reasons.${reasonHint(call.reason)}`)}
                  </p>
                ) : null}
              </li>
            ))}
          </ul>
        ) : (
          <p className="px-5 py-3.5 text-sm text-[var(--color-text-muted)]">
            {t("dotsSettings.noCalls")}
          </p>
        )}
      </SettingsCard>
    </SettingsPage>
  );
}
