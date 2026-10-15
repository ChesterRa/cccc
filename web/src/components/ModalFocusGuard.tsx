import { focusFirst } from "../hooks/useModalA11y";

/**
 * The last stop of a modal that can hold a native media player. A player's
 * controls live in its own shadow tree, so useModalA11y lets the browser walk
 * them only when this guard follows: Tab out of the player lands here and
 * wraps to the modal's start, and a backward wrap steps natively from here
 * into the player's last control.
 */
export function ModalFocusGuard() {
  return (
    <span
      tabIndex={0}
      data-focus-guard
      className="sr-only"
      onFocus={(event) => {
        const guard = event.currentTarget;
        // Passed through on Shift+Tab: the browser continues backward.
        if (guard.dataset.focusPassThrough) {
          delete guard.dataset.focusPassThrough;
          return;
        }
        const modal = guard.closest<HTMLElement>('[role="dialog"]');
        if (modal) focusFirst(modal);
      }}
    />
  );
}
