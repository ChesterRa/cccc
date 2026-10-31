import type { ReactNode } from "react";
import { InfoIcon } from "../../Icons";
import { Popover, PopoverContent, PopoverTrigger } from "../../ui/popover";

interface ScopeTooltipProps {
  title: string;
  content: ReactNode;
}

/** An independent control: scope selection stays separate from its explanation. */
export function ScopeTooltip({ title, content }: ScopeTooltipProps) {
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={title}
          title={title}
          data-scope-help-trigger
          data-lcd-key
          className="flex h-8 w-8 min-h-8 min-w-8 shrink-0 items-center justify-center rounded-lg border border-transparent text-[var(--color-text-secondary)] hover:bg-[var(--glass-tab-bg-hover)] focus-visible:outline-2 pointer-coarse:min-h-11 pointer-coarse:min-w-11"
        >
          <InfoIcon size={16} />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        sideOffset={6}
        collisionPadding={8}
        data-scope-help-popup
        aria-label={title}
        onEscapeKeyDown={(event) => event.stopPropagation()}
        className="w-[min(22rem,calc(100vw-1rem))] max-h-[min(60vh,30rem,var(--radix-popover-content-available-height))] p-3 text-sm text-[var(--color-text-primary)] [overflow-wrap:anywhere]"
      >
        <div className="font-semibold">{title}</div>
        <div className="mt-2 leading-relaxed">{content}</div>
      </PopoverContent>
    </Popover>
  );
}
