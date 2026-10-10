import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "../../../components/ui/button";
import { useUIStore } from "../../../stores/useUIStore";
import { RUNTIME_INFO } from "../../../types";
import * as api from "../../../services/api";
import type { SecretaryRuntimeState } from "../../../services/api/voiceSecretary";
import { secretaryTaskSubject } from "./secretaryTaskPresentation";
import { secretaryReadinessIssue } from "../../../features/voice/secretaryReadiness";

const NativeSessionTerminal = lazy(() =>
  import("../../../features/voice/NativeSessionTerminal").then((module) => ({
    default: module.NativeSessionTerminal,
  })),
);

/** Observes the daemon-owned global session; reads never start model work. */
export function SecretaryRuntimePanel({ active, isDark }: { active: boolean; isDark: boolean }) {
  const { t } = useTranslation("settings");
  const restricted = useUIStore((s) => s.canAccessGlobalSettings) === false;
  const [state, setState] = useState<SecretaryRuntimeState | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const visit = useRef(0);
  const mutation = useRef<number | null>(null);
  const sequence = useRef(0);
  const pendingRead = useRef<number | null>(null);
  const load = useCallback(
    async (id: number) => {
      if (id !== visit.current || pendingRead.current === sequence.current) return;
      const read = ++sequence.current;
      pendingRead.current = read;
      try {
        const response = await api.fetchSecretaryRuntime();
        if (id !== visit.current || read !== sequence.current || mutation.current !== null) return;
        if (response.ok) {
          setState(response.result);
          setError("");
        } else setError(response.error.message);
      } catch {
        if (id === visit.current && read === sequence.current)
          setError(t("voiceSettings.loadFailed"));
      } finally {
        if (pendingRead.current === read) pendingRead.current = null;
      }
    },
    [t],
  );
  useEffect(() => {
    const id = ++visit.current;
    sequence.current++;
    mutation.current = null;
    setBusy(false);
    if (!active || restricted) {
      setState(null);
      return;
    }
    void load(id);
    const timer = window.setInterval(() => {
      if (mutation.current === null) void load(id);
    }, 2000);
    return () => {
      visit.current = id + 1;
      window.clearInterval(timer);
    };
  }, [active, load, restricted]);
  const generation = state?.generation || "";
  const buildWebSocketUrl = useCallback(
    (query: string) => api.getSecretaryTerminalWebSocketUrl(generation, query),
    [generation],
  );
  const act = async (action: "reset" | "cancel") => {
    if (!state || busy || mutation.current !== null) return;
    const id = visit.current;
    mutation.current = id;
    sequence.current++;
    setBusy(true);
    try {
      const response =
        action === "reset"
          ? await api.resetSecretaryRuntime(generation)
          : state.task &&
            (await api.cancelSecretaryTask(state.task.target.group_id, state.task.task_id));
      if (id !== visit.current) return;
      if (response && !response.ok) setError(response.error.message);
      else {
        mutation.current = null;
        await load(id);
      }
    } catch {
      if (id === visit.current) setError(t("voiceSettings.actionFailed"));
    } finally {
      if (mutation.current === id) mutation.current = null;
      if (id === visit.current) setBusy(false);
    }
  };
  if (restricted)
    return (
      <p className="text-sm text-[var(--color-text-secondary)]">
        {t("voiceSettings.resident.adminOnly")}
      </p>
    );
  const running = state?.phase === "working" || state?.phase === "starting";
  const connected = state?.phase === "ready" || running;
  const runtime = state?.runtime;
  const readinessIssue = secretaryReadinessIssue(state);
  const diagnostic = error || readinessIssue?.detail || (!readinessIssue && state?.diagnostic);
  return (
    <section
      data-secretary-runtime
      className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto scrollbar-subtle"
    >
      <header className="flex shrink-0 flex-wrap items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <p role="status" className="text-sm font-medium">
            {runtime ? `${RUNTIME_INFO[runtime]?.label || runtime} · ` : ""}
            {readinessIssue
              ? t(readinessIssue.titleKey)
              : state
                ? t(`voiceSettings.resident.phases.${state.phase}`)
                : t("common:loading")}
          </p>
          {running && state?.task ? (
            <p className="mt-1 break-words text-sm text-[var(--color-text-secondary)]">
              {state.group_title || state.task.target.group_id} ·{" "}
              {t(`voiceSettings.kinds.${state.task.target.kind}`)}
              {secretaryTaskSubject(state.task) ? ` · ${secretaryTaskSubject(state.task)}` : ""}
            </p>
          ) : (
            <p className="mt-1 text-xs leading-5 text-[var(--color-text-muted)]">
              {t(
                readinessIssue
                  ? readinessIssue.hintKey
                  : state?.manual_turn
                    ? "voiceSettings.resident.manualTurn"
                    : "voiceSettings.resident.sharedHint",
              )}
            </p>
          )}
        </div>
        {running && state?.task ? (
          <Button size="sm" variant="secondary" disabled={busy} onClick={() => void act("cancel")}>
            {t("voiceSettings.cancelTask")}
          </Button>
        ) : (
          generation && (
            <Button
              size="sm"
              variant="ghost"
              disabled={busy || running || !connected}
              onClick={() => void act("reset")}
            >
              {t("voiceSettings.resident.newSession")}
            </Button>
          )
        )}
      </header>
      {diagnostic && (
        <p role="alert" className="shrink-0 break-words text-sm text-rose-600 dark:text-rose-300">
          {diagnostic}
        </p>
      )}
      {state?.activity && running && (
        <p className="shrink-0 break-words text-xs text-[var(--color-text-secondary)]">
          {state.activity}
        </p>
      )}
      {state?.native_terminal && generation && connected ? (
        <div
          className="min-h-[14rem] flex-1 overflow-hidden rounded-lg border border-[var(--glass-border-subtle)]"
          data-secretary-native-terminal
        >
          <Suspense fallback={<p>{t("common:loading")}</p>}>
            <NativeSessionTerminal
              generation={generation}
              scopeId="voice-secretary"
              runtime={runtime || ""}
              isVisible={active}
              buildWebSocketUrl={buildWebSocketUrl}
              lifecycleHint={t("voiceSettings.resident.terminalHint")}
              rejectedHint={t("voiceSettings.terminalRejected")}
              reconnectHint={t("voiceSettings.terminalReconnectHint")}
            />
          </Suspense>
        </div>
      ) : (
        <div
          data-secretary-runtime-output
          className={`min-h-0 space-y-3 text-sm leading-6 ${isDark ? "text-slate-200" : "text-slate-700"}`}
        >
          {state?.progress ? (
            <pre className="whitespace-pre-wrap break-words font-sans">{state.progress}</pre>
          ) : (
            <p className="text-[var(--color-text-muted)]">
              {t(
                state?.phase === "not_started"
                  ? "voiceSettings.resident.notStartedHint"
                  : "voiceSettings.resident.outputHint",
              )}
            </p>
          )}
        </div>
      )}
    </section>
  );
}
