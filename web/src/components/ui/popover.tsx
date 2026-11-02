import * as React from "react";
import * as PopoverPrimitive from "@radix-ui/react-popover";

import { cn } from "@/lib/utils";

const Popover = PopoverPrimitive.Root;

const PopoverTrigger = PopoverPrimitive.Trigger;

const PopoverAnchor = PopoverPrimitive.Anchor;

const PopoverContent = React.forwardRef<
  React.ElementRef<typeof PopoverPrimitive.Content>,
  React.ComponentPropsWithoutRef<typeof PopoverPrimitive.Content>
>(({ className, align = "center", sideOffset = 8, style, ...props }, ref) => {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content
        ref={ref}
        align={align}
        sideOffset={sideOffset}
        className={cn(
          "z-[1000] max-h-[var(--radix-popover-content-available-height)] rounded-2xl border border-[var(--glass-border-subtle)] glass-modal text-[var(--color-text-primary)] shadow-2xl outline-none",
          "data-[state=open]:animate-scale-in data-[state=closed]:animate-fade-out",
          className,
        )}
        // glass-modal sets overflow: hidden outside the utility layer, where no overflow
        // utility can reach it. A popover held within the viewport scrolls instead.
        style={{ overflowY: "auto", ...style }}
        {...props}
      />
    </PopoverPrimitive.Portal>
  );
});
PopoverContent.displayName = PopoverPrimitive.Content.displayName;

export { Popover, PopoverTrigger, PopoverContent, PopoverAnchor };
