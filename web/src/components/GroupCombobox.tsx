import { useLayoutEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronsUpDown } from "lucide-react";

import { cn } from "@/lib/utils";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";

export type GroupComboboxItem = {
  value: string;
  label: string;
  description?: string;
  keywords?: string[];
  disabled?: boolean;
};

interface GroupComboboxProps {
  items: GroupComboboxItem[];
  value: string;
  onChange: (nextValue: string) => void;
  disabled?: boolean;
  placeholder: string;
  searchPlaceholder: string;
  emptyText: string;
  ariaLabel: string;
  triggerClassName?: string;
  contentClassName?: string;
  descriptionClassName?: string;
  caretClassName?: string;
  searchable?: boolean;
  matchTriggerWidth?: boolean;
}

export function GroupCombobox({
  items,
  value,
  onChange,
  disabled = false,
  placeholder,
  searchPlaceholder,
  emptyText,
  ariaLabel,
  triggerClassName,
  contentClassName,
  descriptionClassName,
  caretClassName,
  searchable = true,
  matchTriggerWidth = false,
}: GroupComboboxProps) {
  const [open, setOpen] = useState(false);
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const [triggerWidth, setTriggerWidth] = useState<number | null>(null);

  const selectedItem = useMemo(
    () => items.find((item) => item.value === value) ?? null,
    [items, value]
  );
  const triggerLabel = selectedItem?.label || placeholder;
  const isDisabled = disabled || items.length === 0;

  useLayoutEffect(() => {
    if (!matchTriggerWidth || !open) return;
    const node = triggerRef.current;
    if (!node) return;

    const syncWidth = () => {
      const nextWidth = node.getBoundingClientRect().width;
      setTriggerWidth(nextWidth > 0 ? nextWidth : null);
    };

    syncWidth();
    window.addEventListener("resize", syncWidth);
    return () => {
      window.removeEventListener("resize", syncWidth);
    };
  }, [matchTriggerWidth, open]);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          ref={triggerRef}
          type="button"
          role="combobox"
          aria-expanded={open}
          aria-label={ariaLabel}
          disabled={isDisabled}
          className={cn(
            "flex items-center justify-between gap-2 rounded-xl transition-colors outline-none",
            "disabled:cursor-not-allowed disabled:opacity-60",
            "focus-visible:ring-2 focus-visible:ring-[var(--color-accent-primary)]/40 focus-visible:ring-offset-0",
            triggerClassName
          )}
        >
          <span className="min-w-0 truncate text-left">{triggerLabel}</span>
          <ChevronsUpDown className={cn("h-3.5 w-3.5 shrink-0 opacity-70", caretClassName)} />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        style={matchTriggerWidth && triggerWidth ? { width: `${triggerWidth}px` } : undefined}
        className={cn(
          matchTriggerWidth
            ? "max-w-[min(100vw-1rem,48rem)] p-0"
            : "w-[var(--radix-popover-trigger-width)] min-w-[14rem] max-w-[min(22rem,calc(100vw-1rem))] p-0",
          contentClassName
        )}
      >
        <Command>
          {searchable ? <CommandInput placeholder={searchPlaceholder} /> : null}
          <CommandList>
            <CommandEmpty>{emptyText}</CommandEmpty>
            <CommandGroup>
              {items.map((item) => {
                const selected = item.value === value;
                return (
                  <CommandItem
                    key={item.value}
                    value={item.value}
                    disabled={item.disabled}
                    keywords={[item.label, item.description || "", ...(item.keywords || [])]}
                    onSelect={() => {
                      if (item.disabled) return;
                      onChange(item.value);
                      setOpen(false);
                    }}
                  >
                    <Check className={cn("h-4 w-4 shrink-0", selected ? "opacity-100" : "opacity-0")} />
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-sm">{item.label}</div>
                      {item.description ? (
                        <div
                          className={cn(
                            "truncate text-[11px] text-[var(--color-text-muted)]",
                            descriptionClassName
                          )}
                        >
                          {item.description}
                        </div>
                      ) : null}
                    </div>
                  </CommandItem>
                );
              })}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
}
