import { cn } from "@/lib/utils";
import { GroupCombobox, type GroupComboboxItem } from "./GroupCombobox";

export type SelectComboboxItem = GroupComboboxItem;

interface SelectComboboxProps {
  items: SelectComboboxItem[];
  value: string;
  onChange: (nextValue: string) => void;
  ariaLabel: string;
  className?: string;
  contentClassName?: string;
  disabled?: boolean;
  emptyText?: string;
  placeholder?: string;
  searchable?: boolean;
  searchPlaceholder?: string;
}

export function SelectCombobox({
  items,
  value,
  onChange,
  ariaLabel,
  className,
  contentClassName,
  disabled = false,
  emptyText = "No matching results",
  placeholder = ariaLabel,
  searchable = false,
  searchPlaceholder = ariaLabel,
}: SelectComboboxProps) {
  return (
    <GroupCombobox
      items={items}
      value={value}
      onChange={onChange}
      disabled={disabled}
      placeholder={placeholder}
      searchPlaceholder={searchPlaceholder}
      emptyText={emptyText}
      ariaLabel={ariaLabel}
      triggerClassName={cn(className, "cursor-pointer")}
      contentClassName={cn("p-0", contentClassName)}
      searchable={searchable}
      matchTriggerWidth
    />
  );
}
