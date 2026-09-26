import { memo, useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";
import { Mail } from "lucide-react";

import { ActorAvatar } from "../../components/ActorAvatar";
import { PlusIcon } from "../../components/Icons";
import { selectChatBucketState, useGroupStore } from "../../stores";
import { useActorDisplayState } from "../../hooks/useActorDisplayState";
import { ShineBorder } from "@/registry/magicui/shine-border";
import type { Actor, HeadlessStreamEvent } from "../../types";
import { classNames } from "../../utils/classNames";
import type { LiveWorkCard } from "./liveWorkCards";
import { RuntimeDockTicker } from "./RuntimeDockTicker";
import { buildRuntimeDockTickerEntries } from "./runtimeDockTickerEntries";
import { fetchLedgerTailAll } from "../../services/api/messaging";
import { buildRuntimeDockItems, type RuntimeDockItem } from "./runtimeDockItems";
import { getRuntimeRingTone, type RuntimeRingTone } from "./runtimeDockRingTone";
import { buildActorStoppedSinceMap, buildRuntimeDockMailInfo } from "./runtimeDockMail";
import { formatElapsedCompact } from "../../utils/time";

type RuntimeRingPresentation = {
  ringClassName: string;
  ringStyle: CSSProperties;
  unreadBadgeClassName: string;
  avatarClassName?: string;
};

const RUNTIME_RING_GEOMETRY_CLASS = "absolute -inset-[0.5px] rounded-full";
const RUNTIME_RING_STROKE_PX = 7;
const RUNTIME_STATIC_RING_STROKE_CLASS = "border-[4px]";
const RUNTIME_ELAPSED_CAP_MS = 60 * 60 * 1000;
const RUNTIME_ELAPSED_TICK_MS = 10_000;
const RUNTIME_ARC_STROKE_PX = 1.8;

const EMPTY_STOPPED_MAP: Map<string, number | null> = new Map();

const RUNTIME_ARC_COLORS: Record<"active" | "attention" | "idle", string> = {
  active: "rgba(167, 139, 250, 0.95)",
  attention: "rgba(251, 113, 133, 0.9)",
  idle: "rgba(52, 211, 153, 0.85)",
};

function useElapsedNow(active: boolean): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const id = window.setInterval(() => setNow(Date.now()), RUNTIME_ELAPSED_TICK_MS);
    return () => window.clearInterval(id);
  }, [active]);
  return now;
}

function useStateSinceMs(
  isRunning: boolean,
  workingState: string,
  backendSinceIso: string | null | undefined,
  stoppedSinceLedgerMs: number,
): { ms: number; stopped: boolean } | null {
  const stateKey = isRunning ? String(workingState || "running") : "stopped";
  const prevKeyRef = useRef<string | null>(null);
  const baselineRef = useRef<{ key: string; at: number } | null>(null);
  const flipRef = useRef<{ key: string; at: number } | null>(null);
  if (prevKeyRef.current === null) {
    baselineRef.current = { key: stateKey, at: Date.now() };
  } else if (prevKeyRef.current !== stateKey) {
    const at = Date.now();
    baselineRef.current = { key: stateKey, at };
    flipRef.current = { key: stateKey, at };
  }
  prevKeyRef.current = stateKey;

  if (stateKey === "stopped") {
    const flipMs = flipRef.current?.key === "stopped" ? flipRef.current.at : 0;
    const ms = Math.max(stoppedSinceLedgerMs > 0 ? stoppedSinceLedgerMs : 0, flipMs);
    return ms > 0 ? { ms, stopped: true } : null;
  }
  const backendSinceMs = Date.parse(String(backendSinceIso || ""));
  const baseline = baselineRef.current?.at ?? Date.now();
  const ms = Number.isFinite(backendSinceMs) ? Math.min(backendSinceMs, baseline) : baseline;
  return { ms, stopped: false };
}

