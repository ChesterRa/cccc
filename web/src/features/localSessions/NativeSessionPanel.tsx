import { useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import type { Actor, LocalSessionRecovery } from "../../types";
import {
  fetchLocalSessionRecovery,
  refreshLocalSessionRecovery,
} from "../../services/api/localSessions";
import { Button } from "../../components/ui/button";
import { useGroupStore } from "../../stores/useGroupStore";
import { copyTextToClipboard } from "../../utils/copy";

export function NativeSessionPanel({
  actor,
  groupId,
  statusText,
  isVisible,
  canAccessRecovery,
}: {
  actor: Actor;
  groupId: string;
  statusText: string;
  isVisible: boolean;
  canAccessRecovery: boolean;
}) {
  const { t } = useTranslation("actors");
  return (
    <div className="flex h-full min-h-0 min-w-0 flex-1 flex-col gap-3 overflow-y-auto p-4 text-sm text-[var(--color-text-secondary)]">
      <p role="status" className="font-medium text-[var(--color-text-primary)]">
        {statusText}
      </p>
      <p>{t("nativeSession.hint")}</p>
      {isVisible && canAccessRecovery && groupId && actor.external_session_id ? (
        <RecoveryGuidance
          // Private data lives only for this visible, authorized binding.
          key={JSON.stringify([
            groupId,
            actor.id,
            actor.generation,
            actor.external_session_id,
            actor.runtime,
          ])}
          actor={actor}
          groupId={groupId}
        />
      ) : null}
    </div>
  );
}

function RecoveryGuidance({ actor, groupId }: { actor: Actor; groupId: string }) {
  const { t } = useTranslation("actors");
  const groupState = useGroupStore((state) =>
    state.groupDoc?.group_id === groupId ? state.groupDoc.state : undefined,
  );
  const groupRunning = useGroupStore((state) =>
    state.groupDoc?.group_id === groupId ? state.groupDoc.running : undefined,
  );
  const [recovery, setRecovery] = useState<LocalSessionRecovery["recovery"] | null>(null);
  const [readFailed, setReadFailed] = useState(false);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [refreshOutcome, setRefreshOutcome] = useState<
    "connected" | "unavailable" | "failed" | null
  >(null);
  const [expanded, setExpanded] = useState(false);
  const [copyResult, setCopyResult] = useState<{
    field: "cwd" | "command";
    value: string;
    success: boolean;
  } | null>(null);
  const active = useRef(true);
  const requestSeq = useRef(0);
  const refreshInFlight = useRef(false);
  const expansionChosen = useRef(false);

  useEffect(() => {
    active.current = true;
    return () => {
      active.current = false;
      requestSeq.current += 1;
    };
  }, []);

  const load = useCallback(
    async (refresh = false) => {
      // A runtime update caused by qualification must not supersede its result.
      if (refreshInFlight.current) return;
      const request = ++requestSeq.current;
      if (refresh) {
        refreshInFlight.current = true;
        setRefreshing(true);
        setRefreshOutcome(null);
      } else setLoading(true);
      setReadFailed(false);
      setCopyResult(null);
      let confirmedRefresh = false;
      try {
        const response = await (refresh
          ? refreshLocalSessionRecovery(groupId, actor.id)
          : fetchLocalSessionRecovery(groupId, actor.id));
        if (!active.current || request !== requestSeq.current) return;
        const qualificationUnavailable =
          refresh && response.ok && response.result.refresh_status === "unavailable";
        if (
          !response.ok ||
          response.result.group_id !== groupId ||
          response.result.actor_id !== actor.id ||
          // The same URL can now refer to a removed/rejoined Actor's new source.
          response.result.source_id !== actor.external_session_id
        ) {
          throw new Error("Recovery binding could not be confirmed");
        }
        setRecovery(response.result.recovery);
        confirmedRefresh = refresh;
        const connected = response.result.recovery.state === "connected";
        if (!expansionChosen.current) setExpanded(!connected || qualificationUnavailable);
        const outcome = connected
          ? "connected"
          : response.result.recovery.state === "disconnected"
            ? "unavailable"
            : null;
        setRefreshOutcome((previous) =>
          qualificationUnavailable ? "failed" : refresh || previous === outcome ? outcome : null,
        );
      } catch {
        if (active.current && request === requestSeq.current) {
          setRecovery(null);
          setCopyResult(null);
          setRefreshOutcome(refresh ? "failed" : null);
          if (!refresh) setReadFailed(true);
        }
      } finally {
        if (refresh) {
          refreshInFlight.current = false;
          if (active.current) setRefreshing(false);
        }
        if (active.current && request === requestSeq.current) setLoading(false);
      }
      // The status above comes from the Actor list, which the check may
      // have changed; read it once rather than guessing the new state.
      if (confirmedRefresh && active.current) {
        void useGroupStore.getState().refreshActors(groupId);
      }
    },
    [groupId, actor.id, actor.external_session_id],
  );

  useEffect(() => {
    void load();
  }, [load, actor.running, actor.enabled, groupState, groupRunning]);

  const copy = async (field: "cwd" | "command", value: string) => {
    const success = await copyTextToClipboard(value);
    if (active.current) setCopyResult({ field, value, success });
  };
  const shell = recovery ? { posix: "Bash/zsh", powershell: "PowerShell" }[recovery.shell] : "";
  const busy = loading || refreshing;

  return (
    <section
      aria-label={t("nativeSession.recovery.title")}
      aria-busy={busy}
      className="min-w-0 space-y-4 border-t border-[var(--glass-border-subtle)] pt-4"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        {recovery ? (
          <p role="status">
            {t("nativeSession.recovery.stateLabel")}
            {": "}
            <span className="font-medium text-[var(--color-text-primary)]">
              {t(`nativeSession.recovery.states.${recovery.state}`)}
            </span>
          </p>
        ) : loading ? (
          <p role="status">{t("common:loading")}</p>
        ) : null}
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={busy}
          onClick={() => void load(actor.runtime === "codex")}
        >
          {t("nativeSession.recovery.refresh")}
        </Button>
      </div>
      {actor.runtime === "codex" ? (
        <p className="text-xs">{t("nativeSession.recovery.refreshHint")}</p>
      ) : null}
      {readFailed ? <p role="alert">{t("nativeSession.recovery.loadFailed")}</p> : null}
      {refreshing ? <p role="status">{t("nativeSession.recovery.refreshingConnection")}</p> : null}
      {refreshOutcome ? (
        <p role={refreshOutcome === "failed" ? "alert" : "status"}>
          {t(
            refreshOutcome === "connected"
              ? "nativeSession.recovery.refreshConnected"
              : refreshOutcome === "unavailable"
                ? "nativeSession.recovery.refreshUnavailable"
                : "nativeSession.recovery.refreshFailed",
          )}
        </p>
      ) : null}
      {recovery && !refreshOutcome ? (
        <p>{t(`nativeSession.recovery.hints.${recovery.state}`)}</p>
      ) : null}
      {actor.enabled === false || groupState === "paused" || groupState === "stopped" ? (
        <p className="text-xs">{t("nativeSession.recovery.admissionHint")}</p>
      ) : null}
      <details
        open={expanded}
        onToggle={(event) => {
          const open = event.currentTarget.open;
          if (open === expanded) return;
          expansionChosen.current = true;
          setExpanded(open);
        }}
      >
        <summary className="cursor-pointer font-medium text-[var(--color-text-primary)] focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2">
          {t("nativeSession.recovery.title")}
        </summary>
        <div className="mt-3 min-w-0 space-y-4">
          {recovery ? (
            <>
              <div className="space-y-2">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <h4 className="font-medium text-[var(--color-text-primary)]">
                    {t("nativeSession.recovery.cwdLabel")}
                  </h4>
                  {recovery.cwd ? (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => void copy("cwd", recovery.cwd!)}
                    >
                      {t("nativeSession.recovery.copyDirectory")}
                    </Button>
                  ) : null}
                </div>
                {recovery.cwd ? (
                  <code className="block whitespace-pre-wrap break-all text-xs">
                    {recovery.cwd}
                  </code>
                ) : (
                  <p>{t("nativeSession.recovery.cwdUnknown")}</p>
                )}
                {copyResult?.field === "cwd" && copyResult.value === recovery.cwd ? (
                  <p role={copyResult.success ? "status" : "alert"} className="text-xs">
                    {t(copyResult.success ? "common:copied" : "nativeSession.recovery.copyFailed")}
                  </p>
                ) : null}
              </div>
              <div className="space-y-2">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <h4 className="font-medium text-[var(--color-text-primary)]">
                    {t("nativeSession.recovery.commandLabel", { shell })}
                  </h4>
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    onClick={() => void copy("command", recovery.command)}
                  >
                    {t("nativeSession.recovery.copyCommand")}
                  </Button>
                </div>
                <p>{t("nativeSession.recovery.runWarning", { shell })}</p>
                <pre className="min-w-0 whitespace-pre-wrap break-all rounded-md border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)] p-3 text-xs leading-relaxed text-[var(--color-text-primary)]">
                  <code>{recovery.command}</code>
                </pre>
                {copyResult?.field === "command" && copyResult.value === recovery.command ? (
                  <p role={copyResult.success ? "status" : "alert"} className="text-xs">
                    {t(copyResult.success ? "common:copied" : "nativeSession.recovery.copyFailed")}
                  </p>
                ) : null}
              </div>
              {actor.runtime === "codex" ? (
                <p>{t("nativeSession.recovery.codexReconnect")}</p>
              ) : actor.runtime === "claude" ? (
                <p>{t("nativeSession.recovery.claudeReconnect")}</p>
              ) : actor.runtime === "pi" ? (
                <p>{t("nativeSession.recovery.piReconnect")}</p>
              ) : actor.runtime === "opencode" ? (
                <p>{t("nativeSession.recovery.opencodeReconnect")}</p>
              ) : null}
              <p className="text-xs break-all">
                {t("nativeSession.recovery.sourceLabel")}: {actor.external_session_id}
              </p>
            </>
          ) : null}
        </div>
      </details>
      <p className="text-xs">{t("nativeSession.recovery.freshnessHint")}</p>
    </section>
  );
}
