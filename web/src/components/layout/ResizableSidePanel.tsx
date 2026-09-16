import { useMemo, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { useSidePanelLayout } from "../../hooks/useSidePanelLayout";
import type { SidePanelSurface } from "../../hooks/useSidePanelSelection";
import { SIDE_PANEL_COMPACT_WIDTH, SIDE_PANEL_MIN_WIDTH } from "../../utils/sidePanelLayout";
import { classNames } from "../../utils/classNames";

/** Gesture updates belong to this column, not the chat page and its message/composer tree. */
export function ResizableSidePanel({
  groupId,
  surface,
  viewing,
  container,
  isDark,
  children,
}: {
  groupId: string;
  surface: SidePanelSurface;
  viewing: boolean;
  container: RefObject<HTMLDivElement | null>;
  isDark: boolean;
  children: (controls: { compact: boolean; toggleCompact: () => void }) => ReactNode;
}) {
  const { t } = useTranslation("chat");
  const layout = useSidePanelLayout(groupId, surface, viewing, container);
  const { compact, toggleCompact } = layout;
  // Width changes need CSS layout, but not another render of an unchanged file tree/viewer.
  const content = useMemo(
    () => children({ compact, toggleCompact }),
    [children, compact, toggleCompact],
  );
  return (
    <>
      {layout.dragging && (
        <div className="fixed inset-0 z-[1000] cursor-col-resize" aria-hidden="true" />
      )}
      <div
        className="relative hidden w-2 flex-shrink-0 touch-none cursor-col-resize md:block"
        onPointerDown={layout.onPointerDown}
        onKeyDown={layout.onKeyDown}
        role="separator"
        tabIndex={0}
        aria-orientation="vertical"
        aria-label={t("sidePanelResize")}
        aria-controls="group-side-panel"
        aria-valuemin={
          surface === "presentation"
            ? SIDE_PANEL_COMPACT_WIDTH
            : Math.min(SIDE_PANEL_MIN_WIDTH, layout.maxWidth)
        }
        aria-valuemax={layout.maxWidth}
        aria-valuenow={layout.width}
        data-side-panel-resize
      >
        <div
          className={classNames(
            "absolute inset-y-0 left-1/2 w-px -translate-x-1/2",
            isDark ? "bg-white/8" : "bg-black/8",
          )}
        />
        <div
          className={classNames(
            "absolute inset-y-0 -left-1 w-4 rounded-full transition-colors",
            layout.dragging
              ? isDark
                ? "bg-cyan-300/18"
                : "bg-cyan-500/16"
              : isDark
                ? "hover:bg-white/8"
                : "hover:bg-black/6",
          )}
        />
      </div>
      <div
        className={classNames(
          "hidden min-h-0 min-w-0 flex-shrink-0 flex-col overflow-hidden border-l md:flex",
          isDark ? "border-white/8 bg-slate-950/20" : "border-black/8 bg-white/40",
        )}
        id="group-side-panel"
        style={{ width: `${layout.width}px` }}
      >
        {content}
      </div>
    </>
  );
}
