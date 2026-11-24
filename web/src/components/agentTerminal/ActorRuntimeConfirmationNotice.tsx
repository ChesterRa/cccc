import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useStartupInputWaiting, startupInputGeneration } from "../../hooks/useStartupInputWaiting";
import * as api from "../../services/api";
import { useGroupStore, useUIStore } from "../../stores";
import type { Actor } from "../../types";
import { formatDaemonError } from "../../utils/daemonErrorText";
import { runtimeConfirmationKeys } from "../../utils/runtimeConfirmationKeys";
import { Button } from "../ui/button";

const NOTICE_CLASS =
  "shrink-0 border-b border-amber-500/30 bg-amber-500/10 px-4 py-3 text-xs leading-5 text-[var(--color-text-primary)]";

/** What to do while a prompt waits in this terminal, or after the terminal closed before CCCC
 *  could use it; the header already names the state. The terminal stays the user's own control
 *  surface: the startup release lets CCCC send what is waiting, so it is offered only for after
 *  the chat input box shows. A read-only viewer has no controls, so the paused and startup cases
 *  say nothing more. */
export function ActorRuntimeConfirmationNotice({
  actor,
  groupId,
  readOnly = false,
}: {
  actor: Actor;
  groupId: string;
  readOnly?: boolean;
}) {
  const { t } = useTranslation("actors");
  const startupWaiting = useStartupInputWaiting(groupId, actor);
  const [releasing, setReleasing] = useState(false);
  const showError = useUIStore((state) => state.showError);
  const refreshActors = useGroupStore((state) => state.refreshActors);
  if (actor.runtime_confirmation_required) {
    return (
      <p
        role="status"
        data-runtime-confirmation="pending"
        data-status-tone="warning"
        className={NOTICE_CLASS}
      >
        {t(runtimeConfirmationKeys(actor.runtime).hint)}
      </p>
    );
  }
  if (readOnly) return null;
  if (startupWaiting) {
    const release = async () => {
      const generation = startupInputGeneration(actor);
      if (!generation || releasing) return;
      setReleasing(true);
      try {
        const resp = await api.releaseActorStartupInput(groupId, actor.id, generation);
        if (!resp.ok) showError(formatDaemonError(resp.error));
        await refreshActors(groupId);
      } finally {
        setReleasing(false);
      }
    };
    return (
      <div
        role="status"
        data-runtime-confirmation="startup"
        data-status-tone="warning"
        className={`flex flex-wrap items-center gap-x-3 gap-y-2 ${NOTICE_CLASS}`}
      >
        <span className="min-w-0 flex-1 basis-64">{t("runtimeStartupInputHint")}</span>
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={releasing}
          title={t("runtimeStartupInputReleaseTitle")}
          onClick={() => void release()}
        >
          {t("runtimeStartupInputRelease")}
        </Button>
      </div>
    );
  }
  if (actor.runtime_confirmation_paused === true) {
    return (
      <p
        role="status"
        data-runtime-confirmation="paused"
        data-status-tone="warning"
        className={NOTICE_CLASS}
      >
        {t(runtimeConfirmationKeys(actor.runtime).pausedHint)}
      </p>
    );
  }
  return null;
}
