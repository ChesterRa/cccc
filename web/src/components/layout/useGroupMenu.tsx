import { useState, type ReactNode } from "react";
import {
  FloatingFocusManager,
  FloatingPortal,
  autoUpdate,
  flip,
  offset,
  shift,
  useDismiss,
  useFloating,
  useInteractions,
  useRole,
} from "@floating-ui/react";
import { GroupMenuAction, type GroupMenuActionProps } from "./GroupMenuAction";

// Local sortable rows and remote rows share the same actions and focus behavior.
export function useGroupMenu(label: string, actions: GroupMenuActionProps[], heading?: ReactNode) {
  const [open, setOpen] = useState(false);
  const { refs, floatingStyles, context } = useFloating({
    open,
    onOpenChange: setOpen,
    placement: "bottom-end",
    middleware: [offset(8), flip({ padding: 12 }), shift({ padding: 12 })],
    whileElementsMounted: autoUpdate,
    strategy: "fixed",
  });
  const dismiss = useDismiss(context);
  const role = useRole(context, { role: "menu" });
  const { getFloatingProps } = useInteractions([dismiss, role]);
  const focusTrigger = () => {
    const trigger = refs.domReference.current;
    if (trigger instanceof HTMLElement) trigger.focus();
  };
  const menu =
    open && actions.length > 0 ? (
      <FloatingPortal>
        <FloatingFocusManager context={context} modal={false} returnFocus={false}>
          <div
            ref={refs.setFloating}
            style={floatingStyles}
            {...getFloatingProps({
              "aria-label": label,
              onPointerDown: (event) => event.stopPropagation(),
              onMouseDown: (event) => event.stopPropagation(),
              onTouchStart: (event) => event.stopPropagation(),
              onKeyDown(event: React.KeyboardEvent) {
                if (event.key === "Escape" || event.key === "Tab") {
                  if (event.key === "Escape") event.preventDefault();
                  event.stopPropagation();
                  focusTrigger();
                  setOpen(false);
                }
                if (!["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
                event.preventDefault();
                event.stopPropagation();
                const items = Array.from(
                  refs.floating.current?.querySelectorAll<HTMLElement>(
                    '[role="menuitem"]:not(:disabled)',
                  ) || [],
                );
                if (!items.length) return;
                const current = items.indexOf(document.activeElement as HTMLElement);
                const next =
                  event.key === "Home"
                    ? 0
                    : event.key === "End"
                      ? items.length - 1
                      : (current + (event.key === "ArrowDown" ? 1 : -1) + items.length) %
                        items.length;
                items[next]?.focus();
              },
            })}
            className={`z-max min-w-[180px] max-w-[calc(100vw-24px)] rounded-xl p-1.5 shadow-2xl glass-panel ${heading ? "w-72" : ""}`}
          >
            {heading}
            {actions.map((action) => (
              <GroupMenuAction
                {...action}
                key={action.label}
                onClick={() => {
                  // A menu item disappears when it opens a dialog. Focus its durable
                  // trigger first so the dialog has somewhere to return to.
                  focusTrigger();
                  setOpen(false);
                  action.onClick();
                }}
              />
            ))}
          </div>
        </FloatingFocusManager>
      </FloatingPortal>
    ) : null;
  return {
    open,
    available: actions.length > 0,
    menu,
    toggle(button: HTMLButtonElement) {
      refs.setReference(button);
      refs.setPositionReference(button);
      setOpen((current) => !current);
    },
    onContextMenu(event: React.MouseEvent<HTMLElement>) {
      if (!actions.length) return;
      event.preventDefault();
      event.currentTarget.focus();
      refs.setReference(event.currentTarget);
      refs.setPositionReference({
        getBoundingClientRect: () => new DOMRect(event.clientX, event.clientY, 0, 0),
      });
      setOpen(true);
    },
    onKeyDown(event: React.KeyboardEvent<HTMLElement>) {
      if (
        !actions.length ||
        !(event.key === "ContextMenu" || (event.shiftKey && event.key === "F10"))
      )
        return false;
      event.preventDefault();
      refs.setReference(event.currentTarget);
      refs.setPositionReference(event.currentTarget);
      setOpen(true);
      return true;
    },
  };
}
