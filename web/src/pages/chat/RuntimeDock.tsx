import { memo, useId, useMemo } from "react";
import { useTranslation } from "react-i18next";
import { Mail } from "lucide-react";

import { ActorAvatar } from "../../components/ActorAvatar";
import { PlusIcon } from "../../components/Icons";
import { useActorDisplayState } from "../../hooks/useActorDisplayState";
import type { Actor, HeadlessStreamEvent } from "../../types";
import { classNames } from "../../utils/classNames";
import type { LiveWorkCard } from "./liveWorkCards";
import { RuntimeDockTicker } from "./RuntimeDockTicker";
import { buildRuntimeDockTickerEntries } from "./runtimeDockTickerEntries";
import { buildRuntimeDockItems, type RuntimeDockItem } from "./runtimeDockItems";
import { getRuntimeRingTone, type RuntimeRingTone } from "./runtimeDockRingTone";

type RuntimeRingPresentation = {
  ringClassName: string;
  queuedBadgeClassName: string;
  avatarClassName?: string;
};

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
        ringClassName: isDark ? "text-[#C4B5FD]" : "text-[#A07CFE]",
        queuedBadgeClassName: isDark
          ? "bg-emerald-300/[0.18] text-emerald-50"
          : "bg-emerald-500/[0.14] text-emerald-700",
      };
    case "attention":
      return {
        ringClassName: isDark ? "text-rose-400" : "text-rose-500",
        queuedBadgeClassName: isDark
          ? "bg-rose-300/[0.18] text-rose-50"
          : "bg-rose-500/[0.14] text-rose-700",
      };
    case "idle":
      return {
        ringClassName: isDark ? "text-emerald-300/45" : "text-emerald-500/55",
        queuedBadgeClassName: isDark
          ? "bg-emerald-300/[0.12] text-emerald-50"
          : "bg-emerald-500/[0.10] text-emerald-700",
      };
    case "stopped":
    default:
      return {
        ringClassName: "text-slate-400/50",
        queuedBadgeClassName: isDark
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
  onOpenInspector,
}: {
  groupId: string;
  item: RuntimeDockItem;
  isDark: boolean;
  isSmallScreen: boolean;
  isInspectorOpen: boolean;
  actorStatusProvisional: boolean;
  onOpenInspector: (actorId: string) => void;
}) {
  const { t } = useTranslation(["chat", "actors"]);
  const activityGradientId = useId();
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
  const ringFrameSize = isSmallScreen ? 40 : 44;
  const mailCount = item.unreadCount;
  const mailLabel =
    mailCount > 0
      ? t("chat:runtimeDockUnreadMailNoAge", {
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
          "pointer-events-none absolute -top-[1.4rem] left-1/2 z-30 hidden max-w-[3.75rem] -translate-x-1/2 truncate text-center text-[9px] font-medium leading-[1.2] tracking-[0.01em] opacity-0 transition-opacity delay-[3000ms] duration-150 group-hover/runtime-dock:opacity-100 group-hover/runtime-dock:delay-0 group-has-[:focus-visible]/runtime-dock:opacity-100 group-has-[:focus-visible]/runtime-dock:delay-0 sm:block",
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
          "group relative flex h-[50px] shrink-0 items-center justify-center rounded-full shadow-[0_14px_34px_-30px_rgba(15,23,42,0.52)] transition-all duration-200 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(143,163,187)]/40 focus-visible:ring-offset-0",
          "w-[50px] bg-transparent",
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
        title={[item.actorLabel, statusLabel, mailLabel, queuedLabel].filter(Boolean).join("\n")}
      >
        <svg
          aria-hidden="true"
          viewBox="0 0 44 44"
          width={ringFrameSize}
          height={ringFrameSize}
          fill="none"
          stroke="currentColor"
          strokeWidth="2"
          className={classNames(
            "runtime-dock-ring pointer-events-none absolute transition-colors duration-200",
            ringPresentation.ringClassName,
          )}
        >
          <circle
            cx="22"
            cy="22"
            r="21"
            strokeWidth={ringTone === "active" ? 2 : 1.5}
            opacity={ringTone === "active" ? 0.12 : 1}
          />
          {ringTone === "active" ? (
            <>
              <defs>
                <linearGradient
                  id={activityGradientId}
                  gradientUnits="userSpaceOnUse"
                  x1="43"
                  y1="22"
                  x2="11.5"
                  y2="40.2"
                >
                  <stop stopColor="currentColor" stopOpacity="0" />
                  <stop offset="0.55" stopColor="currentColor" stopOpacity="0.85" />
                  <stop offset="0.85" stopColor="currentColor" />
                  <stop offset="1" stopColor="#FE8FB5" />
                </linearGradient>
              </defs>
              <circle
                className="runtime-dock-ring__activity"
                cx="22"
                cy="22"
                r="21"
                stroke={`url(#${activityGradientId})`}
                strokeDasharray="44 88"
                strokeLinecap="round"
              />
            </>
          ) : null}
        </svg>

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
              "pointer-events-none absolute -right-0.5 -bottom-0.5 z-20 flex h-[17px] min-w-[17px] items-center justify-center rounded-full px-1 text-[9px] font-semibold leading-none shadow-[0_8px_18px_-10px_rgba(15,23,42,0.7)]",
              ringPresentation.queuedBadgeClassName,
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
              "pointer-events-none absolute right-0 -top-1 z-20 flex h-4 items-center gap-0.5 whitespace-nowrap rounded-full border px-1 font-semibold leading-none tabular-nums",
              isSmallScreen ? "text-[9px]" : "text-[10px]",
              isDark
                ? "border-slate-900 bg-amber-950 text-amber-200"
                : "border-white bg-amber-100 text-amber-900",
            )}
            aria-hidden="true"
            title={mailLabel}
          >
            <Mail
              className={isSmallScreen ? "h-[9px] w-[9px]" : "h-[10px] w-[10px]"}
              strokeWidth={2}
            />
            {mailCount > 99 ? "99+" : mailCount}
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
    previous.item.unreadCount === next.item.unreadCount &&
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
                ? "max-w-[calc(100vw-2.5rem)] gap-2 overflow-x-auto px-2 pt-2 pb-3 scrollbar-hide"
                : "gap-2",
            )}
          >
            <div className="relative flex items-end gap-2">
              {items.map((item) => (
                <RuntimeDockActorButton
                  key={item.actorId}
                  groupId={groupId}
                  item={item}
                  isDark={isDark}
                  isSmallScreen={isSmallScreen}
                  isInspectorOpen={activeRuntimeActorId === item.actorId}
                  actorStatusProvisional={actorStatusProvisional}
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