function buildFlowRingStyle(args: {
  tone: "active" | "attention";
  isDark: boolean;
}): CSSProperties {
  const palette =
    args.tone === "attention"
      ? args.isDark
        ? {
            base: "rgba(251, 113, 133, 0.24)",
            glow: "rgba(239, 68, 68, 0.42)",
            streamA: "rgba(253, 164, 175, 0.9)",
            streamB: "rgba(251, 113, 133, 0.82)",
            streamC: "rgba(254, 226, 226, 0.52)",
          }
        : {
            base: "rgba(251, 113, 133, 0.18)",
            glow: "rgba(239, 68, 68, 0.28)",
            streamA: "rgba(244, 63, 94, 0.82)",
            streamB: "rgba(251, 113, 133, 0.74)",
            streamC: "rgba(255, 228, 230, 0.52)",
          }
      : args.isDark
        ? {
            base: "rgba(160, 124, 254, 0.22)",
            glow: "rgba(254, 143, 181, 0.34)",
            streamA: "rgba(160, 124, 254, 0.92)",
            streamB: "rgba(254, 143, 181, 0.82)",
            streamC: "rgba(255, 190, 123, 0.56)",
          }
        : {
            base: "rgba(160, 124, 254, 0.14)",
            glow: "rgba(254, 143, 181, 0.22)",
            streamA: "rgba(160, 124, 254, 0.82)",
            streamB: "rgba(254, 143, 181, 0.72)",
            streamC: "rgba(255, 190, 123, 0.46)",
          };

  return {
    ["--runtime-flow-base" as keyof CSSProperties]: palette.base,
    ["--runtime-flow-glow" as keyof CSSProperties]: palette.glow,
    ["--runtime-flow-a" as keyof CSSProperties]: palette.streamA,
    ["--runtime-flow-b" as keyof CSSProperties]: palette.streamB,
    ["--runtime-flow-c" as keyof CSSProperties]: palette.streamC,
  };
}

const RuntimeElapsedArc = memo(function RuntimeElapsedArc(args: {
  fraction: number;
  tone: "active" | "attention" | "idle";
  isSmallScreen: boolean;
}) {
  const radius =
    args.tone === "idle" ? (args.isSmallScreen ? 20.5 : 23.5) : args.isSmallScreen ? 25 : 28;
  const size = Math.ceil(2 * (radius + 3));
  const circumference = 2 * Math.PI * radius;
  const fraction = Math.min(1, Math.max(0, args.fraction));
  const color = RUNTIME_ARC_COLORS[args.tone];
  return (
    <svg
      className="pointer-events-none absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2"
      width={size}
      height={size}
      viewBox={`0 0 ${size} ${size}`}
      aria-hidden="true"
    >
      <circle
        cx={size / 2}
        cy={size / 2}
        r={radius}
        fill="none"
        stroke={color}
        strokeWidth={RUNTIME_ARC_STROKE_PX}
        strokeLinecap="round"
        strokeDasharray={`${fraction * circumference} ${circumference}`}
        transform={`rotate(-90 ${size / 2} ${size / 2})`}
      />
      {fraction >= 1 ? <circle cx={size / 2} cy={size / 2 - radius} r={2.1} fill={color} /> : null}
    </svg>
  );
});

