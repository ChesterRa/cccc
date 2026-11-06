import { useId, useSyncExternalStore, type ReactNode } from "react";
import { ActorAvatar } from "../../components/ActorAvatar";
import { classNames } from "../../utils/classNames";
import type { RuntimeDockItem } from "./runtimeDockItems";
import type { RuntimeRingTone } from "./runtimeDockRingTone";

const subscribeVisibility = (notify: () => void) => {
  document.addEventListener("visibilitychange", notify);
  return () => document.removeEventListener("visibilitychange", notify);
};
const isVisible = () => document.visibilityState !== "hidden";

/** Preserve the original circular status ring while LCD keeps its tile and motion. */
export function RuntimeDockAvatar({
  item,
  isDark,
  isSmallScreen,
  isInspectorOpen,
  isChip,
  classic,
  tone,
  animate,
  lcdRing,
}: {
  item: RuntimeDockItem;
  isDark: boolean;
  isSmallScreen: boolean;
  isInspectorOpen: boolean;
  isChip: boolean;
  classic: boolean;
  tone: RuntimeRingTone;
  animate: boolean;
  lcdRing: ReactNode;
}) {
  const gradientId = useId();
  const visible = useSyncExternalStore(subscribeVisibility, isVisible, () => false);
  if (!classic) {
    return (
      <>
        <ActorAvatar
          avatarUrl={item.actor.avatar_url || undefined}
          runtime={item.runtime}
          title={item.actorLabel}
          isDark={isDark}
          sizeClassName={
            isChip ? "h-8 w-8" : isSmallScreen ? "h-[33px] w-[33px]" : "h-[37px] w-[37px]"
          }
          textClassName={isChip ? "text-[0.8125rem]" : undefined}
          className={classNames(
            "lcd-tile",
            !isChip &&
              "relative z-10 border-transparent shadow-[0_18px_34px_-22px_rgba(15,23,42,0.68)]",
            !isChip && item.runner === "headless" && (isDark ? "bg-slate-900" : "bg-slate-50"),
          )}
          accentRingClassName={
            !isChip && isInspectorOpen ? (isDark ? "ring-white/10" : "ring-black/10") : null
          }
        />
        {lcdRing}
      </>
    );
  }

  const active = tone === "active";
  const size = isSmallScreen ? 40 : 44;
  const ringColor =
    tone === "active"
      ? isDark
        ? "text-[#C4B5FD]"
        : "text-[#A07CFE]"
      : tone === "attention"
        ? isDark
          ? "text-rose-400"
          : "text-rose-500"
        : tone === "idle"
          ? isDark
            ? "text-emerald-300/45"
            : "text-emerald-500/55"
          : "text-slate-400/50";

  return (
    <span className="classic-dock-avatar" style={{ width: size, height: size }}>
      <span
        className={classNames("classic-dock-ring", active && "classic-dock-ring--active")}
        data-animating={active && animate && visible}
        aria-hidden="true"
      >
        <svg
          viewBox="0 0 44 44"
          width={size}
          height={size}
          fill="none"
          stroke="currentColor"
          className={ringColor}
        >
          <circle
            cx="22"
            cy="22"
            r="21"
            strokeWidth={active ? 2 : 1.5}
            opacity={active ? 0.12 : 1}
          />
          {active ? (
            <>
              <defs>
                <linearGradient
                  id={gradientId}
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
                cx="22"
                cy="22"
                r="21"
                stroke={`url(#${gradientId})`}
                strokeWidth="2"
                strokeDasharray="44 88"
                strokeLinecap="round"
              />
            </>
          ) : null}
        </svg>
      </span>
      <ActorAvatar
        avatarUrl={item.actor.avatar_url || undefined}
        runtime={item.runtime}
        title={item.actorLabel}
        isDark={isDark}
        sizeClassName={isSmallScreen ? "h-[33px] w-[33px]" : "h-[37px] w-[37px]"}
        className={classNames(
          "relative z-10 border-transparent shadow-[0_18px_34px_-22px_rgba(15,23,42,0.68)]",
          tone === "stopped" && "opacity-45 grayscale saturate-50",
          item.runner === "headless" && (isDark ? "bg-slate-900" : "bg-slate-50"),
        )}
        accentRingClassName={isInspectorOpen ? (isDark ? "ring-white/10" : "ring-black/10") : null}
      />
    </span>
  );
}
