import { useState } from "react";
import { InfoIcon } from "../Icons";
import { Popover, PopoverContent, PopoverTrigger } from "../ui/popover";

/** Marks a member that came back without its saved conversation. A click or a tap opens the
 * lines a pointer reads on hover, because a touch screen has no hover. */
export function SessionReplacedMark({ summary, reasons }: { summary: string; reasons: string }) {
  const [open, setOpen] = useState(false);
  const title = [summary, reasons].filter(Boolean).join("\n");
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          // The symbol sits in a line of text, so it keeps its size where touch buttons get a
          // minimum one; the area around it takes the finger instead, narrower at the sides,
          // where the next control can be close.
          className="relative inline-flex !min-h-0 !min-w-0 shrink-0 rounded text-[var(--color-text-tertiary)] after:absolute after:-inset-x-2 after:-inset-y-3 hover:text-[var(--color-text-primary)] focus-visible:outline-2"
          aria-label={title}
          // The open popover already shows these lines.
          title={open ? undefined : title}
          data-session-replaced
        >
          <InfoIcon size={12} />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="start"
        // Far enough below the row to clear the close button of a phone-sized header.
        sideOffset={16}
        collisionPadding={8}
        className="max-w-[min(20rem,calc(100vw-2rem))] p-3 text-xs"
        data-session-replaced-details
      >
        <p className="text-[var(--color-text-primary)]">{summary}</p>
        {reasons ? <p className="mt-1 text-[var(--color-text-secondary)]">{reasons}</p> : null}
      </PopoverContent>
    </Popover>
  );
}
