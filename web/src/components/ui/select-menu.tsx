import { useRef, useState, type ComponentPropsWithoutRef, type ReactNode } from "react";
import { ChevronDownIcon } from "../Icons";
import { Popover, PopoverContent, PopoverTrigger } from "./popover";
import { cn } from "@/lib/utils";

export type SelectMenuOption<Value extends string | number> = { value: Value; label: string };

type DataAttributes = Record<`data-${string}`, string | number | undefined>;

/**
 * The app's own dropdown: a button plus a roving-focus menu.
 *
 * A native `<select>` renders with the operating system's own list, which ignores the app theme and
 * cannot be styled. This keeps every choice list in the product looking and behaving alike.
 */
export function SelectMenu<Value extends string | number>({
  value,
  options,
  onChange,
  ariaLabel,
  disabled = false,
  placeholder,
  align = "end",
  className,
  contentClassName,
  triggerProps,
  contentProps,
}: {
  value: Value;
  options: SelectMenuOption<Value>[];
  onChange: (value: Value) => void;
  ariaLabel: string;
  disabled?: boolean;
  /** Shown while `value` matches no option, e.g. before a list has loaded. */
  placeholder?: ReactNode;
  align?: ComponentPropsWithoutRef<typeof PopoverContent>["align"];
  className?: string;
  contentClassName?: string;
  /** Extra hooks for the trigger, such as the data attributes end-to-end tests select on. */
  triggerProps?: DataAttributes;
  contentProps?: DataAttributes;
}) {
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const selected = options.find((option) => option.value === value);

  return (
    <Popover open={open} onOpenChange={setOpen} modal={false}>
      <PopoverTrigger asChild>
        <button
          ref={trigger}
          {...triggerProps}
          data-value={value}
          type="button"
          disabled={disabled}
          aria-label={ariaLabel}
          aria-haspopup="menu"
          className={cn(
            "flex min-h-[44px] min-w-0 items-center justify-between gap-2 rounded-lg border border-[var(--color-border-secondary)] bg-[var(--color-bg-primary)] px-3 text-sm text-[var(--color-text-primary)] focus-visible:outline-2 focus-visible:outline-offset-2 disabled:opacity-50",
            className,
          )}
        >
          <span className="truncate">{selected ? selected.label : placeholder}</span>
          <ChevronDownIcon size={14} className="shrink-0" aria-hidden="true" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        ref={menu}
        {...contentProps}
        role="menu"
        aria-label={ariaLabel}
        align={align}
        sideOffset={4}
        collisionPadding={12}
        className={cn(
          // A long list stays a dropdown rather than a full-height column, but never overflows the viewport.
          // The list is an opaque panel: the shared glass blur behind it would only cost paint work.
          "max-w-[calc(100vw-24px)] max-h-[min(18rem,var(--radix-popover-content-available-height))] overflow-y-auto rounded-lg !bg-[var(--color-bg-primary)] !backdrop-blur-none p-1",
          contentClassName,
        )}
        onOpenAutoFocus={(event) => {
          event.preventDefault();
          menu.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus();
        }}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          // Closing animation cleanup may run after another choice has opened.
          // Restore only abandoned focus, never steal it from the next menu.
          if (
            document.activeElement === document.body ||
            menu.current?.contains(document.activeElement)
          ) {
            trigger.current?.focus();
          }
        }}
        onEscapeKeyDown={(event) => {
          event.preventDefault();
          event.stopPropagation();
          setOpen(false);
          trigger.current?.focus();
        }}
        onKeyDown={(event) => {
          if (event.key === "Tab") {
            event.preventDefault();
            event.stopPropagation();
            setOpen(false);
            return;
          }
          const keys = ["ArrowDown", "ArrowUp", "Home", "End"];
          if (!keys.includes(event.key)) return;
          event.preventDefault();
          event.stopPropagation();
          const items = Array.from(
            menu.current?.querySelectorAll<HTMLButtonElement>('[role="menuitemradio"]') || [],
          );
          const current = items.indexOf(document.activeElement as HTMLButtonElement);
          const index =
            event.key === "Home"
              ? 0
              : event.key === "End"
                ? items.length - 1
                : (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length;
          items[index]?.focus();
        }}
      >
        {options.map((option) => (
          <button
            key={option.value}
            type="button"
            role="menuitemradio"
            data-value={option.value}
            aria-checked={option.value === value}
            tabIndex={-1}
            className="flex min-h-[44px] w-full items-center justify-between gap-2 rounded-md px-3 text-left text-sm text-[var(--color-text-primary)] hover:bg-[var(--glass-tab-bg)] focus:bg-[var(--glass-tab-bg)] focus-visible:outline-2"
            onPointerDown={(event) => event.preventDefault()}
            onClick={() => {
              onChange(option.value);
              setOpen(false);
              trigger.current?.focus();
            }}
          >
            <span className="truncate">{option.label}</span>
            {option.value === value ? <span aria-hidden="true">✓</span> : null}
          </button>
        ))}
      </PopoverContent>
    </Popover>
  );
}
