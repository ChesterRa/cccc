// useSwipeNavigation provides touch swipe navigation between tabs.
import { useRef, useCallback } from "react";

interface UseSwipeNavigationOptions {
  tabs: string[];
  activeTab: string;
  onTabChange: (tab: string) => void;
  /** Minimum horizontal swipe distance (px). */
  minSwipeDistance?: number;
  /** Horizontal/vertical ratio threshold. */
  swipeRatio?: number;
}

const INTERACTIVE_SWIPE_GUARD_SELECTOR = [
  "button",
  "a",
  "input",
  "textarea",
  "select",
  "label",
  "[role='button']",
  "[role='tab']",
  "[role='tablist']",
  "[role='link']",
  "[role='textbox']",
  "[contenteditable='true']",
  "[data-disable-swipe-nav='true']",
].join(", ");

export interface SwipeNavigationGuardNode {
  interactive: boolean;
  scrollable: boolean;
}

function toElement(target: EventTarget | null): Element | null {
  if (target instanceof Element) return target;
  if (target instanceof Node) return target.parentElement;
  return null;
}

function hasScrollableOverflow(node: Element): boolean {
  if (!(node instanceof HTMLElement)) return false;
  const style = window.getComputedStyle(node);
  const scrollableX =
    /auto|scroll|overlay/.test(style.overflowX) && node.scrollWidth > node.clientWidth + 1;
  const scrollableY =
    /auto|scroll|overlay/.test(style.overflowY) && node.scrollHeight > node.clientHeight + 1;
  return scrollableX || scrollableY;
}

export function shouldHandleSwipeNavigationChain(nodes: SwipeNavigationGuardNode[]): boolean {
  return !nodes.some((node) => node.interactive || node.scrollable);
}

export function shouldHandleSwipeNavigation(
  target: EventTarget | null,
  currentTarget: EventTarget | null,
): boolean {
  const targetEl = toElement(target);
  const boundaryEl = toElement(currentTarget);
  if (!targetEl || !boundaryEl) return true;

  const guardNodes: SwipeNavigationGuardNode[] = [];
  for (
    let node: Element | null = targetEl;
    node && node !== boundaryEl;
    node = node.parentElement
  ) {
    guardNodes.push({
      interactive: node.matches(INTERACTIVE_SWIPE_GUARD_SELECTOR),
      scrollable: hasScrollableOverflow(node),
    });
  }

  return shouldHandleSwipeNavigationChain(guardNodes);
}

export function useSwipeNavigation({
  tabs,
  activeTab,
  onTabChange,
  minSwipeDistance = 50,
  swipeRatio = 1.5,
}: UseSwipeNavigationOptions) {
  const touchStartX = useRef<number>(0);
  const touchStartY = useRef<number>(0);
  const touchNavigationEnabled = useRef<boolean>(true);

  const handleTouchStart = useCallback((e: React.TouchEvent) => {
    touchNavigationEnabled.current = shouldHandleSwipeNavigation(e.target, e.currentTarget);
    if (!touchNavigationEnabled.current || e.touches.length !== 1) return;
    touchStartX.current = e.touches[0].clientX;
    touchStartY.current = e.touches[0].clientY;
  }, []);

  const handleTouchEnd = useCallback(
    (e: React.TouchEvent) => {
      if (!touchNavigationEnabled.current || e.changedTouches.length !== 1) {
        touchNavigationEnabled.current = true;
        return;
      }
      const touchEndX = e.changedTouches[0].clientX;
      const touchEndY = e.changedTouches[0].clientY;
      const deltaX = touchEndX - touchStartX.current;
      const deltaY = touchEndY - touchStartY.current;

      // Determine whether this is a valid horizontal swipe.
      if (Math.abs(deltaX) > minSwipeDistance && Math.abs(deltaX) > Math.abs(deltaY) * swipeRatio) {
        const currentIndex = tabs.indexOf(activeTab);
        if (deltaX > 0 && currentIndex > 0) {
          // Swipe right -> previous tab
          onTabChange(tabs[currentIndex - 1]);
        } else if (deltaX < 0 && currentIndex < tabs.length - 1) {
          // Swipe left -> next tab
          onTabChange(tabs[currentIndex + 1]);
        }
      }
      touchNavigationEnabled.current = true;
    },
    [tabs, activeTab, onTabChange, minSwipeDistance, swipeRatio],
  );

  return { handleTouchStart, handleTouchEnd };
}
