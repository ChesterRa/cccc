import { useEffect } from "react";
import { useComposerStore } from "../../stores/useComposerStore";
import { usePendingPageWork } from "../../stores/usePendingPageWork";

/**
 * Pending sends, recordings and recovery snapshots live only in this page. The browser may
 * suppress the warning, and it cannot protect against a crash or a killed browser.
 */
export function PendingPageWorkUnloadGuard() {
  useEffect(() => {
    const protect = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = "";
    };
    let listening = false;
    const update = () => {
      const pending =
        usePendingPageWork.getState().owners.size > 0 ||
        Object.values(useComposerStore.getState().failedSendsByGroup).some(
          (items) => items.length > 0,
        );
      if (pending === listening) return;
      listening = pending;
      if (pending) window.addEventListener("beforeunload", protect);
      else window.removeEventListener("beforeunload", protect);
    };
    // Subscribe synchronously: a send clears its composer before React's next render.
    const unsubscribeWork = usePendingPageWork.subscribe(update);
    const unsubscribeComposer = useComposerStore.subscribe((state, previous) => {
      if (state.failedSendsByGroup !== previous.failedSendsByGroup) update();
    });
    update();
    return () => {
      unsubscribeWork();
      unsubscribeComposer();
      window.removeEventListener("beforeunload", protect);
    };
  }, []);
  return null;
}
