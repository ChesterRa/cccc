import type { ReactNode } from "react";
import { ActorAvatar } from "../../components/ActorAvatar";
import { classNames } from "../../utils/classNames";
import type { RuntimeDockItem } from "./runtimeDockItems";

/** A member's avatar tile with its status ring drawn beside it. */
export function RuntimeDockAvatar({
  item,
  isDark,
  isSmallScreen,
  isInspectorOpen,
  isChip,
  lcdRing,
}: {
  item: RuntimeDockItem;
  isDark: boolean;
  isSmallScreen: boolean;
  isInspectorOpen: boolean;
  isChip: boolean;
  lcdRing: ReactNode;
}) {
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
