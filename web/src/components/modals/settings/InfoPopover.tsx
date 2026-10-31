import { useCallback, useId, useRef, useState } from "react";
import type { HTMLAttributes, ReactNode } from "react";
import {
  FloatingPortal,
  FloatingFocusManager,
  autoUpdate,
  flip,
  offset,
  shift,
  size,
  useClick,
  useDismiss,
  useFloating,
  useInteractions,
  useRole,
} from "@floating-ui/react";

interface InfoPopoverProps {
  isDark: boolean;
  title: string;
  content: ReactNode;
  placement?:
    | "top"
    | "top-start"
    | "top-end"
    | "bottom"
    | "bottom-start"
    | "bottom-end"
    | "right"
    | "left";
  maxWidth?: number;
  children: (
    getReferenceProps: (userProps?: HTMLAttributes<HTMLElement>) => Record<string, unknown>,
    setReference: (node: HTMLElement | null) => void,
  ) => ReactNode;
}

export function InfoPopover({
  isDark: _isDark,
  title,
  content,
  placement = "bottom-end",
  maxWidth = 280,
  children,
}: InfoPopoverProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [lcdScope, setLcdScope] = useState(false);
  const titleId = useId();
  const referenceRef = useRef<HTMLElement | null>(null);
  const { refs, floatingStyles, context } = useFloating({
    open: isOpen,
    onOpenChange: (next) => {
      if (next) setLcdScope(Boolean(referenceRef.current?.closest("[data-lcd-scope]")));
      setIsOpen(next);
    },
    placement,
    middleware: [
      offset(8),
      flip({ padding: 8 }),
      shift({ padding: 8 }),
      size({
        padding: 8,
        apply({ availableWidth, availableHeight, elements }) {
          Object.assign(elements.floating.style, {
            maxWidth: `${Math.max(0, Math.min(maxWidth, availableWidth))}px`,
            maxHeight: `${Math.max(0, availableHeight)}px`,
          });
        },
      }),
    ],
    whileElementsMounted: autoUpdate,
    strategy: "fixed",
  });

  const isPositioned = context.isPositioned;
  const click = useClick(context);
  const dismiss = useDismiss(context);
  const role = useRole(context, { role: "dialog" });
  const { getReferenceProps, getFloatingProps } = useInteractions([click, dismiss, role]);

  const setReference = useCallback(
    (node: HTMLElement | null) => {
      referenceRef.current = node;
      refs.setReference(node);
    },
    [refs],
  );

  const setFloating = useCallback(
    (node: HTMLElement | null) => {
      refs.setFloating(node);
    },
    [refs],
  );

  return (
    <>
      {children(getReferenceProps, setReference)}
      <FloatingPortal>
        {isOpen && (
          <FloatingFocusManager context={context} modal={false} order={["floating", "content"]}>
            <div
              ref={setFloating}
              style={{ ...floatingStyles, overflowY: "auto", overflowWrap: "anywhere" }}
              {...getFloatingProps()}
              tabIndex={0}
              aria-labelledby={titleId}
              data-lcd-scope={lcdScope || undefined}
              data-settings-info-popover
              className={`z-max rounded-xl shadow-2xl px-3 py-3 text-xs leading-6 transition-opacity duration-150 glass-panel ${
                isPositioned ? "opacity-100" : "opacity-0"
              } text-[var(--color-text-secondary)]`}
            >
              <div
                id={titleId}
                className="font-semibold mb-1 text-emerald-500"
                data-settings-info-title
              >
                {title}
              </div>
              {content}
            </div>
          </FloatingFocusManager>
        )}
      </FloatingPortal>
    </>
  );
}
