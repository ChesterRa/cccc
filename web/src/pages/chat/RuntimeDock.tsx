import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { ActorAvatar } from "../../components/ActorAvatar";
import { PlusIcon } from "../../components/Icons";
import { useActorDisplayState } from "../../hooks/useActorDisplayState";
import { ShineBorder } from "@/registry/magicui/shine-border";
import type { Actor } from "../../types";
import { classNames } from "../../utils/classNames";
import type { LiveWorkCard } from "./liveWorkCards";
import {
  createRuntimeDockTickerCache,
  pruneRuntimeDockTickerCache,
  upsertRuntimeDockTickerCache,
  type RuntimeDockTickerCache,
} from "./runtimeDockTickerCache";
import { buildRuntimeDockTickerEntries, type RuntimeDockTickerEntry } from "./runtimeDockTickerEntries";
import { buildRuntimeDockItems, type RuntimeDockItem } from "./runtimeDockItems";
import { getRuntimeRingTone, type RuntimeRingTone } from "./runtimeDockRingTone";

type RuntimeRingPresentation = {
  ringClassName: string;
  ringStyle: CSSProperties;
  unreadBadgeClassName: string;
  avatarClassName?: string;
  customRing?: ReactNode;
};

const RUNTIME_RING_GEOMETRY_CLASS = "absolute -inset-[0.5px] rounded-full";
const RUNTIME_RING_STROKE_PX = 4;
const RUNTIME_STATIC_RING_STROKE_CLASS = "border-[4px]";

