import { useTranslation } from "react-i18next";
import type { Actor } from "../../types";

/** What to do while Claude's prompt waits in this terminal, or after it closed before CCCC
 *  connected; the header already names the state. The terminal stays the user's own control
 *  surface. A read-only viewer has no Launch control, so the paused case says nothing more. */
export function ActorRuntimeConfirmationNotice({
  actor,
  readOnly = false,
}: {
  actor: Actor;
  readOnly?: boolean;
}) {
  const { t } = useTranslation("actors");
  const paused = !actor.runtime_confirmation_required && actor.runtime_confirmation_paused === true;
  if (!actor.runtime_confirmation_required && (!paused || readOnly)) return null;
  return (
    <p
      role="status"
      data-runtime-confirmation={paused ? "paused" : "pending"}
      data-status-tone="warning"
      className="shrink-0 border-b border-amber-500/30 bg-amber-500/10 px-4 py-3 text-xs leading-5 text-[var(--color-text-primary)]"
    >
      {t(paused ? "runtimeConfirmationPausedHint" : "runtimeConfirmationHint")}
    </p>
  );
}
