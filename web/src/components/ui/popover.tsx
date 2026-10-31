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
  const scopeGiven = "data-lcd-scope" in props;
  const setRef = React.useCallback(
    (node: HTMLDivElement | null) => {
      // The portal leaves the surface that opened the popover, so unless the caller
      // decides, the popover takes that surface's LCD scope from its trigger, which
      // Radix links to this content by aria-controls. Anchor-only popovers have none.
      if (node && !scopeGiven) {
        const trigger = document.querySelector(`[aria-controls="${node.id}"]`);
        node.toggleAttribute("data-lcd-scope", Boolean(trigger?.closest("[data-lcd-scope]")));
      }
      if (typeof ref === "function") ref(node);
      else if (ref) ref.current = node;
    },
    [ref, scopeGiven],
  );
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content
        ref={setRef}
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
