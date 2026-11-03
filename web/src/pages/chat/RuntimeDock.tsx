import {
  memo,
  useCallback,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import { useTranslation } from "react-i18next";
import { Hourglass, Mail } from "lucide-react";

import { ActorAvatar } from "../../components/ActorAvatar";
import { HoverTooltip } from "../../components/HoverTooltip";
import { Popover, PopoverContent, PopoverTrigger } from "../../components/ui/popover";
import { LcdIcon, PlusIcon } from "../../components/Icons";
import { useActorDisplayState } from "../../hooks/useActorDisplayState";
import { useGroupStore } from "../../stores";
import type { Actor, HeadlessStreamEvent } from "../../types";
import { classNames } from "../../utils/classNames";
import { hasManagedRuntimeOutput } from "../../utils/headlessRuntimeSupport";
import type { LiveWorkCard } from "./liveWorkCards";
import { RuntimeDockTicker } from "./RuntimeDockTicker";
import { buildRuntimeDockTickerEntries } from "./runtimeDockTickerEntries";
import { buildRuntimeDockItems, type RuntimeDockItem } from "./runtimeDockItems";
import { getRuntimeRingTone, type RuntimeRingTone } from "./runtimeDockRingTone";
import { useUiStylePrefs, type LcdMotion } from "../../utils/uiStyle";

function lcdRingDelay(): number {
  const value = getComputedStyle(document.documentElement)
    .getPropertyValue("--lcd-ring-lap")
    .trim();
  const parsed = Number.parseFloat(value);
  const lap =
    Number.isFinite(parsed) && parsed > 0 ? parsed * (value.endsWith("ms") ? 1 : 1000) : 2000;
  const timelineTime = document.timeline?.currentTime;
  const now = typeof timelineTime === "number" ? timelineTime : performance.now();
  return -(now % lap);
}

/** Align a newly visible ring without resetting its phase on ordinary renders. */
function useLcdRingDelay(moving: boolean, motionPreference: LcdMotion): number {
  const [delay, setDelay] = useState(0);
  useLayoutEffect(() => {
    if (!moving) return;
    const align = () => setDelay(lcdRingDelay());
    align();
    // CSS can recreate the animation when the OS motion preference changes,
    // even though the working and visibility state stayed the same.
    const reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)");
    reduced?.addEventListener?.("change", align);
    return () => reduced?.removeEventListener?.("change", align);
  }, [moving, motionPreference]);
  return delay;
}

