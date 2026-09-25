import { useTranslation } from "react-i18next";
import type { WebModelBrowserSession, WebModelPairing } from "../../services/api";
type apiProvider = "chatgpt_web" | "grok_web";
import { webModelConnectionState } from "./webModelConnectionState";

export function WebModelConnectionStatus({
  pairing,
  session,
  provider = "chatgpt_web",
}: {
  provider?: apiProvider;
  pairing?: WebModelPairing;
  session?: WebModelBrowserSession | null;
}) {
  const { t } = useTranslation("settings");
  const { displayState, failed, bound, connecting } = webModelConnectionState(pairing, session);
  return (
    <div className="min-w-0 space-y-1 text-sm" role="status">
      <span className={failed ? "text-amber-700 dark:text-amber-300" : undefined}>
        {t(
          provider === "grok_web" &&
            [
              "bound",
              "bound_stopped",
              "browser_closed",
              "unpaired",
              "login_required",
              "verification_required",
            ].includes(displayState)
            ? `grokActor.${displayState}`
            : `webModelActor.states.${displayState}`,
          { defaultValue: t("webModelActor.states.loading") },
        )}
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