const RuntimeFlowRing = memo(function RuntimeFlowRing(args: {
  tone: RuntimeRingTone;
  isDark: boolean;
}) {
  const visible = args.tone === "active" || args.tone === "attention";
  const tone = args.tone === "attention" ? "attention" : "active";
  const duration = tone === "attention" ? 5.4 : 6.2;
  const shineColors: [string, string, string] =
    tone === "attention" ? ["#fb7185", "#ef4444", "#fda4af"] : ["#A07CFE", "#FE8FB5", "#FFBE7B"];
  return (
    <span
      className={classNames(
        "runtime-flow-ring",
        visible ? `runtime-flow-ring--${tone}` : "runtime-flow-ring--inactive",
        RUNTIME_RING_GEOMETRY_CLASS,
      )}
      style={buildFlowRingStyle({ tone, isDark: args.isDark })}
      aria-hidden="true"
    >
      <span className="runtime-flow-ring__base" />
      <span className="runtime-flow-ring__stream runtime-flow-ring__stream--primary" />
      <span className="runtime-flow-ring__stream runtime-flow-ring__stream--secondary" />
      <span className="runtime-flow-ring__glow" />
      <ShineBorder
        className={classNames(RUNTIME_RING_GEOMETRY_CLASS, "runtime-flow-ring__shine")}
        borderWidth={RUNTIME_RING_STROKE_PX}
        duration={duration}
        shineColor={shineColors}
        topGlow={true}
      />
    </span>
  );
});

function getRuntimeStatusLabel(
  isRunning: boolean,
  workingState: string,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  if (!isRunning) return t("stopped", { defaultValue: "Stopped" });
  if (workingState === "working") return t("working", { defaultValue: "Working" });
  if (workingState === "waiting") return t("waiting", { defaultValue: "Waiting" });
  if (workingState === "stuck") return t("stuck", { defaultValue: "Stuck" });
  return t("running", { defaultValue: "Running" });
}

function getLiveWorkBadgeLabel(
  card: LiveWorkCard,
  t: (key: string, options?: Record<string, unknown>) => string,
): string {
  if (card.phase === "failed") {
    return t("liveWorkPhaseFailed", { defaultValue: "Failed" });
  }
  if (card.phase === "pending") {
    return t("liveWorkPhaseQueued", { defaultValue: "Queued" });
  }
  if (card.phase === "streaming") {
    return t("liveWorkPhaseWorking", { defaultValue: "Working" });
  }
  if (card.phase === "completed") {
    return t("liveWorkPhaseCompleted", { defaultValue: "Recent" });
  }
  return t("liveWorkPhaseWorking", { defaultValue: "Working" });
}

function getRuntimeRingPresentation(
  tone: RuntimeRingTone,
  isDark: boolean,
): RuntimeRingPresentation {
  switch (tone) {
    case "active":
      return {
        ringClassName: "hidden",
        ringStyle: {},
        unreadBadgeClassName: isDark
          ? "bg-emerald-300/[0.18] text-emerald-50"
          : "bg-emerald-500/[0.14] text-emerald-700",
      };
    case "attention":
      return {
        ringClassName: "hidden",
        ringStyle: {},
        unreadBadgeClassName: isDark
          ? "bg-rose-300/[0.18] text-rose-50"
          : "bg-rose-500/[0.14] text-rose-700",
      };
    case "idle":
      return {
        ringClassName: classNames(
          RUNTIME_RING_GEOMETRY_CLASS,
          RUNTIME_STATIC_RING_STROKE_CLASS,
          "transition-colors duration-200",
          isDark ? "border-emerald-300/75" : "border-emerald-500/75",
        ),
        ringStyle: {},
        unreadBadgeClassName: isDark
          ? "bg-emerald-300/[0.12] text-emerald-50"
          : "bg-emerald-500/[0.10] text-emerald-700",
      };
    case "stopped":
    default:
      return {
        ringClassName: classNames(
          RUNTIME_RING_GEOMETRY_CLASS,
          RUNTIME_STATIC_RING_STROKE_CLASS,
          "transition-colors duration-200",
          isDark ? "border-slate-400/50" : "border-slate-500/50",
        ),
        ringStyle: {},
        unreadBadgeClassName: isDark
          ? "bg-white/10 text-slate-100"
          : "bg-black/[0.08] text-gray-800",
        avatarClassName: "opacity-45 grayscale saturate-50",
      };
  }
}