function useLcdTileTransitions({
  groupId,
  actorId,
  tone,
  working,
  managed,
  liveWorkPhase,
  liveWorkId,
  hidden,
  provisional,
  onHiddenAttention,
}: {
  groupId: string;
  actorId: string;
  tone: RuntimeRingTone;
  working: boolean;
  managed: boolean;
  liveWorkPhase: LiveWorkCard["phase"] | undefined;
  liveWorkId: string;
  hidden: boolean;
  provisional: boolean;
  onHiddenAttention?: (actorId: string) => void;
}) {
  const previous = useRef({
    groupId,
    actorId,
    tone,
    working,
    liveWorkPhase,
    liveWorkId,
    provisional,
  });
  const [attentionFlash, setAttentionFlash] = useState(false);
  const [flourish, setFlourish] = useState(false);

  useLayoutEffect(() => {
    const before = previous.current;
    previous.current = { groupId, actorId, tone, working, liveWorkPhase, liveWorkId, provisional };
    // Hydration or a new group establishes a baseline rather than announcing an old
    // state as a new transition.
    if (
      before.groupId !== groupId ||
      before.actorId !== actorId ||
      before.provisional ||
      provisional
    ) {
      setAttentionFlash(false);
      setFlourish(false);
      return;
    }
    if (tone === "attention" && before.tone !== "attention") {
      if (hidden) onHiddenAttention?.(actorId);
      else setAttentionFlash(true);
    } else if (tone !== "attention" || hidden) {
      setAttentionFlash(false);
    }
    const becameIdle = managed && before.working && !working && tone === "idle";
    const finishedStream =
      !!liveWorkId &&
      before.liveWorkId === liveWorkId &&
      before.liveWorkPhase === "streaming" &&
      liveWorkPhase === "completed";
    // An unmanaged PTY's output signal can expire after five quiet seconds;
    // expiry alone is not evidence that its work finished.
    if ((becameIdle || finishedStream) && tone === "idle" && !hidden) setFlourish(true);
    else if (tone !== "idle" || hidden) setFlourish(false);
  }, [
    actorId,
    groupId,
    hidden,
    liveWorkId,
    liveWorkPhase,
    managed,
    onHiddenAttention,
    provisional,
    tone,
    working,
  ]);

  useEffect(() => {
    if (!attentionFlash) return;
    const timer = window.setTimeout(() => setAttentionFlash(false), 1500);
    return () => window.clearTimeout(timer);
  }, [attentionFlash]);
  useEffect(() => {
    if (!flourish) return;
    const timer = window.setTimeout(() => setFlourish(false), 250);
    return () => window.clearTimeout(timer);
  }, [flourish]);
  return { attentionFlash, flourish };
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
  if (workingState === "error") return t("error", { defaultValue: "Error" });
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

function RuntimeDockActorButtonView({
  groupId,
  item,
  isDark,
  isSmallScreen,
  isInspectorOpen,
  actorStatusProvisional,
  onOpenInspector,
  variant = "ring",
  hidden = false,
  motionVisible = true,
  showName = false,
  onToneChange,
  onHiddenAttention,
}: {
  groupId: string;
  item: RuntimeDockItem;
  isDark: boolean;
  isSmallScreen: boolean;
  isInspectorOpen: boolean;
  actorStatusProvisional: boolean;
  onOpenInspector: (actorId: string) => void;
  /** "chip" is the LCD member row: a tile whose line and dimming show the state. */
  variant?: "ring" | "chip";
  /** A chip clipped by the row's overflow: kept for layout, out of reach until shown. */
  hidden?: boolean;
  /** The whole dock may be hidden while the user is typing or viewing another surface. */
  motionVisible?: boolean;
  /** In the "+N" list the name sits beside the tile. */
  showName?: boolean;
  onToneChange?: (actorId: string, tone: RuntimeRingTone | null) => void;
  onHiddenAttention?: (actorId: string) => void;
}) {
  const { t } = useTranslation(["chat", "actors", "common"]);
  const chipStatusId = useId();
  const motionPreference = useUiStylePrefs().lcdMotion;
  const { isRunning, workingState } = useActorDisplayState({
    groupId,
    actor: item.actor,
    actorStatusProvisional,
  });
  const ringTone = getRuntimeRingTone(item, isRunning, workingState);
  // The existing active tone also covers queued tasks. A moving ring must
  // describe work actually in progress, not merely a pending item.
  const working =
    ringTone === "active" &&
    (workingState === "working" || item.liveWorkCard?.phase === "streaming");
  const moving = working && !hidden && motionVisible;
  const delay = useLcdRingDelay(moving, motionPreference);
  const { attentionFlash, flourish } = useLcdTileTransitions({
    groupId,
    actorId: item.actorId,
    tone: ringTone,
    working,
    managed: hasManagedRuntimeOutput(item.actor),
    liveWorkPhase: item.liveWorkCard?.phase,
    liveWorkId: item.liveWorkCard?.pendingEventId || item.liveWorkCard?.streamId || "",
    hidden,
    provisional: actorStatusProvisional,
    onHiddenAttention,
  });
  // Report the same resolved state that this tile displays. Terminal signals
  // can change independently of the actor object passed to the parent.
  useLayoutEffect(() => {
    onToneChange?.(item.actorId, ringTone);
  }, [item.actorId, onToneChange, ringTone]);
  useLayoutEffect(() => () => onToneChange?.(item.actorId, null), [item.actorId, onToneChange]);
  const useRuntimeStatus = hasManagedRuntimeOutput(item.actor) || workingState === "error";
  const statusLabel =
    item.liveWorkCard && !useRuntimeStatus
      ? getLiveWorkBadgeLabel(item.liveWorkCard, (key, options) => t(`chat:${key}`, options))
      : getRuntimeStatusLabel(isRunning, workingState, (key, options) =>
          t(key === "error" ? "common:error" : `actors:${key}`, options),
        );
  const queuedCount = Math.max(0, Number(item.webModelQueuedCount || 0));
  const queuedLabel =
    queuedCount > 0
      ? t("chat:runtimeDockQueuedForNextTurn", {
          count: queuedCount,
          defaultValue: `${queuedCount} queued for next turn`,
        })
      : "";
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
  const lcdRing =
    ringTone === "active" || flourish ? (
      <span
        className="lcd-member-ring"
        data-ring={moving ? "moving" : "still"}
        style={{ "--lcd-ring-delay": `${delay}ms` } as CSSProperties}
        aria-hidden="true"
      >
        <i />
        <i />
        <i />
        <i />
        <i />
        <i />
        <i />
      </span>
    ) : null;

  if (variant === "chip") {
    // The tile itself shows the state (dimmed when stopped, its line for busy, idle
    // or needing attention); words live in the tooltip and names.
    const tooltip = [item.actorLabel, statusLabel, queuedLabel, mailLabel]
      .filter(Boolean)
      .join(" \u00B7 ");
    const description = [item.actorLabel, item.runtime, statusLabel, queuedLabel, mailLabel]
      .filter(Boolean)
      .join(" \u00B7 ");
    return (
      <div
        className="lcd-member-cell"
        data-lcd-member-chip
        data-lcd-member-id={item.actorId}
        inert={hidden || undefined}
        aria-hidden={hidden || undefined}
      >
        <HoverTooltip label={tooltip} openOnFocus>
          {(getReferenceProps, setReference) => (
            <button
              ref={setReference}
              type="button"
              className="lcd-member"
              data-active={isInspectorOpen || undefined}
              data-tone={ringTone}
              data-listed={showName || undefined}
              data-attention-flash={attentionFlash || undefined}
              data-flourish={flourish || undefined}
              {...getReferenceProps({
                onClick: handleOpenInspector,
                "aria-label":
                  item.runner === "headless"
                    ? t("chat:runtimeDockOpenLiveWork", {
                        name: item.actorLabel,
                        defaultValue: `Open live work for ${item.actorLabel}`,
                      })
                    : t("chat:runtimeDockOpenTerminal", {
                        name: item.actorLabel,
                        defaultValue: `Open terminal for ${item.actorLabel}`,
                      }),
                "aria-describedby": chipStatusId,
              })}
            >
              <span className="lcd-member-tile">
                <ActorAvatar
                  avatarUrl={item.actor.avatar_url || undefined}
                  runtime={item.runtime}
                  title={item.actorLabel}
                  isDark={isDark}
                  sizeClassName="h-8 w-8"
                  textClassName="text-[0.8125rem]"
                  className="lcd-tile"
                />
                {lcdRing}
                {mailCount > 0 ? (
                  <span className="lcd-member-mark" data-mark="mail" aria-hidden="true">
                    {mailCount > 99 ? "99+" : mailCount}
                  </span>
                ) : null}
                {queuedCount > 0 ? (
                  <span className="lcd-member-mark" data-mark="queue" aria-hidden="true">
                    {queuedCount > 99 ? "99+" : queuedCount}
                  </span>
                ) : null}
              </span>
              {showName ? <span className="lcd-member-name">{item.actorLabel}</span> : null}
            </button>
          )}
        </HoverTooltip>
        <span id={chipStatusId} className="sr-only">
          {description}
        </span>
      </div>
    );
  }

  const dockAvatar = (
    <ActorAvatar
      avatarUrl={item.actor.avatar_url || undefined}
      runtime={item.runtime}
      title={item.actorLabel}
      isDark={isDark}
      sizeClassName={isSmallScreen ? "h-[33px] w-[33px]" : "h-[37px] w-[37px]"}
      className={classNames(
        "relative z-10 border-transparent shadow-[0_18px_34px_-22px_rgba(15,23,42,0.68)]",
        "lcd-tile",
        item.runner === "headless" ? (isDark ? "bg-slate-900" : "bg-slate-50") : undefined,
      )}
      accentRingClassName={isInspectorOpen ? (isDark ? "ring-white/10" : "ring-black/10") : null}
    />
  );

  return (
    <div className="relative flex items-end">
      <span
        // Shown at once on hover or focus, and held for 3 s after; no fade.
        className="pointer-events-none absolute -top-[1.4rem] left-1/2 z-30 hidden max-w-[3.75rem] -translate-x-1/2 truncate text-center text-[9px] font-medium leading-[1.2] tracking-[0.01em] text-[var(--color-text-primary)] opacity-0 transition-opacity delay-[3000ms] duration-0 group-hover/runtime-dock:opacity-100 group-hover/runtime-dock:delay-0 group-has-[:focus-visible]/runtime-dock:opacity-100 group-has-[:focus-visible]/runtime-dock:delay-0 sm:block"
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
        data-lcd-dock-button
        data-tone={ringTone}
        data-active={isInspectorOpen || undefined}
        data-attention-flash={attentionFlash || undefined}
        data-flourish={flourish || undefined}
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
        <span className="lcd-member-tile">
          {dockAvatar}
          {lcdRing}
        </span>
        {queuedCount > 0 ? (
          <span
            className="pointer-events-none absolute -right-0.5 -bottom-0.5 z-20 flex h-[17px] min-w-[17px] items-center justify-center px-1 text-[9px] font-semibold leading-none"
            aria-hidden="true"
            title={queuedLabel}
            data-dock-badge="queue"
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
            data-dock-badge="mail"
          >
            <LcdIcon
              icon={Mail}
              glyph="mail"
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
    previous.onOpenInspector === next.onOpenInspector &&
    previous.variant === next.variant &&
    previous.hidden === next.hidden &&
    previous.motionVisible === next.motionVisible &&
    previous.showName === next.showName &&
    previous.onToneChange === next.onToneChange &&
    previous.onHiddenAttention === next.onHiddenAttention,
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
  /** "row": the LCD member strip above the composer (desktop). */
  placement?: "dock" | "row";
  /** False while the caller has hidden the entire dock. */
  motionVisible?: boolean;
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
  placement = "dock",
  motionVisible = true,
}: RuntimeDockProps) {
  const { t } = useTranslation("chat");
  const storeRuntimeEvents = useGroupStore(
    (state) => state.chatByGroup?.[String(groupId || "").trim()]?.rawHeadlessEventsByActorId,
  );
  const effectiveRuntimeEvents = runtimeEvents ?? storeRuntimeEvents;

  const items = useMemo(
    () => buildRuntimeDockItems({ actors: runtimeActors, liveWorkCards }),
    [runtimeActors, liveWorkCards],
  );
  const tickerEntries = useMemo(
    () => buildRuntimeDockTickerEntries(items, effectiveRuntimeEvents),
    [items, effectiveRuntimeEvents],
  );

  if (items.length <= 0) return null;

  if (placement === "row") {
    return (
      <RuntimeDockMemberRow
        key={groupId}
        groupId={groupId}
        items={items}
        activeRuntimeActorId={activeRuntimeActorId}
        isDark={isDark}
        readOnly={readOnly}
        motionVisible={motionVisible}
        actorStatusProvisional={actorStatusProvisional}
        onAddAgent={onAddAgent}
        onOpenRuntimeActor={onOpenRuntimeActor}
      />
    );
  }

  const addButton =
    !readOnly && onAddAgent ? (
      <button
        type="button"
        onClick={onAddAgent}
        className={classNames(
          "group/add-agent relative flex h-[50px] w-[50px] flex-shrink-0 items-center justify-center rounded-full shadow-[0_14px_34px_-30px_rgba(15,23,42,0.52)] transition-all duration-200 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[rgb(143,163,187)]/35 active:scale-[0.97]",
          "bg-transparent hover:scale-[1.02]",
          isSmallScreen && "mb-1",
        )}
        aria-label={t("addAgent", { defaultValue: "Add an agent" })}
        title={t("addAgent", { defaultValue: "Add an agent" })}
        data-lcd-dock-add
      >
        <span
          aria-hidden="true"
          className={classNames(
            "relative z-[1] flex items-center justify-center rounded-full border shadow-[0_18px_34px_-22px_rgba(15,23,42,0.4)] transition-[transform,box-shadow,border-color,background-color,color] duration-200",
            // Add takes the size of the members' tiles.
            isSmallScreen ? "h-[33px] w-[33px]" : "h-[37px] w-[37px]",
            isDark
              ? "border-white/10 bg-slate-900 text-slate-100 group-hover/add-agent:border-white/16 group-hover/add-agent:bg-slate-950"
              : "border-black/10 bg-white text-[rgb(35,36,37)] group-hover/add-agent:border-black/14 group-hover/add-agent:bg-white/96",
          )}
        >
          <PlusIcon size={18} strokeWidth={2.1} />
        </span>
      </button>
    ) : null;

  return (
    <div className="pointer-events-none relative z-30 px-3 md:px-4 md:pt-2.5">
      <div className="mx-auto flex w-full max-w-[1400px] justify-center">
        <div
          className={classNames(
            "group/runtime-dock pointer-events-auto relative flex justify-center",
            isSmallScreen ? "w-full max-w-[calc(100vw-2.5rem)] flex-col items-center" : "",
          )}
        >
          <RuntimeDockTicker
            key={groupId}
            groupId={groupId}
            entries={tickerEntries}
            suppressed={Boolean(activeRuntimeActorId)}
          />
          <div className="flex min-w-0 max-w-full items-end gap-2">
            <div
              className={classNames(
                "flex min-w-0 items-end gap-2",
                isSmallScreen ? "overflow-x-auto px-2 py-1 scrollbar-hide" : "",
              )}
              data-runtime-dock-strip
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
                    motionVisible={motionVisible}
                    onOpenInspector={onOpenRuntimeActor}
                  />
                ))}
              </div>
              {!isSmallScreen ? addButton : null}
            </div>
            {/* On phones only the members scroll; Add stays reachable. */}
            {isSmallScreen ? addButton : null}
          </div>
        </div>
      </div>
    </div>
  );
}

