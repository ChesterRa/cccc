import { useCallback, useState } from "react";
import type { HTMLAttributes, ReactNode } from "react";
import {
  FloatingPortal,
  autoUpdate,
  flip,
  offset,
  shift,
  useDismiss,
  useFloating,
  useHover,
  useInteractions,
  useRole,
} from "@floating-ui/react";

interface HoverTooltipProps {
  label: ReactNode;
  children: (
    getReferenceProps: (userProps?: HTMLAttributes<HTMLElement>) => Record<string, unknown>,
    setReference: (node: HTMLElement | null) => void
  ) => ReactNode;
}

export function HoverTooltip({ label, children }: HoverTooltipProps) {
  const [isOpen, setIsOpen] = useState(false);
  const { refs, floatingStyles, context } = useFloating({
    open: isOpen,
    onOpenChange: setIsOpen,
    placement: "top",
    middleware: [offset(8), flip(), shift({ padding: 8 })],
    whileElementsMounted: autoUpdate,
    strategy: "fixed",
  });

  const isPositioned = context.isPositioned;
  const hover = useHover(context, { delay: 150, restMs: 80 });
  const dismiss = useDismiss(context);
  const role = useRole(context, { role: "tooltip" });
  const { getReferenceProps, getFloatingProps } = useInteractions([hover, dismiss, role]);

  const setReference = useCallback((node: HTMLElement | null) => refs.setReference(node), [refs]);
  const setFloating = useCallback((node: HTMLElement | null) => refs.setFloating(node), [refs]);

  return (
    <>
      {children(getReferenceProps, setReference)}
      <FloatingPortal>
        {isOpen ? (
          <div
            ref={setFloating}
            style={floatingStyles}
            {...getFloatingProps()}
            className={`z-max max-w-[220px] rounded-md border border-[var(--glass-border-subtle)] bg-[var(--color-bg-primary)] px-2 py-1 text-xs text-[var(--color-text-primary)] shadow-xl transition-opacity duration-150 ${
              isPositioned ? "opacity-100" : "opacity-0"
            }`}
          >
            {label}
          </div>
        ) : null}
      </FloatingPortal>
    </>
  );
}