function RuntimeDockActorButtonView({
  groupId,
  item,
  isDark,
  isSmallScreen,
  isInspectorOpen,
  actorStatusProvisional,
  mailCount,
  mailOldestTsMs,
  stoppedSinceMs,
  onOpenInspector,
}: {
  groupId: string;
  item: RuntimeDockItem;
  isDark: boolean;
  isSmallScreen: boolean;
  isInspectorOpen: boolean;
  actorStatusProvisional: boolean;
  mailCount: number;
  mailOldestTsMs: number;
  stoppedSinceMs: number;
  onOpenInspector: (actorId: string) => void;
}) {
  const { t } = useTranslation(["chat", "actors"]);
  const { isRunning, workingState } = useActorDisplayState({
    groupId,
    actor: item.actor,
    actorStatusProvisional,
  });
  const ringTone = getRuntimeRingTone(item, isRunning, workingState);
  const ringPresentation = getRuntimeRingPresentation(ringTone, isDark);
  const statusLabel = item.liveWorkCard
    ? getLiveWorkBadgeLabel(item.liveWorkCard, (key, options) => t(`chat:${key}`, options))
    : getRuntimeStatusLabel(isRunning, workingState, (key, options) => t(`actors:${key}`, options));
  const queuedCount = Math.max(0, Number(item.webModelQueuedCount || 0));
  const queuedLabel =
    queuedCount > 0
      ? t("chat:runtimeDockQueuedForNextTurn", {
          count: queuedCount,
          defaultValue: `${queuedCount} queued for next turn`,
        })
      : "";
  const busyRing = ringTone === "active" || ringTone === "attention";
  const ringFrameSize = isSmallScreen ? (busyRing ? 44 : 35) : busyRing ? 50 : 41;
  const stateSince = useStateSinceMs(
    isRunning,
    workingState,
    item.actor.effective_working_updated_at,
    stoppedSinceMs,
  );
  const now = useElapsedNow(stateSince !== null || mailCount > 0);
  const elapsedMs = stateSince === null ? 0 : Math.max(0, now - stateSince.ms);
  const elapsedLabel = formatElapsedCompact(elapsedMs);
  const elapsedTone = busyRing ? ringTone : "idle";
  const elapsedPillClass = busyRing
    ? "border-black/10 bg-white/95 text-black"
    : "border-white/10 bg-slate-500/75 text-white";
  const mailOldestLabel =
    mailOldestTsMs > 0 ? `${Math.floor(Math.max(0, now - mailOldestTsMs) / 60000)}m` : "";
  const mailLabel =
    mailCount > 0
      ? mailOldestLabel
        ? t("chat:runtimeDockUnreadMail", {
            count: mailCount,
            age: mailOldestLabel,
            defaultValue: `${mailCount} unread mail, oldest ${mailOldestLabel}`,
          })
        : t("chat:runtimeDockUnreadMailNoAge", {
            count: mailCount,
            defaultValue: `${mailCount} unread mail`,
          })
      : "";

  const handleOpenInspector = () => {
    onOpenInspector(item.actorId);
  };

  return (
    <div className="relative flex items-end">
      <span
        className={classNames(
          "pointer-events-none absolute -top-[0.72rem] left-1/2 z-30 hidden max-w-[3.75rem] -translate-x-1/2 truncate text-center text-[9px] font-medium leading-[1.2] tracking-[0.01em] opacity-0 transition-opacity delay-[3000ms] duration-150 group-hover/runtime-dock:opacity-100 group-hover/runtime-dock:delay-0 group-has-[:focus-visible]/runtime-dock:opacity-100 group-has-[:focus-visible]/runtime-dock:delay-0 sm:block",
          "runtime-dock-actor-label",
          isDark
            ? "text-white [text-shadow:0_1px_8px_rgba(2,6,23,0.85)]"
            : "text-[rgb(35,36,37)] [text-shadow:0_1px_7px_rgba(255,255,255,0.9)]",
        )}
        aria-hidden="true"
      >
        {item.actorLabel}
      </span>
      <button
        type="button"
        onClick={handleOpenInspector}
        className={classNames(
          "group relative flex h-[50px] w-[50px] items-center justify-center rounded-full shadow-[0_14px_34px_-30px_rgba(15,23,42,0.52)] transition-all duration-200 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(143,163,187)]/40 focus-visible:ring-offset-0",
          item.runner === "headless"
            ? isDark
              ? "bg-transparent"
              : "bg-transparent"
            : isDark
              ? "bg-transparent"
              : "bg-transparent",
          isInspectorOpen
            ? classNames("scale-[1.04] shadow-[0_18px_40px_-28px_rgba(62,80,103,0.32)]")
            : "hover:scale-[1.05] active:scale-[0.95]",
        )}
        aria-label={
          item.runner === "headless"
            ? t("chat:runtimeDockOpenLiveWork", {
                name: item.actorLabel,
                defaultValue: `Open live work for ${item.actorLabel}`,
              })
            : t("chat:runtimeDockOpenTerminal", {
                name: item.actorLabel,
                defaultValue: `Open terminal for ${item.actorLabel}`,
              })
        }
        aria-describedby={`runtime-dock-status-${item.actorId}`}
      >
        <span
          className="pointer-events-none absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 transition-[width,height] duration-200"
          style={{ width: ringFrameSize, height: ringFrameSize }}
        >
          <span
            className={classNames("pointer-events-none", ringPresentation.ringClassName)}
            style={ringPresentation.ringStyle}
          />
          <RuntimeFlowRing tone={ringTone} isDark={isDark} />
        </span>
        {stateSince !== null ? (
          <>
            {!stateSince.stopped ? (
              <RuntimeElapsedArc
                fraction={elapsedMs / RUNTIME_ELAPSED_CAP_MS}
                tone={elapsedTone}
                isSmallScreen={isSmallScreen}
              />
            ) : null}
            <span
              aria-hidden="true"
              className={classNames(
                "pointer-events-none absolute left-1/2 z-20 -translate-x-1/2 whitespace-nowrap rounded-full border font-semibold leading-none shadow-sm",
                isSmallScreen
                  ? "bottom-[-4px] px-[6px] py-[3px] text-[10px]"
                  : "bottom-[-5px] px-[7px] py-[3px] text-[11px]",
                elapsedPillClass,
              )}
            >
              {elapsedLabel}
            </span>
          </>
        ) : null}

        <ActorAvatar
          avatarUrl={item.actor.avatar_url || undefined}
          runtime={item.runtime}
          title={item.actorLabel}
          isDark={isDark}
          sizeClassName={isSmallScreen ? "h-[33px] w-[33px]" : "h-[37px] w-[37px]"}
          className={classNames(
            "relative z-10 border-transparent shadow-[0_18px_34px_-22px_rgba(15,23,42,0.68)]",
            ringPresentation.avatarClassName,
            item.runner === "headless" ? (isDark ? "bg-slate-900" : "bg-slate-50") : undefined,
          )}
          accentRingClassName={
            isInspectorOpen ? (isDark ? "ring-white/10" : "ring-black/10") : null
          }
        />
        {queuedCount > 0 ? (
          <span
            className={classNames(
              "pointer-events-none absolute -right-0.5 -top-0.5 z-20 flex h-[17px] min-w-[17px] items-center justify-center rounded-full px-1 text-[9px] font-semibold leading-none shadow-[0_8px_18px_-10px_rgba(15,23,42,0.7)]",
              ringPresentation.unreadBadgeClassName,
            )}
            aria-hidden="true"
            title={queuedLabel}
          >
            {queuedCount > 99 ? "99+" : queuedCount}
          </span>
        ) : null}
        {mailCount > 0 ? (
          <span
            className={classNames(
              "pointer-events-none absolute -left-1.5 -top-1 z-20 flex h-[17px] items-center gap-[3px] whitespace-nowrap rounded-full border px-[5px] font-semibold leading-none shadow-[0_8px_18px_-10px_rgba(15,23,42,0.7)]",
              isSmallScreen ? "text-[9px]" : "text-[10px]",
              isDark
                ? "border-amber-300/40 bg-amber-400/25 text-amber-100"
                : "border-amber-400/60 bg-amber-200/95 text-amber-900",
            )}
            aria-hidden="true"
            title={mailLabel}
          >
            {mailCount > 99 ? "99+" : mailCount}
            <Mail
              className={isSmallScreen ? "h-[9px] w-[9px]" : "h-[10px] w-[10px]"}
              strokeWidth={2.4}
            />
            {mailOldestLabel}
          </span>
        ) : null}
      </button>
      <span id={`runtime-dock-status-${item.actorId}`} className="sr-only">
        {item.actorLabel} · {item.runtime} · {statusLabel}
        {queuedLabel ? ` · ${queuedLabel}` : ""}
        {mailLabel ? ` · ${mailLabel}` : ""}
      </span>
    </div>
  );
}