/**
 * Desktop messages view: members in one row. Chips that do not fit wrap onto a
 * clipped second line; they become unreachable there and move into "+N",
 * which still says whether any of them has unread Mail or queued work.
 */
function RuntimeDockMemberRow({
  groupId,
  items,
  activeRuntimeActorId,
  isDark,
  readOnly,
  actorStatusProvisional,
  motionVisible,
  onAddAgent,
  onOpenRuntimeActor,
}: {
  groupId: string;
  items: RuntimeDockItem[];
  activeRuntimeActorId?: string;
  isDark: boolean;
  readOnly?: boolean;
  actorStatusProvisional: boolean;
  motionVisible: boolean;
  onAddAgent?: () => void;
  onOpenRuntimeActor: (actorId: string) => void;
}) {
  const { t } = useTranslation("chat");
  const rowRef = useRef<HTMLDivElement | null>(null);
  const moreRef = useRef<HTMLButtonElement | null>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const foldedFocusRef = useRef<HTMLElement | null>(null);
  const expandedFocusRef = useRef<{ previousFocus: HTMLElement; actorId?: string } | null>(null);
  const preventCloseAutoFocusRef = useRef(false);
  const [hiddenIds, setHiddenIds] = useState<string[]>([]);
  const [tones, setTones] = useState<Record<string, RuntimeRingTone>>({});
  const [moreOpen, setMoreOpen] = useState(false);
  const [moreAttentionFlash, setMoreAttentionFlash] = useState(false);

  const reportHiddenAttention = useCallback(() => setMoreAttentionFlash(true), []);
  useEffect(() => {
    if (!moreAttentionFlash) return;
    const timer = window.setTimeout(() => setMoreAttentionFlash(false), 1500);
    return () => window.clearTimeout(timer);
  }, [moreAttentionFlash]);

  const reportTone = useCallback((actorId: string, tone: RuntimeRingTone | null) => {
    setTones((previous) => {
      if (tone === null) {
        if (!(actorId in previous)) return previous;
        const next = { ...previous };
        delete next[actorId];
        return next;
      }
      return previous[actorId] === tone ? previous : { ...previous, [actorId]: tone };
    });
  }, []);

  const measure = useCallback(() => {
    const row = rowRef.current;
    if (!row) return;
    const cells = Array.from(row.querySelectorAll<HTMLElement>("[data-lcd-member-chip]"));
    const firstTop = cells[0]?.offsetTop ?? 0;
    const rowBounds = row.getBoundingClientRect();
    let next = cells
      .map((cell, index) => {
        const buttonBounds = cell.querySelector("button")?.getBoundingClientRect();
        // Even the first cell can be clipped when the row is narrower than one
        // key. In that case it belongs in +N as much as a wrapped cell does.
        const clipped =
          buttonBounds !== undefined &&
          buttonBounds.width > 0 &&
          (buttonBounds.left < rowBounds.left - 0.5 || buttonBounds.right > rowBounds.right + 0.5);
        return cell.offsetTop > firstTop || clipped ? items[index]?.actorId : undefined;
      })
      .filter((id): id is string => Boolean(id));
    // A visible +N consumes row space. If all tiles fit after giving that
    // space back, remove +N instead of keeping one member hidden indefinitely.
    const more = moreRef.current;
    if (next.length > 0 && more && cells.length > 0) {
      const widths = cells.map((cell) =>
        Math.max(
          cell.getBoundingClientRect().width,
          cell.querySelector("button")?.getBoundingClientRect().width ?? 0,
        ),
      );
      const rowGap = Number.parseFloat(getComputedStyle(row).columnGap) || 0;
      const groupGap = Number.parseFloat(getComputedStyle(row.parentElement!).columnGap) || 0;
      const naturalWidth =
        widths.reduce((total, width) => total + width, 0) + rowGap * (cells.length - 1);
      const availableWithoutMore =
        row.getBoundingClientRect().width + more.getBoundingClientRect().width + groupGap;
      if (widths.every((width) => width > 0) && naturalWidth <= availableWithoutMore) {
        next = [];
      }
    }
    const activeElement = document.activeElement;
    const focusInPopover =
      activeElement instanceof HTMLElement && popoverRef.current?.contains(activeElement);
    const focusedPopoverActor = focusInPopover
      ? activeElement.closest<HTMLElement>("[data-lcd-member-id]")?.dataset.lcdMemberId
      : undefined;
    const focusedPopoverActorIsRevealed =
      focusedPopoverActor !== undefined &&
      !next.includes(focusedPopoverActor) &&
      cells.some((cell) => cell.dataset.lcdMemberId === focusedPopoverActor);
    if (more && (next.length === 0 || focusedPopoverActorIsRevealed)) {
      if (activeElement instanceof HTMLElement && (activeElement === more || focusInPopover)) {
        const actorId =
          focusedPopoverActor ??
          cells.find((cell) => cell.hasAttribute("inert"))?.dataset.lcdMemberId;
        expandedFocusRef.current = { previousFocus: activeElement, actorId };
      }
      // The trigger and its popover are about to unmount. Suppress Radix's
      // return to that removed trigger; focus the restored member below.
      if (popoverRef.current) preventCloseAutoFocusRef.current = true;
      setMoreOpen(false);
    }
    if (
      activeElement instanceof HTMLElement &&
      cells.some(
        (cell, index) => next.includes(items[index]?.actorId) && cell.contains(activeElement),
      )
    ) {
      foldedFocusRef.current = activeElement;
    }
    setHiddenIds((previous) =>
      previous.length === next.length && previous.every((id, index) => id === next[index])
        ? previous
        : next,
    );
  }, [items]);

  useLayoutEffect(() => {
    measure();
    const row = rowRef.current;
    if (!row || typeof ResizeObserver === "undefined") return;
    // Tiles have a fixed size, so only the row's width decides what fits.
    const observer = new ResizeObserver(() => measure());
    observer.observe(row);
    return () => observer.disconnect();
  }, [measure]);

  useLayoutEffect(() => {
    const previousFocus = foldedFocusRef.current;
    foldedFocusRef.current = null;
    if (
      previousFocus &&
      (document.activeElement === previousFocus || document.activeElement === document.body)
    ) {
      moreRef.current?.focus({ preventScroll: true });
    }
    const expanded = expandedFocusRef.current;
    expandedFocusRef.current = null;
    if (
      expanded &&
      (document.activeElement === expanded.previousFocus ||
        document.activeElement === document.body)
    ) {
      const cells = Array.from(
        rowRef.current?.querySelectorAll<HTMLElement>("[data-lcd-member-id]") ?? [],
      );
      const target =
        cells.find((cell) => cell.dataset.lcdMemberId === expanded.actorId) ?? cells[0];
      const button = target?.querySelector<HTMLButtonElement>("button");
      (
        button ?? rowRef.current?.parentElement?.querySelector<HTMLButtonElement>(".lcd-member-add")
      )?.focus({ preventScroll: true });
    }
  }, [hiddenIds]);

  const hidden = new Set(hiddenIds);
  const hiddenItems = items.filter((item) => hidden.has(item.actorId));
  const hiddenWithMail = hiddenItems.filter((item) => item.unreadCount > 0).length;
  const hiddenWithQueued = hiddenItems.filter(
    (item) => Number(item.webModelQueuedCount || 0) > 0,
  ).length;
  const hiddenWithAttention = hiddenItems.filter(
    (item) => tones[item.actorId] === "attention",
  ).length;
  useEffect(() => {
    if (hiddenWithAttention === 0) setMoreAttentionFlash(false);
  }, [hiddenWithAttention]);
  const moreLabel = [
    t("runtimeDockMoreMembers", { count: hiddenItems.length }),
    hiddenWithMail > 0 ? t("runtimeDockMoreMembersMail", { count: hiddenWithMail }) : "",
    hiddenWithQueued > 0 ? t("runtimeDockMoreMembersQueued", { count: hiddenWithQueued }) : "",
    hiddenWithAttention > 0
      ? t("runtimeDockMoreMembersAttention", { count: hiddenWithAttention })
      : "",
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div className="lcd-members" role="group" aria-label={t("runtimeDockMembers")}>
      <div ref={rowRef} className="lcd-members-row">
        {items.map((item) => (
          <RuntimeDockActorButton
            key={item.actorId}
            variant="chip"
            hidden={hidden.has(item.actorId)}
            groupId={groupId}
            item={item}
            isDark={isDark}
            isSmallScreen={false}
            isInspectorOpen={activeRuntimeActorId === item.actorId}
            actorStatusProvisional={actorStatusProvisional}
            motionVisible={motionVisible}
            onOpenInspector={onOpenRuntimeActor}
            onToneChange={reportTone}
            onHiddenAttention={reportHiddenAttention}
          />
        ))}
      </div>
      {hiddenItems.length > 0 ? (
        <Popover open={moreOpen} onOpenChange={setMoreOpen}>
          <PopoverTrigger asChild>
            <button
              ref={moreRef}
              type="button"
              className="lcd-member lcd-member-more"
              data-tone={hiddenWithAttention > 0 ? "attention" : undefined}
              data-attention-flash={moreAttentionFlash || undefined}
              aria-label={moreLabel}
              title={moreLabel}
            >
              +{hiddenItems.length}
              {hiddenWithMail > 0 ? (
                <LcdIcon
                  icon={Mail}
                  glyph="mail"
                  className="h-[10px] w-[10px]"
                  strokeWidth={2}
                  aria-hidden="true"
                />
              ) : null}
              {hiddenWithQueued > 0 ? (
                <LcdIcon
                  icon={Hourglass}
                  glyph="hourglass"
                  className="h-[10px] w-[10px]"
                  strokeWidth={2}
                  aria-hidden="true"
                />
              ) : null}
            </button>
          </PopoverTrigger>
          <PopoverContent
            ref={popoverRef}
            align="end"
            className="lcd-members-popover"
            onCloseAutoFocus={(event) => {
              if (preventCloseAutoFocusRef.current) {
                event.preventDefault();
                preventCloseAutoFocusRef.current = false;
              }
            }}
          >
            <div className="flex flex-col items-stretch gap-1 p-2">
              {/* Only row tiles report tone; closing these duplicates must not clear it. */}
              {hiddenItems.map((item) => (
                <RuntimeDockActorButton
                  key={item.actorId}
                  variant="chip"
                  showName
                  groupId={groupId}
                  item={item}
                  isDark={isDark}
                  isSmallScreen={false}
                  isInspectorOpen={activeRuntimeActorId === item.actorId}
                  actorStatusProvisional={actorStatusProvisional}
                  motionVisible={motionVisible}
                  onOpenInspector={(actorId) => {
                    setMoreOpen(false);
                    onOpenRuntimeActor(actorId);
                  }}
                />
              ))}
            </div>
          </PopoverContent>
        </Popover>
      ) : null}
      {!readOnly && onAddAgent ? (
        <button
          type="button"
          onClick={onAddAgent}
          className="lcd-member lcd-member-add"
          aria-label={t("addAgent", { defaultValue: "Add an agent" })}
          title={t("addAgent", { defaultValue: "Add an agent" })}
        >
          <PlusIcon size={14} strokeWidth={2} />
        </button>
      ) : null}
    </div>
  );
}
