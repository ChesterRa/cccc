// useModalA11y: Escape key, focus trap, and body scroll lock for modals.
import { useCallback, useEffect, useId, useRef } from "react";

const FOCUSABLE_SELECTOR =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

type BodyStyleSnapshot = { overflow: string; position: string; top: string; width: string };

const modalStack: string[] = [];
const modalElements = new Map<string, HTMLDivElement>();
let bodySnapshot: BodyStyleSnapshot | null = null;
let lockedScrollY = 0;

function isTopModal(id: string): boolean {
  return modalStack.length > 0 && modalStack[modalStack.length - 1] === id;
}

function pushModal(id: string): void {
  if (!modalStack.includes(id)) modalStack.push(id);
}

function removeModal(id: string): void {
  const idx = modalStack.indexOf(id);
  if (idx >= 0) modalStack.splice(idx, 1);
}

function focusFirst(modal: HTMLDivElement): void {
  const first = modal.querySelector<HTMLElement>(FOCUSABLE_SELECTOR);
  if (first) {
    first.focus();
    return;
  }
  modal.setAttribute("tabindex", "-1");
  modal.focus();
}

function focusTopModal(previous?: HTMLElement | null): void {
  if (modalStack.length === 0) return;
  const topId = modalStack[modalStack.length - 1];
  const topModal = modalElements.get(topId);
  if (topModal && previous && topModal.contains(previous)) previous.focus();
  else if (topModal) focusFirst(topModal);
}

function lockBodyScroll(): void {
  if (typeof document === "undefined" || typeof window === "undefined") return;
  if (bodySnapshot) return;

  lockedScrollY = window.scrollY;
  bodySnapshot = {
    overflow: document.body.style.overflow,
    position: document.body.style.position,
    top: document.body.style.top,
    width: document.body.style.width,
  };

  document.body.style.overflow = "hidden";
  document.body.style.position = "fixed";
  document.body.style.top = `-${lockedScrollY}px`;
  document.body.style.width = "100%";
}

function unlockBodyScroll(): void {
  if (typeof document === "undefined" || typeof window === "undefined") return;
  if (!bodySnapshot) return;

  document.body.style.overflow = bodySnapshot.overflow;
  document.body.style.position = bodySnapshot.position;
  document.body.style.top = bodySnapshot.top;
  document.body.style.width = bodySnapshot.width;
  window.scrollTo(0, lockedScrollY);

  bodySnapshot = null;
  lockedScrollY = 0;
}

/**
 * Provides modal UX behaviors:
 * 1. Escape key closes only the top-most modal
 * 2. Focus is trapped inside the top-most modal (Tab/Shift+Tab cycle)
 * 3. Body scroll is locked while any modal is open
 */
export function useModalA11y(isOpen: boolean, onClose: () => void) {
  const instanceId = useId();
  const modalRef = useRef<HTMLDivElement>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const onCloseRef = useRef(onClose);

  useEffect(() => {
    onCloseRef.current = onClose;
  }, [onClose]);

  const handleEscape = useCallback(
    (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (!isTopModal(instanceId)) return;
      e.preventDefault();
      e.stopPropagation();
      onCloseRef.current();
    },
    [instanceId],
  );

  const handleTab = useCallback(
    (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      if (!isTopModal(instanceId)) return;

      const modal = modalRef.current;
      if (!modal) return;

      const focusables = Array.from(modal.querySelectorAll<HTMLElement>(FOCUSABLE_SELECTOR));
      if (focusables.length === 0) {
        e.preventDefault();
        modal.setAttribute("tabindex", "-1");
        modal.focus();
        return;
      }

      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      const active = document.activeElement;

      if (e.shiftKey) {
        if (active === first || !modal.contains(active)) {
          e.preventDefault();
          last.focus();
        }
        return;
      }

      if (active === last || !modal.contains(active)) {
        e.preventDefault();
        first.focus();
      }
    },
    [instanceId],
  );

  useEffect(() => {
    if (!isOpen || typeof document === "undefined") return;

    const id = instanceId;
    previousFocusRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;

    pushModal(id);
    lockBodyScroll();

    document.addEventListener("keydown", handleEscape);
    document.addEventListener("keydown", handleTab);

    const raf = requestAnimationFrame(() => {
      const modal = modalRef.current;
      if (!modal) return;
      modalElements.set(id, modal);
      if (isTopModal(id)) focusFirst(modal);
    });

    return () => {
      cancelAnimationFrame(raf);
      document.removeEventListener("keydown", handleEscape);
      document.removeEventListener("keydown", handleTab);
      modalElements.delete(id);
      removeModal(id);

      if (modalStack.length === 0) {
        unlockBodyScroll();
        const previous = previousFocusRef.current;
        if (previous && document.contains(previous) && typeof previous.focus === "function") {
          previous.focus();
        }
      } else {
        const previous = previousFocusRef.current;
        requestAnimationFrame(() => focusTopModal(previous));
      }
    };
  }, [isOpen, instanceId, handleEscape, handleTab]);

  return { modalRef };
}
