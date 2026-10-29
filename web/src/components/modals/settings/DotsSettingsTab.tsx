import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../../services/api";
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

/**
 * Dots on the shared ChatGPT connector: prerequisites and its bound Actor.
 * Shared setup and diagnostics stay in ChatGPT; this page never creates or
 * rotates a connector.
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
    </SettingsPage>
  );
}
