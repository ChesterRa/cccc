import { useRef, useState, type CSSProperties, type ReactNode, type RefObject } from "react";
import { useTranslation } from "react-i18next";
import { ChevronDown } from "lucide-react";
import type { ResourceTab } from "../../stores/useResourcePanelStore";
import { classNames } from "../../utils/classNames";
import { LcdIcon } from "../Icons";
import { WorkspaceEntryMenu } from "../workspace/WorkspaceEntryMenu";
import { clampRatio, MAX_RATIO, MIN_RATIO, percent, type SplitSide } from "./splitLayout";

/** A side's header: which tab it shows, and a menu to show another open tab there. */
export function SplitPaneHeader({
  side,
  stacked,
  tab,
  icon,
  focused,
  unsaved,
  choices,
  isDark,
  style,
  onFocus,
  onPick,
}: {
  side: SplitSide;
  /** On a phone the two sides are stacked: the first is on top. */
  stacked: boolean;
  tab: ResourceTab;
  icon: ReactNode;
  focused: boolean;
  unsaved: boolean;
  /** The open tabs this side can show, the other side's included. */
  choices: ResourceTab[];
  isDark: boolean;
  style: CSSProperties;
  onFocus: () => void;
  onPick: (tab: ResourceTab) => void;
}) {
  const { t } = useTranslation("chat");
  const button = useRef<HTMLButtonElement>(null);
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null);
  const label = t(
    side === "left"
      ? stacked
        ? "resourcePanel.splitTop"
        : "resourcePanel.splitLeft"
      : stacked
        ? "resourcePanel.splitBottom"
        : "resourcePanel.splitRight",
  );
  return (
    <div
      className={classNames(
        "absolute flex min-w-0 items-stretch border-b border-[var(--glass-border-subtle)] text-sm",
        focused
          ? "bg-[var(--glass-tab-bg-active)] font-semibold text-[var(--color-text-primary)] shadow-[inset_0_-2px_0_var(--color-text-primary)]"
          : "text-[var(--color-text-secondary)]",
      )}
      style={style}
      onPointerDown={onFocus}
      data-resource-split-header={side}
    >
      <button
        ref={button}
        type="button"
        className="flex min-w-0 flex-1 items-center gap-1.5 px-2.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-[var(--color-border-focus)]"
        aria-haspopup="menu"
        aria-expanded={!!menu}
        aria-label={t("resourcePanel.splitPick", { side: label, name: tab.title })}
        title={t("resourcePanel.splitPick", { side: label, name: tab.title })}
        onClick={() => {
          onFocus();
          const box = button.current?.getBoundingClientRect();
          if (box) setMenu({ x: box.left, y: box.bottom });
        }}
        data-resource-split-pick={side}
      >
        {icon}
        <span className="truncate">{tab.title}</span>
        {unsaved ? (
          <span className="shrink-0 opacity-60" aria-label={t("resourcePanel.unsaved")}>
            •
          </span>
        ) : null}
        <LcdIcon icon={ChevronDown} glyph="chevron-down" className="h-3.5 w-3.5 shrink-0" />
      </button>
      {menu && button.current ? (
        <WorkspaceEntryMenu
          x={menu.x}
          y={menu.y}
          anchor={button.current}
          label={label}
          isDark={isDark}
          onClose={() => setMenu(null)}
          items={choices.map((choice) => ({
            key: choice.key,
            label: choice.title,
            checked: choice.key === tab.key,
            onSelect: () => onPick(choice),
          }))}
        />
      ) : null}
    </div>
  );
}

/**
 * The line between the two sides. Dragging moves it, the arrow keys move it in steps, and a
 * double click puts it back in the middle.
 */
export function SplitDivider({
  ratio,
  stacked,
  body,
  onRatio,
}: {
  ratio: number;
  stacked: boolean;
  body: RefObject<HTMLDivElement | null>;
  onRatio: (ratio: number) => void;
}) {
  const { t } = useTranslation("chat");
  const place = (clientX: number, clientY: number) => {
    const box = body.current?.getBoundingClientRect();
    if (!box || !box.width || !box.height) return;
    onRatio(
      clampRatio(stacked ? (clientY - box.top) / box.height : (clientX - box.left) / box.width),
    );
  };
  return (
    <div
      role="separator"
      tabIndex={0}
      aria-orientation={stacked ? "horizontal" : "vertical"}
      aria-label={t("resourcePanel.splitDivider")}
      aria-valuemin={MIN_RATIO * 100}
      aria-valuemax={MAX_RATIO * 100}
      aria-valuenow={Math.round(ratio * 100)}
      title={t("resourcePanel.splitDivider")}
      className={classNames(
        "absolute z-10 flex touch-none items-stretch justify-center outline-none focus-visible:bg-[var(--glass-tab-bg)]",
        stacked
          ? "left-0 right-0 h-[7px] cursor-row-resize flex-col"
          : "top-0 bottom-0 w-[7px] cursor-col-resize",
      )}
      style={
        stacked
          ? { top: `calc(${percent(ratio)} - 3px)` }
          : { left: `calc(${percent(ratio)} - 3px)` }
      }
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault();
        // Captured, the drag keeps reaching the line over a frame such as a PDF.
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        if (event.currentTarget.hasPointerCapture(event.pointerId))
          place(event.clientX, event.clientY);
      }}
      onDoubleClick={() => onRatio(0.5)}
      onKeyDown={(event) => {
        const step =
          event.key === (stacked ? "ArrowUp" : "ArrowLeft")
            ? -0.05
            : event.key === (stacked ? "ArrowDown" : "ArrowRight")
              ? 0.05
              : 0;
        if (event.key === "Home" || event.key === "End" || event.key === "Enter") {
          event.preventDefault();
          onRatio(event.key === "Home" ? MIN_RATIO : event.key === "End" ? MAX_RATIO : 0.5);
          return;
        }
        if (!step) return;
        event.preventDefault();
        onRatio(clampRatio(ratio + step));
      }}
      data-resource-split-divider
    >
      <span
        aria-hidden="true"
        className={classNames(
          "bg-[var(--glass-border-subtle)]",
          stacked ? "h-px w-full self-center" : "w-px",
        )}
      />
    </div>
  );
}
