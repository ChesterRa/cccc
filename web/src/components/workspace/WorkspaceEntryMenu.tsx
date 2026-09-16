import { useMemo } from "react";
import { Check } from "lucide-react";
import { classNames } from "../../utils/classNames";
import { Popover, PopoverAnchor, PopoverContent } from "../ui/popover";

export type WorkspaceMenuItem = {
  key: string;
  label: string;
  onSelect?: () => void;
  href?: string;
  download?: string;
  checked?: boolean;
  disabled?: boolean;
};

type Props = {
  x: number;
  y: number;
  anchor: HTMLElement;
  label: string;
  items: WorkspaceMenuItem[];
  isDark: boolean;
  onClose: () => void;
};

/** The same anchored menu serves mouse, touch, and keyboard entry points. */
export function WorkspaceEntryMenu({ x, y, anchor, label, items, isDark, onClose }: Props) {
  const virtualRef = useMemo(
    () => ({
      current: { getBoundingClientRect: () => new DOMRect(x, y, 0, 0), contextElement: anchor },
    }),
    [x, y, anchor],
  );
  return (
    <Popover
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <PopoverAnchor virtualRef={virtualRef} />
      <PopoverContent
        role="menu"
        aria-label={label}
        align="start"
        sideOffset={4}
        collisionPadding={8}
        className="w-60 max-w-[calc(100vw-1rem)] rounded-xl py-1.5"
        onCloseAutoFocus={(event) => event.preventDefault()}
        onEscapeKeyDown={(event) => {
          // Escape dismisses this layer without reaching the enclosing phone surface.
          event.stopPropagation();
          anchor.focus();
        }}
        onKeyDown={(event) => {
          const entries = [
            ...event.currentTarget.querySelectorAll<HTMLElement>(
              '[role^="menuitem"]:not(:disabled)',
            ),
          ];
          const index = entries.indexOf(document.activeElement as HTMLElement);
          let next: number;
          if (event.key === "ArrowDown") next = (index + 1) % entries.length;
          else if (event.key === "ArrowUp") next = (index - 1 + entries.length) % entries.length;
          else if (event.key === "Home") next = 0;
          else if (event.key === "End") next = entries.length - 1;
          else if (event.key === "Tab") {
            anchor.focus();
            onClose();
            return;
          } else return;
          event.preventDefault();
          entries[next]?.focus();
        }}
      >
        {items.map((item) => {
          const className = classNames(
            "flex w-full items-center gap-2 px-3 py-2 text-left text-[13px] outline-none",
            item.disabled
              ? "cursor-not-allowed opacity-40"
              : isDark
                ? "text-slate-100 hover:bg-white/8 focus:bg-white/8"
                : "text-slate-800 hover:bg-black/5 focus:bg-black/5",
          );
          const select = () => {
            anchor.focus();
            onClose();
            item.onSelect?.();
          };
          return item.href && !item.disabled ? (
            <a
              key={item.key}
              role="menuitem"
              href={item.href}
              download={item.download}
              onClick={select}
              className={className}
            >
              {item.label}
            </a>
          ) : (
            <button
              key={item.key}
              type="button"
              role={item.checked === undefined ? "menuitem" : "menuitemcheckbox"}
              aria-checked={item.checked}
              disabled={item.disabled}
              onClick={select}
              className={className}
            >
              <span className="flex-1">{item.label}</span>
              {item.checked && <Check aria-hidden="true" className="h-4 w-4 shrink-0" />}
            </button>
          );
        })}
      </PopoverContent>
    </Popover>
  );
}