const RuntimeDockActorButton = memo(
  RuntimeDockActorButtonView,
  (previous, next) =>
    previous.groupId === next.groupId &&
    previous.item.actor === next.item.actor &&
    previous.item.liveWorkCard === next.item.liveWorkCard &&
    previous.item.actorId === next.item.actorId &&
    previous.item.actorLabel === next.item.actorLabel &&
    previous.item.runtime === next.item.runtime &&
    previous.item.runner === next.item.runner &&
    previous.item.webModelQueuedCount === next.item.webModelQueuedCount &&
    previous.mailCount === next.mailCount &&
    previous.mailOldestTsMs === next.mailOldestTsMs &&
    previous.stoppedSinceMs === next.stoppedSinceMs &&
    previous.isDark === next.isDark &&
    previous.isSmallScreen === next.isSmallScreen &&
    previous.isInspectorOpen === next.isInspectorOpen &&
    previous.actorStatusProvisional === next.actorStatusProvisional &&
    previous.onOpenInspector === next.onOpenInspector,
);

export interface RuntimeDockProps {
  groupId: string;
  runtimeActors: Actor[];
  liveWorkCards: LiveWorkCard[];
  runtimeEvents?: Record<string, HeadlessStreamEvent[]>;
  activeRuntimeActorId?: string;
  isDark: boolean;
  isSmallScreen: boolean;
  readOnly?: boolean;
  actorStatusProvisional: boolean;
  onAddAgent?: () => void;
  onOpenRuntimeActor: (actorId: string) => void;
}

