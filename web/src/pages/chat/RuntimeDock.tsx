import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { useTranslation } from "react-i18next";

import { ActorAvatar } from "../../components/ActorAvatar";
import { PlusIcon } from "../../components/Icons";
import { useActorDisplayState } from "../../hooks/useActorDisplayState";
import type { Actor } from "../../types";
import { classNames } from "../../utils/classNames";
import type { LiveWorkCard } from "./liveWorkCards";
import {
  createRuntimeDockTickerCache,
  pruneRuntimeDockTickerCache,
  upsertRuntimeDockTickerCache,
  type RuntimeDockTickerCache,
} from "./runtimeDockTickerCache";
import { splitRuntimeDockTickerText } from "./runtimeDockTickerText";
import { buildRuntimeDockTickerEntries, type RuntimeDockTickerEntry } from "./runtimeDockTickerEntries";
import { buildRuntimeDockItems, type RuntimeDockItem } from "./runtimeDockItems";
import { getRuntimeRingTone, type RuntimeRingTone } from "./runtimeDockRingTone";

type RuntimeRingPresentation = {
  ringClassName: string;
  ringStyle: CSSProperties;
  haloClassName?: string;
  haloStyle?: CSSProperties;
  secondaryRingClassName?: string;
  secondaryRingStyle?: CSSProperties;
  unreadBadgeClassName: string;
};

const RING_MASK_STYLE: CSSProperties = {
  WebkitMask: "radial-gradient(farthest-side, transparent calc(100% - 3px), #000 calc(100% - 2px))",
  mask: "radial-gradient(farthest-side, transparent calc(100% - 3px), #000 calc(100% - 2px))",
};

function buildMaskedRing(background: string): CSSProperties {
  return {
    ...RING_MASK_STYLE,
    background,
  };
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
    case "queued":
      return {
        ringClassName: "absolute -inset-[2px] rounded-full animate-[spin_10s_linear_infinite] motion-reduce:animate-none",
        ringStyle: buildMaskedRing(
          "conic-gradient(from 90deg, transparent 0deg 40deg, rgba(245,158,11,0.96) 40deg 126deg, transparent 126deg 190deg, rgba(251,191,36,0.72) 190deg 260deg, transparent 260deg 360deg)"
        ),
        haloClassName: classNames(
          "absolute -inset-[4px] rounded-full blur-md",
          isDark ? "bg-amber-300/[0.14]" : "bg-amber-400/[0.18]"
        ),
        unreadBadgeClassName: isDark ? "bg-amber-300/[0.18] text-amber-50" : "bg-amber-500/[0.14] text-amber-700",
      };
    case "active":
      return {
        ringClassName: "absolute -inset-[2px] rounded-full animate-[spin_5s_linear_infinite] motion-reduce:animate-none",
        ringStyle: buildMaskedRing(
          "conic-gradient(from 120deg, rgba(16,185,129,0.18) 0deg, rgba(16,185,129,0.98) 148deg, rgba(45,212,191,0.85) 246deg, rgba(16,185,129,0.22) 360deg)"
        ),
        haloClassName: classNames(
          "absolute -inset-[5px] rounded-full blur-md animate-pulse motion-reduce:animate-none",
          isDark ? "bg-emerald-300/[0.18]" : "bg-emerald-400/20"
        ),
        unreadBadgeClassName: isDark ? "bg-emerald-300/[0.18] text-emerald-50" : "bg-emerald-500/[0.14] text-emerald-700",
      };
    case "attention":
      return {
        ringClassName: "absolute -inset-[2px] rounded-full",
        ringStyle: buildMaskedRing(
          "conic-gradient(from 190deg, transparent 0deg 38deg, rgba(248,113,113,0.96) 38deg 122deg, transparent 122deg 196deg, rgba(239,68,68,0.92) 196deg 258deg, transparent 258deg 360deg)"
        ),
        haloClassName: classNames(
          "absolute -inset-[4px] rounded-full blur-md animate-pulse motion-reduce:animate-none",
          isDark ? "bg-rose-300/[0.14]" : "bg-rose-400/[0.16]"
        ),
        unreadBadgeClassName: isDark ? "bg-rose-300/[0.18] text-rose-50" : "bg-rose-500/[0.14] text-rose-700",
      };
    case "ready":
      return {
        ringClassName: "absolute -inset-[2px] rounded-full",
        ringStyle: buildMaskedRing(
          "conic-gradient(from 180deg, rgba(34,197,94,0.16) 0deg, rgba(34,197,94,0.84) 180deg, rgba(34,197,94,0.18) 360deg)"
        ),
        unreadBadgeClassName: isDark ? "bg-white/10 text-slate-100" : "bg-black/[0.08] text-gray-800",
      };
    case "stopped":
    default:
      return {
        ringClassName: "absolute -inset-[2px] rounded-full",
        ringStyle: buildMaskedRing(
          isDark
            ? "conic-gradient(from 180deg, rgba(148,163,184,0.28) 0deg, rgba(148,163,184,0.56) 180deg, rgba(148,163,184,0.22) 360deg)"
            : "conic-gradient(from 180deg, rgba(148,163,184,0.18) 0deg, rgba(148,163,184,0.48) 180deg, rgba(148,163,184,0.16) 360deg)"
        ),
        unreadBadgeClassName: isDark ? "bg-white/10 text-slate-100" : "bg-black/[0.08] text-gray-800",
      };
  }
}

