import { useEffect, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import * as api from "../../services/api";
import type { GrokActorState } from "../../services/api";
import { formatMessageTimestamp } from "../../utils/time";
import { Button } from "../ui/button";
import { inputClass } from "../modals/settings/types";

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
const card = "rounded-xl border border-[var(--glass-border-subtle)] bg-[var(--glass-panel-bg)]";
function tone(phase: string) {
  if (phase === "ready" || phase === "working")
    return "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
  if (phase === "failed" || phase === "interrupted")
    return "bg-rose-500/10 text-rose-700 dark:text-rose-300";
  if (["pending", "delivering", "delivered", "unclaimed"].includes(phase))
    return "bg-amber-500/10 text-amber-700 dark:text-amber-300";
  return "bg-[var(--glass-tab-bg)] text-[var(--color-text-secondary)]";
}
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
  async function act(
    kind: "instructions" | "configure" | "retry" | "resolve" | "disconnect" | "pause",
  ) {
    if (locked || !canAccessConnection || pending.current || !state) return;
    if (kind === "configure" && unresolved) return;
    pending.current = true;
    revision.current += 1;
    setAction(kind);
    setError("");
    current.current.onBusyChange?.(true);
    try {
      if (kind === "pause") {
        const r = await api.stopActor(groupId, actorId);
        if (!r.ok) throw new Error(r.error.message);
      } else {
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
  const recovery = !readOnly && state && (
    <>
      {unresolved && state.enabled && (
        <Button variant="outline" disabled={locked} onClick={() => void act("pause")}>
          {t("grokWebhook.pause")}
        </Button>
      )}
      {!unresolved && ["failed", "unclaimed"].includes(phase) && (
        <Button variant="outline" disabled={locked} onClick={() => void act("retry")}>
          {t("grokWebhook.retry")}
        </Button>
      )}
      {unresolved && !state.enabled && (
        <div className="space-y-2">
          <p className="text-xs text-[var(--color-text-muted)]">{t("grokWebhook.resolveHint")}</p>
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
              {t(confirm ? "grokWebhook.confirmResolve" : "grokWebhook.resolve")}
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
  const summaries = [
    [
      t("grokActor.routine"),
      t(state?.configured ? "grokActor.configured" : "grokActor.setupRequired"),
    ],
    [t("grokActor.queued"), state ? t("grokWebhook.pending", { count: state.pending_count }) : "—"],
    [t("grokActor.lastTask"), lastTask],
  ];
  const step =
    phase === "working" || phase === "interrupted"
      ? 2
      : ["delivered", "unclaimed"].includes(phase)
        ? 1
        : ["pending", "delivering", "failed"].includes(phase)
          ? 0
          : last?.status === "done"
            ? 4
            : -1;
  return (
    <section
      className={
        "@container/grok flex min-h-0 flex-1 flex-col overflow-y-auto text-sm " +
        (compact ? "gap-2 p-2" : "gap-4")
      }
      aria-label={t("grokActor.title")}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 flex-wrap items-center gap-2">
          {!compact && <h2 className="text-base font-semibold">{t("grokActor.title")}</h2>}
          <p
            role="status"
            className={"rounded-full px-2.5 py-1 text-xs font-semibold " + tone(phase)}
          >
            {state
              ? t("grokWebhook.phase." + phase)
              : t(readError ? "grokActor.unavailable" : "common:loading")}
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          {compact && onExpand && (
            <Button variant="outline" onClick={onExpand}>
              {t("dotsActor.openDetails")}
            </Button>
          )}
          {!setup && !readOnly && onSettings && (
            <Button variant="outline" onClick={onSettings}>
              {t("grokActor.connectionSettings")}
            </Button>
          )}
          {openBot}
        </div>
      </div>
      {!compact && (
        <p className="text-xs leading-relaxed text-[var(--color-text-muted)]">
          {t("grokActor.hint")}
        </p>
      )}
      {notices}
      <dl
        className={
          compact
            ? "flex flex-wrap gap-x-4 gap-y-2 border-b border-[var(--glass-border-subtle)] pb-2 text-xs"
            : card + " grid shrink-0 grid-cols-1 overflow-hidden @xl/grok:grid-cols-3"
        }
      >
        {summaries.map(([label, value], index) => (
          <div
            key={label}
            className={
              compact
                ? "min-w-0"
                : "px-4 py-3 " +
                  (index
                    ? "border-t border-[var(--glass-border-subtle)] @xl/grok:border-t-0 @xl/grok:border-l"
                    : "")
            }
          >
            <dt className="text-xs text-[var(--color-text-muted)]">{label}</dt>
            <dd className="mt-1 font-semibold">{value}</dd>
          </div>
        ))}
      </dl>
      {recovery}
      {step >= 0 && !compact && (
        <div className={card + " overflow-hidden"}>
          <h3 className="border-b border-[var(--glass-border-subtle)] px-4 py-3 font-semibold">
            {t("grokActor.taskProgress")}
          </h3>
          <ol className="grid gap-3 px-4 py-3 @xl/grok:grid-cols-4">
            {["notify", "waiting", "working", "completed"].map((name, index) => (
              <li
                key={name}
                aria-current={index === step ? "step" : undefined}
                className="flex items-center gap-2 text-xs"
              >
                <span
                  className={
                    "flex size-6 shrink-0 items-center justify-center rounded-full " +
                    (index < step
                      ? tone("ready")
                      : index === step
                        ? tone(phase)
                        : tone("unconfigured"))
                  }
                >
                  {index < step ? "✓" : index + 1}
                </span>
                {t("grokActor.steps." + name)}
              </li>
            ))}
          </ol>
        </div>
      )}
      {setup && !readOnly && (
        <div className={card + " divide-y divide-[var(--glass-border-subtle)]"}>
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
      {!compact && (
        <p className="text-xs leading-relaxed text-[var(--color-text-muted)]">
          {t("grokWebhook.runtimeHint")}
        </p>
      )}
    </section>
  );
}
