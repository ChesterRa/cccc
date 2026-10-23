import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../services/api";
import type { GrokActorState } from "../../services/api";
import { formatMessageTimestamp } from "../../utils/time";
import { Button } from "../ui/button";
import { inputClass } from "../modals/settings/types";
import {
  NativeActorDelivery,
  NativeActorSummary,
  nativeActorCard as card,
} from "./NativeActorSections";
import { phaseTone } from "./nativeActorStatus";

type Props = {
  groupId: string;
  actorId: string;
  isVisible: boolean;
  readOnly?: boolean;
  canAccessConnection?: boolean;
  compact?: boolean;
  isDark?: boolean;
  saving?: boolean;
  setup?: boolean;
  targetDirty?: boolean;
  targetEditor?: (state: GrokActorState | null, locked: boolean) => ReactNode;
  onExpand?: () => void;
  onSettings?: () => void;
  onOpenSharedSettings?: () => void;
  onStateChange?: (state: GrokActorState | null) => void;
  onBusyChange?: (busy: boolean) => void;
};
export function GrokActorPanel(props: Props) {
  return <Panel key={props.groupId + "/" + props.actorId} {...props} />;
}
function Panel({
  groupId,
  actorId,
  isVisible,
  readOnly = false,
  canAccessConnection = true,
  compact = false,
  isDark = false,
  saving = false,
  setup = false,
  targetDirty = false,
  targetEditor,
  onExpand,
  onSettings,
  onOpenSharedSettings,
  onStateChange,
  onBusyChange,
}: Props) {
  const { t } = useTranslation("settings");
  const [state, setState] = useState<GrokActorState | null>(null);
  const [readError, setReadError] = useState("");
  const [error, setError] = useState("");
  const [action, setAction] = useState("");
  const [instructions, setInstructions] = useState("");
  const [copied, setCopied] = useState(false);
  const [url, setUrl] = useState("");
  const [key, setKey] = useState("");
  const [confirm, setConfirm] = useState<{ turn_id: string; event_ids: string[] } | null>(null);
  const [disconnecting, setDisconnecting] = useState(false);
  const [refresh, setRefresh] = useState(0);
  const pending = useRef(false);
  const mounted = useRef(true);
  const revision = useRef(0);
  const savedBot = useRef("");
  const current = useRef({ visible: false, onStateChange, onBusyChange, t });
  current.current = { visible: isVisible && canAccessConnection, onStateChange, onBusyChange, t };
  useEffect(() => {
    if (!isVisible || !canAccessConnection) return;
    let cancelled = false,
      loading = false;
    const load = async () => {
      if (loading || pending.current) return;
      loading = true;
      const started = revision.current;
      try {
        const r = await api.fetchGrokActorState(groupId, actorId);
        if (cancelled || pending.current || started !== revision.current) return;
        if (!r.ok) throw new Error(r.error.message);
        if (r.result.group_id !== groupId || r.result.actor_id !== actorId) return;
        const botUrl = r.result.bot_url || "";
        if (savedBot.current !== botUrl) {
          // Routine drafts and setup instructions belong to one saved Bot.
          savedBot.current = botUrl;
          setUrl("");
          setKey("");
          setInstructions("");
          setCopied(false);
          setError("");
          setConfirm(null);
          setDisconnecting(false);
        }
        setState(r.result);
        setReadError("");
        current.current.onStateChange?.(r.result);
      } catch {
        if (!cancelled && !pending.current && started === revision.current)
          setReadError(current.current.t("grokActor.readFailed"));
      } finally {
        loading = false;
      }
    };
    void load();
    const timer = window.setInterval(() => void load(), 2000);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [groupId, actorId, isVisible, canAccessConnection, refresh]);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      current.current.visible = false;
      current.current.onBusyChange?.(false);
    };
  }, []);
  const locked = readOnly || saving || Boolean(action) || !state || !isVisible;
  const unresolved = Boolean(state?.active_turn_id);
  async function act(kind: "instructions" | "configure" | "retry" | "resolve" | "disconnect") {
    if (locked || !canAccessConnection || pending.current || !state) return;
    if (kind === "configure" && unresolved) return;
    pending.current = true;
    revision.current += 1;
    setAction(kind);
    setError("");
    current.current.onBusyChange?.(true);
    try {
      const r = await api.changeGrokWebhook(groupId, actorId, {
        action: kind,
        ...(kind === "configure" ? { url: url.trim(), key: key.trim() } : {}),
        ...(kind === "resolve" && confirm ? confirm : {}),
      });
      if (!current.current.visible) return;
      if (!r.ok)
        throw new Error(
          r.error.code === "delivery_unresolved" ? t("grokWebhook.waitForTask") : r.error.message,
        );
      if (kind === "instructions") {
        setInstructions(r.result.instructions || "");
        setCopied(false);
      }
      if (!current.current.visible) return;
      if (kind === "configure") {
        setUrl("");
        setKey("");
      }
      if (kind === "disconnect") setInstructions("");
      setConfirm(null);
      setDisconnecting(false);
      setRefresh((value) => value + 1);
    } catch (e) {
      if (current.current.visible) setError(e instanceof Error ? e.message : String(e));
      if (kind === "configure" && mounted.current) setRefresh((value) => value + 1);
    } finally {
      pending.current = false;
      if (mounted.current) {
        setAction("");
        current.current.onBusyChange?.(false);
      }
    }
  }
  const phase = state?.phase || "unconfigured";
  const last = state?.last_execution;
  const lastTask = last
    ? t("grokActor.outcomes." + last.status, { defaultValue: last.status }) +
      (last.completed_at ? " · " + formatMessageTimestamp(last.completed_at) : "")
    : t("grokActor.noTask");
  const openBot = state?.bot_url && (
    <a
      href={state.bot_url}
      target="_blank"
      rel="noopener noreferrer"
      className="inline-flex h-9 shrink-0 items-center rounded-lg border border-[var(--glass-border-subtle)] px-3 text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg)] focus-visible:outline-2"
    >
      {t("grokWebhook.openBot")} ↗
    </a>
  );
  if (!canAccessConnection)
    return (
      <section className="p-3 text-sm" aria-label={t("grokActor.title")}>
        <h2 className="font-semibold">{t("grokActor.title")}</h2>
        <p className="mt-2 text-xs text-[var(--color-text-secondary)]">
          {t("grokActor.adminManaged")}
        </p>
      </section>
    );
  const notices = (
    <>
      {readError && (
        <p role="status" className="text-xs text-amber-700 dark:text-amber-300">
          {readError}
        </p>
      )}
      {error && (
        <p role="alert" className="text-xs text-rose-600 dark:text-rose-400">
          {error}
        </p>
      )}
      {state?.error_code && phase === "failed" && (
        <p role="alert" className="text-xs text-rose-600 dark:text-rose-400">
          {t("grokWebhook.deliveryError", { status: state.http_status || "—" })}
        </p>
      )}
    </>
  );
  const recovery = !readOnly &&
    state &&
    (unresolved || ["failed", "unclaimed"].includes(phase)) && (
      <>
        {!unresolved && ["failed", "unclaimed"].includes(phase) && (
          <Button variant="outline" disabled={locked} onClick={() => void act("retry")}>
            {t("grokWebhook.retry")}
          </Button>
        )}
        {unresolved && (
          <div className="space-y-2">
            <p className="text-xs leading-relaxed text-[var(--color-text-muted)]">
              {t(confirm ? "nativeActor.endTurnConfirm" : "nativeActor.endTurnHint", {
                provider: t("grokActor.title"),
              })}
            </p>
            <div className="flex flex-wrap gap-2">
              <Button
                variant="outline"
                disabled={locked}
                onClick={() =>
                  confirm
                    ? void act("resolve")
                    : setConfirm({
                        turn_id: state.active_turn_id!,
                        event_ids: [...(state.active_event_ids || [])],
                      })
                }
              >
                {t(confirm ? "nativeActor.confirmEndTurn" : "nativeActor.endTurn")}
              </Button>
              {confirm && (
                <Button variant="ghost" onClick={() => setConfirm(null)}>
                  {t("common:cancel")}
                </Button>
              )}
            </div>
          </div>
        )}
      </>
    );
  const summaries: Array<[string, string]> = [
    [
      t("grokActor.routine"),
      state
        ? t(state.configured ? "grokActor.configured" : "grokActor.setupRequired")
        : t("grokActor.unavailable"),
    ],
    [t("grokActor.queued"), state ? t("grokWebhook.pending", { count: state.pending_count }) : "—"],
    [t("grokActor.lastTask"), state ? lastTask : t("grokActor.unavailable")],
  ];
  return (
    <section
      className={
        "@container/nativeActor flex min-h-0 flex-1 flex-col overflow-y-auto text-sm " +
        (compact ? "gap-2 p-2" : "gap-4")
      }
      aria-label={t("grokActor.title")}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            {!compact && <h2 className="text-base font-semibold">{t("grokActor.title")}</h2>}
            {!setup && (
              <p
                role="status"
                className={
                  "inline-flex min-h-6 items-center rounded-full px-2.5 py-1 text-xs font-semibold " +
                  phaseTone(phase)
                }
              >
                {state
                  ? t("grokWebhook.phase." + phase)
                  : t(readError ? "grokActor.unavailable" : "common:loading")}
              </p>
            )}
          </div>
          {!compact && !setup && (
            <p className="mt-1 text-xs leading-relaxed text-[var(--color-text-muted)]">
              {t("grokActor.hint")}
            </p>
          )}
        </div>
        <div className="flex flex-wrap gap-2">
          {compact && onExpand && (
            <Button variant="outline" onClick={onExpand}>
              {t("dotsActor.openDetails")}
            </Button>
          )}
          {!setup &&
            !readOnly &&
            onSettings &&
            (compact || (state && !state.configured && !unresolved)) && (
              <Button variant="outline" onClick={onSettings}>
                {t("grokActor.connectionSettings")}
              </Button>
            )}
          {openBot}
        </div>
      </div>
      {notices}
      {!setup && <NativeActorSummary cells={summaries} compact={compact} />}
      {setup ? (
        recovery
      ) : (
        <NativeActorDelivery phase={phase} provider="grok" t={t} compact={compact}>
          {recovery}
        </NativeActorDelivery>
      )}
      {setup && !readOnly && (
        <div className={card + " shrink-0 divide-y divide-[var(--glass-border-subtle)]"}>
          <div className="space-y-2 px-4 py-3">
            <h3 className="font-semibold">{t("grokActor.connectorStep")}</h3>
            <p className="text-xs text-[var(--color-text-muted)]">{t("grokActor.connectorHint")}</p>
            {onOpenSharedSettings && (
              <Button variant="outline" disabled={Boolean(action)} onClick={onOpenSharedSettings}>
                {t("grokActor.shared")}
              </Button>
            )}
          </div>
          <div className="space-y-2 px-4 py-3">
            <h3 className="font-semibold">{t("grokActor.botStep")}</h3>
            {targetEditor?.(state, locked)}
          </div>
          <div className="space-y-3 px-4 py-3">
            <h3 className="font-semibold">{t("grokActor.routineStep")}</h3>
            <p className="text-xs leading-relaxed text-[var(--color-text-muted)]">
              {t("grokWebhook.setupHint")}
            </p>
            <Button
              variant="outline"
              disabled={locked || !state?.bound || targetDirty}
              onClick={() => void act("instructions")}
            >
              {t("grokWebhook.instructions")}
            </Button>
            {instructions && (
              <div className="space-y-2">
                <textarea
                  aria-label={t("grokWebhook.instructions")}
                  readOnly
                  rows={5}
                  value={instructions}
                  className={inputClass(isDark) + " w-full font-mono text-xs"}
                />
                <Button
                  variant="outline"
                  onClick={() =>
                    void navigator.clipboard
                      .writeText(instructions)
                      .then(() => setCopied(true))
                      .catch(() => setError(t("grokWebhook.copyFailed")))
                  }
                >
                  {t(copied ? "common:copied" : "common:copy")}
                </Button>
              </div>
            )}
            <p className="text-xs leading-relaxed text-[var(--color-text-muted)]">
              {t(state?.configured ? "grokWebhook.saved" : "grokWebhook.desktop")}
            </p>
            <label className="block text-xs">
              {t("grokWebhook.url")}
              <input
                type="password"
                autoComplete="off"
                spellCheck={false}
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                className={inputClass(isDark) + " mt-1 w-full"}
                disabled={locked || !state?.bound || targetDirty}
              />
            </label>
            <label className="block text-xs">
              {t("grokWebhook.key")}
              <input
                type="password"
                autoComplete="new-password"
                value={key}
                onChange={(e) => setKey(e.target.value)}
                className={inputClass(isDark) + " mt-1 w-full"}
                disabled={locked || !state?.bound || targetDirty}
              />
            </label>
            <Button
              disabled={
                locked || unresolved || !state?.bound || targetDirty || !url.trim() || !key.trim()
              }
              onClick={() => void act("configure")}
            >
              {t(state?.enabled ? "grokWebhook.saveAndApply" : "grokWebhook.save")}
            </Button>
            <p className="text-xs text-[var(--color-text-muted)]">
              {t(unresolved ? "grokWebhook.waitForTask" : "grokWebhook.applyHint")}
            </p>
          </div>
          {state?.bound && (
            <div className="space-y-2 px-4 py-3">
              <p className="text-xs text-[var(--color-text-muted)]">
                {t("grokActor.disconnectHint")}
              </p>
              <Button
                variant="outline"
                disabled={locked || state.enabled || unresolved || targetDirty}
                onClick={() => (disconnecting ? void act("disconnect") : setDisconnecting(true))}
              >
                {t(disconnecting ? "grokActor.confirmRemove" : "grokActor.remove")}
              </Button>
              {disconnecting && (
                <Button variant="ghost" onClick={() => setDisconnecting(false)}>
                  {t("common:cancel")}
                </Button>
              )}
            </div>
          )}
        </div>
      )}
      {!setup && !state?.configured && !unresolved && (
        <p className="text-xs text-[var(--color-text-secondary)]">{t("grokActor.setupHint")}</p>
      )}
      {!compact && !setup && (
        <p className="text-xs leading-relaxed text-[var(--color-text-muted)]">
          {t("grokWebhook.runtimeHint")}
        </p>
      )}
    </section>
  );
}
