import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../components/ui/button";
import { completeActorRuntimeSwitch } from "../../services/api/actors";
import { useGroupStore, useUIStore } from "../../stores";
import { isWebModelRuntime, type Actor } from "../../types";
import { actorGenerationIdentity } from "../../utils/actorIdentity";
import { joinRuntimeSentences, runtimeSwitchStartError } from "./runtimeSwitchOutcome";
import {
  hasPendingActorSwitch,
  openWebSetup,
  refreshRuntimeSwitchGroup,
} from "./runtimeSwitchState";

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
      const notices: string[] = [];
      if (!response.ok) {
        setError(
          response.error.code === "actor_switch_unknown"
            ? t("runtimeSwitch.unknown")
            : response.error.message,
        );
      } else {
        if (response.result.start === "error")
          notices.push(runtimeSwitchStartError(response.result.start_error, (key) => t(key)));
        if (response.result.start === "pending") notices.push(t("runtimeSwitch.startPending"));
        if (response.result.start === "stopped") notices.push(t("runtimeSwitch.stopped"));
        if (response.result.access_paused) notices.push(t("runtimeSwitch.accessPaused"));
      }
      await refreshRuntimeSwitchGroup(groupId);
      if (response.ok && response.result.completion === "done") {
        const needsWebSetup =
          response.result.switched &&
          isWebModelRuntime(response.result.to.runtime) &&
          response.result.from.runtime !== response.result.to.runtime;
        if (needsWebSetup) notices.push(t("runtimeSwitch.webConnectionRequired"));
        if (notices.length) {
          const expectedRuntime = response.result.switched ? response.result.to.runtime : "";
          const generation = actorGenerationIdentity(actor);
          useUIStore.getState().showNotice({
            message: joinRuntimeSentences(notices),
            ...(needsWebSetup
              ? {
                  actionLabel: t("runtimeSwitch.openWebSetup"),
                  onAction: () => {
                    const result = openWebSetup(groupId, actor.id, expectedRuntime, generation);
                    if (result !== "opened")
                      useUIStore
                        .getState()
                        .showError(
                          t(
                            result === "editor_open"
                              ? "runtimeSwitch.closeEditorFirst"
                              : "runtimeSwitch.refreshFailed",
                          ),
                        );
                  },
                }
              : {}),
          });
        }
      }
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
