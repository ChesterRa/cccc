import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CompositionEvent,
  type FocusEvent,
  type MouseEvent,
  type RefObject,
  type TouchEvent,
  type UIEvent,
} from "react";

type Gesture = {
  tracking: boolean;
  startY: number;
  lastTop: number;
  distance: number;
  timer: number | null;
  interactive: boolean;
  moved: boolean;
};

const INTERACTIVE_TARGET =
  "button, a, input, textarea, select, [role=button], [data-pending-reply] label";
const REPLY_EDITOR = "[data-pending-reply] textarea";
const COMPOSER_EDITOR = "footer [data-composer-input] textarea";
const OPEN_LAYER = '[role="dialog"][data-state="open"], [role="menu"][data-state="open"]';

/** Hide transient chrome only in response to a touch scroll in the message list. */
export function useMobileReadingChrome(
  enabled: boolean,
  groupId: string,
  scrollRef: RefObject<HTMLDivElement | null>,
  onHiddenChange?: (hidden: boolean) => void,
  onReachLatest?: () => void,
) {
  const [hidden, setHidden] = useState(false);
  const composing = useRef(false);
  const gesture = useRef<Gesture>({
    tracking: false,
    startY: 0,
    lastTop: 0,
    distance: 0,
    timer: null,
    interactive: false,
    moved: false,
  });
  const reveal = useCallback(() => setHidden(false), []);

  useEffect(() => {
    setHidden(false);
    const current = gesture.current;
    current.tracking = false;
    if (current.timer !== null) window.clearTimeout(current.timer);
    current.timer = null;
  }, [enabled, groupId]);
  useEffect(() => {
    onHiddenChange?.(enabled && hidden);
  }, [enabled, hidden, onHiddenChange]);
  useEffect(
    () => () => {
      const current = gesture.current;
      if (current.timer !== null) window.clearTimeout(current.timer);
      onHiddenChange?.(false);
    },
    [onHiddenChange],
  );

  const onTouchStartCapture = (event: TouchEvent<HTMLElement>) => {
    const target = event.target;
    const scroll = scrollRef.current;
    const current = gesture.current;
    if (current.timer !== null) window.clearTimeout(current.timer);
    current.timer = null;
    current.tracking = false;
    if (
      !enabled ||
      !scroll ||
      event.touches.length !== 1 ||
      !(target instanceof Element) ||
      !scroll.contains(target)
    )
      return;
    current.tracking = true;
    current.interactive = !!target.closest(INTERACTIVE_TARGET);
    current.moved = false;
    current.startY = event.touches[0]?.clientY ?? 0;
    current.lastTop = scroll.scrollTop;
    current.distance = 0;
  };
  const onTouchMoveCapture = (event: TouchEvent<HTMLElement>) => {
    const current = gesture.current;
    if (!current.tracking || event.touches.length !== 1) return;
    if (Math.abs((event.touches[0]?.clientY ?? current.startY) - current.startY) >= 8) {
      current.moved = true;
    }
  };
  const onTouchEndCapture = (event: TouchEvent<HTMLElement>) => {
    const current = gesture.current;
    if (!current.tracking) return;
    const endY = event.changedTouches[0]?.clientY ?? current.startY;
    if (Math.abs(endY - current.startY) < 8 && Math.abs(current.distance) < 8) {
      // Keep interactive targets in place until their click has been delivered.
      if (!current.interactive) reveal();
      current.tracking = false;
      return;
    }
    // Allow the browser's inertial scroll to finish the gesture without
    // interpreting later programmatic scrolling as a touch gesture.
    current.timer = window.setTimeout(() => {
      current.tracking = false;
      current.timer = null;
    }, 600);
  };
  const onTouchCancelCapture = () => {
    const current = gesture.current;
    current.tracking = false;
    if (current.timer !== null) window.clearTimeout(current.timer);
    current.timer = null;
  };
  const onClickCapture = (event: MouseEvent<HTMLElement>) => {
    const target = event.target;
    if (
      target instanceof Element &&
      scrollRef.current?.contains(target) &&
      target.closest(INTERACTIVE_TARGET)
    ) {
      // Deliver the click before the Dock returns and can clip its target.
      reveal();
    }
  };
  const onFocusCapture = (event: FocusEvent<HTMLElement>) => {
    if (event.target instanceof Element && event.target.closest(REPLY_EDITOR)) reveal();
    if (event.target instanceof Element && event.target.matches(COMPOSER_EDITOR)) {
      composing.current = false;
    }
  };
  const onCompositionStartCapture = (event: CompositionEvent<HTMLElement>) => {
    if (event.target instanceof Element && event.target.matches(COMPOSER_EDITOR)) {
      composing.current = true;
    }
  };
  const onCompositionEndCapture = (event: CompositionEvent<HTMLElement>) => {
    if (event.target instanceof Element && event.target.matches(COMPOSER_EDITOR)) {
      composing.current = false;
    }
  };
  const onScrollCapture = (event: UIEvent<HTMLElement>) => {
    const scroll = scrollRef.current;
    const current = gesture.current;
    if (!enabled || !scroll || event.target !== scroll) return;
    const top = scroll.scrollTop;
    const atLatest = scroll.scrollHeight - scroll.clientHeight - top < 8;
    let delta = 0;
    if (current.tracking && current.moved) {
      delta = top - current.lastTop;
      current.lastTop = top;
      if (delta) {
        if (Math.sign(delta) !== Math.sign(current.distance)) current.distance = 0;
        current.distance += delta;
      }
      const active = document.activeElement;
      if (active instanceof HTMLTextAreaElement && active.matches(COMPOSER_EDITOR)) {
        // Scrolling the conversation dismisses the keyboard without changing
        // the draft. Never interrupt an input method's uncommitted text.
        if (composing.current && !atLatest) return;
        if (
          !composing.current &&
          Math.abs(current.distance) >= 12 &&
          !active.closest('[role="dialog"], [role="menu"], [role="listbox"]') &&
          !document.querySelector(OPEN_LAYER)
        ) {
          active.blur();
        }
      }
    } else if (current.tracking) {
      // New messages can scroll the list while a finger is resting on a
      // control. Wait for real touch movement before moving that control.
      current.lastTop = top;
      current.distance = 0;
    }
    if (atLatest) {
      if (hidden) onReachLatest?.();
      if (current.tracking && current.interactive && !current.moved) {
        // Do not move a touched action before its click is delivered.
        current.lastTop = top;
        current.distance = 0;
        return;
      }
      reveal();
      current.lastTop = top;
      current.distance = 0;
      return;
    }
    if (!current.tracking || !current.moved || !delta) return;
    if (current.distance <= -12) {
      reveal();
      current.distance = 0;
    } else if (current.distance >= 24) {
      const active = document.activeElement;
      const openLayer = document.querySelector(OPEN_LAYER);
      // A touched filter or header button can retain focus through a later
      // drag. Once the list actually scrolls, it is no longer being operated;
      // move focus out before hiding the controls. Keep editors and open
      // layers protected, including keyboard-driven interactions.
      if (
        !openLayer &&
        active instanceof HTMLElement &&
        active.closest("header, [data-message-filters]") &&
        !active.closest(`[role="dialog"], [role="menu"], [role="listbox"], ${REPLY_EDITOR}`) &&
        !active.matches("input, textarea, select") &&
        !active.isContentEditable
      ) {
        active.blur();
      }
      if (
        !document.activeElement?.closest(
          `header, [data-message-filters], [role="dialog"], [role="menu"], [role="listbox"], ${REPLY_EDITOR}`,
        ) &&
        !openLayer
      ) {
        setHidden(true);
      }
      current.distance = 0;
    }
  };
  return {
    hidden: enabled && hidden,
    reveal,
    onTouchStartCapture,
    onTouchMoveCapture,
    onTouchEndCapture,
    onTouchCancelCapture,
    onCompositionStartCapture,
    onCompositionEndCapture,
    onClickCapture,
    onFocusCapture,
    onScrollCapture,
  };
}