export function RuntimeDock({
  groupId,
  runtimeActors,
  liveWorkCards,
  runtimeEvents,
  activeRuntimeActorId,
  isDark,
  isSmallScreen,
  readOnly,
  actorStatusProvisional,
  onAddAgent,
  onOpenRuntimeActor,
}: RuntimeDockProps) {
  const { t } = useTranslation("chat");

  const items = useMemo(
    () => buildRuntimeDockItems({ actors: runtimeActors, liveWorkCards }),
    [runtimeActors, liveWorkCards],
  );
  const ledgerEvents = useGroupStore((state) => selectChatBucketState(state, groupId).events);
  const mailInfoByActorId = useMemo(() => buildRuntimeDockMailInfo(ledgerEvents), [ledgerEvents]);
  const [historyStopped, setHistoryStopped] =
    useState<Map<string, number | null>>(EMPTY_STOPPED_MAP);
  useEffect(() => {
    let cancelled = false;
    setHistoryStopped(EMPTY_STOPPED_MAP);
    void fetchLedgerTailAll(groupId)
      .then((resp) => {
        if (cancelled || !resp.ok) return;
        setHistoryStopped(buildActorStoppedSinceMap(resp.result?.events || []));
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [groupId]);
  const stoppedSinceByActorId = useMemo(() => {
    const live = buildActorStoppedSinceMap(ledgerEvents);
    const merged = new Map<string, number>();
    const ids = new Set([...live.keys(), ...historyStopped.keys()]);
    for (const id of ids) {
      const outcome = live.has(id) ? live.get(id) : historyStopped.get(id);
      if (typeof outcome === "number" && outcome > 0) merged.set(id, outcome);
    }
    return merged;
  }, [ledgerEvents, historyStopped]);
  const tickerEntries = useMemo(
    () => buildRuntimeDockTickerEntries(items, runtimeEvents),
    [items, runtimeEvents],
  );

  if (items.length <= 0) return null;

  return (
    <div className="pointer-events-none relative z-30 px-3 pt-2 sm:px-4 sm:pt-2.5">
      <div className="mx-auto flex w-full max-w-[1400px] justify-center">
        <div
          className={classNames(
            "group/runtime-dock pointer-events-auto relative flex justify-center",
            isSmallScreen ? "max-w-[calc(100vw-2.5rem)]" : "",
          )}
        >
          <RuntimeDockTicker
            key={groupId}
            groupId={groupId}
            entries={tickerEntries}
            isDark={isDark}
            suppressed={Boolean(activeRuntimeActorId)}
          />
          <div
            className={classNames(
              "flex items-end opacity-[0.72] transition-opacity delay-[3000ms] duration-200 ease-out group-hover/runtime-dock:opacity-100 group-hover/runtime-dock:delay-0 group-has-[:focus-visible]/runtime-dock:opacity-100 group-has-[:focus-visible]/runtime-dock:delay-0",
              isSmallScreen
                ? "max-w-[calc(100vw-2.5rem)] gap-2 overflow-x-auto pb-3 scrollbar-hide"
                : "gap-2",
            )}
          >
            <div className="relative flex items-end gap-2">
              {items.map((item) => {
                const mailInfo = mailInfoByActorId.get(item.actorId);
                return (
                  <RuntimeDockActorButton
                    key={item.actorId}
                    groupId={groupId}
                    item={item}
                    isDark={isDark}
                    isSmallScreen={isSmallScreen}
                    isInspectorOpen={activeRuntimeActorId === item.actorId}
                    actorStatusProvisional={actorStatusProvisional}
                    mailCount={mailInfo?.count ?? 0}
                    mailOldestTsMs={mailInfo?.oldestTsMs ?? 0}
                    stoppedSinceMs={stoppedSinceByActorId.get(item.actorId) ?? 0}
                    onOpenInspector={onOpenRuntimeActor}
                  />
                );
              })}
            </div>

            {!readOnly && onAddAgent ? (
              <button
                type="button"
                onClick={onAddAgent}
                className={classNames(
                  "group/add-agent relative flex h-[50px] w-[50px] flex-shrink-0 items-center justify-center rounded-full shadow-[0_14px_34px_-30px_rgba(15,23,42,0.52)] transition-all duration-200 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(143,163,187)]/35 active:scale-[0.97]",
                  "bg-transparent hover:scale-[1.02]",
                )}
                aria-label={t("addAgent", { defaultValue: "Add an agent" })}
                title={t("addAgent", { defaultValue: "Add an agent" })}
              >
                <span
                  aria-hidden="true"
                  className={classNames(
                    "relative z-[1] flex items-center justify-center rounded-full border shadow-[0_18px_34px_-22px_rgba(15,23,42,0.4)] transition-[transform,box-shadow,border-color,background-color,color] duration-200",
                    isDark
                      ? "h-[37px] w-[37px] border-white/10 bg-slate-900 text-slate-100 group-hover/add-agent:border-white/16 group-hover/add-agent:bg-slate-950"
                      : "h-[37px] w-[37px] border-black/10 bg-white text-[rgb(35,36,37)] group-hover/add-agent:border-black/14 group-hover/add-agent:bg-white/96",
                  )}
                >
                  <PlusIcon size={18} strokeWidth={2.1} />
                </span>
              </button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
