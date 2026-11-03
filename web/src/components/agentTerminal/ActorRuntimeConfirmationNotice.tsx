import { useTranslation } from "react-i18next";
import type { Actor } from "../../types";

/** This is status only; the original terminal remains the user's control surface. */
export function ActorRuntimeConfirmationNotice({ actor }: { actor: Actor }) {
  const { t } = useTranslation("actors");
  if (!actor.runtime_confirmation_required) return null;
  return (
    <div
      role="status"
      data-runtime-confirmation
      data-status-tone="warning"
      className="shrink-0 border-b border-amber-500/30 bg-amber-500/10 px-4 py-3 text-xs leading-5 text-[var(--color-text-primary)]"
    >
      <p className="font-medium">{t("runtimeConfirmationPending")}</p>
      <p>{t("runtimeConfirmationRequired", { label: actor.title || actor.id })}</p>
    </div>
  );
}