function buildFlowRingStyle(args: {
  tone: "active" | "attention";
  isDark: boolean;
}): CSSProperties {
  const palette = args.tone === "attention"
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

function renderFlowRing(args: {
  tone: "active" | "attention";
  isDark: boolean;
  shineColors: [string, string, string];
  duration: number;
}): ReactNode {
  return (
    <span
      className={classNames(
        "runtime-flow-ring",
        args.tone === "attention" ? "runtime-flow-ring--attention" : "runtime-flow-ring--active",
        RUNTIME_RING_GEOMETRY_CLASS,
      )}
      style={buildFlowRingStyle({ tone: args.tone, isDark: args.isDark })}
    >
      <span className="runtime-flow-ring__base" />
      <span className="runtime-flow-ring__stream runtime-flow-ring__stream--primary" />
      <span className="runtime-flow-ring__stream runtime-flow-ring__stream--secondary" />
      <span className="runtime-flow-ring__glow" />
      <ShineBorder
        className={classNames(RUNTIME_RING_GEOMETRY_CLASS, "runtime-flow-ring__shine")}
        borderWidth={RUNTIME_RING_STROKE_PX}
        duration={args.duration}
        shineColor={args.shineColors}
        topGlow={true}
      />
    </span>
  );
}

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

function getLiveWorkBadgeLabel(card: LiveWorkCard, t: (key: string, options?: Record<string, unknown>) => string): string {
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

function getRuntimeRingPresentation(tone: RuntimeRingTone, isDark: boolean): RuntimeRingPresentation {
  switch (tone) {
    case "active":
      return {
        ringClassName: "hidden",
        ringStyle: {},
        customRing: renderFlowRing({
          tone: "active",
          isDark,
          duration: 6.2,
          shineColors: ["#A07CFE", "#FE8FB5", "#FFBE7B"],
        }),
        unreadBadgeClassName: isDark ? "bg-emerald-300/[0.18] text-emerald-50" : "bg-emerald-500/[0.14] text-emerald-700",
      };
    case "attention":
      return {
        ringClassName: "hidden",
        ringStyle: {},
        customRing: renderFlowRing({
          tone: "attention",
          isDark,
          duration: 5.4,
          shineColors: ["#fb7185", "#ef4444", "#fda4af"],
        }),
        unreadBadgeClassName: isDark ? "bg-rose-300/[0.18] text-rose-50" : "bg-rose-500/[0.14] text-rose-700",
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
        unreadBadgeClassName: isDark ? "bg-emerald-300/[0.12] text-emerald-50" : "bg-emerald-500/[0.10] text-emerald-700",
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
        unreadBadgeClassName: isDark ? "bg-white/10 text-slate-100" : "bg-black/[0.08] text-gray-800",
        avatarClassName: "opacity-45 grayscale saturate-50",
      };
  }
}

function RuntimeDockTicker({
  entries,
  isDark,
  suppressed,
}: {
  entries: RuntimeDockTickerEntry[];
  isDark: boolean;
  suppressed: boolean;
}) {
  const cacheRef = useRef<RuntimeDockTickerCache>(createRuntimeDockTickerCache());
  const [visibleEntries, setVisibleEntries] = useState<RuntimeDockTickerEntry[]>([]);

  useEffect(() => {
    const timer = window.setTimeout(() => {
      setVisibleEntries(upsertRuntimeDockTickerCache(cacheRef.current, entries, Date.now()));
    }, 0);
    return () => window.clearTimeout(timer);
  }, [entries]);

  useEffect(() => {
    const timer = window.setInterval(() => {
      setVisibleEntries(pruneRuntimeDockTickerCache(cacheRef.current, Date.now()));
    }, 250);
    return () => window.clearInterval(timer);
  }, []);

  if (visibleEntries.length <= 0) return null;
  return (
    <div
      className={classNames(
        "pointer-events-none absolute bottom-[calc(100%+0.8rem)] left-1/2 z-20 h-[5.25rem] w-[min(92vw,560px)] -translate-x-1/2 overflow-hidden transition-opacity duration-200 ease-out",
        suppressed ? "invisible opacity-0" : "visible opacity-100"
      )}
      style={{
        WebkitMaskImage: "linear-gradient(to top, #000 0%, #000 72%, rgba(0,0,0,0.78) 84%, transparent 100%)",
        maskImage: "linear-gradient(to top, #000 0%, #000 72%, rgba(0,0,0,0.78) 84%, transparent 100%)",
      }}
      aria-hidden="true"
    >
      <div className="absolute inset-x-0 bottom-0 flex flex-col items-center gap-1 px-1">
        {visibleEntries.map((entry, index) => {
          const slotFromLatest = visibleEntries.length - index - 1;
          const isMessage = entry.kind === "message";
          return (
            <div
              key={entry.id}
              className={classNames(
                "runtime-dock-ticker-entry break-words border shadow-[0_14px_36px_-30px_rgba(15,23,42,0.55)] backdrop-blur-xl transition-opacity duration-500 ease-out motion-reduce:animate-none motion-reduce:transition-none",
                isMessage
                  ? "w-fit min-w-0 max-w-[min(84vw,380px)] rounded-2xl px-3 py-1.5 text-left text-[11px] leading-[1.28] whitespace-pre-wrap [overflow-wrap:anywhere] hyphens-auto"
                  : "w-fit max-w-full rounded-full px-2.5 py-1 text-[11px] leading-[1.15] whitespace-pre-wrap",
                slotFromLatest === 0
                  ? "opacity-100"
                  : slotFromLatest === 1
                    ? "opacity-[0.66]"
                    : slotFromLatest === 2
                      ? "opacity-[0.38]"
                      : "opacity-[0.2]",
                isDark
                  ? "border-white/[0.08] bg-slate-950/70 text-slate-200"
                  : "border-black/[0.08] bg-white/[0.78] text-gray-700"
              )}
            >
              <span className={classNames("font-semibold", isDark ? "text-white" : "text-[rgb(35,36,37)]")}>
                {entry.actorLabel}
              </span>
              <span className={isDark ? "text-slate-500" : "text-gray-400"}>: </span>
              <span>{entry.text}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

function RuntimeDockActorButton({
  groupId,
  item,
  isDark,
  isSmallScreen,
  isInspectorOpen,
  selectedGroupRunning,
  selectedGroupActorsHydrating,
  onOpenInspector,
}: {
  groupId: string;
  item: RuntimeDockItem;
  isDark: boolean;
  isSmallScreen: boolean;
  isInspectorOpen: boolean;
  selectedGroupRunning: boolean;
  selectedGroupActorsHydrating: boolean;
  onOpenInspector: (actorId: string) => void;
}) {
  const { t } = useTranslation(["chat", "actors"]);
  const { isRunning, workingState } = useActorDisplayState({
    groupId,
    actor: item.actor,
    selectedGroupRunning,
    selectedGroupActorsHydrating,
  });
  const ringTone = getRuntimeRingTone(item, isRunning, workingState);
  const ringPresentation = getRuntimeRingPresentation(ringTone, isDark);
  const statusLabel = item.liveWorkCard
    ? getLiveWorkBadgeLabel(item.liveWorkCard, (key, options) => t(`chat:${key}`, options))
    : getRuntimeStatusLabel(isRunning, workingState, (key, options) => t(`actors:${key}`, options));
  const queuedCount = Math.max(0, Number(item.webModelQueuedCount || 0));
  const queuedLabel = queuedCount > 0
    ? t("chat:runtimeDockQueuedForNextTurn", {
        count: queuedCount,
        defaultValue: `${queuedCount} queued for next turn`,
      })
    : "";
  const ringFrameClassName = isSmallScreen
    ? "pointer-events-none absolute left-1/2 top-1/2 h-[35px] w-[35px] -translate-x-1/2 -translate-y-1/2"
    : "pointer-events-none absolute left-1/2 top-1/2 h-[39px] w-[39px] -translate-x-1/2 -translate-y-1/2";

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
            : "text-[rgb(35,36,37)] [text-shadow:0_1px_7px_rgba(255,255,255,0.9)]"
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
            ? classNames(
                "scale-[1.04] shadow-[0_18px_40px_-28px_rgba(62,80,103,0.32)]"
              )
            : "hover:scale-[1.02]",
        )}
        aria-label={item.runner === "headless"
          ? t("chat:runtimeDockOpenLiveWork", {
              name: item.actorLabel,
              defaultValue: `Open live work for ${item.actorLabel}`,
            })
          : t("chat:runtimeDockOpenTerminal", {
              name: item.actorLabel,
              defaultValue: `Open terminal for ${item.actorLabel}`,
            })}
        aria-describedby={`runtime-dock-status-${item.actorId}`}
      >
        <span className={ringFrameClassName}>
          <span className={classNames("pointer-events-none", ringPresentation.ringClassName)} style={ringPresentation.ringStyle} />
          {ringPresentation.customRing ? ringPresentation.customRing : null}
        </span>

        <ActorAvatar
            avatarUrl={item.actor.avatar_url || undefined}
            runtime={item.runtime}
            title={item.actorLabel}
            isDark={isDark}
            sizeClassName={isSmallScreen ? "h-[33px] w-[33px]" : "h-[37px] w-[37px]"}
            className={classNames(
              "relative z-10 border-transparent shadow-[0_18px_34px_-22px_rgba(15,23,42,0.68)]",
              ringPresentation.avatarClassName,
              item.runner === "headless"
                ? isDark
                  ? "bg-slate-900"
                  : "bg-slate-50"
                : undefined
            )}
          accentRingClassName={isInspectorOpen ? (isDark ? "ring-white/10" : "ring-black/10") : null}
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

      </button>
      <span id={`runtime-dock-status-${item.actorId}`} className="sr-only">
        {item.actorLabel} · {item.runtime} · {statusLabel}{queuedLabel ? ` · ${queuedLabel}` : ""}
      </span>
    </div>
  );
}

export interface RuntimeDockProps {
  groupId: string;
  runtimeActors: Actor[];
  liveWorkCards: LiveWorkCard[];
  activeRuntimeActorId?: string;
  isDark: boolean;
  isSmallScreen: boolean;
  readOnly?: boolean;
  selectedGroupRunning: boolean;
  selectedGroupActorsHydrating: boolean;
  onAddAgent?: () => void;
  onOpenRuntimeActor: (actorId: string) => void;
}

export function RuntimeDock({
  groupId,
  runtimeActors,
  liveWorkCards,
  activeRuntimeActorId,
  isDark,
  isSmallScreen,
  readOnly,
  selectedGroupRunning,
  selectedGroupActorsHydrating,
  onAddAgent,
  onOpenRuntimeActor,
}: RuntimeDockProps) {
  const { t } = useTranslation("chat");

  const items = useMemo(() => buildRuntimeDockItems({ actors: runtimeActors, liveWorkCards }), [runtimeActors, liveWorkCards]);
  const tickerEntries = useMemo(() => buildRuntimeDockTickerEntries(items), [items]);

  if (items.length <= 0) return null;

  return (
    <div className="pointer-events-none relative z-30 px-3 pt-2 sm:px-4 sm:pt-2.5">
      <div className="mx-auto flex w-full max-w-[1400px] justify-center">
        <div
          className={classNames(
            "group/runtime-dock pointer-events-auto relative flex justify-center",
            isSmallScreen ? "max-w-[calc(100vw-2.5rem)]" : ""
          )}
        >
          <div
            className={classNames(
              "flex items-end opacity-[0.72] transition-opacity delay-[3000ms] duration-200 ease-out group-hover/runtime-dock:opacity-100 group-hover/runtime-dock:delay-0 group-has-[:focus-visible]/runtime-dock:opacity-100 group-has-[:focus-visible]/runtime-dock:delay-0",
              isSmallScreen ? "max-w-[calc(100vw-2.5rem)] gap-2 overflow-x-auto pb-1 scrollbar-hide" : "gap-2.5",
            )}
          >
            <div className={classNames("relative flex items-end", isSmallScreen ? "gap-2" : "gap-2.5")}>
              <RuntimeDockTicker
                key={groupId}
                entries={tickerEntries}
                isDark={isDark}
                suppressed={Boolean(activeRuntimeActorId)}
              />
              {items.map((item) => (
                <RuntimeDockActorButton
                  key={item.actorId}
                  groupId={groupId}
                  item={item}
                  isDark={isDark}
                  isSmallScreen={isSmallScreen}
                  isInspectorOpen={activeRuntimeActorId === item.actorId}
                  selectedGroupRunning={selectedGroupRunning}
                  selectedGroupActorsHydrating={selectedGroupActorsHydrating}
                  onOpenInspector={onOpenRuntimeActor}
                />
              ))}
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