function isLiveWorkCardActive(card: LiveWorkCard | null | undefined): boolean {
  if (!card) return false;
  return card.phase === "pending" || card.phase === "streaming";
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
        "pointer-events-none absolute bottom-[calc(100%+1.5rem)] left-1/2 z-20 h-[5.75rem] w-[min(92vw,560px)] -translate-x-1/2 overflow-hidden transition-opacity duration-200 ease-out",
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
          const textLines = isMessage ? splitRuntimeDockTickerText(entry.text) : [entry.text];
          const [firstLine, ...restLines] = textLines;
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
                  ? "border-cyan-300/[0.14] bg-slate-950/70 text-slate-200"
                  : "border-cyan-500/[0.14] bg-white/[0.78] text-gray-700"
              )}
            >
              {isMessage ? (
                <>
                  <div className="block">
                    <span className={classNames("font-semibold", isDark ? "text-cyan-100" : "text-cyan-700")}>
                      {entry.actorLabel}
                    </span>
                    <span className={isDark ? "text-slate-500" : "text-gray-400"}>: </span>
                    <span>{firstLine}</span>
                  </div>
                  <div className={classNames(restLines.length > 0 ? "mt-1 space-y-0.5" : "hidden")}>
                    {restLines.map((line, lineIndex) => (
                      <div key={`${entry.id}:line:${lineIndex}`} className="block">
                        {line}
                      </div>
                    ))}
                  </div>
                </>
              ) : (
                <>
                  <span className={classNames("font-semibold", isDark ? "text-cyan-100" : "text-cyan-700")}>
                    {entry.actorLabel}
                  </span>
                  <span className={isDark ? "text-slate-500" : "text-gray-400"}>: </span>
                  <span>{entry.text}</span>
                </>
              )}
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
  const hasLiveIndicator = isLiveWorkCardActive(item.liveWorkCard);

  const handleOpenInspector = () => {
    onOpenInspector(item.actorId);
  };

  return (
    <div className="relative flex items-end">
      <span
        className={classNames(
          "pointer-events-none absolute -top-[1.05rem] left-1/2 z-30 hidden max-w-[3.75rem] -translate-x-1/2 truncate text-center text-[9px] font-medium leading-none tracking-[0.01em] opacity-0 transition-opacity delay-[3000ms] duration-150 group-hover/runtime-dock:opacity-100 group-hover/runtime-dock:delay-0 group-has-[:focus-visible]/runtime-dock:opacity-100 group-has-[:focus-visible]/runtime-dock:delay-0 sm:block",
          "runtime-dock-actor-label",
          isDark
            ? "text-slate-200 [text-shadow:0_1px_8px_rgba(2,6,23,0.85)]"
            : "text-gray-700 [text-shadow:0_1px_7px_rgba(255,255,255,0.9)]"
        )}
        aria-hidden="true"
      >
        {item.actorLabel}
      </span>
      <button
        type="button"
        onClick={handleOpenInspector}
        className={classNames(
          "group relative flex h-[52px] w-[52px] items-center justify-center rounded-full border shadow-[0_14px_34px_-30px_rgba(15,23,42,0.52)] backdrop-blur-xl transition-all duration-200 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-500/[0.55] focus-visible:ring-offset-0",
          item.runner === "headless"
            ? isDark
              ? "border-cyan-300/10 bg-slate-950/70 hover:border-cyan-300/25 hover:bg-slate-950/90"
              : "border-cyan-500/10 bg-white/75 hover:border-cyan-500/25 hover:bg-white/95"
            : isDark
              ? "border-white/10 bg-slate-950/60 hover:border-white/20 hover:bg-slate-950/90"
              : "border-black/10 bg-white/70 hover:border-black/20 hover:bg-white/95",
          isInspectorOpen
            ? classNames(
                "scale-[1.05] shadow-[0_18px_40px_-28px_rgba(14,165,233,0.65)]",
                isDark ? "bg-slate-950/95" : "bg-white/95"
              )
            : "hover:scale-[1.03]",
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
        {ringPresentation.haloClassName ? (
          <span className={classNames("pointer-events-none", ringPresentation.haloClassName)} style={ringPresentation.haloStyle} />
        ) : null}
        <span className={classNames("pointer-events-none", ringPresentation.ringClassName)} style={ringPresentation.ringStyle} />
        {ringPresentation.secondaryRingClassName ? (
          <span className={classNames("pointer-events-none", ringPresentation.secondaryRingClassName)} style={ringPresentation.secondaryRingStyle} />
        ) : null}

        <ActorAvatar
          avatarUrl={item.actor.avatar_url || undefined}
          runtime={item.runtime}
          title={item.actorLabel}
          isDark={isDark}
          sizeClassName={isSmallScreen ? "h-9 w-9" : "h-10 w-10"}
          className={classNames(
            "relative z-10 shadow-[0_18px_34px_-22px_rgba(15,23,42,0.68)]",
            item.runner === "headless"
              ? isDark
                ? "bg-slate-900"
                : "bg-slate-50"
              : undefined
          )}
          accentRingClassName={isInspectorOpen ? (isDark ? "ring-white/10" : "ring-black/10") : null}
        />

        {item.unreadCount > 0 ? (
          <span className={classNames("absolute -right-1 -top-1 z-20 inline-flex h-5 min-w-[20px] items-center justify-center rounded-full px-1.5 text-[10px] font-bold shadow-sm", ringPresentation.unreadBadgeClassName)}>
            {item.unreadCount}
          </span>
        ) : null}

        {hasLiveIndicator ? (
          <span className={classNames(
            "absolute -bottom-1 left-1/2 z-20 inline-flex -translate-x-1/2 whitespace-nowrap items-center gap-1 rounded-full border px-1.5 py-0.5 text-[9px] font-semibold uppercase tracking-[0.12em] shadow-sm",
            isDark ? "border-cyan-300/20 bg-slate-950/90 text-cyan-100" : "border-cyan-500/20 bg-white text-cyan-700"
          )}>
            <span className="h-1.5 w-1.5 rounded-full bg-current animate-pulse motion-reduce:animate-none" />
            {t("chat:liveWorkDockIndicator", { defaultValue: "Live" })}
          </span>
        ) : null}
      </button>
      <span id={`runtime-dock-status-${item.actorId}`} className="sr-only">
        {item.actorLabel} · {item.runtime} · {statusLabel}
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
    <div className="pointer-events-none relative z-30 px-3 sm:px-4">
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
              isSmallScreen ? "max-w-[calc(100vw-2.5rem)] gap-2.5 overflow-x-auto pb-1 scrollbar-hide" : "gap-3.5",
            )}
          >
            <div className={classNames("relative flex items-end", isSmallScreen ? "gap-2.5" : "gap-3.5")}>
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
                  "relative flex h-12 w-12 flex-shrink-0 items-center justify-center rounded-full border shadow-[0_12px_30px_-28px_rgba(15,23,42,0.54)] transition-all duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-cyan-500/[0.45]",
                  isDark
                    ? "border-white/10 bg-slate-950/60 text-slate-100 hover:border-white/20 hover:bg-slate-950/90"
                    : "border-black/10 bg-white/70 text-gray-900 hover:border-black/20 hover:bg-white/95",
                )}
                aria-label={t("addAgent", { defaultValue: "Add an agent" })}
                title={t("addAgent", { defaultValue: "Add an agent" })}
              >
                <PlusIcon size={18} />
              </button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}
