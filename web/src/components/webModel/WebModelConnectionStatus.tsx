import { useTranslation } from "react-i18next";
import type { WebModelBrowserSession, WebModelPairing } from "../../services/api";
import { webModelConnectionState } from "./webModelConnectionState";

export function WebModelManagedConnection() {
  const { t } = useTranslation("settings");
  return (
    <section className="min-w-0 space-y-2 text-sm" aria-label={t("webModelActor.title")}>
      <h3 className="font-semibold">{t("webModelActor.title")}</h3>
      <p role="status" className="text-[var(--color-text-secondary)]">
        {t("webModelActor.adminManaged")}
      </p>
    </section>
  );
}

export function WebModelConnectionStatus({
  pairing,
  session,
}: {
  pairing?: WebModelPairing;
  session?: WebModelBrowserSession | null;
}) {
  const { t, i18n } = useTranslation("settings");
  const { displayState, failed, bound, connecting } = webModelConnectionState(pairing, session);
  return (
    <div
      className="min-w-0 space-y-1 text-sm"
      role="status"
      title={
        session?.checked_at
          ? t("webModelActor.checkedAt", {
              time: new Date(session.checked_at).toLocaleString(i18n.language),
            })
          : undefined
      }
    >
      <span className={failed ? "text-amber-700 dark:text-amber-300" : undefined}>
        {t(`webModelActor.states.${displayState}`, {
          defaultValue: t("webModelActor.states.loading"),
        })}
      </span>
      {failed && pairing?.error_code && (
        <p role="alert" className="break-words text-amber-700 dark:text-amber-300">
          {t(`webModelActor.errors.${pairing.error_code}`, {
            defaultValue: t("webModelActor.errors.pairing_failed"),
          })}
        </p>
      )}
      {bound && (failed || connecting) && (
        <p className="text-[var(--color-text-secondary)]">{t("webModelActor.bindingRetained")}</p>
      )}
    </div>
  );
}
