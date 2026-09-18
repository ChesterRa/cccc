import { Fragment, useState, type ReactNode } from "react";
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
import { GroupMenuAction } from "./GroupMenuAction";

export type GroupMenuActionItem = {
  label: string;
  onClick: () => void;
  icon?: ReactNode;
  disabled?: boolean;
  tone?: "default" | "danger";
  /** Adjacent items with different sections are separated by a divider. */
  section?: string;
};

// Local sortable rows and remote rows share the same actions and focus behavior.
export function useGroupMenu(label: string, actions: GroupMenuActionItem[]) {
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
  const hasIcons = actions.some((action) => action.icon);
  const menu =
    open && actions.length > 0 ? (
      <FloatingPortal>
        <FloatingFocusManager context={context} modal={false} returnFocus={false}>
          <div
            ref={refs.setFloating}
            style={floatingStyles}
            {...getFloatingProps({
              "aria-label": label,
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
            className="z-max min-w-[180px] max-w-[calc(100vw-24px)] rounded-xl p-1.5 shadow-2xl glass-panel"
          >
            {actions.map((action, index) => (
              <Fragment key={action.label}>
                {index > 0 && actions[index - 1]?.section !== action.section ? (
                  <div
                    className="mx-2 my-1 border-t border-[var(--glass-border-subtle)]"
                    role="separator"
                  />
                ) : null}
                <GroupMenuAction
                  label={action.label}
                  icon={action.icon}
                  iconSlot={hasIcons}
                  tone={action.tone}
                  disabled={action.disabled}
                  onClick={() => {
                    // A menu item disappears when it opens a dialog. Focus its durable
                    // trigger first so the dialog has somewhere to return to.
                    focusTrigger();
                    setOpen(false);
                    action.onClick();
                  }}
                />
              </Fragment>
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
