import {
  useCallback,
  useEffect,
  useRef,
  useState,
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
};

const INTERACTIVE_TARGET =
  "button, a, input, textarea, select, [role=button], [data-pending-reply] label";
const REPLY_EDITOR = "[data-pending-reply] textarea";

/** Hide transient chrome only in response to a touch scroll in the message list. */
export function useMobileReadingChrome(
  enabled: boolean,
  groupId: string,
  scrollRef: RefObject<HTMLDivElement | null>,
  onHiddenChange?: (hidden: boolean) => void,
  onReachLatest?: () => void,
) {
  const [hidden, setHidden] = useState(false);
  const gesture = useRef<Gesture>({
    tracking: false,
    startY: 0,
    lastTop: 0,
    distance: 0,
    timer: null,
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
    if (target.closest(INTERACTIVE_TARGET)) {
      return;
    }
    current.tracking = true;
    current.startY = event.touches[0]?.clientY ?? 0;
    current.lastTop = scroll.scrollTop;
    current.distance = 0;
  };
  const onTouchEndCapture = (event: TouchEvent<HTMLElement>) => {
    const current = gesture.current;
    if (!current.tracking) return;
    const endY = event.changedTouches[0]?.clientY ?? current.startY;
    if (Math.abs(endY - current.startY) < 8 && Math.abs(current.distance) < 8) {
      reveal();
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
  };
  const onScrollCapture = (event: UIEvent<HTMLElement>) => {
    const scroll = scrollRef.current;
    const current = gesture.current;
    if (!enabled || !scroll || event.target !== scroll) return;
    const top = scroll.scrollTop;
    if (scroll.scrollHeight - scroll.clientHeight - top < 8) {
      if (hidden) onReachLatest?.();
      reveal();
      current.lastTop = top;
      current.distance = 0;
      return;
    }
    if (!current.tracking) return;
    const delta = top - current.lastTop;
    current.lastTop = top;
    if (!delta) return;
    if (Math.sign(delta) !== Math.sign(current.distance)) current.distance = 0;
    current.distance += delta;
    if (current.distance <= -12) {
      reveal();
      current.distance = 0;
    } else if (current.distance >= 24) {
      const active = document.activeElement;
      const openLayer = document.querySelector(
        '[role="dialog"][data-state="open"], [role="menu"][data-state="open"]',
      );
      if (
        !active?.closest(
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
    onTouchEndCapture,
    onTouchCancelCapture,
    onClickCapture,
    onFocusCapture,
    onScrollCapture,
  };
}
