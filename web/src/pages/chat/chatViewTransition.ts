import { flushSync } from "react-dom";
import "./chatViewTransition.css";

/** Marks the message list a view change animates; the list sets it on its own outer element. */
export const CHAT_MESSAGE_VIEW_CLASS = "chat-message-view";
/** "true" on that element once the view has its messages, or is known to have none. */
export const CHAT_VIEW_SETTLED_ATTRIBUTE = "data-chat-view-settled";

const TRANSITION_NAME = "chat-messages";
/** How long the old view may stay on screen while the new one reads its messages. */
export const CHAT_VIEW_SETTLE_LIMIT_MS = 350;
const SETTLE_POLL_MS = 30;

type ViewTransitionDocument = Document & {
  startViewTransition?: (update: () => void | Promise<void>) => { finished: Promise<void> };
};

function visibleMessageView(): HTMLElement | null {
  for (const view of document.querySelectorAll<HTMLElement>(`.${CHAT_MESSAGE_VIEW_CLASS}`)) {
    // Another conversation kept mounted off screen has no box; only the shown one animates.
    if (view.getClientRects().length > 0) return view;
  }
  return null;
}

function prefersReducedMotion(): boolean {
  return (
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  );
}

/** Resolves when the list reports its view settled, or the limit passes, whichever is first. */
function settled(view: HTMLElement): Promise<void> {
  return new Promise((resolve) => {
    const deadline = Date.now() + CHAT_VIEW_SETTLE_LIMIT_MS;
    const check = () => {
      if (
        !view.isConnected ||
        view.getAttribute(CHAT_VIEW_SETTLED_ATTRIBUTE) === "true" ||
        Date.now() >= deadline
      )
        // One more beat, so the rows the list has just received are laid out before they show.
        window.setTimeout(resolve, SETTLE_POLL_MS);
      else window.setTimeout(check, SETTLE_POLL_MS);
    };
    check();
  });
}

/**
 * Changes which messages the chat shows, such as by switching a filter, with the old messages
 * leaving and the new ones arriving instead of one set replacing the other in a single frame.
 * The old view stays up until the new one has its messages, within a short limit, so a view
 * read from the server does not animate in empty and then fill abruptly. Where the browser
 * cannot do this, or the user asked for reduced motion, the change simply happens.
 */
export function runChatViewTransition(update: () => void): void {
  const doc = document as ViewTransitionDocument;
  const view = visibleMessageView();
  if (!view || typeof doc.startViewTransition !== "function" || prefersReducedMotion()) {
    update();
    return;
  }
  // Named only for this change: a permanent name would enrol the list in every other
  // transition on the page and must be unique among everything mounted.
  const named = new Set<HTMLElement>();
  const name = (target: HTMLElement) => {
    target.style.setProperty("view-transition-name", TRANSITION_NAME);
    named.add(target);
  };
  const release = () => {
    for (const target of named) target.style.removeProperty("view-transition-name");
  };
  name(view);
  let applied = false;
  try {
    const transition = doc.startViewTransition(async () => {
      applied = true;
      flushSync(update);
      // A different view is usually a different list element; the arriving one takes the
      // name over, or it would appear without its half of the animation.
      const next = visibleMessageView();
      if (next && next !== view) {
        view.style.removeProperty("view-transition-name");
        name(next);
      }
      await settled(next ?? view);
    });
    void transition.finished.then(release, release);
  } catch {
    release();
    // The change itself must never be lost to a transition the browser refused.
    if (!applied) update();
  }
}
