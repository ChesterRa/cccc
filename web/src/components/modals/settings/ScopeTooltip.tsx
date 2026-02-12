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

interface ScopeTooltipProps {
  isDark: boolean;
  title: string;
  content: ReactNode;
  children: (
    getReferenceProps: (userProps?: HTMLAttributes<HTMLElement>) => Record<string, unknown>,
    setReference: (node: HTMLElement | null) => void
  ) => ReactNode;
}

export function ScopeTooltip({ isDark, title, content, children }: ScopeTooltipProps) {
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
  const hover = useHover(context, { delay: 150, restMs: 100 });
  const dismiss = useDismiss(context);
  const role = useRole(context, { role: "tooltip" });
  const { getReferenceProps, getFloatingProps } = useInteractions([hover, dismiss, role]);

  const setReference = useCallback(
    (node: HTMLElement | null) => {
      refs.setReference(node);
    },
    [refs]
  );

  const setFloating = useCallback(
    (node: HTMLElement | null) => {
      refs.setFloating(node);
    },
    [refs]
  );

  return (
    <>
      {children(getReferenceProps, setReference)}
      <FloatingPortal>
        {isOpen && (
          <div
            ref={setFloating}
            style={floatingStyles}
            {...getFloatingProps()}
            className={`z-max w-max max-w-[220px] rounded-lg border shadow-xl px-3 py-2 text-[11px] transition-opacity duration-150 ${
              isPositioned ? "opacity-100" : "opacity-0"
            } ${isDark ? "bg-slate-900 border-slate-700 text-slate-300" : "bg-white border-gray-200 text-gray-600"}`}
          >
            <div className="font-semibold mb-1 text-emerald-500">{title}</div>
            {content}
          </div>
        )}
      </FloatingPortal>
    </>
  );
}
