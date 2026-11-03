import { useTranslation } from "react-i18next";
import type { Actor } from "../../types";

/** What to do while Claude's prompt waits in this terminal; the header already names the state.
 *  The terminal stays the user's own control surface. */
export function ActorRuntimeConfirmationNotice({ actor }: { actor: Actor }) {
  const { t } = useTranslation("actors");
  if (!actor.runtime_confirmation_required) return null;
  return (
    <p
      role="status"
      data-runtime-confirmation
      data-status-tone="warning"
      className="shrink-0 border-b border-amber-500/30 bg-amber-500/10 px-4 py-3 text-xs leading-5 text-[var(--color-text-primary)]"
    >
      {t("runtimeConfirmationHint")}
    </p>
  );
}
