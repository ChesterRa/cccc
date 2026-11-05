import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { completeActorRuntimeSwitch } from "../../services/api/actors";
import { useGroupStore, useUIStore } from "../../stores";
import type { Actor } from "../../types";
import { joinRuntimeSentences, runtimeSwitchStartError } from "./runtimeSwitchOutcome";
import { hasPendingActorSwitch, refreshRuntimeSwitchGroup } from "./runtimeSwitchState";

export function RuntimeSwitchPendingNotice({ actor, groupId }: { actor: Actor; groupId: string }) {
  const { t } = useTranslation("actors");
  const pending = useGroupStore((state) => hasPendingActorSwitch(state.groupDoc, actor, groupId));
  const canRetry = useUIStore((state) => state.isWebAdmin === true && !state.webReadOnly);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  if (!pending) return null;

  const retry = async () => {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      const response = await completeActorRuntimeSwitch(groupId, actor.id);
      if (!response.ok) {
        setError(
          response.error.code === "actor_switch_unknown"
            ? t("runtimeSwitch.unknown")
            : response.error.message,
        );
      } else {
        const notices: string[] = [];
        if (response.result.start === "error")
          notices.push(runtimeSwitchStartError(response.result.start_error, (key) => t(key)));
        if (response.result.start === "pending") notices.push(t("runtimeSwitch.startPending"));
        if (response.result.start === "stopped") notices.push(t("runtimeSwitch.stopped"));
        if (response.result.access_paused) notices.push(t("runtimeSwitch.accessPaused"));
        if (notices.length)
          useUIStore.getState().showNotice({ message: joinRuntimeSentences(notices) });
      }
      await refreshRuntimeSwitchGroup(groupId);
    } catch {
      setError(t("runtimeSwitch.refreshFailed"));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="flex flex-wrap items-center gap-2 border-b border-amber-500/20 bg-amber-500/5 px-4 py-2 text-xs text-amber-700 dark:text-amber-300"
      role="status"
      data-status-tone="warning"
    >
      <span>{t("runtimeSwitch.pending")}</span>
      {canRetry ? (
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={busy}
          onClick={() => void retry()}
        >
          {t(busy ? "runtimeSwitch.retrying" : "runtimeSwitch.retry")}
        </Button>
      ) : null}
      {error ? <span role="alert">{error}</span> : null}
    </div>
  );
}
